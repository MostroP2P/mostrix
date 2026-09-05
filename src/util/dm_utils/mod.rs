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
use crate::ui::helpers::user_dispute_chat_since_from_file;
use crate::ui::order_message_to_notification;
use crate::ui::orders::{
    add_invoice_is_after_failed_payment, merge_order_snapshots, small_order_from_payload,
};
use crate::ui::{MessageNotification, OrderMessage};
use crate::util::chat_listener::{
    maybe_track_order_chat, track_user_dispute_chat, untrack_order_chat, untrack_user_dispute_chat,
};
use crate::util::chat_utils::derive_shared_key_hex;
use crate::util::db_utils::{delete_order_by_id, save_order, update_order_status};
use crate::util::filters::filter_protocol_dm_from_mostro;
use crate::util::mostro_info::{
    nostr_pow_for_protocol_dm, transport_from_instance, MostroInstanceInfo,
};
use crate::util::order_utils::{
    inferred_status_from_trade_action, map_action_to_status, should_apply_status_transition,
    should_strictly_advance_status,
};
use futures::StreamExt;
use std::collections::BTreeSet;

pub const FETCH_EVENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const PENDING_WAITER_GC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_PENDING_WAITERS: usize = 32;
/// Backoff between waiter re-registrations after a reconnect/listener abort (MOSTRO-080).
const WAIT_FOR_DM_REREGISTER_BACKOFF: std::time::Duration = std::time::Duration::from_millis(50);
/// Clock-skew padding when catching up waiters after a reconnect gap (MOSTRO-080).
const WAIT_FOR_DM_CATCHUP_SKEW_SECS: u64 = 60;
/// Bound relay backlog fetched when resurrecting a waiter across a reconnect gap.
const WAIT_FOR_DM_CATCHUP_LIMIT: usize = 32;
/// Cap catch-up `fetch_events` so RegisterWaiter does not block the listener for the full
/// startup fetch timeout.
const WAIT_FOR_DM_CATCHUP_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Default NIP-40 expiration window for outbound v2 protocol DMs (mirrors daemon `dm_days`).
const DEFAULT_DM_EXPIRATION_DAYS: u64 = 30;

fn default_dm_expiration() -> Timestamp {
    Timestamp::from(
        Timestamp::now()
            .as_secs()
            .saturating_add(DEFAULT_DM_EXPIRATION_DAYS * 24 * 60 * 60),
    )
}

/// Own outbound v2 protocol DMs are signed kind-14 events authored by the trade key.
///
/// When admin == Mostro, `wait_for_dm` registers a subscription that also matches that
/// outbound event. Mostro daemon replies are unsigned (`signed: false`), so treating
/// signed self-authored kind-14 as non-replies avoids consuming the request as the response.
fn is_own_signed_v2_outbound(
    event: &Event,
    trade_keys: &Keys,
    unwrapped: &UnwrappedMessage,
) -> bool {
    event.kind == nostr_sdk::prelude::Kind::PrivateDirectMessage
        && event.pubkey == trade_keys.public_key()
        && unwrapped.signature.is_some()
}

/// Whether an unwrapped protocol DM may complete a [`wait_for_dm`] waiter.
///
/// GiftWrap filters match by recipient only; the inner rumor sender must still be Mostro
/// before we treat the event as a daemon reply. Also excludes echoed own v2 outbound.
fn is_mostro_waiter_reply(
    event: &Event,
    trade_keys: &Keys,
    unwrapped: &UnwrappedMessage,
    mostro_pubkey: PublicKey,
) -> bool {
    unwrapped.sender == mostro_pubkey && !is_own_signed_v2_outbound(event, trade_keys, unwrapped)
}

#[derive(Clone, Copy)]
struct CachedDmUnwrap {
    can_decrypt: bool,
    skip_for_waiter: bool,
}

/// Immediate outcome of [`DmRouterCmd::RegisterWaiter`] before an event is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaiterAdmitResult {
    /// Waiter was queued (and/or already satisfied via catch-up fetch).
    Admitted,
    /// Listener refused the waiter because the pending-waiter capacity was reached.
    CapacityFull,
}

#[derive(Debug)]
pub enum DmRouterCmd {
    TrackOrder {
        order_id: Uuid,
        trade_index: i64,
    },
    RegisterWaiter {
        trade_keys: Keys,
        response_tx: oneshot::Sender<Event>,
        /// Ack before the event oneshot: capacity vs admitted (MOSTRO-080 / CodeRabbit).
        admit_tx: oneshot::Sender<WaiterAdmitResult>,
        /// When `Some`, subscribe/fetch from this timestamp to cover replies published while
        /// the listener was down (reconnect gap — MOSTRO-080 / ermeme).
        catch_up_since: Option<Timestamp>,
    },
}

pub type OrderDmSubscriptionCmd = DmRouterCmd;

static DM_ROUTER_CMD_TX: Mutex<Option<mpsc::UnboundedSender<DmRouterCmd>>> = Mutex::new(None);

/// Relay subscription ids owned by the trade DM listener (not order/dispute schedulers).
static DM_LISTENER_SUBSCRIPTION_IDS: Mutex<Vec<SubscriptionId>> = Mutex::new(Vec::new());

pub(crate) fn register_dm_listener_subscription(id: SubscriptionId) {
    if let Ok(mut guard) = DM_LISTENER_SUBSCRIPTION_IDS.lock() {
        if !guard.iter().any(|existing| existing == &id) {
            guard.push(id);
        }
    }
}

pub(crate) fn unregister_dm_listener_subscription(id: &SubscriptionId) {
    if let Ok(mut guard) = DM_LISTENER_SUBSCRIPTION_IDS.lock() {
        guard.retain(|existing| existing != id);
    }
}

/// Unsubscribe only DM-listener relay subscriptions; leaves order/dispute scheduler subs intact.
pub async fn unsubscribe_dm_listener_subscriptions(client: &Client) {
    let ids: Vec<SubscriptionId> = DM_LISTENER_SUBSCRIPTION_IDS
        .lock()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or_default();
    for id in ids {
        if let Err(e) = client.unsubscribe(&id).await {
            log::debug!("unsubscribe failed: {e}");
        }
    }
}

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

/// P2P chat untrack set: aligned with [`crate::models::TERMINAL_DM_STATUSES`] (omits `success`).
fn order_status_untracks_chat(status: Status) -> bool {
    matches!(
        status,
        Status::Canceled
            | Status::CanceledByAdmin
            | Status::SettledByAdmin
            | Status::CompletedByAdmin
            | Status::Expired
            | Status::CooperativelyCanceled
    )
}

