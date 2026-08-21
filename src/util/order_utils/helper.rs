// Helper functions for order utilities
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use crate::ui::state::{OperationResult, OrderChatStaticHeader, OrderSuccess, TakeOrderState};
use crate::util::db_utils::save_order;
use crate::util::dm_utils::{send_track_order_cmd, FETCH_EVENTS_TIMEOUT};
use crate::util::filters::create_filter;
use crate::util::types::{get_cant_do_description, Event, ListKind, MostroCantDoError};
use crate::util::OrderDmSubscriptionCmd;
use sqlx::SqlitePool;
use std::collections::BTreeSet;
use tokio::sync::mpsc::UnboundedSender;

/// Nostr events from relays (distinct from [`Event`] in `util::types`).
type NostrEvents = BTreeSet<nostr_sdk::prelude::Event>;

fn has_mostro_event_kind(event: &nostr_sdk::prelude::Event, kind: u16) -> bool {
    event.kind == nostr_sdk::prelude::Kind::Custom(kind)
}

fn is_mostro_order_event(event: &nostr_sdk::prelude::Event, mostro_pubkey: PublicKey) -> bool {
    event.pubkey == mostro_pubkey && has_mostro_event_kind(event, NOSTR_ORDER_EVENT_KIND)
}

fn is_mostro_dispute_event(event: &nostr_sdk::prelude::Event, mostro_pubkey: PublicKey) -> bool {
    event.pubkey == mostro_pubkey && has_mostro_event_kind(event, NOSTR_DISPUTE_EVENT_KIND)
}

/// Parse order from nostr tags
pub fn order_from_tags(tags: Tags) -> Result<SmallOrder> {
    let mut order = SmallOrder::default();

    for tag in tags {
        let t = tag.to_vec(); // Vec<String>
        if t.is_empty() {
            continue;
        }

        let key = t[0].as_str();
        let values = &t[1..];

        let v = values.first().map(|s| s.as_str()).unwrap_or_default();

        match key {
            "d" => {
                order.id = Uuid::parse_str(v).ok();
            }
            "k" => {
                order.kind = mostro_core::order::Kind::from_str(v).ok();
            }
            "f" => {
                order.fiat_code = v.to_string();
            }
            "s" => {
                order.status = Status::from_str(v).ok().or(Some(Status::Pending));
            }
            "amt" => {
                order.amount = v.parse::<i64>().unwrap_or(0);
            }
            "fa" => {
                if v.contains('.') {
                    continue;
                }
                if let Some(max_str) = values.get(1) {
                    order.min_amount = v.parse::<i64>().ok();
                    order.max_amount = max_str.parse::<i64>().ok();
                } else {
                    order.fiat_amount = v.parse::<i64>().unwrap_or(0);
                }
            }
            "pm" => {
                order.payment_method = values.join(",");
            }
            "premium" => {
                order.premium = v.parse::<i64>().unwrap_or(0);
            }
            _ => {}
        }
    }

    Ok(order)
}

/// Infer `Status` from the message `action` when there is no `SmallOrder` payload
/// (e.g. daemon sends `action: "canceled"` with `payload: null` but `id` on the kind).
pub fn inferred_status_from_trade_action(action: &Action) -> Option<Status> {
    match action {
        Action::Canceled => Some(Status::Canceled),
        Action::CooperativeCancelAccepted => Some(Status::CooperativelyCanceled),
        Action::WaitingBuyerInvoice | Action::AddInvoice => Some(Status::WaitingBuyerInvoice),
        Action::WaitingSellerToPay | Action::PayInvoice => Some(Status::WaitingPayment),
        Action::PayBondInvoice => Some(Status::WaitingTakerBond),
        Action::AdminCanceled => Some(Status::CanceledByAdmin),
        Action::FiatSentOk => Some(Status::FiatSent),
        // Seller `release` settles the hold invoice and starts the buyer payout.
        // Mostro stays on `settled-hold-invoice` until Lightning actually succeeds
        // (`purchase-completed`). Inferring Success here hid the later `add-invoice`
        // after failed payout retries.
        Action::Release | Action::Released | Action::HoldInvoicePaymentSettled => {
            Some(Status::SettledHoldInvoice)
        }
        Action::PurchaseCompleted => Some(Status::Success),
        _ => None,
    }
}

/// Map a Mostro `Action` plus the current `SmallOrder` into a new `Status`,
/// when the transition is clear from protocol semantics.
///
/// For intermediate states where Mostro already sets `order.status` on the
/// `SmallOrder`, callers can simply rely on that value instead of this helper.
pub fn map_action_to_status(action: &Action, order: &SmallOrder) -> Option<Status> {
    // If the order already has an explicit status from Mostro, prefer that.
    if let Some(status) = order.status {
        return Some(status);
    }

    inferred_status_from_trade_action(action)
}

fn status_phase_rank_for_actor(
    status: Status,
    kind: Option<mostro_core::order::Kind>,
) -> Option<u8> {
    match status {
        Status::Pending | Status::WaitingTakerBond | Status::WaitingMakerBond => Some(0),
        // Stage ordering follows listing kind progression (same for maker/taker):
        // Buy listing:  waiting-payment -> waiting-buyer-invoice
        // Sell listing: waiting-buyer-invoice -> waiting-payment
        Status::WaitingPayment => match kind {
            Some(mostro_core::order::Kind::Buy) => Some(1),
            Some(mostro_core::order::Kind::Sell) => Some(2),
            None => None,
        },
        Status::WaitingBuyerInvoice => match kind {
            Some(mostro_core::order::Kind::Buy) => Some(2),
            Some(mostro_core::order::Kind::Sell) => Some(1),
            None => None,
        },
        Status::InProgress | Status::Active => Some(3),
        Status::FiatSent => Some(4),
        // After seller release: hold invoice is settled, buyer payout is in flight.
        Status::SettledHoldInvoice => Some(5),
        Status::Success => Some(6),
        _ => None,
    }
}

pub(crate) fn is_terminal_trade_status(status: Status) -> bool {
    matches!(
        status,
        Status::Canceled
            | Status::CanceledByAdmin
            | Status::SettledByAdmin
            | Status::CompletedByAdmin
            | Status::Expired
            | Status::CooperativelyCanceled
            | Status::Success
    )
}

