// Direct message manager module
// Contains functions for handling direct messages, order channels, and notifications

mod dm_helpers;
mod notifications_ch_mng;
mod order_ch_mng;

pub use notifications_ch_mng::handle_message_notification;
pub use order_ch_mng::handle_order_result;

use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::models::User;
use crate::ui::{MessageNotification, OrderMessage};
use crate::util::types::{determine_message_type, MessageType};
use crate::SETTINGS;

pub const FETCH_EVENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, Clone)]
pub enum OrderDmSubscriptionCmd {
    Subscribe {
        order_id: uuid::Uuid,
        trade_index: i64,
    },
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
/// Subscribes first, then sends the message (to avoid missing messages)
pub async fn wait_for_dm<F>(
    client: &Client,
    trade_keys: &Keys,
    timeout: std::time::Duration,
    sent_message: F,
) -> Result<Events>
where
    F: std::future::Future<Output = Result<()>> + Send,
{
    let mut notifications = client.notifications();
    let opts =
        SubscribeAutoCloseOptions::default().exit_policy(ReqExitPolicy::WaitForEventsAfterEOSE(4));
    let subscription = Filter::new()
        .pubkey(trade_keys.public_key())
        .kind(nostr_sdk::Kind::GiftWrap)
        .limit(0);
    let subscription_output = client.subscribe(subscription, Some(opts)).await?;
    let expected_subscription_id = subscription_output.val;

    // Send message here after opening notifications to avoid missing messages.
    sent_message.await?;

    let event = tokio::time::timeout(timeout, async move {
        loop {
            match notifications.recv().await {
                Ok(notification) => match notification {
                    RelayPoolNotification::Event {
                        subscription_id,
                        event,
                        ..
                    } => {
                        // Ignore events from unrelated subscriptions.
                        if subscription_id != expected_subscription_id {
                            continue;
                        }
                        return Ok(*event);
                    }
                    _ => continue,
                },
                Err(e) => {
                    return Err(anyhow::anyhow!("Error receiving notification: {:?}", e));
                }
            }
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Timeout waiting for DM or gift wrap event"))?
    .map_err(|e| anyhow::anyhow!("Error: {}", e))?;

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
    order_id: uuid::Uuid,
    trade_index: i64,
    message: Message,
    timestamp: i64,
    sender: PublicKey,
) {
    let inner_kind = message.get_inner_message_kind();
    let action = inner_kind.action.clone();

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
                )
            })
    };

    // Only increment pending notifications if this is a truly new message.
    let is_new_message = match existing_message_data {
        None => true,
        Some((existing_timestamp, existing_action)) => {
            timestamp > existing_timestamp
                || (timestamp == existing_timestamp && action != existing_action)
        }
    };

    if is_new_message {
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
        sat_amount,
        buyer_invoice: invoice.clone(),
        auto_popup_shown: false,
    };

    let mut messages_lock = messages.lock().unwrap();
    // Keep one row per order, but ensure the newly accepted message is the one kept.
    // This avoids dropping same-timestamp/different-action updates during dedup.
    messages_lock.retain(|m| m.order_id != Some(order_id));
    messages_lock.push(order_message);
    messages_lock.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let action_str = match &action {
        Action::AddInvoice => "Invoice Request",
        Action::PayInvoice => "Payment Request",
        Action::TakeSell => "Take Sell",
        Action::TakeBuy => "Take Buy",
        Action::FiatSent => "Fiat Sent",
        Action::FiatSentOk => "Fiat Received",
        Action::Release | Action::Released => "Release",
        Action::Dispute | Action::DisputeInitiatedByYou => "Dispute",
        Action::WaitingSellerToPay => "Waiting for Seller to Pay",
        Action::Rate => "Rate Counterparty",
        Action::RateReceived => "Rate Counterparty received",
        _ => "New Message",
    };

    let notification = MessageNotification {
        order_id: Some(order_id),
        message_preview: action_str.to_string(),
        timestamp,
        action,
        sat_amount,
        invoice,
    };

    let _ = message_notification_tx.send(notification);
}