/// Whether a trade DM should drop the shared-key order chat subscription.
///
/// [`trade_message_is_terminal`] still includes [`Status::Success`] so the trade DM listener can
/// tear down its per-trade-key subscription; chat stays tracked through the post-success window.
fn trade_message_should_untrack_order_chat(message: &Message) -> bool {
    let kind = message.get_inner_message_kind();
    if matches!(
        &kind.action,
        Action::AdminCanceled | Action::Canceled | Action::CooperativeCancelAccepted
    ) {
        return true;
    }
    kind.payload
        .as_ref()
        .and_then(|payload| match payload {
            Payload::Order(order) => order.status,
            _ => None,
        })
        .is_some_and(order_status_untracks_chat)
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
pub async fn send_dm(
    client: &Client,
    identity_keys: Option<&Keys>,
    trade_keys: &Keys,
    receiver_pubkey: &PublicKey,
    payload: String,
    expiration: Option<Timestamp>,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<()> {
    let message = Message::from_json(&payload)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize message: {e}"))?;
    let action = message.get_inner_message_kind().action.clone();
    let pow = nostr_pow_for_protocol_dm(mostro_instance, &action);
    let identity_keys = identity_keys.unwrap_or(trade_keys);
    let transport = transport_from_instance(mostro_instance);
    let expiration = match (transport, expiration) {
        (Transport::Nip44Direct, None) => Some(default_dm_expiration()),
        (_, exp) => exp,
    };
    let wrap_opts = WrapOptions {
        pow,
        expiration,
        signed: true,
    };
    // nostr 0.45 no longer strips self `#p` tags from EventBuilder, so the
    // core `wrap_message_nip44` path is correct for admin self-addressed DMs.
    let event = wrap_message_with(
        transport,
        &message,
        identity_keys,
        trade_keys,
        *receiver_pubkey,
        wrap_opts,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to wrap protocol message: {e}"))?;

    client.send_event(&event).await?;
    Ok(())
}

/// Wait for a direct message response from Mostro.
///
/// Registers a router waiter, then sends the outbound message **once**. If the
/// trade-DM listener is aborted or respawned mid-wait (connectivity reconnect,
/// supervised restart — MOSTRO-080), the oneshot is canceled; this re-registers
/// on the current router **without resending** so a daemon reply that arrives
/// after subscriptions are rebuilt can still complete the command.
///
/// Re-registration requests a bounded catch-up from the original wait start so
/// replies published while the listener was down are not missed (live-only
/// `.limit(0)` alone is insufficient across a reconnect gap).
///
/// `timeout` is an end-to-end deadline: registration, outbound send, and the
/// response wait all share the same remaining budget.
///
/// Exhausting the timeout budget still surfaces as
/// `"Timeout waiting for DM or gift wrap event"` (not a spuriously immediate
/// cancel), so callers can distinguish transport loss from a daemon `CantDo`.
/// Capacity rejection fails immediately (does not spam 50 ms retries).
///
/// # Errors
///
/// - Timeout with no matching reply within `timeout`
/// - Outbound `sent_message` failure (returned before any resurrection loop)
/// - Pending-waiter capacity full (`"DM waiter rejected: too many pending waiters"`)
/// - Fatal restart requested (`"DM waiter canceled before receiving an event"`)
/// - DM router still unavailable when the remaining budget is too small to retry
pub async fn wait_for_dm<F>(
    trade_keys: &Keys,
    timeout: std::time::Duration,
    sent_message: F,
) -> Result<BTreeSet<Event>>
where
    F: std::future::Future<Output = Result<()>> + Send,
{
    let deadline = tokio::time::Instant::now() + timeout;
    // Anchor catch-up to the original wait (pre-send) so gap replies are in-window.
    let catch_up_anchor = Timestamp::now();
    let mut sent = false;
    // After listener abort/channel loss, next RegisterWaiter must catch up.
    let mut needs_catch_up = false;
    // `sent_message` is a one-shot future; pin it so we can poll once across retries.
    let mut sent_message = std::pin::pin!(sent_message);

    loop {
        if crate::util::fatal_requested() {
            return Err(anyhow::anyhow!(
                "DM waiter canceled before receiving an event"
            ));
        }

        let remaining = match remaining_until_deadline(deadline) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };

        let dm_router_tx = match dm_router_cmd_sender() {
            Ok(tx) => tx,
            Err(e) => {
                // Global sender missing/poisoned during reconnect window — keep
                // trying until the deadline rather than failing the protocol cmd.
                if remaining <= WAIT_FOR_DM_REREGISTER_BACKOFF {
                    return Err(e);
                }
                log::warn!(
                    "[wait_for_dm] DM router not ready ({e}); retrying until deadline (MOSTRO-080)"
                );
                needs_catch_up = true;
                tokio::time::sleep(WAIT_FOR_DM_REREGISTER_BACKOFF).await;
                continue;
            }
        };

        let (response_tx, response_rx) = oneshot::channel::<Event>();
        let (admit_tx, admit_rx) = oneshot::channel::<WaiterAdmitResult>();
        let catch_up_since = if needs_catch_up {
            Some(Timestamp::from(
                catch_up_anchor
                    .as_secs()
                    .saturating_sub(WAIT_FOR_DM_CATCHUP_SKEW_SECS),
            ))
        } else {
            None
        };

        if dm_router_tx
            .send(DmRouterCmd::RegisterWaiter {
                trade_keys: trade_keys.clone(),
                response_tx,
                admit_tx,
                catch_up_since,
            })
            .is_err()
        {
            log::warn!(
                "[wait_for_dm] RegisterWaiter channel closed; re-registering without resend (MOSTRO-080)"
            );
            needs_catch_up = true;
            tokio::time::sleep(WAIT_FOR_DM_REREGISTER_BACKOFF.min(remaining)).await;
            continue;
        }

        let remaining = match remaining_until_deadline(deadline) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };
        match tokio::time::timeout(remaining, admit_rx).await {
            Ok(Ok(WaiterAdmitResult::Admitted)) => {}
            Ok(Ok(WaiterAdmitResult::CapacityFull)) => {
                return Err(anyhow::anyhow!(
                    "DM waiter rejected: too many pending waiters"
                ));
            }
            Ok(Err(_admit_dropped)) => {
                // Listener died before admitting — treat as reconnect abort.
                let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                if !should_reregister_dm_waiter_after_cancel(left) {
                    return Err(anyhow::anyhow!("Timeout waiting for DM or gift wrap event"));
                }
                log::warn!(
                    "[wait_for_dm] admit canceled; re-registering without resend (MOSTRO-080)"
                );
                needs_catch_up = true;
                tokio::time::sleep(WAIT_FOR_DM_REREGISTER_BACKOFF.min(left)).await;
                continue;
            }
            Err(_elapsed) => {
                return Err(anyhow::anyhow!("Timeout waiting for DM or gift wrap event"));
            }
        }

        if !sent {
            let remaining = match remaining_until_deadline(deadline) {
                Ok(r) => r,
                Err(e) => return Err(e),
            };
            match tokio::time::timeout(remaining, sent_message.as_mut()).await {
                Ok(Ok(())) => sent = true,
                Ok(Err(e)) => return Err(e),
                Err(_elapsed) => {
                    return Err(anyhow::anyhow!("Timeout waiting for DM or gift wrap event"));
                }
            }
        }

        // Recompute after send so the response wait cannot overrun the end-to-end deadline.
        let remaining = match remaining_until_deadline(deadline) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };
        match tokio::time::timeout(remaining, response_rx).await {
            Ok(Ok(event)) => {
                let mut events = BTreeSet::new();
                events.insert(event);
                return Ok(events);
            }
            Ok(Err(_canceled)) => {
                // Listener aborted or rotated the channel (capacity already handled via admit).
                let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                if !should_reregister_dm_waiter_after_cancel(left) {
                    return Err(anyhow::anyhow!("Timeout waiting for DM or gift wrap event"));
                }
                log::warn!(
                    "[wait_for_dm] waiter canceled mid-flight; re-registering without resend (MOSTRO-080)"
                );
                needs_catch_up = true;
                tokio::time::sleep(WAIT_FOR_DM_REREGISTER_BACKOFF.min(left)).await;
            }
            Err(_elapsed) => {
                return Err(anyhow::anyhow!("Timeout waiting for DM or gift wrap event"));
            }
        }
    }
}

fn remaining_until_deadline(deadline: tokio::time::Instant) -> Result<std::time::Duration> {
    let now = tokio::time::Instant::now();
    if now >= deadline {
        Err(anyhow::anyhow!("Timeout waiting for DM or gift wrap event"))
    } else {
        Ok(deadline - now)
    }
}

/// Relay filter for waiter catch-up after reconnect (bounded `since` + `limit`).
fn waiter_catch_up_filter(
    transport: Transport,
    mostro_pubkey: PublicKey,
    waiter_pubkey: PublicKey,
    since: Timestamp,
) -> Filter {
    filter_protocol_dm_from_mostro(transport, mostro_pubkey, waiter_pubkey)
        .since(since)
        .limit(WAIT_FOR_DM_CATCHUP_LIMIT)
}

/// Fetch recent protocol DMs and deliver the first matching reply to the waiter.
///
/// Returns `None` when the oneshot was satisfied; `Some(response_tx)` when the waiter
/// should still be queued for live notifications.
async fn try_deliver_waiter_catch_up(
    client: &Client,
    transport: Transport,
    mostro_pubkey: PublicKey,
    trade_keys: &Keys,
    catch_up_since: Timestamp,
    response_tx: oneshot::Sender<Event>,
) -> Option<oneshot::Sender<Event>> {
    if response_tx.is_closed() {
        return None;
    }
    let filter = waiter_catch_up_filter(
        transport,
        mostro_pubkey,
        trade_keys.public_key(),
        catch_up_since,
    );
    let events = match client
        .fetch_events(filter)
        .timeout(WAIT_FOR_DM_CATCHUP_FETCH_TIMEOUT)
        .await
    {
        Ok(events) => events,
        Err(e) => {
            log::warn!(
                "[dm_listener] waiter catch-up fetch failed for {}: {} (MOSTRO-080)",
                trade_keys.public_key(),
                e
            );
            return Some(response_tx);
        }
    };
    if events.is_empty() {
        return Some(response_tx);
    }

    let expected_kind = transport.event_kind();
    let mut events: Vec<Event> = events.into_iter().collect();
    // Prefer newest envelopes so a gap reply wins over older traffic.
    events.sort_by_key(|e| std::cmp::Reverse(e.created_at.as_secs()));
    for event in events {
        if event.kind != expected_kind {
            continue;
        }
        match unwrap_incoming(&event, trade_keys).await {
            Ok(Some(u)) if is_mostro_waiter_reply(&event, trade_keys, &u, mostro_pubkey) => {
                log::info!(
                    "[dm_listener] waiter catch-up delivered event {} for {} (MOSTRO-080)",
                    event.id,
                    trade_keys.public_key()
                );
                let _ = response_tx.send(event);
                return None;
            }
            _ => continue,
        }
    }
    Some(response_tx)
}

/// Current global DM router sender used by [`wait_for_dm`] / [`send_track_order_cmd`].
fn dm_router_cmd_sender() -> Result<UnboundedSender<OrderDmSubscriptionCmd>> {
    match DM_ROUTER_CMD_TX.lock() {
        Ok(guard) => guard.clone().ok_or_else(|| {
            anyhow::anyhow!("DM router is not ready. Please retry after listener initialization.")
        }),
        Err(_) => {
            crate::util::request_fatal_restart(
                "Mostrix encountered an internal error (poisoned DM router lock). Please restart the app."
                    .to_string(),
            );
            Err(anyhow::anyhow!(
                "DM router mutex poisoned; restart the application."
            ))
        }
    }
}

/// Whether a canceled waiter oneshot should be retried within the remaining budget.
///
/// Used by [`wait_for_dm`] after listener abort/reject, and by unit tests locking
/// MOSTRO-080 reconnect resurrection behavior.
fn should_reregister_dm_waiter_after_cancel(remaining: std::time::Duration) -> bool {
    !remaining.is_zero() && !crate::util::fatal_requested()
}

