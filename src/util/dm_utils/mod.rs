// Direct message manager module
// Contains functions for handling direct messages, order channels, and notifications

mod dm_helpers;
mod notifications_ch_mng;
mod order_ch_mng;
mod order_result_tx;

pub use dm_helpers::seed_admin_chat_last_seen;
pub use notifications_ch_mng::{
    apply_saved_ln_address_invoice_choice, handle_message_notification, present_add_invoice_popup,
};
pub use order_ch_mng::handle_operation_result;
pub use order_result_tx::{set_order_result_tx, try_notify_my_trades_maker_book_changed};

use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::models::{Order, User};
use crate::ui::order_message_to_notification;
use crate::ui::{MessageNotification, OrderMessage};
use crate::util::db_utils::{delete_order_by_id, save_order, update_order_status};
use crate::util::filters::filter_giftwrap_to_recipient;
use crate::util::mostro_info::{nostr_pow_from_instance, MostroInstanceInfo};
use crate::util::order_utils::{
    inferred_status_from_trade_action, map_action_to_status, should_apply_status_transition,
    should_strictly_advance_status,
};

pub const FETCH_EVENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const PENDING_WAITER_GC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_PENDING_WAITERS: usize = 32;

#[derive(Debug)]
pub enum DmRouterCmd {
    TrackOrder {
        order_id: Uuid,
        trade_index: i64,
    },
    RegisterWaiter {
        trade_keys: Keys,
        response_tx: oneshot::Sender<Event>,
    },
}

pub type OrderDmSubscriptionCmd = DmRouterCmd;

static DM_ROUTER_CMD_TX: Mutex<Option<mpsc::UnboundedSender<DmRouterCmd>>> = Mutex::new(None);

/// Cumulative count of GiftWrap routes that ran the linear active-order decrypt fallback
/// (`resolve_order_for_event`). Useful for monitoring how often the O(n) path runs.
static GIFTWRAP_FALLBACK_DECRYPT_TOTAL: AtomicU64 = AtomicU64::new(0);
/// Last fallback scan: number of active orders considered.
static GIFTWRAP_FALLBACK_LAST_ACTIVE_COUNT: AtomicU64 = AtomicU64::new(0);
/// Last fallback scan: loop duration in milliseconds.
static GIFTWRAP_FALLBACK_LAST_DURATION_MS: AtomicU64 = AtomicU64::new(0);

pub struct StartupDmHydration {
    pub active_order_trade_indices: HashMap<Uuid, i64>,
    pub order_last_seen_dm_ts: HashMap<Uuid, i64>,
}

impl StartupDmHydration {
    /// Empty maps when DB hydration fails; same value used at startup, reconnect, and key reload.
    pub fn empty() -> Self {
        Self {
            active_order_trade_indices: HashMap::new(),
            order_last_seen_dm_ts: HashMap::new(),
        }
    }
}

/// Publishes the global sender consumed by `listen_for_order_messages` and `wait_for_dm`.
///
/// Returns `Err` if the mutex is poisoned (the sender was **not** updated).
pub fn set_dm_router_cmd_tx(tx: mpsc::UnboundedSender<DmRouterCmd>) -> Result<(), &'static str> {
    match DM_ROUTER_CMD_TX.lock() {
        Ok(mut guard) => {
            *guard = Some(tx);
            Ok(())
        }
        Err(_) => {
            crate::util::request_fatal_restart(
                "Mostrix encountered an internal error (poisoned DM router lock). Please restart the app."
                    .to_string(),
            );
            Err("DM_ROUTER_CMD_TX mutex poisoned")
        }
    }
}

/// Full DM-terminal set including [`Status::Success`]. Startup SQL hydration uses
/// [`crate::models::TERMINAL_DM_STATUSES`] instead, which omits `success` so rating/follow-up DMs still load.
fn is_terminal_order_status(status: Status) -> bool {
    matches!(
        status,
        Status::Success
            | Status::Canceled
            | Status::CanceledByAdmin
            | Status::SettledByAdmin
            | Status::CompletedByAdmin
            | Status::Expired
            | Status::CooperativelyCanceled
    )
}

/// Loads active-order rows for DM bootstrap; status filter is [`crate::models::TERMINAL_DM_STATUSES`].
pub async fn hydrate_startup_active_order_dm_state(
    pool: &sqlx::sqlite::SqlitePool,
) -> Result<StartupDmHydration> {
    let rows = Order::get_startup_active_orders(pool).await?;
    let mut active_order_trade_indices: HashMap<Uuid, i64> = HashMap::new();
    let mut order_last_seen_dm_ts: HashMap<Uuid, i64> = HashMap::new();

    for row in rows {
        let Ok(order_id) = Uuid::parse_str(&row.id) else {
            continue;
        };
        let Some(trade_index) = row.trade_index else {
            log::error!(
                "Order {} is non-terminal but missing trade_index in DB; skipping DM startup hydration for this row",
                row.id
            );
            continue;
        };
        active_order_trade_indices.insert(order_id, trade_index);
        if let Some(ts) = row.last_seen_dm_ts {
            order_last_seen_dm_ts.insert(order_id, ts);
        }
    }

    Ok(StartupDmHydration {
        active_order_trade_indices,
        order_last_seen_dm_ts,
    })
}

fn message_has_terminal_order_status(message: &Message) -> bool {
    message
        .get_inner_message_kind()
        .payload
        .as_ref()
        .and_then(|payload| match payload {
            Payload::Order(order) => order.status,
            _ => None,
        })
        .map(is_terminal_order_status)
        .unwrap_or(false)
}

/// Terminal end of trade: either `SmallOrder.status` in the payload, or actions that
/// Mostro sends with `payload: null` (e.g. `canceled`).
fn trade_message_is_terminal(message: &Message) -> bool {
    let kind = message.get_inner_message_kind();
    if matches!(
        &kind.action,
        Action::AdminCanceled | Action::Canceled | Action::CooperativeCancelAccepted
    ) {
        return true;
    }
    message_has_terminal_order_status(message)
}

