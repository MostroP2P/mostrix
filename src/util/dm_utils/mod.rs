// Direct message manager module
// Contains functions for handling direct messages, order channels, and notifications

mod dm_helpers;
mod notifications_ch_mng;
mod order_ch_mng;

pub use notifications_ch_mng::handle_message_notification;
pub use order_ch_mng::handle_operation_result;

use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::models::{Order, User};
use crate::ui::order_message_to_notification;
use crate::ui::{MessageNotification, OrderMessage};
use crate::util::db_utils::update_order_status;
use crate::util::order_utils::{inferred_status_from_trade_action, map_action_to_status};
use crate::util::types::{determine_message_type, MessageType};
use crate::SETTINGS;

pub const FETCH_EVENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

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
            log::warn!("[dm_listener] Failed to set DM router sender due to poisoned lock");
            Err("DM_ROUTER_CMD_TX mutex poisoned")
        }
    }
}

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
    if matches!(&kind.action, Action::Canceled | Action::AdminCanceled) {
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
    to_user: bool,
) -> Result<()> {
    let pow: u8 = SETTINGS
        .get()
        .ok_or_else(|| {
            anyhow::anyhow!("Settings not initialized. Please restart the application.")
        })?
        .pow;
    let message_type = determine_message_type(to_user, false);

    let event = match message_type {
        MessageType::PrivateDirectMessage => {
            dm_helpers::create_private_dm_event(trade_keys, receiver_pubkey, payload, pow).await?
        }
        MessageType::PrivateGiftWrap => {
            dm_helpers::create_gift_wrap_event(
                trade_keys,
                identity_keys,
                receiver_pubkey,
                payload,
                pow,
                expiration,
                false,
            )
            .await?
        }
        MessageType::SignedGiftWrap => {
            dm_helpers::create_gift_wrap_event(
                trade_keys,
                identity_keys,
                receiver_pubkey,
                payload,
                pow,
                expiration,
                true,
            )
            .await?
        }
    };

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
            nostr_sdk::Kind::GiftWrap => {
                let unwrapped_gift = match nip59::extract_rumor(pubkey, dm).await {
                    Ok(u) => u,
                    Err(e) => {
                        log::warn!("Could not decrypt gift wrap (event {}): {}", dm.id, e);
                        continue;
                    }
                };
                let (message, _): (Message, Option<String>) =
                    match serde_json::from_str(&unwrapped_gift.rumor.content) {
                        Ok(msg) => msg,
                        Err(e) => {
                            log::warn!("Could not parse message content (event {}): {}", dm.id, e);
                            continue;
                        }
                    };

                (
                    unwrapped_gift.rumor.created_at,
                    message,
                    unwrapped_gift.sender,
                )
            }
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

            if (created_at.as_u64() as i64) < since_time {
                continue;
            }
        }
        direct_messages.push((message, created_at.as_u64() as i64, sender));
    }
    direct_messages.sort_by(|a, b| a.1.cmp(&b.1));
    direct_messages
}