/// Continuously listen for messages on trade keys for active orders using subscriptions.
/// This function should be spawned as a background task.
pub async fn listen_for_order_messages(
    client: Client,
    pool: sqlx::sqlite::SqlitePool,
    active_order_trade_indices: Arc<Mutex<HashMap<uuid::Uuid, i64>>>,
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
    let mut subscription_to_order: HashMap<SubscriptionId, (uuid::Uuid, i64)> = HashMap::new();

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
        if subscribed_pubkeys.insert(pubkey) {
            let filter = Filter::new()
                .pubkey(pubkey)
                .kind(nostr_sdk::Kind::GiftWrap)
                .limit(0);
            match client.subscribe(filter, None).await {
                Ok(output) => {
                    subscription_to_order.insert(output.val, (order_id, trade_index));
                }
                Err(e) => {
                    log::warn!(
                        "Failed startup subscribe for trade pubkey {} (index {}): {}",
                        pubkey,
                        trade_index,
                        e
                    );
                    subscribed_pubkeys.remove(&pubkey);
                }
            }
        }
    }

    loop {
        tokio::select! {
            cmd = dm_subscription_rx.recv() => {
                let Some(cmd) = cmd else {
                    // Sender dropped; keep listener alive for existing subscriptions.
                    log::warn!("[dm_listener] dm_subscription_rx closed; no new dynamic subscriptions will be received");
                    continue;
                };

                match cmd {
                    OrderDmSubscriptionCmd::Subscribe { order_id, trade_index } => {
                        log::info!(
                            "[dm_listener] Received subscribe command order_id={}, trade_index={}",
                            order_id,
                            trade_index
                        );
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
                        if subscribed_pubkeys.insert(pubkey) {
                            let filter = Filter::new()
                                .pubkey(pubkey)
                                .kind(nostr_sdk::Kind::GiftWrap)
                                .limit(0);

                            match client.subscribe(filter, None).await {
                                Ok(output) => {
                                    log::info!(
                                        "[dm_listener] Subscribed GiftWrap: subscription_id={}, order_id={}, trade_index={}",
                                        output.val,
                                        order_id,
                                        trade_index
                                    );
                                    subscription_to_order
                                        .insert(output.val, (order_id, trade_index));
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to subscribe for trade pubkey {} (index {}): {}",
                                        pubkey,
                                        trade_index,
                                        e
                                    );
                                    subscribed_pubkeys.remove(&pubkey);
                                    continue;
                                }
                            }
                        }
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

                    if let Some((order_id, trade_index)) = subscription_to_order.get(&subscription_id).copied() {
                        log::info!(
                            "[dm_listener] Routed GiftWrap by subscription_id={} to order_id={}, trade_index={}",
                            subscription_id,
                            order_id,
                            trade_index
                        );

                        // Derive trade keys again for decryption
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
                        for (message, timestamp, sender) in parsed_messages {
                            let has_terminal_status = message_has_terminal_order_status(&message);
                            log::info!(
                                "[dm_listener] Handling message action={:?} ts={} order_id={} trade_index={}",
                                message.get_inner_message_kind().action,
                                timestamp,
                                order_id,
                                trade_index
                            );
                            handle_trade_dm_for_order(
                                &messages,
                                &pending_notifications,
                                &message_notification_tx,
                                order_id,
                                trade_index,
                                message,
                                timestamp,
                                sender,
                            )
                            .await;

                            // Event-driven cleanup: when Mostro sends terminal order status,
                            // stop tracking this order/subscription immediately.
                            if has_terminal_status {
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

                                subscription_to_order.remove(&subscription_id);
                                client.unsubscribe(&subscription_id).await;
                                break;
                            }
                        }
                    } else {
                        // Fallback path: some valid GiftWrap events can arrive under a
                        // subscription id not tracked by this listener (e.g. parallel wait_for_dm
                        // temporary subscriptions). Try active trade keys before dropping.
                        log::info!(
                            "[dm_listener] Unknown subscription_id={}, trying active trade-key fallback",
                            subscription_id
                        );
                        let active_orders = {
                            let indices = active_order_trade_indices.lock().unwrap();
                            indices.clone()
                        };

                        let mut routed = false;
                        for (order_id, trade_index) in active_orders {
                            let trade_keys = match user.derive_trade_keys(trade_index) {
                                Ok(k) => k,
                                Err(_) => continue,
                            };
                            let mut events = Events::default();
                            events.insert(event.clone());
                            let parsed_messages = parse_dm_events(events, &trade_keys, None).await;
                            if parsed_messages.is_empty() {
                                continue;
                            }

                            log::info!(
                                "[dm_listener] Fallback routed GiftWrap to order_id={}, trade_index={} (parsed {} message(s))",
                                order_id,
                                trade_index,
                                parsed_messages.len()
                            );
                            for (message, timestamp, sender) in parsed_messages {
                                let has_terminal_status = message_has_terminal_order_status(&message);
                                handle_trade_dm_for_order(
                                    &messages,
                                    &pending_notifications,
                                    &message_notification_tx,
                                    order_id,
                                    trade_index,
                                    message,
                                    timestamp,
                                    sender,
                                )
                                .await;

                                if has_terminal_status {
                                    {
                                        let mut indices = active_order_trade_indices.lock().unwrap();
                                        indices.remove(&order_id);
                                    }
                                    if let Ok(keys) = user.derive_trade_keys(trade_index) {
                                        subscribed_pubkeys.remove(&keys.public_key());
                                    }
                                }
                            }
                            routed = true;
                            break;
                        }

                        if !routed {
                            log::info!(
                                "[dm_listener] Fallback failed for unknown subscription_id={}",
                                subscription_id
                            );
                        }
                    }
                }
            }
        }
    }
}