/// Send a direct message to a receiver
#[allow(clippy::too_many_arguments)]
pub async fn send_dm(
    client: &Client,
    identity_keys: Option<&Keys>,
    trade_keys: &Keys,
    receiver_pubkey: &PublicKey,
    payload: String,
    expiration: Option<Timestamp>,
    _to_user: bool,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<()> {
    let pow = nostr_pow_from_instance(mostro_instance);
    let message = Message::from_json(&payload)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize message: {e}"))?;
    let identity_keys = identity_keys.unwrap_or(trade_keys);
    let event = wrap_message(
        &message,
        identity_keys,
        trade_keys,
        *receiver_pubkey,
        WrapOptions {
            pow,
            expiration,
            signed: true,
        },
    )
    .await?;

    client.send_event(&event).await?;
    Ok(())
}

/// Wait for a direct message response from Mostro
/// Registers a router waiter, then sends the message (to avoid missing responses).
pub async fn wait_for_dm<F>(
    trade_keys: &Keys,
    timeout: std::time::Duration,
    sent_message: F,
) -> Result<Events>
where
    F: std::future::Future<Output = Result<()>> + Send,
{
    let dm_router_tx = match DM_ROUTER_CMD_TX.lock() {
        Ok(guard) => guard.clone().ok_or_else(|| {
            anyhow::anyhow!("DM router is not ready. Please retry after listener initialization.")
        })?,
        Err(_) => {
            crate::util::request_fatal_restart(
                "Mostrix encountered an internal error (poisoned DM router lock). Please restart the app."
                    .to_string(),
            );
            return Err(anyhow::anyhow!(
                "DM router mutex poisoned; restart the application."
            ));
        }
    };
    let (response_tx, response_rx) = oneshot::channel::<Event>();
    dm_router_tx
        .send(DmRouterCmd::RegisterWaiter {
            trade_keys: trade_keys.clone(),
            response_tx,
        })
        .map_err(|_| anyhow::anyhow!("Failed to register DM waiter: router channel closed"))?;

    // Send message only after waiter registration to avoid races.
    sent_message.await?;
    let event = tokio::time::timeout(timeout, response_rx)
        .await
        .map_err(|_| anyhow::anyhow!("Timeout waiting for DM or gift wrap event"))?
        .map_err(|_| anyhow::anyhow!("DM waiter canceled before receiving an event"))?;

    let mut events = Events::default();
    events.insert(event);
    Ok(events)
}

/// Parse DM events to extract Messages
pub async fn parse_dm_events(
    events: Events,
    pubkey: &Keys,
    since: Option<&i64>,
) -> Vec<(Message, i64, PublicKey)> {
    use base64::engine::general_purpose;
    use base64::Engine;
    use nip44::v2::{decrypt_to_bytes, ConversationKey};

    let mut id_set = HashSet::<EventId>::new();
    let mut direct_messages: Vec<(Message, i64, PublicKey)> = Vec::new();

    for dm in events.iter() {
        // Skip if already processed
        if !id_set.insert(dm.id) {
            continue;
        }

        let (created_at, message, sender) = match dm.kind {
            nostr_sdk::Kind::GiftWrap => match unwrap_message(dm, pubkey).await {
                Ok(None) => continue,
                Err(e) => {
                    log::warn!("Could not unwrap gift wrap (event {}): {}", dm.id, e);
                    continue;
                }
                Ok(Some(u)) => (u.created_at, u.message, u.sender),
            },
            nostr_sdk::Kind::PrivateDirectMessage => {
                let ck = if let Ok(ck) = ConversationKey::derive(pubkey.secret_key(), &dm.pubkey) {
                    ck
                } else {
                    continue;
                };
                let b64decoded_content =
                    match general_purpose::STANDARD.decode(dm.content.as_bytes()) {
                        Ok(b64decoded_content) => b64decoded_content,
                        Err(_) => {
                            continue;
                        }
                    };
                let unencrypted_content = match decrypt_to_bytes(&ck, &b64decoded_content) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        continue;
                    }
                };
                let message_str = match String::from_utf8(unencrypted_content) {
                    Ok(s) => s,
                    Err(_) => {
                        continue;
                    }
                };
                let message = match Message::from_json(&message_str) {
                    Ok(m) => m,
                    Err(_) => {
                        continue;
                    }
                };
                (dm.created_at, message, dm.pubkey)
            }
            _ => continue,
        };

        // Check if the message is older than the since time if it is, skip it
        if let Some(since_time) = since {
            let since_time = chrono::Utc::now()
                .checked_sub_signed(chrono::Duration::minutes(*since_time))
                .unwrap()
                .timestamp();

            if (created_at.as_secs() as i64) < since_time {
                continue;
            }
        }
        direct_messages.push((message, created_at.as_secs() as i64, sender));
    }
    direct_messages.sort_by(|a, b| a.1.cmp(&b.1));
    direct_messages
}

/// Parse one GiftWrap [`Event`] with the trade key (`since: None`). Shared by relay notifications and startup replay.
///
/// When `pre_unwrapped` is set (e.g. after a successful `unwrap_message`), skips a second unwrap.
async fn parse_dm_events_single(
    event: &Event,
    trade_keys: &Keys,
    pre_unwrapped: Option<UnwrappedMessage>,
) -> Vec<(Message, i64, PublicKey)> {
    if let Some(u) = pre_unwrapped {
        return vec![(u.message, u.created_at.as_secs() as i64, u.sender)];
    }
    let mut batch = Events::default();
    batch.insert(event.clone());
    parse_dm_events(batch, trade_keys, None).await
}

/// `SmallOrder` embedded in the payload when present (standalone order or pay-invoice with a full order).
fn small_order_ref_from_payload(payload: &Option<Payload>) -> Option<&SmallOrder> {
    match payload.as_ref()? {
        Payload::Order(o) => Some(o),
        Payload::PaymentRequest(Some(o), _, _) => Some(o),
        _ => None,
    }
}

fn resolved_status_candidate(action: &Action, payload: &Option<Payload>) -> Option<Status> {
    if let Some(order_payload) = small_order_ref_from_payload(payload) {
        return map_action_to_status(action, order_payload);
    }
    inferred_status_from_trade_action(action)
}

fn is_pre_active_status(status: Status) -> bool {
    matches!(
        status,
        Status::Pending
            | Status::WaitingTakerBond
            | Status::WaitingPayment
            | Status::WaitingBuyerInvoice
            | Status::SettledHoldInvoice
    )
}

fn order_status_from_row(row: &Order) -> Option<Status> {
    row.status.as_ref().and_then(|s| Status::from_str(s).ok())
}

fn is_pre_active_taker_take(row: &Order) -> bool {
    !row.is_mine
        && order_status_from_row(row)
            .map(is_pre_active_status)
            .unwrap_or(false)
}

fn is_pre_active_maker_listing(row: &Order) -> bool {
    row.is_mine
        && order_status_from_row(row)
            .map(is_pre_active_status)
            .unwrap_or(false)
}

/// Drop SQLite row and Messages-tab entries for a taker take that ended before Active.
async fn drop_pre_active_taker_take(
    pool: &sqlx::SqlitePool,
    messages: &Arc<Mutex<Vec<OrderMessage>>>,
    order_id: Uuid,
    log_context: &str,
) {
    if let Err(e) = delete_order_by_id(pool, &order_id.to_string()).await {
        log::warn!(
            "Failed to delete pre-Active taker row on {} {}: {}",
            log_context,
            order_id,
            e
        );
    }
    remove_order_from_messages(messages, order_id);
}

/// Refreshes the local `orders` row from embedded order data on trade DMs that carry a full
/// `SmallOrder` (e.g. `add-invoice`, `pay-invoice`, `buyer-took-order`, `hold-invoice-payment-accepted`).
async fn upsert_order_from_trade_dm(
    pool: &sqlx::SqlitePool,
    order_id: Uuid,
    action: &Action,
    payload: &Option<Payload>,
    request_id: Option<u64>,
    trade_keys: &Keys,
) {
    let (label, small_order) = match (action, payload.as_ref()) {
        (Action::AddInvoice, Some(Payload::Order(o))) => ("AddInvoice", o.clone()),
        (Action::PayInvoice, Some(Payload::PaymentRequest(Some(o), _, _))) => {
            ("PayInvoice", o.clone())
        }
        (Action::PayBondInvoice, Some(Payload::PaymentRequest(Some(o), _, _))) => {
            ("PayBondInvoice", o.clone())
        }
        (Action::BuyerTookOrder, Some(Payload::Order(o))) => ("BuyerTookOrder", o.clone()),
        (Action::HoldInvoicePaymentAccepted, Some(Payload::Order(o))) => {
            ("HoldInvoicePaymentAccepted", o.clone())
        }
        (Action::NewOrder, Some(Payload::Order(o))) => ("NewOrder", o.clone()),
        _ => return,
    };
    let msg_request_id = request_id.and_then(|u| i64::try_from(u).ok());
    let status_for_log = small_order.status;
    match Order::upsert_from_small_order_dm(pool, order_id, small_order, trade_keys, msg_request_id)
        .await
    {
        Ok(_) => log::info!(
            "Persisted order {} to database from {} DM (status={:?})",
            order_id,
            label,
            status_for_log
        ),
        Err(e) => log::error!(
            "Failed to persist order {} from {} DM: {}",
            order_id,
            label,
            e
        ),
    }
}