/// Handle a single decoded trade DM for a given order/trade index.
#[allow(clippy::too_many_arguments)]
async fn handle_trade_dm_for_order(
    messages: &Arc<Mutex<Vec<OrderMessage>>>,
    pending_notifications: &Arc<Mutex<usize>>,
    message_notification_tx: &tokio::sync::mpsc::UnboundedSender<MessageNotification>,
    order_id: Uuid,
    trade_index: i64,
    message: Message,
    timestamp: i64,
    sender: PublicKey,
    pool: &sqlx::SqlitePool,
    trade_keys: &Keys,
) {
    let inner_kind = message.get_inner_message_kind();
    let action = inner_kind.action.clone();

    if matches!(&action, Action::AddInvoice) {
        if let Some(Payload::Order(ref small_order)) = inner_kind.payload {
            let msg_request_id = inner_kind.request_id.and_then(|u| i64::try_from(u).ok());
            match Order::upsert_from_small_order_dm(
                pool,
                order_id,
                small_order.clone(),
                trade_keys,
                msg_request_id,
            )
            .await
            {
                Ok(_) => log::info!(
                    "Persisted order {} to database from AddInvoice DM (status={:?})",
                    order_id,
                    small_order.status
                ),
                Err(e) => log::error!(
                    "Failed to persist order {} from AddInvoice DM: {}",
                    order_id,
                    e
                ),
            }
        }
    }

    // Extract invoice and sat_amount from payload based on action type
    let (sat_amount, invoice) = match &action {
        Action::PayInvoice => match &inner_kind.payload {
            Some(Payload::PaymentRequest(_, invoice, _)) => (None, Some(invoice.clone())),
            _ => (None, None),
        },
        Action::AddInvoice => match &inner_kind.payload {
            Some(Payload::Order(order)) => (Some(order.amount), None),
            _ => (None, None),
        },
        _ => (None, None),
    };

    // Persist status: `Payload::Order`, or action-only messages (`canceled` + `payload: null`
    // with `id` on [`MessageKind`] — see mostro daemon JSON).
    if let Some(Payload::Order(ref order_payload)) = inner_kind.payload {
        if let Some(status) = map_action_to_status(&action, order_payload) {
            let oid = order_payload.id.or(inner_kind.id).unwrap_or(order_id);
            if let Err(e) = update_order_status(pool, &oid.to_string(), status).await {
                log::warn!(
                    "Failed to update status for order {} from DM action {:?}: {}",
                    oid,
                    action,
                    e
                );
            }
        }
    } else if let Some(status) = inferred_status_from_trade_action(&action) {
        let oid = inner_kind.id.unwrap_or(order_id);
        if let Err(e) = update_order_status(pool, &oid.to_string(), status).await {
            log::warn!(
                "Failed to update status for order {} from DM action {:?} (no order payload): {}",
                oid,
                action,
                e
            );
        }
    }

    // Only show PayInvoice popup/notification when an invoice is actually present.
    let is_actionable_notification = match &action {
        Action::PayInvoice => invoice.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
        Action::AddInvoice => sat_amount.is_some(),
        _ => true,
    };

    if matches!(action, Action::PayInvoice) && !is_actionable_notification {
        return;
    }

    // Lock `messages` only long enough to extract comparison data, then drop it
    // before touching `pending_notifications` to avoid lock-order deadlocks.
    let existing_message_data = {
        let messages_lock = messages.lock().unwrap();
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
        Some((existing_timestamp, existing_action, _, _, _)) => {
            if action != *existing_action {
                true
            } else {
                timestamp > *existing_timestamp
            }
        }
    };

    let prior_sat_amount = existing_message_data
        .as_ref()
        .and_then(|(_, _, amt, _, _)| *amt);
    let prior_invoice = existing_message_data
        .as_ref()
        .and_then(|(_, _, _, inv, _)| inv.clone());
    let prior_auto_popup_shown = existing_message_data
        .as_ref()
        .map(|(_, existing_action, _, _, shown)| *shown && *existing_action == action)
        .unwrap_or(false);

    let effective_sat_amount = sat_amount.or(prior_sat_amount);
    let effective_invoice = invoice.clone().or(prior_invoice);

    if is_new_message && is_actionable_notification {
        let mut pending_notifications = pending_notifications.lock().unwrap();
        *pending_notifications += 1;
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
        // Preserve popup-shown state for same-action updates (e.g. duplicate AddInvoice
        // carrying peer reputation payload but no amount), preventing noisy re-popups.
        auto_popup_shown: prior_auto_popup_shown,
    };

    let mut messages_lock = messages.lock().unwrap();
    // Keep one row per order, but ensure the newly accepted message is the one kept.
    // This avoids dropping same-timestamp/different-action updates during dedup.
    messages_lock.retain(|m| m.order_id != Some(order_id));
    messages_lock.push(order_message.clone());
    messages_lock.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // Send notification only for actionable/new updates; this avoids follow-up AddInvoice
    // payload variants (without order amount) from retriggering invoice popups with 0 sats.
    if is_new_message && is_actionable_notification {
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
) {
    let log_each_message = matches!(
        terminal_policy,
        GiftWrapTerminalPolicy::TrackedSubscription(_)
    );

    for (message, timestamp, sender) in parsed_messages {
        let has_terminal_status = trade_message_is_terminal(&message);
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
        )
        .await;

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
                        let mut indices = active_order_trade_indices.lock().unwrap();
                        indices.remove(&order_id);
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
                        let mut indices = active_order_trade_indices.lock().unwrap();
                        indices.remove(&order_id);
                    }
                    if let Ok(keys) = user.derive_trade_keys(trade_index) {
                        subscribed_pubkeys.remove(&keys.public_key());
                    }
                }
            }
        }
    }
}