/// Outcome of an admin settle/cancel request after Mostro replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminFinalizeAck {
    /// Mostro confirmed `AdminSettled` or `AdminCanceled`.
    Confirmed,
    /// Order was already cooperatively canceled; Mostro replied `CooperativeCancelAccepted`.
    AlreadyCooperativelyCanceled,
}

/// Map a settle/cancel DM action to [`AdminFinalizeAck`].
pub(super) fn admin_finalize_ack(action: Action, expected: Action) -> Result<AdminFinalizeAck> {
    if action == expected {
        Ok(AdminFinalizeAck::Confirmed)
    } else if action == Action::CooperativeCancelAccepted {
        Ok(AdminFinalizeAck::AlreadyCooperativelyCanceled)
    } else {
        Err(anyhow::anyhow!(
            "Unexpected action in response: {:?}",
            action
        ))
    }
}

/// Guard status writes against backward transitions from stale/out-of-order DMs.
///
/// Returns `true` when `candidate` is equal/newer than `current` in the actor-aware phase graph.
/// Terminal states are sticky: once terminal, only the same terminal status is accepted —
/// except post-retry [`Action::AddInvoice`], which may reopen `Success` → `SettledHoldInvoice`
/// when an older client wrongly promoted `released` to Success before the replacement invoice.
///
/// Pass `action` from the DM being applied. Relay reconcile (no action) must pass `None` so
/// completed orders are never reopened from snapshots alone.
pub fn should_apply_status_transition(
    current: Option<Status>,
    candidate: Status,
    kind: Option<mostro_core::order::Kind>,
    action: Option<&Action>,
) -> bool {
    let Some(current) = current else {
        return true;
    };
    if current == candidate {
        return true;
    }
    if is_terminal_trade_status(current) {
        // Only the post-retry replacement-invoice DM may reopen Success → SettledHoldInvoice.
        // Action::Release / Released / HoldInvoicePaymentSettled must stay blocked.
        return matches!(action, Some(Action::AddInvoice))
            && current == Status::Success
            && candidate == Status::SettledHoldInvoice;
    }
    if is_terminal_trade_status(candidate) {
        return true;
    }
    match (
        status_phase_rank_for_actor(current, kind),
        status_phase_rank_for_actor(candidate, kind),
    ) {
        (Some(cur), Some(next)) => next >= cur,
        // Unknown transition edge: keep existing status (safer than downgrade).
        _ => false,
    }
}

/// Like [`should_apply_status_transition`], but never treats **equal** status as an advance.
///
/// Use when an **older** Nostr `timestamp` must not replace the Messages row unless the payload
/// status is strictly newer than what the current row already shows.
///
/// `action` is forwarded unchanged (needed for the post-retry [`Action::AddInvoice`] reopen).
pub fn should_strictly_advance_status(
    current: Option<Status>,
    candidate: Status,
    kind: Option<mostro_core::order::Kind>,
    action: Option<&Action>,
) -> bool {
    if let Some(cur) = current {
        if cur == candidate {
            return false;
        }
    }
    should_apply_status_transition(current, candidate, kind, action)
}

/// Validates the range amount input against min/max limits.
pub fn validate_range_amount(take_state: &mut TakeOrderState) {
    if take_state.amount_input.is_empty() {
        take_state.validation_error = None;
        return;
    }

    let amount = match take_state.amount_input.parse::<f64>() {
        Ok(val) if val.is_finite() => val,
        Ok(_) => {
            take_state.validation_error = Some("Invalid number format".to_string());
            return;
        }
        Err(_) => {
            take_state.validation_error = Some("Invalid number format".to_string());
            return;
        }
    };

    let min_opt = take_state.order.min_amount.map(|m| m as f64);
    let max_opt = take_state.order.max_amount.map(|m| m as f64);

    let below_min = min_opt.is_some_and(|min| amount < min);
    let above_max = max_opt.is_some_and(|max| amount > max);

    if below_min || above_max {
        let fiat = &take_state.order.fiat_code;
        take_state.validation_error = Some(match (min_opt, max_opt) {
            (Some(min), Some(max)) => {
                format!("Amount must be between {} and {} {}", min, max, fiat)
            }
            (Some(min), None) => format!("Amount must be at least {} {}", min, fiat),
            (None, Some(max)) => format!("Amount must be at most {} {}", max, fiat),
            (None, None) => "Amount is outside allowed range".to_string(),
        });
    } else {
        take_state.validation_error = None;
    }
}

/// Parse dispute from nostr tags.
///
/// When present, the `created_at` tag is the dispute open time from Mostro's
/// SQLite (`disputes.created_at` on kind 38386). It is independent of the Nostr
/// event's `created_at` (publish/replace time used for NIP-33 ordering).
pub fn dispute_from_tags(tags: Tags) -> Result<Dispute> {
    let mut dispute = Dispute::default();
    for tag in tags {
        let t = tag.to_vec();

        // Check if tag has at least 2 elements
        if t.len() < 2 {
            continue;
        }

        let key = t.first().map(|s| s.as_str()).unwrap_or("");
        let value = t.get(1).map(|s| s.as_str()).unwrap_or("");

        match key {
            "d" => {
                let id = value
                    .parse::<Uuid>()
                    .map_err(|_| anyhow::anyhow!("Invalid dispute id"))?;
                dispute.id = id;
            }
            "s" => {
                let status = DisputeStatus::from_str(value)
                    .map_err(|_| anyhow::anyhow!("Invalid dispute status"))?;
                dispute.status = status.to_string();
            }
            "created_at" => {
                // Prefer a positive unix-seconds open time; ignore malformed tags.
                if let Ok(ts) = value.parse::<i64>() {
                    if ts > 0 {
                        dispute.created_at = ts;
                    }
                }
            }
            _ => {}
        }
    }

    Ok(dispute)
}