/// Resolve maker/taker for [`crate::ui::OrderMessage::is_mine`] after a trade DM is stored.
///
/// Callers must pass SQLite **after** [`upsert_order_from_trade_dm`], not the pre-upsert snapshot
/// (see `handle_trade_dm_for_order`).
///
/// # Why `Option<bool>`
///
/// - [`crate::util::db_utils::save_order`] always writes `is_mine` (`true` = maker, `false` = taker).
/// - [`crate::models::Order::upsert_from_small_order_dm`] may insert a row first and defaults new
///   rows to maker; that must not be treated as role-known until `save_order` runs.
///
/// # Branches
///
/// - **Row existed before upsert** (create/take already persisted): post-upsert `is_mine` is trusted.
/// - **No row before upsert** (typical taker race: `TrackOrder` before `save_order(false)`): keep
///   `None` unless an earlier Messages row already carried role; UI helpers then default to taker.
fn try_send_track_order(order_id: Uuid, trade_index: i64) {
    let Ok(guard) = DM_ROUTER_CMD_TX.lock() else {
        return;
    };
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(DmRouterCmd::TrackOrder {
            order_id,
            trade_index,
        });
    }
}

/// `NewOrder` + `Payload::Order` with `status: pending` — book republish or range child listing.
fn small_order_pending_from_new_order_payload(payload: &Option<Payload>) -> Option<SmallOrder> {
    match payload.as_ref()? {
        Payload::Order(o) if o.status == Some(Status::Pending) => Some(o.clone()),
        _ => None,
    }
}

/// Maker listing returns to the book after a pre-Active taker cancel (`NewOrder` republish).
async fn revert_maker_to_pending_on_book_republish(
    pool: &sqlx::SqlitePool,
    messages: &Arc<Mutex<Vec<OrderMessage>>>,
    order_id: Uuid,
    trade_index: i64,
    inner_kind: &MessageKind,
    trade_keys: &Keys,
) {
    upsert_order_from_trade_dm(
        pool,
        order_id,
        &Action::NewOrder,
        &inner_kind.payload,
        inner_kind.request_id,
        trade_keys,
    )
    .await;
    if let Err(e) = update_order_status(pool, &order_id.to_string(), Status::Pending).await {
        log::warn!(
            "Failed to revert maker order {} to pending after NewOrder republish: {}",
            order_id,
            e
        );
    }

    remove_order_from_messages(messages, order_id);
    try_notify_my_trades_maker_book_changed();

    log::info!(
        "Order {} reverted to pending on book (NewOrder republish, trade_index={}); removed from Messages",
        order_id,
        trade_index
    );
}

/// Range-order child listing on a fresh trade key when no local row exists yet.
async fn persist_range_child_listing_from_new_order(
    pool: &sqlx::SqlitePool,
    order_id: Uuid,
    trade_index: i64,
    small_order: &SmallOrder,
    request_id: u64,
    trade_keys: &Keys,
) -> bool {
    if let Err(e) = save_order(
        small_order.clone(),
        trade_keys,
        request_id,
        trade_index,
        pool,
        true,
    )
    .await
    {
        log::error!(
            "Failed to persist range child order {} from NewOrder DM: {}",
            order_id,
            e
        );
        return false;
    }

    try_send_track_order(order_id, trade_index);
    try_notify_my_trades_maker_book_changed();

    log::info!(
        "Persisted new pending child listing {} from NewOrder DM (trade_index={})",
        order_id,
        trade_index
    );
    true
}

/// Handle `Action::NewOrder` on the trade-DM listener (not the create-order waiter).
///
/// Returns `true` when the message was consumed (caller should return).
async fn try_handle_new_order_trade_dm(
    messages: &Arc<Mutex<Vec<OrderMessage>>>,
    order_id: Uuid,
    trade_index: i64,
    inner_kind: &MessageKind,
    pool: &sqlx::SqlitePool,
    trade_keys: &Keys,
) -> bool {
    let Some(small_order) = small_order_pending_from_new_order_payload(&inner_kind.payload) else {
        return false;
    };

    let db_order = Order::get_by_id(pool, &order_id.to_string()).await.ok();

    if let Some(ref row) = db_order {
        if is_pre_active_taker_take(row) {
            drop_pre_active_taker_take(pool, messages, order_id, "NewOrder book republish").await;
            return true;
        }
        if is_pre_active_maker_listing(row) {
            revert_maker_to_pending_on_book_republish(
                pool,
                messages,
                order_id,
                trade_index,
                inner_kind,
                trade_keys,
            )
            .await;
            return true;
        }
        return false;
    }

    persist_range_child_listing_from_new_order(
        pool,
        order_id,
        trade_index,
        &small_order,
        inner_kind.request_id.unwrap_or(0),
        trade_keys,
    )
    .await
}

fn effective_is_mine_for_trade_dm_message(
    had_local_row_before_upsert: bool,
    post_upsert_is_mine: Option<bool>,
    prior_message_is_mine: Option<bool>,
) -> Option<bool> {
    if had_local_row_before_upsert {
        // Maker after `send_new_order`, or taker after `take_order` — DB role is authoritative.
        return post_upsert_is_mine.or(prior_message_is_mine);
    }
    // First DM before `save_order`: ignore upsert's maker default (`true` in SQLite).
    prior_message_is_mine
}