/// Parse DM events to extract Messages (v1 GiftWrap and v2 kind 14 via [`unwrap_incoming`]).
pub async fn parse_dm_events(
    events: BTreeSet<Event>,
    pubkey: &Keys,
    since: Option<&i64>,
) -> Vec<(Message, i64, PublicKey)> {
    let mut id_set = HashSet::<EventId>::new();
    let mut direct_messages: Vec<(Message, i64, PublicKey)> = Vec::new();

    for dm in events.iter() {
        // Skip if already processed
        if !id_set.insert(dm.id) {
            continue;
        }

        let (created_at, message, sender) = match unwrap_incoming(dm, pubkey).await {
            Ok(None) => continue,
            Err(e) => {
                log::warn!("Could not unwrap protocol DM (event {}): {}", dm.id, e);
                continue;
            }
            Ok(Some(u)) => (u.created_at, u.message, u.sender),
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
    direct_messages.sort_by_key(|a| a.1);
    direct_messages
}

/// Parse one protocol DM [`Event`] with the trade key (`since: None`). Shared by relay notifications and startup replay.
///
/// When `pre_unwrapped` is set (e.g. after a successful [`unwrap_incoming`]), skips a second unwrap.
async fn parse_dm_events_single(
    event: &Event,
    trade_keys: &Keys,
    pre_unwrapped: Option<UnwrappedMessage>,
) -> Vec<(Message, i64, PublicKey)> {
    if let Some(u) = pre_unwrapped {
        return vec![(u.message, u.created_at.as_secs() as i64, u.sender)];
    }
    let mut batch = BTreeSet::new();
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

/// Build a TRADE-card snapshot from a persisted SQLite order row.
fn small_order_from_db_order(row: &Order) -> SmallOrder {
    SmallOrder {
        id: row.id.as_ref().and_then(|s| uuid::Uuid::parse_str(s).ok()),
        kind: row
            .kind
            .as_ref()
            .and_then(|s| mostro_core::order::Kind::from_str(s).ok()),
        status: row.status.as_ref().and_then(|s| Status::from_str(s).ok()),
        amount: row.amount,
        fiat_code: row.fiat_code.clone(),
        min_amount: row.min_amount,
        max_amount: row.max_amount,
        fiat_amount: row.fiat_amount,
        payment_method: row.payment_method.clone(),
        premium: row.premium,
        buyer_invoice: row.buyer_invoice.clone(),
        created_at: row.created_at,
        expires_at: row.expires_at,
        ..Default::default()
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
            | Status::WaitingMakerBond
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
    // Row deleted: stop the P2P order chat subscription for this order.
    untrack_order_chat(order_id.to_string());
    untrack_user_dispute_chat(order_id.to_string());
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
        (Action::AddInvoice, Some(Payload::Order(o))) => {
            // MOSTRO-078: never hydrate SQLite from unvalidated daemon sats.
            // TrackOrder-before-save_order has no local row yet — defer until
            // take_order persists a trusted amount. If a row exists but amount
            // is still 0, wait as well.
            let Ok(existing) = Order::get_by_id(pool, &order_id.to_string()).await else {
                log::info!(
                    "Deferring AddInvoice DB hydration for order {} until a local trusted amount exists",
                    order_id
                );
                return;
            };
            if existing.amount <= 0 {
                log::info!(
                    "Deferring AddInvoice DB hydration for order {}: local amount is not trusted yet",
                    order_id
                );
                return;
            }
            let mut order = o.clone();
            order.amount = existing.amount;
            ("AddInvoice", order)
        }
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
        (Action::AddBondInvoice, Some(Payload::BondPayoutRequest(req))) => {
            ("AddBondInvoice", req.order.clone())
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

/// Send [`DmRouterCmd::TrackOrder`] via the **current** global DM router sender.
///
/// Always locks [`DM_ROUTER_CMD_TX`] at send time so a supervised listener channel
/// rotation cannot leave callers holding a closed `UnboundedSender` clone (main-loop
/// locals are refreshed asynchronously via [`crate::util::FatalNotify::DmRouterSender`]).
pub fn send_track_order_cmd(order_id: Uuid, trade_index: i64) {
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

fn try_send_track_order(order_id: Uuid, trade_index: i64) {
    send_track_order_cmd(order_id, trade_index);
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
    // Back on the book as a pending maker listing (no counterparty): stop order chat subscription.
    untrack_order_chat(order_id.to_string());
    untrack_user_dispute_chat(order_id.to_string());
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

/// Returns `true` when a replayed trade-DM `NewOrder` would overwrite a non-`NewOrder` Messages row.
fn new_order_would_regress_messages_row(action: &Action, existing_action: &Action) -> bool {
    matches!(action, Action::NewOrder) && !matches!(existing_action, Action::NewOrder)
}

/// Handle `Action::NewOrder` on the trade-DM listener (not the create-order waiter).
///
/// Returns `true` only for handled special cases (caller may return early):
/// - pre-Active taker republish (drop stale take row),
/// - pre-Active maker republish (revert to `pending`, refresh maker-book cache),
/// - range child listing (no local row, `save_order` ok).
///
/// Returns `false` for all other trade-DM `NewOrder` shapes; caller continues generic hydration.
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

/// Snapshot of the newest existing Messages row for an order, captured under a
/// short `messages` lock so later dedup/merge logic can compare without holding it.
struct PriorMessageSnapshot {
    timestamp: i64,
    action: Action,
    sat_amount: Option<i64>,
    buyer_invoice: Option<String>,
    auto_popup_shown: bool,
    order_kind: Option<mostro_core::order::Kind>,
    is_mine: Option<bool>,
    order_status: Option<Status>,
    order_snapshot: Option<SmallOrder>,
}

/// Take-sell buyer still waiting to submit a payout invoice (MOSTRO-078).
///
/// The DM listener must not frame sat amounts from an unvalidated daemon
/// `Payload::Order` for this phase — only the take_order execute path (or a
/// prior trusted Messages row) may supply sats.
///
/// `is_mine == None` is treated as taker: that is the TrackOrder-before-`save_order`
/// race where role is not yet persisted.
fn is_take_sell_buyer_waiting_invoice(
    action: &Action,
    is_mine: Option<bool>,
    order_kind: Option<mostro_core::order::Kind>,
    order_status: Option<Status>,
) -> bool {
    matches!(action, Action::AddInvoice)
        && !matches!(is_mine, Some(true))
        && order_kind == Some(mostro_core::order::Kind::Sell)
        && matches!(order_status, Some(Status::WaitingBuyerInvoice) | None)
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
    // Trade-DM `NewOrder` special cases only (create-order `NewOrder` uses the waiter path).
    // Unhandled shapes fall through to generic hydration with `new_order_would_regress_messages_row`.
    if matches!(action, Action::NewOrder) {
        if try_handle_new_order_trade_dm(
            messages,
            order_id,
            trade_index,
            inner_kind,
            pool,
            trade_keys,
        )
        .await
        {
            return;
        }
        log::debug!(
            "Trade-DM NewOrder not handled by republish/range-child path; continuing generic hydration order_id={}",
            order_id
        );
    }

    // Snapshot before upsert: used for status/cancel paths and to detect whether `save_order`
    // already established maker/taker (vs a DM-only row inserted below).
    let db_order = Order::get_by_id(pool, &order_id.to_string()).await.ok();
    let had_local_row_before_upsert = db_order.is_some();
    let status_from_db = db_order.as_ref().and_then(order_status_from_row);

    // `CantDo` reports that a requested action was rejected; surface it without
    // applying any status carried by its payload to the local order.
    let status_candidate = if matches!(action, Action::CantDo) {
        None
    } else {
        resolved_status_candidate(&action, &inner_kind.payload)
    };

    // Taker pre-Active cancel returns the order to the book; drop stale local row instead of
    // keeping it as terminal trade state.
    if matches!(action, Action::Canceled)
        && inner_kind.payload.is_none()
        && db_order.as_ref().is_some_and(is_pre_active_taker_take)
    {
        drop_pre_active_taker_take(pool, messages, order_id, "Canceled").await;
        return;
    }

    if !matches!(action, Action::CantDo) {
        upsert_order_from_trade_dm(
            pool,
            order_id,
            &action,
            &inner_kind.payload,
            inner_kind.request_id,
            trade_keys,
        )
        .await;
    }

    if matches!(
        action,
        Action::DisputeInitiatedByYou | Action::DisputeInitiatedByPeer
    ) {
        if let Some(Payload::Dispute(dispute_id, _)) = inner_kind.payload.as_ref() {
            if let Err(e) =
                Order::update_dispute_id(pool, &order_id.to_string(), &dispute_id.to_string()).await
            {
                log::warn!("Failed to persist dispute id for order {order_id}: {e}");
            }
        }
    }

    if matches!(action, Action::AdminTookDispute) {
        if let Some(Payload::Peer(peer)) = inner_kind.payload.as_ref() {
            match PublicKey::parse(&peer.pubkey) {
                Ok(solver_pubkey) => {
                    if let Some(shared_hex) =
                        derive_shared_key_hex(Some(trade_keys), Some(&peer.pubkey))
                    {
                        if let Err(e) = Order::update_solver_chat(
                            pool,
                            &order_id.to_string(),
                            &peer.pubkey,
                            &shared_hex,
                        )
                        .await
                        {
                            log::warn!("Failed to persist solver chat for order {order_id}: {e}");
                        } else {
                            let since = user_dispute_chat_since_from_file(&order_id.to_string());
                            track_user_dispute_chat(
                                order_id.to_string(),
                                shared_hex,
                                trade_keys.public_key(),
                                solver_pubkey,
                                since,
                            );
                        }
                    }
                }
                Err(e) => log::warn!(
                    "AdminTookDispute carried invalid solver pubkey for order {order_id}: {e}"
                ),
            }
        }
    }

    // Keep the P2P order chat subscription live once the shared key is resolvable (idempotent).
    maybe_track_order_chat(pool, order_id, trade_keys).await;

    // Extract invoice and sat_amount from payload based on action type.
    // For `PayBondInvoice` mostrod populates the bond satoshis in the third
    // `Option<Amount>` field of `Payload::PaymentRequest` (the SmallOrder is
    // `None` per mostro-core 0.11.0 wire format); for `PayInvoice` it may come
    // either as that explicit override or via the embedded order's `amount`.
    //
    // MOSTRO-078: `AddInvoice` sats from `Payload::Order` are **not** trusted on
    // this listener path for take-sell waiting-buyer framing — see
    // [`is_take_sell_buyer_waiting_invoice`] / `effective_sat_amount` below.
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
        Action::AddBondInvoice => match &inner_kind.payload {
            Some(Payload::BondPayoutRequest(req)) => (Some(req.order.amount), None),
            _ => (None, None),
        },
        _ => (None, None),
    };

    // PayInvoice/PayBondInvoice: require a non-empty invoice before continuing.
    // AddInvoice actionable-ness is decided after role/status merge (MOSTRO-078).
    if matches!(action, Action::PayInvoice | Action::PayBondInvoice)
        && !invoice.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
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
            .map(|m| PriorMessageSnapshot {
                timestamp: m.timestamp,
                action: m.message.get_inner_message_kind().action.clone(),
                sat_amount: m.sat_amount,
                buyer_invoice: m.buyer_invoice.clone(),
                auto_popup_shown: m.auto_popup_shown,
                order_kind: m.order_kind,
                is_mine: m.is_mine,
                order_status: m.order_status,
                order_snapshot: m.order_snapshot.clone(),
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
        Some(prior) => {
            if action != prior.action {
                true
            } else {
                timestamp > prior.timestamp
            }
        }
    };

    let prior_sat_amount = existing_message_data.as_ref().and_then(|p| p.sat_amount);
    let prior_invoice = existing_message_data
        .as_ref()
        .and_then(|p| p.buyer_invoice.clone());
    let prior_auto_popup_shown = existing_message_data
        .as_ref()
        .map(|p| p.auto_popup_shown && p.action == action)
        .unwrap_or(false);
    let prior_order_kind = existing_message_data.as_ref().and_then(|p| p.order_kind);
    let prior_is_mine = existing_message_data.as_ref().and_then(|p| p.is_mine);
    let prior_order_status = existing_message_data.as_ref().and_then(|p| p.order_status);
    let prior_order_snapshot = existing_message_data
        .as_ref()
        .and_then(|p| p.order_snapshot.clone());

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
            should_apply_status_transition(
                baseline_status,
                candidate,
                effective_order_kind,
                Some(&action),
            )
        })
        .unwrap_or(false);
    let effective_order_status = if should_accept_candidate {
        status_candidate.or(baseline_status)
    } else {
        baseline_status
    };

    if let Some(candidate) = status_candidate {
        let oid = order_id;
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

    let take_sell_waiting = is_take_sell_buyer_waiting_invoice(
        &action,
        effective_is_mine,
        effective_order_kind,
        effective_order_status,
    );
    let effective_sat_amount = if take_sell_waiting {
        // MOSTRO-078: ignore daemon Payload::Order.amount until execute-path
        // (or a prior trusted Messages row) supplies sats.
        prior_sat_amount
    } else {
        sat_amount.or(prior_sat_amount)
    };
    let effective_invoice = invoice.clone().or(prior_invoice);

    let is_actionable_notification = match &action {
        Action::PayInvoice | Action::PayBondInvoice => effective_invoice
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        Action::AddInvoice | Action::AddBondInvoice => effective_sat_amount.is_some(),
        _ => true,
    };

    let snapshot_from_db = db_order.as_ref().map(small_order_from_db_order);
    let mut effective_order_snapshot = merge_order_snapshots(
        small_order_from_payload(&inner_kind.payload),
        prior_order_snapshot,
        snapshot_from_db,
    );
    if take_sell_waiting {
        // Keep snapshot sats aligned with framing: never leave a fabricated daemon
        // amount for Messages Enter to fall back to when sat_amount is absent.
        if let Some(ref mut snap) = effective_order_snapshot {
            snap.amount = prior_sat_amount.unwrap_or(0);
        }
    }

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
        order_snapshot: effective_order_snapshot,
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
        Some(prior) => {
            let existing_timestamp = prior.timestamp;
            let existing_action = &prior.action;
            let existing_order_status = prior.order_status;
            // Post-retry `add-invoice` must win over `released` / `payment-failed` even when
            // GiftWrap rumor timestamps are out of order — Mostro will not resend it, and
            // Enter on Messages must be able to reopen the invoice popup from this row.
            let force_post_retry_add_invoice = matches!(action, Action::AddInvoice)
                && add_invoice_is_after_failed_payment(
                    status_candidate
                        .or(effective_order_status)
                        .or(existing_order_status),
                );
            if force_post_retry_add_invoice {
                true
            } else if new_order_would_regress_messages_row(&action, existing_action) {
                false
            } else if timestamp > existing_timestamp {
                true
            } else if timestamp == existing_timestamp {
                action != *existing_action
            } else {
                // Older-than-current replay: only replace if the payload status **strictly** advances
                // the status already shown on the row (not merely equal; `should_accept_candidate`
                // allows equality vs baseline for DB updates).
                status_candidate.is_some_and(|c| {
                    should_strictly_advance_status(
                        existing_order_status,
                        c,
                        effective_order_kind,
                        Some(&action),
                    )
                })
            }
        }
    };
    if should_replace_row {
        messages_lock.retain(|m| m.order_id != Some(order_id));
        messages_lock.push(order_message.clone());
        messages_lock.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
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
        let should_untrack_chat = trade_message_should_untrack_order_chat(&message);
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

        if should_untrack_chat {
            untrack_order_chat(order_id.to_string());
            untrack_user_dispute_chat(order_id.to_string());
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
                    unregister_dm_listener_subscription(subscription_id);
                    if let Err(e) = client.unsubscribe(subscription_id).await {
                        log::debug!("unsubscribe failed: {e}");
                    }
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

/// Outcome of replaying trade DMs for one active order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayTradeDmOutcome {
    Hydrated,
    SkippedNoKeys,
    FetchFailed,
    EmptyFetch,
    ParseFailed,
}

/// Aggregate result of [`replay_active_trade_dms`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TradeDmReplaySummary {
    pub attempted: usize,
    pub hydrated: usize,
    pub fetch_failed: usize,
    pub empty: usize,
    pub parse_failed: usize,
    pub skipped_no_keys: usize,
}

impl TradeDmReplaySummary {
    fn record(&mut self, outcome: ReplayTradeDmOutcome) {
        self.attempted += 1;
        match outcome {
            ReplayTradeDmOutcome::Hydrated => self.hydrated += 1,
            ReplayTradeDmOutcome::SkippedNoKeys => self.skipped_no_keys += 1,
            ReplayTradeDmOutcome::FetchFailed => self.fetch_failed += 1,
            ReplayTradeDmOutcome::EmptyFetch => self.empty += 1,
            ReplayTradeDmOutcome::ParseFailed => self.parse_failed += 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TradeDmReplayDispatchMode {
    TrackedSubscription,
    UntrackedFallback,
}

fn trade_dm_replay_dispatch_mode(
    pubkey: &PublicKey,
    pubkey_to_subscription: &HashMap<PublicKey, SubscriptionId>,
    subscription_to_order: &HashMap<SubscriptionId, (Uuid, i64)>,
) -> TradeDmReplayDispatchMode {
    match pubkey_to_subscription.get(pubkey) {
        Some(sub_id) if subscription_to_order.contains_key(sub_id) => {
            TradeDmReplayDispatchMode::TrackedSubscription
        }
        _ => TradeDmReplayDispatchMode::UntrackedFallback,
    }
}

/// Relay fetch filter for trade-DM replay (`fetch_events` on startup or post-restore).
///
/// When `last_seen_dm_ts` is missing (post-wipe restore, first session), mirrors
/// [`dm_helpers::DmSubscriptionMode::StartupCatchUp`]: no `since`, only `limit`, so relay
/// retention — not the 12h cold lookback — bounds how far back we hydrate.
fn trade_dm_replay_fetch_filter(
    transport: Transport,
    mostro_pubkey: PublicKey,
    trade_pubkey: PublicKey,
    last_seen_dm_ts: Option<i64>,
    lookback_start: u64,
) -> Filter {
    let base = filter_protocol_dm_from_mostro(transport, mostro_pubkey, trade_pubkey);
    match last_seen_dm_ts.and_then(|ts| u64::try_from(ts).ok()) {
        None => base.limit(STARTUP_TRADE_DM_FETCH_LIMIT),
        Some(last_seen) => {
            // `last_seen_dm_ts` is rumor time; relay `since` is envelope time — see
            // `STARTUP_GIFTWRAP_ENVELOPE_SKEW_SECS`. Combine with lookback (cold Messages list)
            // then widen backward so the last processed DM's GiftWrap is not filtered out.
            let combined_since = last_seen.min(lookback_start);
            let since_ts = combined_since.saturating_sub(STARTUP_GIFTWRAP_ENVELOPE_SKEW_SECS);
            base.since(Timestamp::from(since_ts))
                .limit(STARTUP_TRADE_DM_FETCH_LIMIT)
        }
    }
}

/// Snapshot of `listen_for_order_messages` locals passed into startup protocol DM replay.
struct DmListenerStartupReplay<'a> {
    client: &'a Client,
    mostro_pubkey: PublicKey,
    transport: Transport,
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

/// Fetch and hydrate the newest trade DM rumor for one active order.
#[allow(clippy::too_many_arguments)]
async fn replay_single_trade_dm(
    client: &Client,
    mostro_pubkey: PublicKey,
    transport: Transport,
    pool: &sqlx::sqlite::SqlitePool,
    user: &User,
    messages: &Arc<Mutex<Vec<OrderMessage>>>,
    pending_notifications: &Arc<Mutex<usize>>,
    message_notification_tx: &UnboundedSender<MessageNotification>,
    active_order_trade_indices: &Arc<Mutex<HashMap<Uuid, i64>>>,
    subscribed_pubkeys: &mut HashSet<PublicKey>,
    subscription_to_order: &mut HashMap<SubscriptionId, (Uuid, i64)>,
    pubkey_to_subscription: &HashMap<PublicKey, SubscriptionId>,
    dropped_user_history_order_ids: &Arc<Mutex<HashSet<Uuid>>>,
    order_id: Uuid,
    trade_index: i64,
    order_last_seen_dm_ts: &HashMap<Uuid, i64>,
    lookback_start: u64,
) -> ReplayTradeDmOutcome {
    let trade_keys = match user.derive_trade_keys(trade_index) {
        Ok(k) => k,
        Err(e) => {
            log::error!(
                "Trade DM replay: failed to derive trade keys for index {}: {}",
                trade_index,
                e
            );
            return ReplayTradeDmOutcome::SkippedNoKeys;
        }
    };
    let pubkey = trade_keys.public_key();

    let filter = trade_dm_replay_fetch_filter(
        transport,
        mostro_pubkey,
        pubkey,
        order_last_seen_dm_ts.get(&order_id).copied(),
        lookback_start,
    );

    let events = match client
        .fetch_events(filter)
        .timeout(FETCH_EVENTS_TIMEOUT)
        .await
    {
        Ok(e) => e,
        Err(e) => {
            log::warn!(
                "Trade DM replay: fetch_events failed for order_id={}: {}",
                order_id,
                e
            );
            return ReplayTradeDmOutcome::FetchFailed;
        }
    };

    if events.is_empty() {
        return ReplayTradeDmOutcome::EmptyFetch;
    }

    let event_list: Vec<Event> = events.into_iter().collect();
    let fetched_n = event_list.len();

    let mut best: Option<(i64, EventId, (Message, i64, PublicKey))> = None;
    for event in &event_list {
        let unwrapped = match unwrap_incoming(event, &trade_keys).await {
            Ok(Some(u)) => u,
            Ok(None) => continue,
            Err(e) => {
                log::warn!(
                    "Trade DM replay: unwrap_incoming failed (event {}): {}",
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
                    ts > *best_ts || (ts == *best_ts && event.id.as_bytes() > best_eid.as_bytes())
                }
            };
            if take {
                best = Some((ts, event.id, triple));
            }
        }
    }

    let Some((max_rumor_ts, _, freshest)) = best else {
        log::trace!(
            "Trade DM replay: order_id={} trade_index={} had {} event(s) but none decrypted/parsed",
            order_id,
            trade_index,
            fetched_n
        );
        return ReplayTradeDmOutcome::ParseFailed;
    };

    log::info!(
        "Trade DM replay: order_id={} trade_index={} fetched {} protocol DM event(s); hydrating newest rumor ts={}",
        order_id,
        trade_index,
        fetched_n,
        max_rumor_ts
    );

    let dispatch_mode =
        trade_dm_replay_dispatch_mode(&pubkey, pubkey_to_subscription, subscription_to_order);
    if matches!(dispatch_mode, TradeDmReplayDispatchMode::UntrackedFallback) {
        log::debug!(
            "Trade DM replay: order_id={} using untracked dispatch (no live subscription mapping)",
            order_id
        );
    }

    let terminal_policy = match dispatch_mode {
        TradeDmReplayDispatchMode::TrackedSubscription => {
            let sub_id = pubkey_to_subscription
                .get(&pubkey)
                .expect("TrackedSubscription requires subscription id");
            GiftWrapTerminalPolicy::TrackedSubscription(sub_id)
        }
        TradeDmReplayDispatchMode::UntrackedFallback => GiftWrapTerminalPolicy::UntrackedFallback,
    };

    dispatch_giftwrap_batch(
        vec![freshest],
        order_id,
        trade_index,
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
        terminal_policy,
        false,
        dropped_user_history_order_ids,
    )
    .await;

    ReplayTradeDmOutcome::Hydrated
}

/// One-shot relay fetch + replay for all active trade orders (awaitable).
#[allow(clippy::too_many_arguments)]
pub async fn replay_active_trade_dms(
    client: &Client,
    mostro_pubkey: PublicKey,
    transport: Transport,
    pool: &sqlx::sqlite::SqlitePool,
    user: &User,
    messages: Arc<Mutex<Vec<OrderMessage>>>,
    pending_notifications: Arc<Mutex<usize>>,
    message_notification_tx: UnboundedSender<MessageNotification>,
    active_order_trade_indices: Arc<Mutex<HashMap<Uuid, i64>>>,
    dropped_user_history_order_ids: Arc<Mutex<HashSet<Uuid>>>,
    startup_active_orders: HashMap<Uuid, i64>,
    order_last_seen_dm_ts: HashMap<Uuid, i64>,
) -> TradeDmReplaySummary {
    let mut subscribed_pubkeys = HashSet::new();
    let mut subscription_to_order = HashMap::new();
    let pubkey_to_subscription = HashMap::new();
    replay_active_trade_dms_with_subscription_maps(
        client,
        mostro_pubkey,
        transport,
        pool,
        user,
        &messages,
        &pending_notifications,
        &message_notification_tx,
        &active_order_trade_indices,
        &mut subscribed_pubkeys,
        &mut subscription_to_order,
        &pubkey_to_subscription,
        &dropped_user_history_order_ids,
        &startup_active_orders,
        &order_last_seen_dm_ts,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn replay_active_trade_dms_with_subscription_maps(
    client: &Client,
    mostro_pubkey: PublicKey,
    transport: Transport,
    pool: &sqlx::sqlite::SqlitePool,
    user: &User,
    messages: &Arc<Mutex<Vec<OrderMessage>>>,
    pending_notifications: &Arc<Mutex<usize>>,
    message_notification_tx: &UnboundedSender<MessageNotification>,
    active_order_trade_indices: &Arc<Mutex<HashMap<Uuid, i64>>>,
    subscribed_pubkeys: &mut HashSet<PublicKey>,
    subscription_to_order: &mut HashMap<SubscriptionId, (Uuid, i64)>,
    pubkey_to_subscription: &HashMap<PublicKey, SubscriptionId>,
    dropped_user_history_order_ids: &Arc<Mutex<HashSet<Uuid>>>,
    startup_active_orders: &HashMap<Uuid, i64>,
    order_last_seen_dm_ts: &HashMap<Uuid, i64>,
) -> TradeDmReplaySummary {
    let lookback_start = Timestamp::now()
        .as_secs()
        .saturating_sub(STARTUP_TRADE_DM_LOOKBACK_SECS);
    let mut summary = TradeDmReplaySummary::default();

    for (&order_id, &trade_index) in startup_active_orders {
        let outcome = replay_single_trade_dm(
            client,
            mostro_pubkey,
            transport,
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
            order_id,
            trade_index,
            order_last_seen_dm_ts,
            lookback_start,
        )
        .await;
        summary.record(outcome);
    }

    summary
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
        mostro_pubkey,
        transport,
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

    let summary = replay_active_trade_dms_with_subscription_maps(
        client,
        mostro_pubkey,
        transport,
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
        startup_active_orders,
        order_last_seen_dm_ts,
    )
    .await;

    log::info!(
        "Startup trade DM replay: attempted={} hydrated={} empty={} fetch_failed={} parse_failed={} skipped_no_keys={}",
        summary.attempted,
        summary.hydrated,
        summary.empty,
        summary.fetch_failed,
        summary.parse_failed,
        summary.skipped_no_keys
    );
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
        match unwrap_incoming(event, &trade_keys).await {
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

/// Background DM router for Mostro protocol DM events (GiftWrap or signed kind 14).
///
/// Responsibilities:
/// - maintain relay subscriptions for tracked orders (`TrackOrder`) and temporary
///   request/response waiters (`RegisterWaiter` / `wait_for_dm`)
/// - route each incoming protocol DM through two complementary paths:
///   1) waiter path: satisfy in-flight `wait_for_dm` calls
///   2) tracked-order path: parse and dispatch updates to the order/UI pipeline
/// - reuse decryptability checks across both paths for the same incoming event
///   and trade pubkey (`HashMap<PublicKey, bool>` scoped to one notification; the
///   unknown-subscription fallback reuses the `UnwrappedMessage` from `resolve_order_for_event`
///   so it does not unwrap twice there)
///
/// Lifecycle notes:
/// - spawned via [`crate::util::spawn_supervised_trade_dm_listener`] (startup, reload,
///   reconnect, and panic/exit recovery); on failure the supervisor publishes a fresh
///   command sender **before** backoff so `TrackOrder` / waiters buffer, then this
///   loop re-bootstraps from `active_order_trade_indices` (merged with DB hydration)
/// - aborting this task drops in-memory `pending_waiters`; [`wait_for_dm`] re-registers
///   on the rebuilt router without resending (MOSTRO-080)
/// - bootstrap subscriptions for already-active orders at startup
/// - continue processing relay notifications even if `dm_subscription_rx` is closed
///   (no new dynamic subscriptions, existing ones remain active)
#[allow(clippy::too_many_arguments)]
pub async fn listen_for_order_messages(
    client: Client,
    mostro_pubkey: PublicKey,
    transport: Transport,
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
            Some(ts) => dm_helpers::DmSubscriptionMode::StartupSince(ts),
            None => dm_helpers::DmSubscriptionMode::StartupCatchUp,
        };
        let _ = dm_helpers::ensure_order_dm_subscription(
            &client,
            transport,
            mostro_pubkey,
            &mut subscribed_pubkeys,
            &mut subscription_to_order,
            &mut pubkey_to_subscription,
            pubkey,
            dm_helpers::DmOrderSubscription {
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
            mostro_pubkey,
            transport,
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
                        if !dm_helpers::ensure_order_dm_subscription(
                            &client,
                            transport,
                            mostro_pubkey,
                            &mut subscribed_pubkeys,
                            &mut subscription_to_order,
                            &mut pubkey_to_subscription,
                            pubkey,
                            dm_helpers::DmOrderSubscription {
                                order_id,
                                trade_index,
                                error_label: "Failed to subscribe for trade pubkey",
                                info_label: Some("[dm_listener] Subscribed protocol DM:"),
                                mode: dm_helpers::DmSubscriptionMode::LiveOnly,
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
                        admit_tx,
                        catch_up_since,
                    } => {
                        prune_closed_waiters(&mut pending_waiters);
                        if pending_waiters.len() >= MAX_PENDING_WAITERS {
                            log::warn!(
                                "[dm_listener] rejecting waiter registration: pending_waiters={} (cap={})",
                                pending_waiters.len(),
                                MAX_PENDING_WAITERS
                            );
                            let _ = admit_tx.send(WaiterAdmitResult::CapacityFull);
                            // Drop `response_tx`; wait_for_dm fails on CapacityFull (no retry storm).
                            continue;
                        }
                        let before = pending_waiters.len();
                        let waiter_pubkey = trade_keys.public_key();
                        if subscribed_pubkeys.insert(waiter_pubkey) {
                            let filter = match catch_up_since {
                                Some(since) => waiter_catch_up_filter(
                                    transport,
                                    mostro_pubkey,
                                    waiter_pubkey,
                                    since,
                                ),
                                None => filter_protocol_dm_from_mostro(
                                    transport,
                                    mostro_pubkey,
                                    waiter_pubkey,
                                )
                                .limit(0),
                            };
                            match client.subscribe(filter).await {
                                Ok(output) => {
                                    // Remember the subscription id so a later TrackOrder can
                                    // rebind this pubkey to a concrete order_id without requiring
                                    // a second relay subscription.
                                    register_dm_listener_subscription(output.value.clone());
                                    pubkey_to_subscription.insert(waiter_pubkey, output.value);
                                }
                                Err(e) => {
                                    subscribed_pubkeys.remove(&waiter_pubkey);
                                    log::warn!(
                                        "Failed to subscribe waiter pubkey {}: {}",
                                        waiter_pubkey,
                                        e
                                    );
                                    // Do not admit — dropping admit/response cancels; wait_for_dm
                                    // will re-register with catch-up until timeout (MOSTRO-080).
                                    continue;
                                }
                            }
                        }

                        // Always fetch when resurrecting: TrackOrder may already hold a live-only
                        // subscription, so subscribe above is skipped for that pubkey.
                        let response_tx = if let Some(since) = catch_up_since {
                            try_deliver_waiter_catch_up(
                                &client,
                                transport,
                                mostro_pubkey,
                                &trade_keys,
                                since,
                                response_tx,
                            )
                            .await
                        } else {
                            Some(response_tx)
                        };

                        match response_tx {
                            None => {
                                // Catch-up already delivered the reply.
                                let _ = admit_tx.send(WaiterAdmitResult::Admitted);
                                log::trace!(
                                    "[dm_listener] waiter satisfied via catch-up; pending_before={}",
                                    before
                                );
                            }
                            Some(response_tx) => {
                                let _ = admit_tx.send(WaiterAdmitResult::Admitted);
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
                }
            }
            notification = notifications.next() => {
                let Some(notification) = notification else {
                    log::warn!("DM notification stream ended");
                    break;
                };

                if let ClientNotification::Event {
                    subscription_id,
                    event,
                    ..
                } = notification
                {
                    let event = *event;
                    let expected_kind = transport.event_kind();
                    if event.kind != expected_kind {
                        continue;
                    }
                    // One protocol DM event can be consumed by:
                    // 1) request/response waiters (`wait_for_dm`) and
                    // 2) tracked order subscriptions (UI/order state pipeline).
                    // Shared cache for this single incoming event: decryptability is keyed only
                    // by trade pubkey; `event.id` is fixed for the whole handler block.
                    // This avoids duplicate `unwrap_incoming` calls between waiter and tracked paths.
                    let mut rumor_cache: HashMap<PublicKey, CachedDmUnwrap> = HashMap::new();

                    if !pending_waiters.is_empty() {
                        let mut still_pending: Vec<PendingDmWaiter> =
                            Vec::with_capacity(pending_waiters.len());
                        // Try to satisfy in-flight `wait_for_dm` calls first.
                        // Non-matching waiters are re-queued and will be checked again on the
                        // next protocol DM event.
                        for waiter in pending_waiters.drain(..) {
                            // Drop promptly when wait_for_dm timed out (receiver gone); no decrypt.
                            if waiter.response_tx.is_closed() {
                                continue;
                            }
                            let key = waiter.trade_keys.public_key();
                            let cached = if let Some(cached) = rumor_cache.get(&key) {
                                *cached
                            } else {
                                let cached = match unwrap_incoming(&event, &waiter.trade_keys).await
                                {
                                    Ok(Some(u)) => CachedDmUnwrap {
                                        can_decrypt: true,
                                        skip_for_waiter: !is_mostro_waiter_reply(
                                            &event,
                                            &waiter.trade_keys,
                                            &u,
                                            mostro_pubkey,
                                        ),
                                    },
                                    _ => CachedDmUnwrap {
                                        can_decrypt: false,
                                        skip_for_waiter: false,
                                    },
                                };
                                rumor_cache.insert(key, cached);
                                cached
                            };

                            if cached.can_decrypt && !cached.skip_for_waiter {
                                let _ = waiter.response_tx.send(event.clone());
                            } else {
                                // Not for this waiter, or our own echoed outbound request.
                                still_pending.push(waiter);
                            }
                        }
                        pending_waiters = still_pending;
                    }

                    if let Some((order_id, trade_index)) = subscription_to_order.get(&subscription_id).copied() {
                        log::info!(
                            "[dm_listener] Routed protocol DM by subscription_id={} to order_id={}, trade_index={}",
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
                        let can_decrypt = if let Some(cached) = rumor_cache.get(&key) {
                            cached.can_decrypt
                        } else {
                            let ok = matches!(
                                unwrap_incoming(&event, &trade_keys).await,
                                Ok(Some(_))
                            );
                            rumor_cache.insert(
                                key,
                                CachedDmUnwrap {
                                    can_decrypt: ok,
                                    skip_for_waiter: false,
                                },
                            );
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
                            "[dm_listener] Routed protocol DM by active-order key for unknown subscription_id={} to order_id={}, trade_index={}",
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
    use super::{
        default_dm_expiration, effective_is_mine_for_trade_dm_message, handle_trade_dm_for_order,
        is_mostro_waiter_reply, is_own_signed_v2_outbound, is_pre_active_maker_listing,
        is_pre_active_taker_take, is_take_sell_buyer_waiting_invoice,
        new_order_would_regress_messages_row, set_dm_router_cmd_tx,
        should_reregister_dm_waiter_after_cancel, small_order_pending_from_new_order_payload,
        trade_dm_replay_dispatch_mode, trade_dm_replay_fetch_filter, trade_message_is_terminal,
        trade_message_should_untrack_order_chat, upsert_order_from_trade_dm, wait_for_dm,
        waiter_catch_up_filter, DmRouterCmd, TradeDmReplayDispatchMode, WaiterAdmitResult,
        STARTUP_GIFTWRAP_ENVELOPE_SKEW_SECS, STARTUP_TRADE_DM_FETCH_LIMIT,
        WAIT_FOR_DM_CATCHUP_LIMIT,
    };
    use crate::models::Order;
    use crate::ui::orders::message_action_compact_label_for_message;
    use mostro_core::prelude::{
        Action, Message, Payload, SmallOrder, Status, Transport, UnwrappedMessage,
    };
    use nostr_sdk::prelude::{EventBuilder, FinalizeEvent, Keys, Tag, Timestamp};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    /// `wait_for_dm` uses the process-global DM router sender; serialize tests that publish it.
    static WAIT_FOR_DM_TEST_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

    async fn lock_wait_for_dm_router_tests() -> tokio::sync::MutexGuard<'static, ()> {
        WAIT_FOR_DM_TEST_LOCK.lock().await
    }

    #[tokio::test]
    async fn cant_do_surfaces_rejection_without_changing_order_status() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        sqlx::query(
            r#"
            CREATE TABLE orders (
                id TEXT PRIMARY KEY, kind TEXT, status TEXT, amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL, min_amount INTEGER, max_amount INTEGER,
                fiat_amount INTEGER NOT NULL, payment_method TEXT NOT NULL,
                premium INTEGER NOT NULL, trade_keys TEXT, counterparty_pubkey TEXT,
                order_chat_shared_key_hex TEXT, dispute_id TEXT, solver_pubkey TEXT,
                dispute_chat_shared_key_hex TEXT, is_mine INTEGER NOT NULL,
                buyer_invoice TEXT, request_id INTEGER, trade_index INTEGER,
                created_at INTEGER, expires_at INTEGER, last_seen_dm_ts INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("orders table");

        let order_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO orders (id, kind, status, amount, fiat_code, fiat_amount, \
             payment_method, premium, is_mine) VALUES (?, 'buy', 'active', 1000, 'USD', 10, \
             'bank', 0, 1)",
        )
        .bind(order_id.to_string())
        .execute(&pool)
        .await
        .expect("active order");

        let messages = Arc::new(Mutex::new(Vec::new()));
        let pending_notifications = Arc::new(Mutex::new(0));
        let (notification_tx, mut notification_rx) = tokio::sync::mpsc::unbounded_channel();
        let trade_keys = Keys::generate();
        let message = Message::new_order(
            Some(order_id),
            None,
            Some(7),
            Action::CantDo,
            Some(Payload::Order(SmallOrder {
                status: Some(Status::Canceled),
                ..Default::default()
            })),
        );

        handle_trade_dm_for_order(
            &messages,
            &pending_notifications,
            &notification_tx,
            order_id,
            7,
            message,
            100,
            Keys::generate().public_key(),
            &pool,
            &trade_keys,
            true,
        )
        .await;

        let stored = Order::get_by_id(&pool, &order_id.to_string())
            .await
            .expect("stored order");
        assert_eq!(stored.status.as_deref(), Some("active"));
        let messages = messages.lock().expect("messages lock");
        assert_eq!(messages.len(), 1);
        assert_eq!(
            message_action_compact_label_for_message(&messages[0]),
            "Action Rejected"
        );
        assert_eq!(*pending_notifications.lock().expect("pending lock"), 1);
        assert!(notification_rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn payload_id_mismatch_cannot_redirect_status_update() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        sqlx::query(
            r#"
            CREATE TABLE orders (
                id TEXT PRIMARY KEY, kind TEXT, status TEXT, amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL, min_amount INTEGER, max_amount INTEGER,
                fiat_amount INTEGER NOT NULL, payment_method TEXT NOT NULL,
                premium INTEGER NOT NULL, trade_keys TEXT, counterparty_pubkey TEXT,
                order_chat_shared_key_hex TEXT, dispute_id TEXT, solver_pubkey TEXT,
                dispute_chat_shared_key_hex TEXT, is_mine INTEGER NOT NULL,
                buyer_invoice TEXT, request_id INTEGER, trade_index INTEGER,
                created_at INTEGER, expires_at INTEGER, last_seen_dm_ts INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("orders table");

        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO orders (id, kind, status, amount, fiat_code, fiat_amount, payment_method, premium, is_mine) VALUES (?, 'buy', 'active', 1000, 'USD', 10, 'bank', 0, 1)",
        )
        .bind(id_a.to_string())
        .execute(&pool)
        .await
        .expect("order a");
        sqlx::query(
            "INSERT INTO orders (id, kind, status, amount, fiat_code, fiat_amount, payment_method, premium, is_mine) VALUES (?, 'buy', 'active', 1000, 'USD', 10, 'bank', 0, 1)",
        )
        .bind(id_b.to_string())
        .execute(&pool)
        .await
        .expect("order b");

        let messages = Arc::new(Mutex::new(Vec::new()));
        let pending_notifications = Arc::new(Mutex::new(0));
        let (notification_tx, _notification_rx) = tokio::sync::mpsc::unbounded_channel();
        let trade_keys = Keys::generate();
        let message = Message::new_order(
            Some(id_a),
            None,
            Some(8),
            Action::Released,
            Some(Payload::Order(SmallOrder {
                id: Some(id_b),
                status: Some(Status::Success),
                ..Default::default()
            })),
        );

        handle_trade_dm_for_order(
            &messages,
            &pending_notifications,
            &notification_tx,
            id_a,
            8,
            message,
            100,
            Keys::generate().public_key(),
            &pool,
            &trade_keys,
            true,
        )
        .await;

        let row_a = Order::get_by_id(&pool, &id_a.to_string())
            .await
            .expect("order a present");
        let row_b = Order::get_by_id(&pool, &id_b.to_string())
            .await
            .expect("order b present");
        assert_eq!(row_a.status.as_deref(), Some("success"));
        assert_eq!(row_b.status.as_deref(), Some("active"));
    }

    #[test]
    fn own_signed_v2_outbound_is_skipped_by_waiter_guard() {
        let keys = Keys::generate();
        let event = EventBuilder::new(nostr_sdk::prelude::Kind::PrivateDirectMessage, "ciphertext")
            .tags([Tag::public_key(keys.public_key())])
            .finalize(&keys)
            .expect("sign kind-14");
        let message = Message::new_dispute(None, None, None, Action::AdminTakeDispute, None);
        let sig = Message::sign(message.as_json().expect("json"), &keys);
        let signed = UnwrappedMessage {
            message: message.clone(),
            signature: Some(sig),
            sender: keys.public_key(),
            identity: keys.public_key(),
            created_at: Timestamp::now(),
        };
        assert!(is_own_signed_v2_outbound(&event, &keys, &signed));

        let unsigned = UnwrappedMessage {
            message,
            signature: None,
            sender: keys.public_key(),
            identity: keys.public_key(),
            created_at: Timestamp::now(),
        };
        assert!(!is_own_signed_v2_outbound(&event, &keys, &unsigned));
    }

    #[test]
    fn waiter_reply_requires_mostro_sender() {
        let trade_keys = Keys::generate();
        let mostro = Keys::generate().public_key();
        let stranger = Keys::generate().public_key();
        let event = EventBuilder::new(nostr_sdk::prelude::Kind::GiftWrap, "ciphertext")
            .tags([Tag::public_key(trade_keys.public_key())])
            .finalize(&Keys::generate())
            .expect("sign giftwrap envelope");
        let message = Message::new_order(None, None, None, Action::NewOrder, None);

        let from_mostro = UnwrappedMessage {
            message: message.clone(),
            signature: None,
            sender: mostro,
            identity: mostro,
            created_at: Timestamp::now(),
        };
        assert!(is_mostro_waiter_reply(
            &event,
            &trade_keys,
            &from_mostro,
            mostro
        ));

        let from_stranger = UnwrappedMessage {
            message,
            signature: None,
            sender: stranger,
            identity: stranger,
            created_at: Timestamp::now(),
        };
        assert!(!is_mostro_waiter_reply(
            &event,
            &trade_keys,
            &from_stranger,
            mostro
        ));
    }

    #[test]
    fn action_only_canceled_is_terminal() {
        let message = Message::new_order(None, None, None, Action::Canceled, None);
        assert!(trade_message_is_terminal(&message));
        assert!(trade_message_should_untrack_order_chat(&message));
    }

    #[test]
    fn success_order_payload_is_terminal_for_dm_but_keeps_chat_tracked() {
        let message = Message::new_order(
            None,
            None,
            None,
            Action::Released,
            Some(Payload::Order(SmallOrder {
                status: Some(Status::Success),
                ..Default::default()
            })),
        );
        assert!(trade_message_is_terminal(&message));
        assert!(!trade_message_should_untrack_order_chat(&message));
    }

    #[test]
    fn canceled_order_payload_untracks_chat() {
        let message = Message::new_order(
            None,
            None,
            None,
            Action::Canceled,
            Some(Payload::Order(SmallOrder {
                status: Some(Status::Canceled),
                ..Default::default()
            })),
        );
        assert!(trade_message_should_untrack_order_chat(&message));
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

    fn sample_order_row(is_mine: bool, status: &str) -> Order {
        Order {
            id: Some("00000000-0000-0000-0000-000000000001".to_string()),
            kind: Some("buy".to_string()),
            status: Some(status.to_string()),
            amount: 1000,
            fiat_code: "USD".to_string(),
            min_amount: None,
            max_amount: None,
            fiat_amount: 100,
            payment_method: "bank".to_string(),
            premium: 0,
            trade_keys: None,
            counterparty_pubkey: None,
            order_chat_shared_key_hex: None,
            dispute_id: None,
            solver_pubkey: None,
            dispute_chat_shared_key_hex: None,
            is_mine,
            buyer_invoice: None,
            request_id: Some(1),
            trade_index: Some(1),
            created_at: None,
            expires_at: None,
            last_seen_dm_ts: None,
        }
    }

    #[test]
    fn small_order_pending_from_new_order_requires_pending_status() {
        let pending = SmallOrder {
            status: Some(Status::Pending),
            ..Default::default()
        };
        let active = SmallOrder {
            status: Some(Status::Active),
            ..Default::default()
        };
        assert!(
            small_order_pending_from_new_order_payload(&Some(Payload::Order(pending))).is_some()
        );
        assert!(
            small_order_pending_from_new_order_payload(&Some(Payload::Order(active))).is_none()
        );
        assert!(small_order_pending_from_new_order_payload(&None).is_none());
    }

    #[test]
    fn pre_active_taker_take_predicate() {
        let row = sample_order_row(false, "waiting-payment");
        assert!(is_pre_active_taker_take(&row));
        assert!(!is_pre_active_maker_listing(&row));
    }

    #[test]
    fn pre_active_maker_listing_predicate() {
        let row = sample_order_row(true, "waiting-taker-bond");
        assert!(is_pre_active_maker_listing(&row));
        assert!(!is_pre_active_taker_take(&row));
    }

    #[test]
    fn pre_active_maker_waiting_maker_bond_predicate() {
        let row = sample_order_row(true, "waiting-maker-bond");
        assert!(is_pre_active_maker_listing(&row));
        assert!(!is_pre_active_taker_take(&row));
    }

    #[test]
    fn active_trade_is_not_pre_active_special_case() {
        let maker = sample_order_row(true, "active");
        let taker = sample_order_row(false, "active");
        assert!(!is_pre_active_maker_listing(&maker));
        assert!(!is_pre_active_taker_take(&taker));
    }

    #[test]
    fn new_order_does_not_regress_non_new_order_messages_row() {
        assert!(new_order_would_regress_messages_row(
            &Action::NewOrder,
            &Action::PayBondInvoice,
        ));
        assert!(!new_order_would_regress_messages_row(
            &Action::NewOrder,
            &Action::NewOrder,
        ));
        assert!(!new_order_would_regress_messages_row(
            &Action::PayInvoice,
            &Action::NewOrder,
        ));
    }

    #[test]
    fn take_sell_buyer_waiting_invoice_gate_covers_taker_race() {
        use mostro_core::order::Kind;

        // TrackOrder-before-save_order race: role unknown, sell AddInvoice.
        assert!(is_take_sell_buyer_waiting_invoice(
            &Action::AddInvoice,
            None,
            Some(Kind::Sell),
            Some(Status::WaitingBuyerInvoice),
        ));
        assert!(is_take_sell_buyer_waiting_invoice(
            &Action::AddInvoice,
            Some(false),
            Some(Kind::Sell),
            None,
        ));
        // Maker buy AddInvoice must still allow listener framing.
        assert!(!is_take_sell_buyer_waiting_invoice(
            &Action::AddInvoice,
            Some(true),
            Some(Kind::Buy),
            Some(Status::WaitingBuyerInvoice),
        ));
        // Post-retry settled-hold is not the take-sell waiting gate.
        assert!(!is_take_sell_buyer_waiting_invoice(
            &Action::AddInvoice,
            Some(false),
            Some(Kind::Sell),
            Some(Status::SettledHoldInvoice),
        ));
    }

    #[tokio::test]
    async fn add_invoice_dm_without_local_row_does_not_persist_forged_amount() {
        // MOSTRO-078: TrackOrder-before-save_order race — forged AddInvoice
        // sats must not create/update an orders row with the payload amount.
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        sqlx::query(
            r#"
            CREATE TABLE orders (
                id TEXT PRIMARY KEY, kind TEXT, status TEXT, amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL, min_amount INTEGER, max_amount INTEGER,
                fiat_amount INTEGER NOT NULL, payment_method TEXT NOT NULL,
                premium INTEGER NOT NULL, trade_keys TEXT, counterparty_pubkey TEXT,
                order_chat_shared_key_hex TEXT, dispute_id TEXT, solver_pubkey TEXT,
                dispute_chat_shared_key_hex TEXT, is_mine INTEGER NOT NULL,
                buyer_invoice TEXT, request_id INTEGER, trade_index INTEGER,
                created_at INTEGER, expires_at INTEGER, last_seen_dm_ts INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("orders table");

        let order_id = Uuid::new_v4();
        let forged_amount = 1_i64;
        let trade_keys = Keys::generate();
        upsert_order_from_trade_dm(
            &pool,
            order_id,
            &Action::AddInvoice,
            &Some(Payload::Order(SmallOrder {
                id: Some(order_id),
                kind: Some(mostro_core::order::Kind::Sell),
                status: Some(Status::WaitingBuyerInvoice),
                amount: forged_amount,
                fiat_code: "USD".to_string(),
                fiat_amount: 100,
                payment_method: "SEPA".to_string(),
                ..Default::default()
            })),
            Some(1),
            &trade_keys,
        )
        .await;

        match Order::get_by_id(&pool, &order_id.to_string()).await {
            Ok(row) => assert_ne!(
                row.amount, forged_amount,
                "persisted amount must not equal forged AddInvoice payload"
            ),
            Err(_) => {
                // Expected: hydration deferred — no row until take_order saves
                // a trusted amount.
            }
        }
    }

    #[tokio::test]
    async fn add_invoice_dm_preserves_trusted_local_amount_over_forged_payload() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        sqlx::query(
            r#"
            CREATE TABLE orders (
                id TEXT PRIMARY KEY, kind TEXT, status TEXT, amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL, min_amount INTEGER, max_amount INTEGER,
                fiat_amount INTEGER NOT NULL, payment_method TEXT NOT NULL,
                premium INTEGER NOT NULL, trade_keys TEXT, counterparty_pubkey TEXT,
                order_chat_shared_key_hex TEXT, dispute_id TEXT, solver_pubkey TEXT,
                dispute_chat_shared_key_hex TEXT, is_mine INTEGER NOT NULL,
                buyer_invoice TEXT, request_id INTEGER, trade_index INTEGER,
                created_at INTEGER, expires_at INTEGER, last_seen_dm_ts INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("orders table");

        let order_id = Uuid::new_v4();
        let trusted_amount = 20_895_i64;
        let forged_amount = 1_i64;
        let trade_keys = Keys::generate();
        sqlx::query(
            "INSERT INTO orders (id, kind, status, amount, fiat_code, fiat_amount, \
             payment_method, premium, trade_keys, is_mine, trade_index) \
             VALUES (?, 'sell', 'waiting-buyer-invoice', ?, 'USD', 100, 'SEPA', 0, ?, 0, 3)",
        )
        .bind(order_id.to_string())
        .bind(trusted_amount)
        .bind(trade_keys.secret_key().to_secret_hex())
        .execute(&pool)
        .await
        .expect("seed trusted take-order row");

        upsert_order_from_trade_dm(
            &pool,
            order_id,
            &Action::AddInvoice,
            &Some(Payload::Order(SmallOrder {
                id: Some(order_id),
                kind: Some(mostro_core::order::Kind::Sell),
                status: Some(Status::WaitingBuyerInvoice),
                amount: forged_amount,
                fiat_code: "USD".to_string(),
                fiat_amount: 100,
                payment_method: "SEPA".to_string(),
                ..Default::default()
            })),
            Some(1),
            &trade_keys,
        )
        .await;

        let stored = Order::get_by_id(&pool, &order_id.to_string())
            .await
            .expect("order still present");
        assert_eq!(stored.amount, trusted_amount);
        assert_ne!(stored.amount, forged_amount);
    }

    #[test]
    fn default_dm_expiration_is_thirty_days_ahead() {
        let now = Timestamp::now().as_secs();
        let exp = default_dm_expiration().as_secs();
        let thirty_days: u64 = 30 * 24 * 60 * 60;
        assert!(exp >= now + thirty_days.saturating_sub(2));
        assert!(exp <= now + thirty_days + 2);
    }

    #[test]
    fn trade_dm_replay_uses_untracked_fallback_without_subscription_mapping() {
        use nostr_sdk::prelude::SubscriptionId;
        use std::collections::HashMap;

        let pubkey = Keys::generate().public_key();
        let empty_pubkey_to_sub: HashMap<_, SubscriptionId> = HashMap::new();
        let empty_sub_to_order: HashMap<SubscriptionId, (Uuid, i64)> = HashMap::new();
        assert_eq!(
            trade_dm_replay_dispatch_mode(&pubkey, &empty_pubkey_to_sub, &empty_sub_to_order),
            TradeDmReplayDispatchMode::UntrackedFallback
        );
    }

    #[test]
    fn trade_dm_replay_uses_untracked_fallback_when_subscription_not_tracked() {
        use nostr_sdk::prelude::SubscriptionId;
        use std::collections::HashMap;

        let pubkey = Keys::generate().public_key();
        let sub_id = SubscriptionId::generate();
        let mut pubkey_to_sub = HashMap::new();
        pubkey_to_sub.insert(pubkey, sub_id);
        let empty_sub_to_order: HashMap<SubscriptionId, (Uuid, i64)> = HashMap::new();
        assert_eq!(
            trade_dm_replay_dispatch_mode(&pubkey, &pubkey_to_sub, &empty_sub_to_order),
            TradeDmReplayDispatchMode::UntrackedFallback
        );
    }

    #[test]
    fn trade_dm_replay_uses_tracked_subscription_when_mapping_complete() {
        use nostr_sdk::prelude::SubscriptionId;
        use std::collections::HashMap;

        let pubkey = Keys::generate().public_key();
        let sub_id = SubscriptionId::generate();
        let mut pubkey_to_sub = HashMap::new();
        pubkey_to_sub.insert(pubkey, sub_id.clone());
        let mut sub_to_order = HashMap::new();
        sub_to_order.insert(sub_id, (Uuid::new_v4(), 1));
        assert_eq!(
            trade_dm_replay_dispatch_mode(&pubkey, &pubkey_to_sub, &sub_to_order),
            TradeDmReplayDispatchMode::TrackedSubscription
        );
    }

    #[test]
    fn trade_dm_replay_no_cursor_uses_catch_up_fetch_without_since() {
        let trade = Keys::generate().public_key();
        let mostro = Keys::generate().public_key();
        let lookback_start = Timestamp::now().as_secs();
        let filter = trade_dm_replay_fetch_filter(
            Transport::Nip44Direct,
            mostro,
            trade,
            None,
            lookback_start,
        );
        let json = filter.as_json();
        assert!(json.contains(&format!("\"limit\":{}", STARTUP_TRADE_DM_FETCH_LIMIT)));
        assert!(!json.contains("\"since\""));
    }

    #[test]
    fn trade_dm_replay_no_cursor_omits_since_even_when_lookback_is_recent() {
        // Regression: post-restore orders have no `last_seen_dm_ts`; the old path used
        // `lookback_start` (now - 12h) and missed DMs older than the cold window.
        let trade = Keys::generate().public_key();
        let mostro = Keys::generate().public_key();
        let recent_lookback = Timestamp::now().as_secs();
        let filter = trade_dm_replay_fetch_filter(
            Transport::Nip44Direct,
            mostro,
            trade,
            None,
            recent_lookback,
        );
        assert!(!filter.as_json().contains("\"since\""));
    }

    #[test]
    fn trade_dm_replay_with_cursor_applies_since_and_skew() {
        let trade = Keys::generate().public_key();
        let mostro = Keys::generate().public_key();
        let last_seen: i64 = 1_700_000_000;
        let lookback_start = last_seen as u64 + 3600;
        let filter = trade_dm_replay_fetch_filter(
            Transport::Nip44Direct,
            mostro,
            trade,
            Some(last_seen),
            lookback_start,
        );
        let json = filter.as_json();
        let expected_since = (last_seen as u64).saturating_sub(STARTUP_GIFTWRAP_ENVELOPE_SKEW_SECS);
        assert!(json.contains(&format!("\"since\":{expected_since}")));
        assert!(json.contains(&format!("\"limit\":{}", STARTUP_TRADE_DM_FETCH_LIMIT)));
    }

    #[test]
    fn waiter_cancel_reregisters_while_budget_remains() {
        // MOSTRO-080: mid-flight cancel must not fail the command while time remains.
        assert!(should_reregister_dm_waiter_after_cancel(
            std::time::Duration::from_millis(100)
        ));
        assert!(!should_reregister_dm_waiter_after_cancel(
            std::time::Duration::ZERO
        ));
    }

    #[tokio::test]
    async fn wait_for_dm_reregisters_after_listener_abort_without_resending() {
        let _guard = lock_wait_for_dm_router_tests().await;
        // Simulate reconnect aborting the first waiter oneshot; the second
        // RegisterWaiter receives the daemon reply. Outbound send runs once.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DmRouterCmd>();
        set_dm_router_cmd_tx(tx).expect("publish router sender");

        let sends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sends_for_task = Arc::clone(&sends);
        let trade_keys = Keys::generate();
        let reply_keys = Keys::generate();
        let reply = EventBuilder::new(nostr_sdk::prelude::Kind::TextNote, "mostro-reply")
            .finalize(&reply_keys)
            .expect("sign reply");

        let router = tokio::spawn(async move {
            let mut saw_first = false;
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    DmRouterCmd::RegisterWaiter {
                        response_tx,
                        admit_tx,
                        catch_up_since,
                        ..
                    } => {
                        let _ = admit_tx.send(WaiterAdmitResult::Admitted);
                        if !saw_first {
                            saw_first = true;
                            assert!(catch_up_since.is_none(), "first registration is live-only");
                            // Drop = reconnect abort of in-flight waiter.
                            drop(response_tx);
                        } else {
                            assert!(
                                catch_up_since.is_some(),
                                "resurrection must request catch-up"
                            );
                            let _ = response_tx.send(reply.clone());
                            break;
                        }
                    }
                    DmRouterCmd::TrackOrder { .. } => {}
                }
            }
        });

        let result = wait_for_dm(&trade_keys, std::time::Duration::from_secs(2), async {
            sends_for_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .await;

        assert!(result.is_ok(), "expected resurrected waiter: {result:?}");
        assert_eq!(
            sends.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "protocol message must be sent only once across waiter resurrection"
        );
        let _ = router.await;
    }

    #[tokio::test]
    async fn wait_for_dm_fails_immediately_on_capacity_full() {
        let _guard = lock_wait_for_dm_router_tests().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DmRouterCmd>();
        set_dm_router_cmd_tx(tx).expect("publish router sender");

        let registrations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let registrations_router = Arc::clone(&registrations);
        let sends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sends_for_task = Arc::clone(&sends);
        let trade_keys = Keys::generate();

        let router = tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                if let DmRouterCmd::RegisterWaiter { admit_tx, .. } = cmd {
                    registrations_router.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let _ = admit_tx.send(WaiterAdmitResult::CapacityFull);
                }
            }
        });

        let result = wait_for_dm(&trade_keys, std::time::Duration::from_secs(2), async {
            sends_for_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .await;

        let err = result.expect_err("capacity must fail closed");
        assert!(
            err.to_string().contains("too many pending waiters"),
            "unexpected error: {err}"
        );
        assert_eq!(
            sends.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "must not send protocol message after capacity rejection"
        );
        assert_eq!(
            registrations.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "capacity rejection must not retry-spam RegisterWaiter"
        );
        router.abort();
        let _ = router.await;
    }

    #[tokio::test]
    async fn wait_for_dm_catch_up_observes_reply_published_during_reconnect_gap() {
        let _guard = lock_wait_for_dm_router_tests().await;
        // Critical ordering (ermeme / MOSTRO-080):
        // send succeeds → listener aborted → reply published before new sub is live →
        // resurrected waiter with catch_up_since still observes the reply (no resend).
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DmRouterCmd>();
        set_dm_router_cmd_tx(tx).expect("publish router sender");

        let sends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sends_for_task = Arc::clone(&sends);
        let trade_keys = Keys::generate();
        let reply_keys = Keys::generate();
        let reply = EventBuilder::new(nostr_sdk::prelude::Kind::TextNote, "gap-reply")
            .finalize(&reply_keys)
            .expect("sign reply");

        let gap_reply = Arc::new(Mutex::new(None::<nostr_sdk::prelude::Event>));
        let gap_reply_router = Arc::clone(&gap_reply);

        let router = tokio::spawn(async move {
            let mut phase = 0u8;
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    DmRouterCmd::RegisterWaiter {
                        response_tx,
                        admit_tx,
                        catch_up_since,
                        ..
                    } => match phase {
                        0 => {
                            phase = 1;
                            assert!(catch_up_since.is_none());
                            let _ = admit_tx.send(WaiterAdmitResult::Admitted);
                            // Abort after admit so send can complete, then drop waiter.
                            drop(response_tx);
                        }
                        1 => {
                            // Reply was published during the gap (before this sub is "live").
                            let stored = gap_reply_router
                                .lock()
                                .expect("gap reply")
                                .clone()
                                .expect("gap reply must be published before resurrection");
                            assert!(
                                catch_up_since.is_some(),
                                "second registration must catch up"
                            );
                            let _ = admit_tx.send(WaiterAdmitResult::Admitted);
                            // Deliver via catch-up path (immediate), not a later live notify.
                            let _ = response_tx.send(stored);
                            break;
                        }
                        _ => break,
                    },
                    DmRouterCmd::TrackOrder { .. } => {}
                }
            }
        });

        let gap_reply_send = Arc::clone(&gap_reply);
        let reply_for_send = reply.clone();
        let result = wait_for_dm(&trade_keys, std::time::Duration::from_secs(2), async move {
            sends_for_task.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Publish reply during the reconnect gap (before resurrected registration).
            *gap_reply_send.lock().expect("store gap reply") = Some(reply_for_send);
            Ok(())
        })
        .await;

        assert!(
            result.is_ok(),
            "catch-up must observe gap reply: {result:?}"
        );
        assert_eq!(sends.load(std::sync::atomic::Ordering::SeqCst), 1);
        let _ = router.await;
    }

    #[test]
    fn waiter_catch_up_filter_uses_since_and_bounded_limit() {
        let mostro = Keys::generate().public_key();
        let waiter = Keys::generate().public_key();
        let since = Timestamp::from(1_700_000_000u64);
        let filter = waiter_catch_up_filter(Transport::Nip44Direct, mostro, waiter, since);
        let json = filter.as_json();
        assert!(json.contains(&format!("\"since\":{}", since.as_secs())));
        assert!(json.contains(&format!("\"limit\":{WAIT_FOR_DM_CATCHUP_LIMIT}")));
    }
}