/// Parse disputes from events.
///
/// Keeps only the latest NIP-33 revision per dispute id (greatest Nostr
/// `event.created_at`). For display, prefers the kind-38386 `created_at` **tag**
/// (dispute open time from Mostro) and falls back to `event.created_at` when the
/// tag is missing (older daemons / unreposted events).
pub fn parse_disputes_events(events: NostrEvents) -> Vec<Dispute> {
    // (published_at, dispute) — published_at drives latest-wins; dispute.created_at is open time.
    let mut latest_by_id: HashMap<Uuid, (i64, Dispute)> = HashMap::new();

    for event in events.iter() {
        if !has_mostro_event_kind(event, NOSTR_DISPUTE_EVENT_KIND) {
            continue;
        }
        let mut dispute = match dispute_from_tags(event.tags.clone()) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Failed to parse dispute from tags: {:?}", e);
                continue;
            }
        };

        let published_at = event.created_at.as_secs() as i64;
        // Tag open time wins for UI; event stamp is only a fallback for legacy events.
        if dispute.created_at <= 0 {
            dispute.created_at = published_at;
        }

        latest_by_id
            .entry(dispute.id)
            .and_modify(|(existing_published_at, existing)| {
                if published_at > *existing_published_at {
                    *existing_published_at = published_at;
                    *existing = dispute.clone();
                }
            })
            .or_insert((published_at, dispute));
    }

    // Newest dispute open time first (Pending "Created" column).
    let mut disputes_list: Vec<Dispute> = latest_by_id.into_values().map(|(_, d)| d).collect();
    disputes_list.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    disputes_list
}

/// Latest [`SmallOrder`] per order id from Mostro nostr order events (newest event wins).
///
/// Does not apply currency, status, or kind filters — use [`parse_orders_events`] for that.
pub fn aggregate_latest_orders_by_id(events: &NostrEvents) -> HashMap<Uuid, SmallOrder> {
    let mut latest_by_id: HashMap<Uuid, SmallOrder> = HashMap::new();

    for event in events.iter() {
        if !has_mostro_event_kind(event, NOSTR_ORDER_EVENT_KIND) {
            continue;
        }
        let mut order = match order_from_tags(event.tags.clone()) {
            Ok(o) => o,
            Err(e) => {
                log::error!("{e:?}");
                continue;
            }
        };
        let order_id = match order.id {
            Some(id) => id,
            None => {
                log::info!("Order ID is none");
                continue;
            }
        };
        if order.kind.is_none() {
            log::info!("Order kind is none");
            continue;
        }
        order.created_at = Some(event.created_at.as_secs() as i64);
        latest_by_id
            .entry(order_id)
            .and_modify(|existing| {
                let new_ts = order.created_at.unwrap_or(0);
                let old_ts = existing.created_at.unwrap_or(0);
                if new_ts > old_ts {
                    *existing = order.clone();
                }
            })
            .or_insert(order);
    }

    latest_by_id
}

/// Parse orders from events
pub fn parse_orders_events(
    events: NostrEvents,
    currencies: Option<Vec<String>>,
    status: Option<Status>,
    kind: Option<mostro_core::order::Kind>,
) -> Vec<SmallOrder> {
    let latest_by_id = aggregate_latest_orders_by_id(&events);

    let mut requested: Vec<SmallOrder> = latest_by_id
        .into_values()
        .filter(|o| status.map(|s| o.status == Some(s)).unwrap_or(true))
        .filter(|o| {
            // If currencies filter is provided and not empty, filter by any currency in the list
            // If currencies is None or empty, show all orders (no filter)
            order_matches_currency_filter(o, currencies.as_deref())
        })
        .filter(|o| {
            kind.as_ref()
                .map(|k| o.kind.as_ref() == Some(k))
                .unwrap_or(true)
        })
        .collect();

    requested.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    requested
}

/// Fetch raw Mostro order-kind events from relays (same filter as [`fetch_events_list`] for orders).
///
/// Events are filtered client-side to include only those authored by `mostro_pubkey`, since relay-side
/// author filtering cannot be trusted.
///
/// Relay queries are capped (see [`crate::util::filters::MOSTRO_LIST_FETCH_EVENT_LIMIT`]); very old
/// order updates may not be included in the snapshot.
pub async fn fetch_mostro_order_events(
    client: &Client,
    mostro_pubkey: PublicKey,
) -> Result<NostrEvents> {
    let filters = create_filter(ListKind::Orders, mostro_pubkey, None)?;
    let events = client
        .fetch_events(filters)
        .timeout(FETCH_EVENTS_TIMEOUT)
        .await?;
    Ok(events
        .into_iter()
        .filter(|e| is_mostro_order_event(e, mostro_pubkey))
        .collect())
}

/// Pending listings for the public order book from an aggregated relay snapshot.
///
/// Applies the same currency rules as [`parse_orders_events`] when `status` is pending-only:
/// empty `currencies` list means no filter; `None` means no filter.
pub fn pending_orders_for_book(
    latest: &HashMap<Uuid, SmallOrder>,
    currencies: Option<Vec<String>>,
) -> Vec<SmallOrder> {
    let mut requested: Vec<SmallOrder> = latest
        .values()
        .filter(|o| {
            o.status == Some(Status::Pending)
                && order_matches_currency_filter(o, currencies.as_deref())
        })
        .cloned()
        .collect();
    requested.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    requested
}

fn order_matches_currency_filter(order: &SmallOrder, currencies: Option<&[String]>) -> bool {
    currencies
        .map(|currencies| {
            currencies.is_empty()
                || currencies
                    .iter()
                    .any(|currency| currency.eq_ignore_ascii_case(&order.fiat_code))
        })
        .unwrap_or(true)
}

fn latest_small_order_by_id(
    events: &NostrEvents,
    mostro_pubkey: PublicKey,
    order_id: Uuid,
) -> Option<SmallOrder> {
    events
        .iter()
        .filter(|event| is_mostro_order_event(event, mostro_pubkey))
        .filter_map(|event| {
            let mut order = match order_from_tags(event.tags.clone()) {
                Ok(order) => order,
                Err(e) => {
                    log::warn!("Failed to parse order from tags: {:?}", e);
                    return None;
                }
            };
            if order.id != Some(order_id) || order.kind.is_none() {
                return None;
            }
            order.created_at = Some(event.created_at.as_secs() as i64);
            Some((event.created_at, event.id, order))
        })
        .max_by(|(a_ts, a_id, _), (b_ts, b_id, _)| {
            compare_revision_keys(*a_ts, *a_id, *b_ts, *b_id)
        })
        .map(|(_, _, order)| order)
}