/// Handle a single decoded trade DM for a given order/trade index.
#[allow(clippy::too_many_arguments)]
async fn handle_trade_dm_for_order(
    messages: &Arc<Mutex<Vec<OrderMessage>>>,
    pending_notifications: &Arc<Mutex<usize>>,
    message_notification_tx: &UnboundedSender<MessageNotification>,
    order_id: Uuid,
    trade_index: i64,
    message: Message,
    timestamp: i64,
    sender: PublicKey,
    pool: &sqlx::SqlitePool,
    trade_keys: &Keys,
    // When false (startup relay replay), hydrate Messages without bumping counters or UI toasts.
    notify: bool,
) {
    let inner_kind = message.get_inner_message_kind();
    let action = inner_kind.action.clone();
    if matches!(action, Action::CantDo) {
        return;
    }
    if matches!(action, Action::NewOrder) {
        let _ = try_handle_new_order_trade_dm(
            messages,
            order_id,
            trade_index,
            inner_kind,
            pool,
            trade_keys,
        )
        .await;
        return;
    }

    // Snapshot before upsert: used for status/cancel paths and to detect whether `save_order`
    // already established maker/taker (vs a DM-only row inserted below).
    let db_order = Order::get_by_id(pool, &order_id.to_string()).await.ok();
    let had_local_row_before_upsert = db_order.is_some();
    let status_from_db = db_order.as_ref().and_then(order_status_from_row);

    let status_candidate = resolved_status_candidate(&action, &inner_kind.payload);

    // Taker pre-Active cancel returns the order to the book; drop stale local row instead of
    // keeping it as terminal trade state.
    if matches!(action, Action::Canceled)
        && inner_kind.payload.is_none()
        && db_order.as_ref().is_some_and(is_pre_active_taker_take)
    {
        drop_pre_active_taker_take(pool, messages, order_id, "Canceled").await;
        return;
    }

    upsert_order_from_trade_dm(
        pool,
        order_id,
        &action,
        &inner_kind.payload,
        inner_kind.request_id,
        trade_keys,
    )
    .await;

    // Extract invoice and sat_amount from payload based on action type.
    // For `PayBondInvoice` mostrod populates the bond satoshis in the third
    // `Option<Amount>` field of `Payload::PaymentRequest` (the SmallOrder is
    // `None` per mostro-core 0.11.0 wire format); for `PayInvoice` it may come
    // either as that explicit override or via the embedded order's `amount`.
    let (sat_amount, invoice) = match &action {
        Action::PayInvoice | Action::PayBondInvoice => match &inner_kind.payload {
            Some(Payload::PaymentRequest(opt_order, invoice, opt_amount)) => {
                let amount = opt_amount.or_else(|| opt_order.as_ref().map(|o| o.amount));
                (amount, Some(invoice.clone()))
            }
            _ => (None, None),
        },
        Action::AddInvoice => match &inner_kind.payload {
            Some(Payload::Order(order)) => (Some(order.amount), None),
            _ => (None, None),
        },
        _ => (None, None),
    };

    // Only show PayInvoice/PayBondInvoice popup/notification when an invoice is actually present.
    let is_actionable_notification = match &action {
        Action::PayInvoice | Action::PayBondInvoice => {
            invoice.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
        }
        Action::AddInvoice => sat_amount.is_some(),
        _ => true,
    };

    if matches!(action, Action::PayInvoice | Action::PayBondInvoice) && !is_actionable_notification
    {
        return;
    }

    // Lock `messages` only long enough to extract comparison data, then drop it
    // before touching `pending_notifications` to avoid lock-order deadlocks.
    let existing_message_data = {
        let messages_lock = match messages.lock() {
            Ok(g) => g,
            Err(e) => {
                crate::util::request_fatal_restart(format!(
                    "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
                ));
                return;
            }
        };
        messages_lock
            .iter()
            .filter(|m| m.order_id == Some(order_id))
            .max_by_key(|m| m.timestamp)
            .map(|m| {
                (
                    m.timestamp,
                    m.message.get_inner_message_kind().action.clone(),
                    m.sat_amount,
                    m.buyer_invoice.clone(),
                    m.auto_popup_shown,
                    m.order_kind,
                    m.is_mine,
                    m.order_status,
                )
            })
    };

    // Only increment pending notifications if this is a truly new message.
    // Relay delivery can be out-of-order: a later protocol step may carry an older Nostr
    // `created_at` than a message we already stored. If we only compared timestamps,
    // `waiting-seller-to-pay` after `add-invoice` would not bump the counter. Treat any
    // **different action** as a new notification; for the **same** action, require a
    // strictly newer timestamp (dedup stale/duplicate events).
    let is_new_message = match &existing_message_data {
        None => true,
        Some((existing_timestamp, existing_action, _, _, _, _, _, _)) => {
            if action != *existing_action {
                true
            } else {
                timestamp > *existing_timestamp
            }
        }
    };

    let prior_sat_amount = existing_message_data
        .as_ref()
        .and_then(|(_, _, amt, _, _, _, _, _)| *amt);
    let prior_invoice = existing_message_data
        .as_ref()
        .and_then(|(_, _, _, inv, _, _, _, _)| inv.clone());
    let prior_auto_popup_shown = existing_message_data
        .as_ref()
        .map(|(_, existing_action, _, _, shown, _, _, _)| *shown && *existing_action == action)
        .unwrap_or(false);
    let prior_order_kind = existing_message_data
        .as_ref()
        .and_then(|(_, _, _, _, _, k, _, _)| *k);
    let prior_is_mine = existing_message_data
        .as_ref()
        .and_then(|(_, _, _, _, _, _, im, _)| *im);
    let prior_order_status = existing_message_data
        .as_ref()
        .and_then(|(_, _, _, _, _, _, _, st)| *st);

    let kind_from_payload = small_order_ref_from_payload(&inner_kind.payload).and_then(|o| o.kind);
    let kind_from_take_action = match &action {
        Action::TakeSell => Some(mostro_core::order::Kind::Sell),
        Action::TakeBuy => Some(mostro_core::order::Kind::Buy),
        _ => None,
    };

    let mut effective_order_kind = kind_from_payload
        .or(prior_order_kind)
        .or(kind_from_take_action);

    if effective_order_kind.is_none() {
        if let Some(ref row) = db_order {
            effective_order_kind = row
                .kind
                .as_ref()
                .and_then(|s| mostro_core::order::Kind::from_str(s).ok());
        }
    }

    // Re-read after upsert so `OrderMessage.is_mine` matches SQLite once `save_order` ran.
    // Without this, we kept the pre-upsert `db_order` snapshot and makers could stay `None`
    // while invoice/waiting popups gated on [`crate::ui::orders::local_user_must_act_on_invoice_popup`].
    let post_upsert_is_mine = Order::get_by_id(pool, &order_id.to_string())
        .await
        .ok()
        .map(|r| r.is_mine);
    let effective_is_mine = effective_is_mine_for_trade_dm_message(
        had_local_row_before_upsert,
        post_upsert_is_mine,
        prior_is_mine,
    );

    let baseline_status = status_from_db.or(prior_order_status);
    let should_accept_candidate = status_candidate
        .map(|candidate| {
            should_apply_status_transition(baseline_status, candidate, effective_order_kind)
        })
        .unwrap_or(false);
    let effective_order_status = if should_accept_candidate {
        status_candidate.or(baseline_status)
    } else {
        baseline_status
    };

    if let Some(candidate) = status_candidate {
        let oid = small_order_ref_from_payload(&inner_kind.payload)
            .and_then(|o| o.id)
            .or(inner_kind.id)
            .unwrap_or(order_id);
        if should_accept_candidate {
            if baseline_status != Some(candidate) {
                if let Err(e) = update_order_status(pool, &oid.to_string(), candidate).await {
                    log::warn!(
                        "Failed to update status for order {} from DM action {:?}: {}",
                        oid,
                        action,
                        e
                    );
                }
            }
        } else if let Some(existing_status) = status_from_db {
            // `upsert_order_from_trade_dm` may have persisted stale payload status; restore monotonic status.
            if let Err(e) = update_order_status(pool, &oid.to_string(), existing_status).await {
                log::warn!(
                    "Failed to restore monotonic status for order {} after stale {:?}: {}",
                    oid,
                    action,
                    e
                );
            }
        }
    }

    let effective_sat_amount = sat_amount.or(prior_sat_amount);
    let effective_invoice = invoice.clone().or(prior_invoice);

    if notify && is_new_message && is_actionable_notification {
        match pending_notifications.lock() {
            Ok(mut pending_notifications) => {
                *pending_notifications += 1;
            }
            Err(e) => {
                crate::util::request_fatal_restart(format!(
                    "Mostrix encountered an internal error (poisoned pending notifications lock: {e}). Please restart the app."
                ));
                return;
            }
        }
    }

    let order_message = crate::ui::OrderMessage {
        message: message.clone(),
        timestamp,
        sender,
        order_id: Some(order_id),
        trade_index,
        read: false,
        sat_amount: effective_sat_amount,
        buyer_invoice: effective_invoice,
        order_kind: effective_order_kind,
        is_mine: effective_is_mine,
        order_status: effective_order_status,
        // Preserve popup-shown state for same-action updates (e.g. duplicate AddInvoice
        // carrying peer reputation payload but no amount), preventing noisy re-popups.
        auto_popup_shown: prior_auto_popup_shown,
    };

    let mut messages_lock = match messages.lock() {
        Ok(g) => g,
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
            return;
        }
    };
    // Keep one row per order, but do not let older stale replay messages overwrite the
    // currently selected action row after startup/reconnect hydration.
    let should_replace_row = match &existing_message_data {
        None => true,
        Some((existing_timestamp, existing_action, _, _, _, _, _, existing_order_status)) => {
            if timestamp > *existing_timestamp {
                true
            } else if timestamp == *existing_timestamp {
                action != *existing_action
            } else {
                // Older-than-current replay: only replace if the payload status **strictly** advances
                // the status already shown on the row (not merely equal; `should_accept_candidate`
                // allows equality vs baseline for DB updates).
                status_candidate.is_some_and(|c| {
                    should_strictly_advance_status(*existing_order_status, c, effective_order_kind)
                })
            }
        }
    };
    if should_replace_row {
        messages_lock.retain(|m| m.order_id != Some(order_id));
        messages_lock.push(order_message.clone());
        messages_lock.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    }

    // Send notification only for actionable/new updates; this avoids follow-up AddInvoice
    // payload variants (without order amount) from retriggering invoice popups with 0 sats.
    if notify && is_new_message && is_actionable_notification {
        let notification = order_message_to_notification(&order_message);
        let _ = message_notification_tx.send(notification);
    }
}