struct PendingDmWaiter {
    trade_keys: Keys,
    response_tx: oneshot::Sender<Event>,
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
) -> Option<(Uuid, i64, Keys)> {
    GIFTWRAP_FALLBACK_DECRYPT_TOTAL.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();

    let active_orders = match active_order_trade_indices.lock() {
        Ok(indices) => indices.clone(),
        Err(e) => {
            log::warn!(
                "[dm_listener] giftwrap_fallback_decrypt: poisoned active_order_trade_indices lock ({})",
                e
            );
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
        if nip59::extract_rumor(&trade_keys, event).await.is_ok() {
            let duration_ms = started.elapsed().as_millis() as u64;
            GIFTWRAP_FALLBACK_LAST_DURATION_MS.store(duration_ms, Ordering::Relaxed);
            log_giftwrap_fallback_decrypt_stats(active_count, decrypt_attempts, duration_ms, true);
            return Some((order_id, trade_index, trade_keys));
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
/// - reuse per-event decryptability checks across both paths to avoid duplicate
///   `nip59::extract_rumor` work for the same `(event_id, trade_pubkey)`
///
/// Lifecycle notes:
/// - bootstrap subscriptions for already-active orders at startup
/// - continue processing relay notifications even if `dm_subscription_rx` is closed
///   (no new dynamic subscriptions, existing ones remain active)
pub async fn listen_for_order_messages(
    client: Client,
    pool: sqlx::sqlite::SqlitePool,
    active_order_trade_indices: Arc<Mutex<HashMap<Uuid, i64>>>,
    messages: Arc<Mutex<Vec<OrderMessage>>>,
    message_notification_tx: tokio::sync::mpsc::UnboundedSender<MessageNotification>,
    pending_notifications: Arc<Mutex<usize>>,
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
    let mut pending_waiters: Vec<PendingDmWaiter> = Vec::new();

    // Bootstrap subscriptions for orders already known at startup.
    let startup_active_orders = {
        let indices = active_order_trade_indices.lock().unwrap();
        indices.clone()
    };
    for (order_id, trade_index) in startup_active_orders {
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
        let _ = dm_helpers::ensure_order_giftwrap_subscription(
            &client,
            &mut subscribed_pubkeys,
            &mut subscription_to_order,
            pubkey,
            dm_helpers::GiftWrapOrderSubscription {
                order_id,
                trade_index,
                error_label: "Failed startup subscribe for trade pubkey",
                info_label: None,
                mode: dm_helpers::GiftWrapSubscriptionMode::StartupCatchUp,
            },
        )
        .await;
    }

    loop {
        tokio::select! {
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
                            let mut indices = active_order_trade_indices.lock().unwrap();
                            indices.insert(order_id, trade_index);
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
                        let before = pending_waiters.len();
                        let waiter_pubkey = trade_keys.public_key();
                        if subscribed_pubkeys.insert(waiter_pubkey) {
                            let filter = Filter::new()
                                .pubkey(waiter_pubkey)
                                .kind(nostr_sdk::Kind::GiftWrap)
                                .limit(0);
                            match client.subscribe(filter, None).await {
                                Ok(_) => {}
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
                    // Keep a shared per-event cache so we only test decryptability once per
                    // (event_id, trade_pubkey), then reuse the result in both paths.
                    // This avoids duplicate `extract_rumor` calls while preserving behavior.
                    let event_id = event.id;
                    // Cache decryptability for this event across both waiter and tracked paths.
                    // Keep it event-scoped to avoid unbounded growth over runtime.
                    let mut rumor_cache: HashMap<(EventId, PublicKey), bool> = HashMap::new();

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
                            // Cache key: this event + this waiter's trade pubkey.
                            let key = (event_id, waiter.trade_keys.public_key());
                            let can_decrypt = if let Some(boolean) = rumor_cache.get(&key) {
                                *boolean
                            } else {
                                let ok = nip59::extract_rumor(&waiter.trade_keys, &event).await.is_ok();
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
                        let key = (event_id, trade_keys.public_key());
                        let can_decrypt = if let Some(boolean) = rumor_cache.get(&key) {
                            *boolean
                        } else {
                            let ok = nip59::extract_rumor(&trade_keys, &event).await.is_ok();
                            rumor_cache.insert(key, ok);
                            ok
                        };

                        if !can_decrypt {
                            continue;
                        }

                        // Decrypt succeeded for this tracked order, so parse and dispatch.
                        let mut events = Events::default();
                        events.insert(event.clone());

                        let parsed_messages = parse_dm_events(events, &trade_keys, None).await;
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
                        )
                        .await;
                    } else if let Some((order_id, trade_index, trade_keys)) =
                        resolve_order_for_event(&event, &user, &active_order_trade_indices).await
                    {
                        let mut events = Events::default();
                        events.insert(event.clone());
                        let parsed_messages = parse_dm_events(events, &trade_keys, None).await;
                        if !parsed_messages.is_empty() {
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
                                )
                                .await;
                        }
                    }
                }
            }
        }
    }
}