fn latest_dispute_by_id(
    events: &NostrEvents,
    mostro_pubkey: PublicKey,
    dispute_id: Uuid,
) -> Option<Dispute> {
    events
        .iter()
        .filter(|event| is_mostro_dispute_event(event, mostro_pubkey))
        .filter_map(|event| {
            let mut dispute = match dispute_from_tags(event.tags.clone()) {
                Ok(dispute) => dispute,
                Err(e) => {
                    log::warn!("Failed to parse dispute from tags: {:?}", e);
                    return None;
                }
            };
            if dispute.id != dispute_id {
                return None;
            }
            if dispute.created_at <= 0 {
                dispute.created_at = event.created_at.as_secs() as i64;
            }
            Some((event.created_at, event.id, dispute))
        })
        .max_by(|(a_ts, a_id, _), (b_ts, b_id, _)| {
            compare_revision_keys(*a_ts, *a_id, *b_ts, *b_id)
        })
        .map(|(_, _, dispute)| dispute)
}

fn compare_revision_keys(
    a_ts: Timestamp,
    a_id: EventId,
    b_ts: Timestamp,
    b_id: EventId,
) -> std::cmp::Ordering {
    a_ts.cmp(&b_ts)
        .then_with(|| b_id.as_bytes().cmp(a_id.as_bytes()))
}

/// Fetch events list using the same logic as mostro-cli (adapted for mostrix)
pub async fn fetch_events_list(
    list_kind: ListKind,
    status: Option<Status>,
    currencies: Option<Vec<String>>,
    kind: Option<mostro_core::order::Kind>,
    client: &Client,
    mostro_pubkey: PublicKey,
    _since: Option<&i64>,
) -> Result<Vec<Event>> {
    match list_kind {
        ListKind::Orders => {
            let fetched_events = fetch_mostro_order_events(client, mostro_pubkey).await?;
            let orders = parse_orders_events(fetched_events, currencies, status, kind);
            Ok(orders.into_iter().map(Event::SmallOrder).collect())
        }
        ListKind::Disputes => {
            let filters = create_filter(list_kind, mostro_pubkey, None)?;
            let fetched_events: NostrEvents = client
                .fetch_events(filters)
                .timeout(FETCH_EVENTS_TIMEOUT)
                .await?
                .into_iter()
                .filter(|e| is_mostro_dispute_event(e, mostro_pubkey))
                .collect();
            let disputes = parse_disputes_events(fetched_events);
            Ok(disputes.into_iter().map(Event::Dispute).collect())
        }
        _ => Err(anyhow::anyhow!("Unsupported ListKind for mostrix")),
    }
}

/// Fetch orders from the Mostro network
/// Returns a vector of SmallOrder items filtered by the specified status and currencies
pub async fn get_orders(
    client: &Client,
    mostro_pubkey: PublicKey,
    status: Option<Status>,
    currencies: Option<Vec<String>>,
) -> Result<Vec<SmallOrder>> {
    let fetched_events = fetch_mostro_order_events(client, mostro_pubkey).await?;
    Ok(parse_orders_events(
        fetched_events,
        currencies,
        status,
        None,
    ))
}

/// Fetch disputes from the Mostro network
/// Returns a vector of Dispute items
pub async fn get_disputes(client: &Client, mostro_pubkey: PublicKey) -> Result<Vec<Dispute>> {
    let fetched_events = fetch_events_list(
        ListKind::Disputes,
        None,
        None,
        None,
        client,
        mostro_pubkey,
        None,
    )
    .await?;

    let disputes: Vec<Dispute> = fetched_events
        .into_iter()
        .filter_map(|event| {
            if let Event::Dispute(dispute) = event {
                Some(dispute)
            } else {
                None
            }
        })
        .collect();

    Ok(disputes)
}

/// Fetch the latest [`SmallOrder`] for one order id from relays (author + custom order kind + `d` tag).
///
/// Only events authored by `mostro_pubkey` are considered (client-side verification).
/// Uses `limit(10)` and picks the event with the greatest [`Event::created_at`] so relays that return
/// multiple revisions for the same identifier still resolve to the newest snapshot.
pub async fn fetch_small_order_by_id_from_relay(
    client: &Client,
    mostro_pubkey: PublicKey,
    order_id: Uuid,
) -> Result<Option<SmallOrder>> {
    let filter = Filter::new()
        .author(mostro_pubkey)
        .kind(nostr_sdk::prelude::Kind::Custom(NOSTR_ORDER_EVENT_KIND))
        .identifier(order_id.to_string())
        .limit(10);
    let events = client
        .fetch_events(filter)
        .timeout(FETCH_EVENTS_TIMEOUT)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch order from relay by id: {}", e))?;
    Ok(latest_small_order_by_id(&events, mostro_pubkey, order_id))
}

/// Fetch the latest kind-38386 [`Dispute`] for one dispute id (`d` tag).
///
/// Only events authored by `mostro_pubkey` are considered (client-side verification).
/// Uses `limit(10)` and picks the event with the greatest [`Event::created_at`].
pub async fn fetch_dispute_by_id_from_relay(
    client: &Client,
    mostro_pubkey: PublicKey,
    dispute_id: Uuid,
) -> Result<Option<Dispute>> {
    let filter = Filter::new()
        .author(mostro_pubkey)
        .kind(nostr_sdk::prelude::Kind::Custom(NOSTR_DISPUTE_EVENT_KIND))
        .identifier(dispute_id.to_string())
        .limit(10);
    let events = client
        .fetch_events(filter)
        .timeout(FETCH_EVENTS_TIMEOUT)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch dispute from relay by id: {}", e))?;
    Ok(latest_dispute_by_id(&events, mostro_pubkey, dispute_id))
}

/// Fetch a single order's fiat code from the relay by order id (identifier "d" tag).
/// Used when the order is not in the local DB (e.g. admin taking a dispute for an order they did not create).
pub async fn fetch_order_fiat_from_relay(
    client: &Client,
    mostro_pubkey: PublicKey,
    order_id: Uuid,
) -> Result<Option<String>> {
    let order = fetch_small_order_by_id_from_relay(client, mostro_pubkey, order_id).await?;
    Ok(order.and_then(|o| {
        let fiat = o.fiat_code;
        if fiat.is_empty() {
            None
        } else {
            Some(fiat)
        }
    }))
}