/// How terminal order status is handled after each decoded GiftWrap in a batch.
enum GiftWrapTerminalPolicy<'a> {
    /// Known `listen_for_order_messages` subscription: unsubscribe relay sub and stop batch.
    TrackedSubscription(&'a SubscriptionId),
    /// Unknown subscription id (e.g. parallel `wait_for_dm`): only local index/pubkey cleanup;
    /// do not unsubscribe (id not ours). Process the full batch like the pre-refactor path.
    UntrackedFallback,
}

fn remove_order_from_messages(messages: &Arc<Mutex<Vec<OrderMessage>>>, order_id: Uuid) {
    match messages.lock() {
        Ok(mut guard) => {
            guard.retain(|m| m.order_id != Some(order_id));
        }
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
        }
    }
}

/// Shared path for parsed GiftWrap batches: `handle_trade_dm_for_order` plus terminal cleanup.
#[allow(clippy::too_many_arguments)]
async fn dispatch_giftwrap_batch(
    parsed_messages: Vec<(Message, i64, PublicKey)>,
    order_id: Uuid,
    trade_index: i64,
    trade_keys: &Keys,
    messages: &Arc<Mutex<Vec<OrderMessage>>>,
    pending_notifications: &Arc<Mutex<usize>>,
    message_notification_tx: &tokio::sync::mpsc::UnboundedSender<MessageNotification>,
    pool: &sqlx::SqlitePool,
    user: &User,
    active_order_trade_indices: &Arc<Mutex<HashMap<Uuid, i64>>>,
    subscribed_pubkeys: &mut HashSet<PublicKey>,
    client: &Client,
    subscription_to_order: &mut HashMap<SubscriptionId, (Uuid, i64)>,
    terminal_policy: GiftWrapTerminalPolicy<'_>,
    notify: bool,
    dropped_user_history_order_ids: &Arc<Mutex<HashSet<Uuid>>>,
) {
    if let Ok(guard) = dropped_user_history_order_ids.lock() {
        if guard.contains(&order_id) {
            log::info!(
                "[dm_listener] Skipping trade DMs for order_id={} (removed from local history by user)",
                order_id
            );
            return;
        }
    }
    let log_each_message = matches!(
        terminal_policy,
        GiftWrapTerminalPolicy::TrackedSubscription(_)
    );

    for (message, timestamp, sender) in parsed_messages {
        let has_terminal_status = trade_message_is_terminal(&message);
        log::info!(
            "order id: {} has_terminal_status: {:?}",
            order_id,
            has_terminal_status
        );
        if log_each_message {
            log::info!(
                "[dm_listener] Handling message action={:?} ts={} order_id={} trade_index={}",
                message.get_inner_message_kind().action,
                timestamp,
                order_id,
                trade_index
            );
        }
        handle_trade_dm_for_order(
            messages,
            pending_notifications,
            message_notification_tx,
            order_id,
            trade_index,
            message,
            timestamp,
            sender,
            pool,
            trade_keys,
            notify,
        )
        .await;

        if let Err(e) = Order::update_last_seen_dm_ts(pool, &order_id.to_string(), timestamp).await
        {
            log::warn!(
                "[dm_listener] Failed to persist last_seen_dm_ts for order_id={}: {}",
                order_id,
                e
            );
        }

        if has_terminal_status {
            match terminal_policy {
                GiftWrapTerminalPolicy::TrackedSubscription(subscription_id) => {
                    log::info!(
                        "[dm_listener] Terminal order status detected, cleaning up order_id={}, trade_index={}, subscription_id={}",
                        order_id,
                        trade_index,
                        subscription_id
                    );
                    {
                        match active_order_trade_indices.lock() {
                            Ok(mut indices) => {
                                indices.remove(&order_id);
                            }
                            Err(e) => {
                                crate::util::request_fatal_restart(format!(
                                    "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
                                ));
                                return;
                            }
                        }
                    }
                    if let Ok(keys) = user.derive_trade_keys(trade_index) {
                        subscribed_pubkeys.remove(&keys.public_key());
                    }
                    subscription_to_order.remove(subscription_id);
                    client.unsubscribe(subscription_id).await;
                    break;
                }
                GiftWrapTerminalPolicy::UntrackedFallback => {
                    {
                        match active_order_trade_indices.lock() {
                            Ok(mut indices) => {
                                indices.remove(&order_id);
                            }
                            Err(e) => {
                                crate::util::request_fatal_restart(format!(
                                    "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
                                ));
                                return;
                            }
                        }
                    }
                    if let Ok(keys) = user.derive_trade_keys(trade_index) {
                        subscribed_pubkeys.remove(&keys.public_key());
                    }
                }
            }
        }
    }
}

/// Look back window for startup GiftWrap replay (in-memory Messages tab has no local DB).
const STARTUP_TRADE_DM_LOOKBACK_SECS: u64 = 12 * 60 * 60;
/// Max events per trade key per startup fetch (relay-dependent; cap bandwidth).
const STARTUP_TRADE_DM_FETCH_LIMIT: usize = 100;
/// NIP-01 `since` matches the GiftWrap **envelope** `created_at`, but `last_seen_dm_ts` stores the
/// decrypted **rumor** `created_at` (`parse_dm_events`). If the rumor clock runs ahead of the
/// envelope (seen with Mostro), using the raw cursor as `since` drops that GiftWrap on replay and
/// only newer envelopes (e.g. `waiting-seller-to-pay`) are returned.
const STARTUP_GIFTWRAP_ENVELOPE_SKEW_SECS: u64 = 3 * 24 * 60 * 60;

/// Snapshot of `listen_for_order_messages` locals passed into startup GiftWrap replay.
struct DmListenerStartupReplay<'a> {
    client: &'a Client,
    pool: &'a sqlx::sqlite::SqlitePool,
    user: &'a User,
    messages: &'a Arc<Mutex<Vec<OrderMessage>>>,
    pending_notifications: &'a Arc<Mutex<usize>>,
    message_notification_tx: &'a UnboundedSender<MessageNotification>,
    active_order_trade_indices: &'a Arc<Mutex<HashMap<Uuid, i64>>>,
    subscribed_pubkeys: &'a mut HashSet<PublicKey>,
    subscription_to_order: &'a mut HashMap<SubscriptionId, (Uuid, i64)>,
    pubkey_to_subscription: &'a HashMap<PublicKey, SubscriptionId>,
    dropped_user_history_order_ids: &'a Arc<Mutex<HashSet<Uuid>>>,
}

/// One-shot relay query + replay so restart shows trade DMs. `subscribe` alone often does not
/// replay enough stored events into the notification stream for the UI to hydrate.
async fn fetch_and_replay_startup_trade_dms(
    replay: DmListenerStartupReplay<'_>,
    startup_active_orders: &HashMap<Uuid, i64>,
    order_last_seen_dm_ts: &HashMap<Uuid, i64>,
) {
    let DmListenerStartupReplay {
        client,
        pool,
        user,
        messages,
        pending_notifications,
        message_notification_tx,
        active_order_trade_indices,
        subscribed_pubkeys,
        subscription_to_order,
        pubkey_to_subscription,
        dropped_user_history_order_ids,
    } = replay;

    let lookback_start = Timestamp::now()
        .as_secs()
        .saturating_sub(STARTUP_TRADE_DM_LOOKBACK_SECS);

    for (order_id, trade_index) in startup_active_orders {
        let trade_keys = match user.derive_trade_keys(*trade_index) {
            Ok(k) => k,
            Err(e) => {
                log::error!(
                    "Startup DM replay: failed to derive trade keys for index {}: {}",
                    trade_index,
                    e
                );
                continue;
            }
        };
        let pubkey = trade_keys.public_key();
        let Some(sub_id) = pubkey_to_subscription.get(&pubkey).cloned() else {
            log::trace!(
                "Startup DM replay: no subscription id for order_id={} (subscribe may have failed)",
                order_id
            );
            continue;
        };

        // `last_seen_dm_ts` is rumor time; relay `since` is envelope time — see
        // `STARTUP_GIFTWRAP_ENVELOPE_SKEW_SECS`. Combine with lookback (cold Messages list) then
        // widen backward so the last processed DM's GiftWrap is not filtered out.
        let combined_since = order_last_seen_dm_ts
            .get(order_id)
            .and_then(|ts| u64::try_from(*ts).ok())
            .map(|last_seen| last_seen.min(lookback_start))
            .unwrap_or(lookback_start);
        let since_ts = combined_since.saturating_sub(STARTUP_GIFTWRAP_ENVELOPE_SKEW_SECS);

        let filter = filter_giftwrap_to_recipient(pubkey)
            .since(Timestamp::from(since_ts))
            .limit(STARTUP_TRADE_DM_FETCH_LIMIT);

        let events = match client.fetch_events(filter, FETCH_EVENTS_TIMEOUT).await {
            Ok(e) => e,
            Err(e) => {
                log::warn!(
                    "Startup DM replay: fetch_events failed for order_id={}: {}",
                    order_id,
                    e
                );
                continue;
            }
        };

        if events.is_empty() {
            continue;
        }

        // Fetch the full relay window (same filter as before), then hydrate from the **single**
        // parsed line with the greatest **rumor** `created_at`. Envelope order can disagree with
        // rumor time; replaying every line in envelope order could leave the UI on an older step.
        let event_list: Vec<Event> = events.into_iter().collect();
        let fetched_n = event_list.len();

        let mut best: Option<(i64, EventId, (Message, i64, PublicKey))> = None;
        for event in &event_list {
            let unwrapped = match unwrap_message(event, &trade_keys).await {
                Ok(Some(u)) => u,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!(
                        "Startup DM replay: unwrap_message failed (event {}): {}",
                        event.id,
                        e
                    );
                    continue;
                }
            };
            let parsed_messages = parse_dm_events_single(event, &trade_keys, Some(unwrapped)).await;
            if parsed_messages.is_empty() {
                continue;
            }
            for triple in parsed_messages {
                let (ref _msg, ts, ref _sender) = triple;
                let take = match &best {
                    None => true,
                    Some((best_ts, best_eid, _)) => {
                        ts > *best_ts
                            || (ts == *best_ts && event.id.as_bytes() > best_eid.as_bytes())
                    }
                };
                if take {
                    best = Some((ts, event.id, triple));
                }
            }
        }

        let Some((max_rumor_ts, _, freshest)) = best else {
            log::trace!(
                "Startup DM replay: order_id={} trade_index={} had {} event(s) but none decrypted/parsed",
                order_id,
                trade_index,
                fetched_n
            );
            continue;
        };

        log::info!(
            "Startup DM replay: order_id={} trade_index={} fetched {} GiftWrap event(s); hydrating newest rumor ts={}",
            order_id,
            trade_index,
            fetched_n,
            max_rumor_ts
        );

        if !subscription_to_order.contains_key(&sub_id) {
            continue;
        }

        dispatch_giftwrap_batch(
            vec![freshest],
            *order_id,
            *trade_index,
            &trade_keys,
            messages,
            pending_notifications,
            message_notification_tx,
            pool,
            user,
            active_order_trade_indices,
            subscribed_pubkeys,
            client,
            subscription_to_order,
            GiftWrapTerminalPolicy::TrackedSubscription(&sub_id),
            false,
            dropped_user_history_order_ids,
        )
        .await;
    }
}

struct PendingDmWaiter {
    trade_keys: Keys,
    response_tx: oneshot::Sender<Event>,
}

fn prune_closed_waiters(pending_waiters: &mut Vec<PendingDmWaiter>) {
    let before = pending_waiters.len();
    pending_waiters.retain(|w| !w.response_tx.is_closed());
    let pruned = before.saturating_sub(pending_waiters.len());
    if pruned > 0 {
        log::debug!(
            "[dm_listener] pruned {} closed waiter(s); pending_waiters={}",
            pruned,
            pending_waiters.len()
        );
    }
}

fn log_giftwrap_fallback_decrypt_stats(
    active_orders_scanned: usize,
    decrypt_attempts: u32,
    duration_ms: u64,
    matched: bool,
) {
    let cumulative = GIFTWRAP_FALLBACK_DECRYPT_TOTAL.load(Ordering::Relaxed);
    log::debug!(
        "[dm_listener] giftwrap_fallback_decrypt: cumulative_calls={} active_orders_scanned={} decrypt_attempts={} duration_ms={} matched={}",
        cumulative,
        active_orders_scanned,
        decrypt_attempts,
        duration_ms,
        matched
    );
    // Keep warn low-volume: large scans, slow decrypt loop, or successful match.
    if active_orders_scanned > 5 || duration_ms > 50 || matched {
        log::warn!(
            "[dm_listener] giftwrap_fallback_decrypt(significant): cumulative_calls={} active_orders_scanned={} decrypt_attempts={} duration_ms={} matched={}",
            cumulative,
            active_orders_scanned,
            decrypt_attempts,
            duration_ms,
            matched
        );
    }
}

async fn resolve_order_for_event(
    event: &Event,
    user: &User,
    active_order_trade_indices: &Arc<Mutex<HashMap<Uuid, i64>>>,
) -> Option<(Uuid, i64, Keys, UnwrappedMessage)> {
    GIFTWRAP_FALLBACK_DECRYPT_TOTAL.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();

    let active_orders = match active_order_trade_indices.lock() {
        Ok(indices) => indices.clone(),
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
            ));
            return None;
        }
    };

    let active_count = active_orders.len();
    GIFTWRAP_FALLBACK_LAST_ACTIVE_COUNT.store(active_count as u64, Ordering::Relaxed);

    let mut decrypt_attempts: u32 = 0;
    for (order_id, trade_index) in active_orders {
        decrypt_attempts = decrypt_attempts.saturating_add(1);
        let trade_keys = match user.derive_trade_keys(trade_index) {
            Ok(k) => k,
            Err(_) => continue,
        };
        match unwrap_message(event, &trade_keys).await {
            Ok(Some(unwrapped)) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                GIFTWRAP_FALLBACK_LAST_DURATION_MS.store(duration_ms, Ordering::Relaxed);
                log_giftwrap_fallback_decrypt_stats(
                    active_count,
                    decrypt_attempts,
                    duration_ms,
                    true,
                );
                return Some((order_id, trade_index, trade_keys, unwrapped));
            }
            Ok(None) | Err(_) => continue,
        }
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    GIFTWRAP_FALLBACK_LAST_DURATION_MS.store(duration_ms, Ordering::Relaxed);
    log_giftwrap_fallback_decrypt_stats(active_count, decrypt_attempts, duration_ms, false);
    None
}