/// Build My Trades static header for one trade (maker or taker).
pub(super) fn build_order_chat_static_header(
    order: &SmallOrder,
    trade_index: i64,
    trade_keys: &Keys,
    is_mine: bool,
) -> Option<OrderChatStaticHeader> {
    let order_id = order.id?;
    Some(OrderChatStaticHeader {
        order_id,
        kind: order.kind,
        created_at: order.created_at,
        trade_index,
        initiator_trade_pubkey: trade_keys.public_key().to_string(),
        is_mine,
        solver_pubkey: None,
        dispute_id: None,
    })
}

/// Persist order + track subscription, then build `PaymentRequestRequired` for invoice popups.
#[allow(clippy::too_many_arguments)]
pub(super) async fn payment_request_operation_result(
    inner_action: Action,
    opt_order: Option<SmallOrder>,
    invoice_string: String,
    opt_amount: Option<i64>,
    fallback_order_id: Option<Uuid>,
    request_id: u64,
    next_idx: i64,
    pool: &SqlitePool,
    trade_keys: &Keys,
    is_mine: bool,
    dm_subscription_tx: Option<&UnboundedSender<OrderDmSubscriptionCmd>>,
    log_prefix: &str,
) -> Result<OperationResult> {
    let popup_action = match inner_action {
        Action::PayBondInvoice => Action::PayBondInvoice,
        _ => Action::PayInvoice,
    };

    let mut order_to_save =
        opt_order.ok_or_else(|| anyhow::anyhow!("Order details are missing from payload"))?;

    if order_to_save.id.is_none() {
        if let Some(fallback) = fallback_order_id {
            log::warn!(
                "[{log_prefix}] Mostro PaymentRequest payload order missing id; falling back to order_id={fallback}"
            );
            order_to_save.id = Some(fallback);
        } else {
            return Err(anyhow::anyhow!(
                "Order details are missing id in PaymentRequest payload"
            ));
        }
    }

    let effective_order_id = order_to_save
        .id
        .or(fallback_order_id)
        .ok_or_else(|| anyhow::anyhow!("Order id missing after PaymentRequest normalization"))?;

    log::info!(
        "[{log_prefix}] Action::{popup_action:?} response mapped to effective_order_id={effective_order_id}, trade_index={next_idx}"
    );

    if let Err(e) = save_order(
        order_to_save.clone(),
        trade_keys,
        request_id,
        next_idx,
        pool,
        is_mine,
    )
    .await
    {
        log::error!("Failed to save order to database: {e}");
    }

    if dm_subscription_tx.is_some() {
        log::info!(
            "[{log_prefix}] Sending DM subscription command for order_id={effective_order_id}, trade_index={next_idx}"
        );
        send_track_order_cmd(effective_order_id, next_idx);
    }

    log::info!("Received {popup_action:?} for order {effective_order_id} with invoice");

    let static_header =
        build_order_chat_static_header(&order_to_save, next_idx, trade_keys, is_mine).ok_or_else(
            || {
                anyhow::anyhow!(
                    "failed to build static header for order id {:?}",
                    order_to_save.id
                )
            },
        )?;

    let sat_amount = opt_amount.or(Some(order_to_save.amount));

    Ok(OperationResult::PaymentRequestRequired {
        order: order_to_save,
        invoice: invoice_string,
        sat_amount,
        trade_index: next_idx,
        static_header,
        action: popup_action,
    })
}

/// Helper function to create OperationResult::Success from an order
pub(super) fn create_order_result_success(
    order: &SmallOrder,
    trade_index: i64,
    trade_keys: &Keys,
    is_mine: bool,
) -> OperationResult {
    OperationResult::Success(OrderSuccess {
        order_id: order.id,
        kind: order.kind,
        amount: order.amount,
        fiat_code: order.fiat_code.clone(),
        fiat_amount: order.fiat_amount,
        min_amount: order.min_amount,
        max_amount: order.max_amount,
        payment_method: order.payment_method.clone(),
        premium: order.premium,
        status: order.status,
        trade_index: Some(trade_index),
        static_header: build_order_chat_static_header(order, trade_index, trade_keys, is_mine),
    })
}