/// Background DM router for GiftWrap events.
///
/// Responsibilities:
/// - maintain relay subscriptions for tracked orders (`TrackOrder`) and temporary
///   request/response waiters (`RegisterWaiter` / `wait_for_dm`)
/// - route each incoming GiftWrap through two complementary paths:
///   1) waiter path: satisfy in-flight `wait_for_dm` calls
///   2) tracked-order path: parse and dispatch updates to the order/UI pipeline
/// - reuse decryptability checks across both paths for the same incoming GiftWrap
///   and trade pubkey (`HashMap<PublicKey, bool>` scoped to one notification; the
///   unknown-subscription fallback reuses the `UnwrappedMessage` from `resolve_order_for_event`
///   so it does not unwrap twice there)
///
/// Lifecycle notes:
/// - bootstrap subscriptions for already-active orders at startup
/// - continue processing relay notifications even if `dm_subscription_rx` is closed
///   (no new dynamic subscriptions, existing ones remain active)
#[allow(clippy::too_many_arguments)]
pub async fn listen_for_order_messages(
    client: Client,
    pool: sqlx::sqlite::SqlitePool,
    active_order_trade_indices: Arc<Mutex<HashMap<Uuid, i64>>>,
    order_last_seen_dm_ts: HashMap<Uuid, i64>,
    messages: Arc<Mutex<Vec<OrderMessage>>>,
    message_notification_tx: tokio::sync::mpsc::UnboundedSender<MessageNotification>,
    pending_notifications: Arc<Mutex<usize>>,
    dropped_user_history_order_ids: Arc<Mutex<HashSet<Uuid>>>,
    mut dm_subscription_rx: tokio::sync::mpsc::UnboundedReceiver<OrderDmSubscriptionCmd>,
) {
    // Get user key from db (for deriving trade keys)
    let user = match User::get(&pool).await {
        Ok(u) => u,
        Err(e) => {
            log::error!("Failed to get user: {}", e);
            return;
        }
    };

    let mut notifications = client.notifications();
    let mut subscribed_pubkeys: HashSet<PublicKey> = HashSet::new();
    let mut subscription_to_order: HashMap<SubscriptionId, (Uuid, i64)> = HashMap::new();
    let mut pubkey_to_subscription: HashMap<PublicKey, SubscriptionId> = HashMap::new();
    let mut pending_waiters: Vec<PendingDmWaiter> = Vec::new();
    let mut waiter_gc_interval = tokio::time::interval(PENDING_WAITER_GC_INTERVAL);
    // First tick is immediate; skip it so the first cleanup runs after the interval.
    waiter_gc_interval.tick().await;

    // Bootstrap subscriptions for orders already known at startup.
    let startup_active_orders = {
        match active_order_trade_indices.lock() {
            Ok(indices) => indices.clone(),
            Err(e) => {
                crate::util::request_fatal_restart(format!(
                    "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
                ));
                return;
            }
        }
    };

    // Bootstrap subscriptions for orders already known at startup.
    for (&order_id, &trade_index) in startup_active_orders.iter() {
        let trade_keys = match user.derive_trade_keys(trade_index) {
            Ok(k) => k,
            Err(e) => {
                log::error!(
                    "Failed to derive trade keys for startup trade index {}: {}",
                    trade_index,
                    e
                );
                continue;
            }
        };
        let pubkey = trade_keys.public_key();
        let startup_mode = match order_last_seen_dm_ts.get(&order_id).copied() {
            Some(ts) => dm_helpers::GiftWrapSubscriptionMode::StartupSince(ts),
            None => dm_helpers::GiftWrapSubscriptionMode::StartupCatchUp,
        };
        let _ = dm_helpers::ensure_order_giftwrap_subscription(
            &client,
            &mut subscribed_pubkeys,
            &mut subscription_to_order,
            &mut pubkey_to_subscription,
            pubkey,
            dm_helpers::GiftWrapOrderSubscription {
                order_id,
                trade_index,
                error_label: "Failed startup subscribe for trade pubkey",
                info_label: None,
                mode: startup_mode,
            },
        )
        .await;
    }

    fetch_and_replay_startup_trade_dms(
        DmListenerStartupReplay {
            client: &client,
            pool: &pool,
            user: &user,
            messages: &messages,
            pending_notifications: &pending_notifications,
            message_notification_tx: &message_notification_tx,
            active_order_trade_indices: &active_order_trade_indices,
            subscribed_pubkeys: &mut subscribed_pubkeys,
            subscription_to_order: &mut subscription_to_order,
            pubkey_to_subscription: &pubkey_to_subscription,
            dropped_user_history_order_ids: &dropped_user_history_order_ids,
        },
        &startup_active_orders,
        &order_last_seen_dm_ts,
    )
    .await;

    loop {
        tokio::select! {
            _ = waiter_gc_interval.tick() => {
                prune_closed_waiters(&mut pending_waiters);
            }
            new_subscription_cmd = dm_subscription_rx.recv() => {
                let Some(cmd_subscription) = new_subscription_cmd else {
                    // Sender dropped; keep listener alive for existing subscriptions.
                    log::warn!("[dm_listener] dm_subscription_rx closed; no new dynamic subscriptions will be received");
                    continue;
                };

                match cmd_subscription {
                    DmRouterCmd::TrackOrder { order_id, trade_index } => {
                        log::info!(
                            "[dm_listener] Received subscribe command order_id={}, trade_index={}",
                            order_id,
                            trade_index
                        );
                        // Must run before any GiftWrap for this trade can hit the unknown-
                        // subscription_id fallback (e.g. wait_for_dm's temporary subscribe). Main
                        // thread only inserts this map when take_order completes — too late.
                        {
                            match active_order_trade_indices.lock() {
                                Ok(mut indices) => {
                                    // TrackOrder should be idempotent per `trade_index`: when the
                                    // optimistic order_id differs from the effective order_id
                                    // (Mostro-filled), drop any prior entries for this trade_index
                                    // so we don't keep phantom order_ids forever.
                                    let stale: Vec<Uuid> = indices
                                        .iter()
                                        .filter_map(|(oid, idx)| {
                                            if *idx == trade_index && *oid != order_id {
                                                Some(*oid)
                                            } else {
                                                None
                                            }
                                        })
                                        .collect();
                                    for oid in stale {
                                        indices.remove(&oid);
                                    }
                                    indices.insert(order_id, trade_index);
                                }
                                Err(e) => {
                                    crate::util::request_fatal_restart(format!(
                                        "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
                                    ));
                                    return;
                                }
                            }
                        }
                        let trade_keys = match user.derive_trade_keys(trade_index) {
                            Ok(k) => k,
                            Err(e) => {
                                log::error!(
                                    "Failed to derive trade keys for index {}: {}",
                                    trade_index,
                                    e
                                );
                                continue;
                            }
                        };

                        let pubkey = trade_keys.public_key();
                        if !dm_helpers::ensure_order_giftwrap_subscription(
                            &client,
                            &mut subscribed_pubkeys,
                            &mut subscription_to_order,
                            &mut pubkey_to_subscription,
                            pubkey,
                            dm_helpers::GiftWrapOrderSubscription {
                                order_id,
                                trade_index,
                                error_label: "Failed to subscribe for trade pubkey",
                                info_label: Some("[dm_listener] Subscribed GiftWrap:"),
                                mode: dm_helpers::GiftWrapSubscriptionMode::LiveOnly,
                            },
                        )
                        .await
                        {
                            continue;
                        }
                    }
                    DmRouterCmd::RegisterWaiter {
                        trade_keys,
                        response_tx,
                    } => {
                        prune_closed_waiters(&mut pending_waiters);
                        if pending_waiters.len() >= MAX_PENDING_WAITERS {
                            log::warn!(
                                "[dm_listener] rejecting waiter registration: pending_waiters={} (cap={})",
                                pending_waiters.len(),
                                MAX_PENDING_WAITERS
                            );
                            // Dropping `response_tx` cancels waiter immediately in `wait_for_dm`.
                            continue;
                        }
                        let before = pending_waiters.len();
                        let waiter_pubkey = trade_keys.public_key();
                        if subscribed_pubkeys.insert(waiter_pubkey) {
                            let filter = filter_giftwrap_to_recipient(waiter_pubkey).limit(0);
                            match client.subscribe(filter, None).await {
                                Ok(output) => {
                                    // Remember the subscription id so a later TrackOrder can
                                    // rebind this pubkey to a concrete order_id without requiring
                                    // a second relay subscription.
                                    pubkey_to_subscription.insert(waiter_pubkey, output.val);
                                }
                                Err(e) => {
                                    subscribed_pubkeys.remove(&waiter_pubkey);
                                    log::warn!(
                                        "Failed to subscribe waiter pubkey {}: {}",
                                        waiter_pubkey,
                                        e
                                    );
                                    // Immediate waiter cancellation path: do not queue this waiter
                                    // when we could not subscribe. Dropping response_tx here makes
                                    // wait_for_dm receive oneshot cancellation right away.
                                    continue;
                                }
                            }
                        }
                        pending_waiters.push(PendingDmWaiter {
                            trade_keys,
                            response_tx,
                        });
                        log::trace!(
                            "[dm_listener] waiter queued pending_before={} pending_after={}",
                            before,
                            pending_waiters.len()
                        );
                    }
                }
            }
            notification = notifications.recv() => {
                let notification = match notification {
                    Ok(n) => n,
                    Err(e) => {
                        log::warn!("Error receiving relay notification: {:?}", e);
                        continue;
                    }
                };

                if let RelayPoolNotification::Event {
                    subscription_id,
                    event,
                    ..
                } = notification
                {
                    let event = *event;
                    if event.kind != nostr_sdk::Kind::GiftWrap {
                        continue;
                    }
                    // One GiftWrap event can be consumed by:
                    // 1) request/response waiters (`wait_for_dm`) and
                    // 2) tracked order subscriptions (UI/order state pipeline).
                    // Shared cache for this single incoming GiftWrap: decryptability is keyed only
                    // by trade pubkey; `event.id` is fixed for the whole handler block.
                    // This avoids duplicate `unwrap_message` calls between waiter and tracked paths.
                    let mut rumor_cache: HashMap<PublicKey, bool> = HashMap::new();

                    if !pending_waiters.is_empty() {
                        let mut still_pending: Vec<PendingDmWaiter> =
                            Vec::with_capacity(pending_waiters.len());
                        // Try to satisfy in-flight `wait_for_dm` calls first.
                        // Non-matching waiters are re-queued and will be checked again on the
                        // next GiftWrap event.
                        for waiter in pending_waiters.drain(..) {
                            // Drop promptly when wait_for_dm timed out (receiver gone); no decrypt.
                            if waiter.response_tx.is_closed() {
                                continue;
                            }
                            let key = waiter.trade_keys.public_key();
                            let can_decrypt = if let Some(boolean) = rumor_cache.get(&key) {
                                *boolean
                            } else {
                                let ok = matches!(
                                    unwrap_message(&event, &waiter.trade_keys).await,
                                    Ok(Some(_))
                                );
                                rumor_cache.insert(key, ok);
                                ok
                            };

                            if can_decrypt {
                                let _ = waiter.response_tx.send(event.clone());
                            } else {
                                // If the rumor cannot be extracted, the waiter is still pending
                                // push it back to the pending waiters vector
                                still_pending.push(waiter);
                            }
                        }
                        pending_waiters = still_pending;
                    }

                    if let Some((order_id, trade_index)) = subscription_to_order.get(&subscription_id).copied() {
                        log::info!(
                            "[dm_listener] Routed GiftWrap by subscription_id={} to order_id={}, trade_index={}",
                            subscription_id,
                            order_id,
                            trade_index
                        );

                        // Tracked subscription path: decode and dispatch into the main
                        // order/message handling flow.
                        let trade_keys = match user.derive_trade_keys(trade_index) {
                            Ok(k) => k,
                            Err(e) => {
                                log::error!(
                                    "Failed to derive trade keys for index {} while handling DM: {}",
                                    trade_index,
                                    e
                                );
                                continue;
                            }
                        };
                        // Reuse per-event decryptability result if waiter path already checked
                        // this same trade pubkey.
                        let key = trade_keys.public_key();
                        let can_decrypt = if let Some(boolean) = rumor_cache.get(&key) {
                            *boolean
                        } else {
                            let ok = matches!(
                                unwrap_message(&event, &trade_keys).await,
                                Ok(Some(_))
                            );
                            rumor_cache.insert(key, ok);
                            ok
                        };

                        if !can_decrypt {
                            continue;
                        }

                        let parsed_messages =
                            parse_dm_events_single(&event, &trade_keys, None).await;
                        if parsed_messages.is_empty() {
                            continue;
                        }
                        log::info!(
                            "[dm_listener] Parsed {} message(s) for order_id={}, trade_index={}, subscription_id={}",
                            parsed_messages.len(),
                            order_id,
                            trade_index,
                            subscription_id
                        );
                        dispatch_giftwrap_batch(
                            parsed_messages,
                            order_id,
                            trade_index,
                            &trade_keys,
                            &messages,
                            &pending_notifications,
                            &message_notification_tx,
                            &pool,
                            &user,
                            &active_order_trade_indices,
                            &mut subscribed_pubkeys,
                            &client,
                            &mut subscription_to_order,
                            GiftWrapTerminalPolicy::TrackedSubscription(&subscription_id),
                            true,
                            &dropped_user_history_order_ids,
                        )
                        .await;
                    } else if let Some((order_id, trade_index, trade_keys, unwrapped)) =
                        resolve_order_for_event(&event, &user, &active_order_trade_indices).await
                    {
                        let parsed_messages = parse_dm_events_single(
                            &event,
                            &trade_keys,
                            Some(unwrapped),
                        )
                        .await;
                        if parsed_messages.is_empty() {
                            continue;
                        }
                        log::info!(
                            "[dm_listener] Routed GiftWrap by active-order key for unknown subscription_id={} to order_id={}, trade_index={}",
                            subscription_id,
                            order_id,
                            trade_index
                        );
                        dispatch_giftwrap_batch(
                            parsed_messages,
                            order_id,
                            trade_index,
                            &trade_keys,
                            &messages,
                            &pending_notifications,
                            &message_notification_tx,
                            &pool,
                            &user,
                            &active_order_trade_indices,
                            &mut subscribed_pubkeys,
                            &client,
                            &mut subscription_to_order,
                            GiftWrapTerminalPolicy::UntrackedFallback,
                            true,
                            &dropped_user_history_order_ids,
                        )
                        .await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{effective_is_mine_for_trade_dm_message, trade_message_is_terminal};
    use mostro_core::prelude::{Action, Message};

    #[test]
    fn action_only_canceled_is_terminal() {
        let message = Message::new_order(None, None, None, Action::Canceled, None);
        assert!(trade_message_is_terminal(&message));
    }

    #[test]
    fn effective_is_mine_uses_post_upsert_db_when_row_existed() {
        assert_eq!(
            effective_is_mine_for_trade_dm_message(true, Some(true), None),
            Some(true)
        );
        assert_eq!(
            effective_is_mine_for_trade_dm_message(true, Some(false), None),
            Some(false)
        );
    }

    #[test]
    fn effective_is_mine_ignores_dm_upsert_default_without_prior_save_order_row() {
        // DM-only insert defaults to maker in SQLite; do not treat as authoritative yet.
        assert_eq!(
            effective_is_mine_for_trade_dm_message(false, Some(true), None),
            None
        );
    }

    #[test]
    fn effective_is_mine_keeps_prior_message_role_before_db_row() {
        assert_eq!(
            effective_is_mine_for_trade_dm_message(false, Some(true), Some(false)),
            Some(false)
        );
    }
}