/// Helper function to handle Mostro response and check for errors
pub(super) fn handle_mostro_response(
    response_message: &Message,
    expected_request_id: u64,
) -> Result<&mostro_core::message::MessageKind> {
    let inner_message = response_message.get_inner_message_kind();

    // Check for CantDo payload first (error response)
    if let Some(Payload::CantDo(reason)) = &inner_message.payload {
        if let Some(r) = reason {
            log::error!("Received CantDo error: {}", get_cant_do_description(r));
            return Err(MostroCantDoError { reason: r.clone() }.into());
        }
        log::error!("Received CantDo error without reason");
        return Err(anyhow::anyhow!(
            "Unknown error - Mostro couldn't process your request"
        ));
    }

    // Waiter path: every response must carry the in-flight request_id (see take_order.rs).
    match inner_message.request_id {
        Some(id) if id != expected_request_id => {
            log::warn!(
                "Received response with mismatched request_id. Expected: {}, Got: {}",
                expected_request_id,
                id
            );
            Err(anyhow::anyhow!("Mismatched request_id"))
        }
        Some(_) => Ok(inner_message),
        None => {
            log::warn!(
                "Received response with null request_id. Expected: {}",
                expected_request_id
            );
            Err(anyhow::anyhow!("Response with null request_id"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        admin_finalize_ack, dispute_from_tags, handle_mostro_response,
        inferred_status_from_trade_action, is_terminal_trade_status, latest_dispute_by_id,
        latest_small_order_by_id, parse_disputes_events, parse_orders_events,
        pending_orders_for_book, should_apply_status_transition, should_strictly_advance_status,
        AdminFinalizeAck,
    };
    use crate::models::TERMINAL_ORDER_HISTORY_STATUSES;
    use mostro_core::prelude::{
        Action, DisputeStatus, Message, SmallOrder, Status, NOSTR_DISPUTE_EVENT_KIND,
        NOSTR_ORDER_EVENT_KIND,
    };
    use nostr_sdk::prelude::*;
    use std::collections::BTreeSet;
    use std::str::FromStr;
    use uuid::Uuid;

    fn dispute_tags(id: Uuid, status: &str, opened_at: Option<i64>) -> Tags {
        let mut tags = vec![
            Tag::identifier(id.to_string()),
            Tag::custom("s", vec![status.to_string()]),
        ];
        if let Some(ts) = opened_at {
            tags.push(Tag::custom("created_at", vec![ts.to_string()]));
        }
        Tags::from_list(tags)
    }

    fn dispute_event(
        keys: &Keys,
        id: Uuid,
        status: &str,
        opened_at: Option<i64>,
        published_at: u64,
    ) -> Event {
        EventBuilder::new(Kind::Custom(NOSTR_DISPUTE_EVENT_KIND), "")
            .tags(dispute_tags(id, status, opened_at))
            .custom_created_at(Timestamp::from(published_at))
            .finalize(keys)
            .expect("dispute event")
    }

    fn order_tags(id: Uuid, fiat_code: &str, status: &str) -> Tags {
        Tags::from_list(vec![
            Tag::identifier(id.to_string()),
            Tag::custom("k", vec!["buy".to_string()]),
            Tag::custom("f", vec![fiat_code.to_string()]),
            Tag::custom("s", vec![status.to_string()]),
            Tag::custom("amt", vec!["1000".to_string()]),
        ])
    }

    fn order_tags_without_kind(id: Uuid, fiat_code: &str, status: &str) -> Tags {
        Tags::from_list(vec![
            Tag::identifier(id.to_string()),
            Tag::custom("f", vec![fiat_code.to_string()]),
            Tag::custom("s", vec![status.to_string()]),
            Tag::custom("amt", vec!["1000".to_string()]),
        ])
    }

    fn order_event(
        keys: &Keys,
        id: Uuid,
        fiat_code: &str,
        status: &str,
        kind: u16,
        published_at: u64,
    ) -> Event {
        EventBuilder::new(Kind::Custom(kind), "")
            .tags(order_tags(id, fiat_code, status))
            .custom_created_at(Timestamp::from(published_at))
            .finalize(keys)
            .expect("order event")
    }

    fn order_event_without_kind(
        keys: &Keys,
        id: Uuid,
        fiat_code: &str,
        status: &str,
        published_at: u64,
    ) -> Event {
        EventBuilder::new(Kind::Custom(NOSTR_ORDER_EVENT_KIND), "")
            .tags(order_tags_without_kind(id, fiat_code, status))
            .custom_created_at(Timestamp::from(published_at))
            .finalize(keys)
            .expect("order event without kind")
    }

    #[test]
    fn dispute_from_tags_reads_created_at_open_time() {
        let id = Uuid::new_v4();
        let dispute =
            dispute_from_tags(dispute_tags(id, "initiated", Some(1_700_000_100))).unwrap();
        assert_eq!(dispute.id, id);
        assert_eq!(dispute.status, DisputeStatus::Initiated.to_string());
        assert_eq!(dispute.created_at, 1_700_000_100);
    }

    #[test]
    fn dispute_from_tags_ignores_invalid_or_non_positive_created_at() {
        let id = Uuid::new_v4();
        let tags = Tags::from_list(vec![
            Tag::identifier(id.to_string()),
            Tag::custom("s", vec!["initiated".to_string()]),
            Tag::custom("created_at", vec!["not-a-number".to_string()]),
        ]);
        let dispute = dispute_from_tags(tags).unwrap();
        assert_eq!(dispute.created_at, 0);

        let tags_zero = Tags::from_list(vec![
            Tag::identifier(id.to_string()),
            Tag::custom("s", vec!["initiated".to_string()]),
            Tag::custom("created_at", vec!["0".to_string()]),
        ]);
        assert_eq!(dispute_from_tags(tags_zero).unwrap().created_at, 0);
    }

    #[test]
    fn parse_disputes_prefers_created_at_tag_over_event_stamp() {
        let keys = Keys::generate();
        let id = Uuid::new_v4();
        let events: BTreeSet<_> = [dispute_event(
            &keys,
            id,
            "initiated",
            Some(1_700_000_100),
            1_800_000_000,
        )]
        .into_iter()
        .collect();

        let parsed = parse_disputes_events(events);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].created_at, 1_700_000_100);
        assert_eq!(parsed[0].status, DisputeStatus::Initiated.to_string());
    }

    #[test]
    fn parse_disputes_falls_back_to_event_stamp_without_tag() {
        let keys = Keys::generate();
        let id = Uuid::new_v4();
        let events: BTreeSet<_> = [dispute_event(&keys, id, "initiated", None, 1_800_000_000)]
            .into_iter()
            .collect();

        let parsed = parse_disputes_events(events);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].created_at, 1_800_000_000);
    }

    #[test]
    fn pending_orders_match_currency_filter_case_insensitively() {
        let id = Uuid::new_v4();
        let mut latest = std::collections::HashMap::new();
        latest.insert(
            id,
            SmallOrder {
                id: Some(id),
                status: Some(Status::Pending),
                fiat_code: "usd".to_string(),
                ..Default::default()
            },
        );

        let filtered = pending_orders_for_book(&latest, Some(vec!["USD".to_string()]));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, Some(id));
    }

    #[test]
    fn parse_orders_ignores_events_with_wrong_kind() {
        let keys = Keys::generate();
        let id = Uuid::new_v4();
        let events: BTreeSet<_> = [
            order_event(&keys, id, "USD", "pending", NOSTR_ORDER_EVENT_KIND + 1, 20),
            order_event(&keys, id, "USD", "pending", NOSTR_ORDER_EVENT_KIND, 10),
        ]
        .into_iter()
        .collect();

        let parsed = parse_orders_events(events, None, Some(Status::Pending), None);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, Some(id));
        assert_eq!(parsed[0].created_at, Some(10));
    }

    #[test]
    fn parse_disputes_ignores_events_with_wrong_kind() {
        let keys = Keys::generate();
        let id = Uuid::new_v4();
        let events: BTreeSet<_> = [
            EventBuilder::new(Kind::Custom(NOSTR_DISPUTE_EVENT_KIND + 1), "")
                .tags(dispute_tags(id, "in-progress", Some(1_700_000_200)))
                .custom_created_at(Timestamp::from(20))
                .finalize(&keys)
                .expect("wrong kind dispute event"),
            dispute_event(&keys, id, "initiated", Some(1_700_000_100), 10),
        ]
        .into_iter()
        .collect();

        let parsed = parse_disputes_events(events);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, id);
        assert_eq!(parsed[0].status, DisputeStatus::Initiated.to_string());
    }

    #[test]
    fn targeted_order_lookup_ignores_newer_event_with_wrong_id() {
        let keys = Keys::generate();
        let wanted_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        let events: BTreeSet<_> = [
            order_event(
                &keys,
                wanted_id,
                "USD",
                "pending",
                NOSTR_ORDER_EVENT_KIND,
                10,
            ),
            order_event(
                &keys,
                other_id,
                "EUR",
                "pending",
                NOSTR_ORDER_EVENT_KIND,
                20,
            ),
        ]
        .into_iter()
        .collect();

        let order = latest_small_order_by_id(&events, keys.public_key(), wanted_id).unwrap();

        assert_eq!(order.id, Some(wanted_id));
        assert_eq!(order.fiat_code, "USD");
    }

    #[test]
    fn targeted_order_lookup_breaks_equal_timestamp_ties_by_lower_event_id() {
        let keys = Keys::generate();
        let order_id = Uuid::new_v4();
        let usd_event = order_event(
            &keys,
            order_id,
            "USD",
            "pending",
            NOSTR_ORDER_EVENT_KIND,
            10,
        );
        let eur_event = order_event(
            &keys,
            order_id,
            "EUR",
            "pending",
            NOSTR_ORDER_EVENT_KIND,
            10,
        );
        let expected_fiat = if usd_event.id.as_bytes() < eur_event.id.as_bytes() {
            "USD"
        } else {
            "EUR"
        };
        let events: BTreeSet<_> = [usd_event, eur_event].into_iter().collect();

        let order = latest_small_order_by_id(&events, keys.public_key(), order_id).unwrap();

        assert_eq!(order.id, Some(order_id));
        assert_eq!(order.fiat_code, expected_fiat);
    }

    #[test]
    fn targeted_order_lookup_rejects_order_without_kind() {
        let keys = Keys::generate();
        let order_id = Uuid::new_v4();
        let events: BTreeSet<_> = [
            order_event_without_kind(&keys, order_id, "EUR", "pending", 20),
            order_event(
                &keys,
                order_id,
                "USD",
                "pending",
                NOSTR_ORDER_EVENT_KIND,
                10,
            ),
        ]
        .into_iter()
        .collect();

        let order = latest_small_order_by_id(&events, keys.public_key(), order_id).unwrap();

        assert_eq!(order.id, Some(order_id));
        assert_eq!(order.fiat_code, "USD");
    }

    #[test]
    fn targeted_dispute_lookup_ignores_newer_event_with_wrong_id() {
        let keys = Keys::generate();
        let wanted_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        let events: BTreeSet<_> = [
            dispute_event(&keys, wanted_id, "initiated", Some(1_700_000_100), 10),
            dispute_event(&keys, other_id, "in-progress", Some(1_700_000_200), 20),
        ]
        .into_iter()
        .collect();

        let dispute = latest_dispute_by_id(&events, keys.public_key(), wanted_id).unwrap();

        assert_eq!(dispute.id, wanted_id);
        assert_eq!(dispute.status, DisputeStatus::Initiated.to_string());
    }

    #[test]
    fn targeted_dispute_lookup_breaks_equal_timestamp_ties_by_lower_event_id() {
        let keys = Keys::generate();
        let dispute_id = Uuid::new_v4();
        let initiated_event =
            dispute_event(&keys, dispute_id, "initiated", Some(1_700_000_100), 10);
        let in_progress_event =
            dispute_event(&keys, dispute_id, "in-progress", Some(1_700_000_100), 10);
        let expected_status = if initiated_event.id.as_bytes() < in_progress_event.id.as_bytes() {
            DisputeStatus::Initiated.to_string()
        } else {
            DisputeStatus::InProgress.to_string()
        };
        let events: BTreeSet<_> = [initiated_event, in_progress_event].into_iter().collect();

        let dispute = latest_dispute_by_id(&events, keys.public_key(), dispute_id).unwrap();

        assert_eq!(dispute.id, dispute_id);
        assert_eq!(dispute.status, expected_status);
    }

    #[test]
    fn parse_disputes_latest_revision_wins_by_event_stamp_not_open_tag() {
        let keys = Keys::generate();
        let id = Uuid::new_v4();
        let open = 1_700_000_100;
        // Older publish still initiated; newer publish taken — same open-time tag.
        let events: BTreeSet<_> = [
            dispute_event(&keys, id, "initiated", Some(open), 1_800_000_000),
            dispute_event(&keys, id, "in-progress", Some(open), 1_800_000_100),
        ]
        .into_iter()
        .collect();

        let parsed = parse_disputes_events(events);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].status, DisputeStatus::InProgress.to_string());
        assert_eq!(parsed[0].created_at, open);
    }

    #[test]
    fn release_infers_settled_hold_invoice_until_payout_succeeds() {
        assert_eq!(
            inferred_status_from_trade_action(&Action::Released),
            Some(Status::SettledHoldInvoice)
        );
        assert_eq!(
            inferred_status_from_trade_action(&Action::HoldInvoicePaymentSettled),
            Some(Status::SettledHoldInvoice)
        );
        assert_eq!(
            inferred_status_from_trade_action(&Action::PurchaseCompleted),
            Some(Status::Success)
        );
    }

    #[test]
    fn fiat_sent_can_advance_to_settled_hold_invoice() {
        assert!(should_apply_status_transition(
            Some(Status::FiatSent),
            Status::SettledHoldInvoice,
            Some(mostro_core::order::Kind::Sell),
            None,
        ));
    }

    #[test]
    fn add_invoice_can_reopen_success_to_settled_hold_invoice() {
        assert!(should_apply_status_transition(
            Some(Status::Success),
            Status::SettledHoldInvoice,
            Some(mostro_core::order::Kind::Sell),
            Some(&Action::AddInvoice),
        ));
    }

    #[test]
    fn release_actions_cannot_reopen_success_to_settled_hold_invoice() {
        let kind = Some(mostro_core::order::Kind::Sell);
        for action in [
            Action::Release,
            Action::Released,
            Action::HoldInvoicePaymentSettled,
        ] {
            assert!(
                !should_apply_status_transition(
                    Some(Status::Success),
                    Status::SettledHoldInvoice,
                    kind,
                    Some(&action),
                ),
                "{action:?} must not reopen a completed order"
            );
        }
        // Relay reconcile has no action context — also blocked.
        assert!(!should_apply_status_transition(
            Some(Status::Success),
            Status::SettledHoldInvoice,
            kind,
            None,
        ));
    }

    #[test]
    fn payment_failed_does_not_infer_a_status() {
        // `payment-failed` is a notification only; the order must stay in its current
        // status (`settled-hold-invoice`) while Mostro retries, so no status is inferred.
        assert_eq!(
            inferred_status_from_trade_action(&Action::PaymentFailed),
            None
        );
    }

    #[test]
    fn terminal_order_history_statuses_match_is_terminal_trade_status() {
        let terminal_variants = [
            Status::Success,
            Status::Canceled,
            Status::CanceledByAdmin,
            Status::SettledByAdmin,
            Status::CompletedByAdmin,
            Status::Expired,
            Status::CooperativelyCanceled,
        ];
        for s in terminal_variants {
            assert!(
                is_terminal_trade_status(s),
                "test variant list must stay aligned with is_terminal_trade_status"
            );
            let display = s.to_string();
            assert!(
                TERMINAL_ORDER_HISTORY_STATUSES.contains(&display.as_str()),
                "Status::to_string() for {s:?} must appear in TERMINAL_ORDER_HISTORY_STATUSES for targeted reconcile SQL"
            );
        }
        for &kebab in TERMINAL_ORDER_HISTORY_STATUSES {
            let parsed =
                Status::from_str(kebab).expect("TERMINAL_ORDER_HISTORY_STATUSES must parse");
            assert!(
                is_terminal_trade_status(parsed),
                "{kebab} in TERMINAL_ORDER_HISTORY_STATUSES must be terminal for relay reconcile"
            );
        }
    }

    #[test]
    fn buy_maker_blocks_waiting_payment_downgrade() {
        let allow = should_apply_status_transition(
            Some(Status::WaitingBuyerInvoice),
            Status::WaitingPayment,
            Some(mostro_core::order::Kind::Buy),
            None,
        );
        assert!(!allow);
    }

    #[test]
    fn sell_maker_allows_waiting_buyer_invoice_to_waiting_payment() {
        let allow = should_apply_status_transition(
            Some(Status::WaitingBuyerInvoice),
            Status::WaitingPayment,
            Some(mostro_core::order::Kind::Sell),
            None,
        );
        assert!(allow);
    }

    #[test]
    fn terminal_status_is_sticky() {
        let allow = should_apply_status_transition(
            Some(Status::CooperativelyCanceled),
            Status::WaitingPayment,
            Some(mostro_core::order::Kind::Buy),
            None,
        );
        assert!(!allow);
    }

    #[test]
    fn waiting_taker_bond_can_revert_to_pending() {
        let kind = Some(mostro_core::order::Kind::Buy);
        assert!(should_apply_status_transition(
            Some(Status::WaitingTakerBond),
            Status::Pending,
            kind,
            None,
        ));
    }

    #[test]
    fn apply_allows_equal_status_but_strict_does_not() {
        let kind = Some(mostro_core::order::Kind::Buy);
        assert!(should_apply_status_transition(
            Some(Status::WaitingPayment),
            Status::WaitingPayment,
            kind,
            None,
        ));
        assert!(!should_strictly_advance_status(
            Some(Status::WaitingPayment),
            Status::WaitingPayment,
            kind,
            None,
        ));
    }

    #[test]
    fn admin_finalize_ack_accepts_expected_and_cooperative_cancel() {
        assert_eq!(
            admin_finalize_ack(Action::AdminCanceled, Action::AdminCanceled).unwrap(),
            AdminFinalizeAck::Confirmed
        );
        assert_eq!(
            admin_finalize_ack(Action::AdminSettled, Action::AdminSettled).unwrap(),
            AdminFinalizeAck::Confirmed
        );
        assert_eq!(
            admin_finalize_ack(Action::CooperativeCancelAccepted, Action::AdminCanceled).unwrap(),
            AdminFinalizeAck::AlreadyCooperativelyCanceled
        );
        assert_eq!(
            admin_finalize_ack(Action::CooperativeCancelAccepted, Action::AdminSettled).unwrap(),
            AdminFinalizeAck::AlreadyCooperativelyCanceled
        );
        assert!(admin_finalize_ack(Action::Canceled, Action::AdminCanceled).is_err());
    }

    #[test]
    fn handle_mostro_response_rejects_null_request_id_on_waiter_path() {
        const EXPECTED_RID: u64 = 0xC0FF_EE00_1234_5678;

        for action in [
            Action::RateReceived,
            Action::NewOrder,
            Action::AddInvoice,
            Action::AddBondInvoice,
            Action::PayInvoice,
            Action::PayBondInvoice,
        ] {
            let message =
                Message::new_order(Some(Uuid::new_v4()), None, None, action.clone(), None);
            let err = handle_mostro_response(&message, EXPECTED_RID)
                .expect_err("null request_id must fail closed on waiter path");
            assert!(
                err.to_string().contains("null request_id"),
                "action {action:?} should reject null request_id"
            );
        }
    }

    #[test]
    fn handle_mostro_response_accepts_matching_request_id() {
        const EXPECTED_RID: u64 = 0xC0FF_EE00_1234_5678;
        let message = Message::new_order(
            Some(Uuid::new_v4()),
            Some(EXPECTED_RID),
            None,
            Action::PayInvoice,
            None,
        );
        let inner = handle_mostro_response(&message, EXPECTED_RID).expect("matching rid");
        assert_eq!(inner.request_id, Some(EXPECTED_RID));
        assert_eq!(inner.action, Action::PayInvoice);
    }

    #[test]
    fn handle_mostro_response_rejects_mismatched_request_id() {
        const EXPECTED_RID: u64 = 0xC0FF_EE00_1234_5678;
        let message = Message::new_order(
            Some(Uuid::new_v4()),
            Some(EXPECTED_RID.wrapping_add(1)),
            None,
            Action::PayInvoice,
            None,
        );
        let err = handle_mostro_response(&message, EXPECTED_RID)
            .expect_err("mismatched request_id must be rejected");
        assert!(err.to_string().contains("Mismatched request_id"));
    }
}
