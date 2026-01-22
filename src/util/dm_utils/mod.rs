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
use crate::util::types::{determine_message_type, MessageType};
use crate::SETTINGS;

pub const FETCH_EVENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

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
    client.subscribe(subscription, Some(opts)).await?;

    // Send message here after opening notifications to avoid missing messages.
    sent_message.await?;

    let event = tokio::time::timeout(timeout, async move {
        loop {
            match notifications.recv().await {
                Ok(notification) => match notification {
                    RelayPoolNotification::Event { event, .. } => {
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
) -> Vec<(Message, u64, PublicKey)> {
    use base64::engine::general_purpose;
    use base64::Engine;
    use nip44::v2::{decrypt_to_bytes, ConversationKey};

    let mut id_set = HashSet::<EventId>::new();
    let mut direct_messages: Vec<(Message, u64, PublicKey)> = Vec::new();

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
                .timestamp() as u64;

            if created_at.as_u64() < since_time {
                continue;
            }
        }
        direct_messages.push((message, created_at.as_u64(), sender));
    }
    direct_messages.sort_by(|a, b| a.1.cmp(&b.1));
    direct_messages
}

/// Continuously listen for messages on trade keys for active orders
/// This function should be spawned as a background task
pub async fn listen_for_order_messages(
    client: Client,
    pool: sqlx::sqlite::SqlitePool,
    active_order_trade_indices: Arc<Mutex<HashMap<uuid::Uuid, i64>>>,
    messages: Arc<Mutex<Vec<crate::ui::OrderMessage>>>,
    message_notification_tx: tokio::sync::mpsc::UnboundedSender<crate::ui::MessageNotification>,
    pending_notifications: Arc<Mutex<usize>>,
) {
    let mut refresh_interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
    // Get user key from db
    let user = match User::get(&pool).await {
        Ok(u) => u,
        Err(e) => {
            log::error!("Failed to get user: {}", e);
            return;
        }
    };

    loop {
        refresh_interval.tick().await;

        // Get current active orders
        let active_orders = {
            let indices = active_order_trade_indices.lock().unwrap();
            indices.clone()
        };

        if active_orders.is_empty() {
            continue;
        }

        // For each active order, check for new messages
        for (order_id, trade_index) in active_orders.iter() {
            // Derive trade key for message decode
            let trade_keys = match user.derive_trade_keys(*trade_index) {
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

            // Fetch recent messages for this trade key
            let filter_giftwrap = Filter::new()
                .pubkey(trade_keys.public_key())
                .kind(nostr_sdk::Kind::GiftWrap)
                .limit(5);

            let events = match client
                .fetch_events(filter_giftwrap, FETCH_EVENTS_TIMEOUT)
                .await
            {
                Ok(e) => e,
                Err(e) => {
                    log::warn!(
                        "Failed to fetch giftwrap events for trade index {}: {}",
                        trade_index,
                        e
                    );
                    continue;
                }
            };

            // Parse messages
            let parsed_messages = parse_dm_events(events, &trade_keys, None).await;

            // Get only the latest message (with the highest timestamp)
            // Index 1 in the tuple is the timestamp
            let latest_message = parsed_messages.into_iter().max_by_key(|msg| msg.1);

            // Check if we have new messages
            let mut messages_lock = messages.lock().unwrap();

            if let Some((message, timestamp, sender)) = latest_message {
                // Only add if it's a new message
                let inner_kind = message.get_inner_message_kind();
                let action = inner_kind.action.clone();
                // Extract invoice and sat_amount from payload based on action type
                // PayInvoice: PaymentRequest payload contains invoice
                // AddInvoice: Order payload contains sat amount
                let (sat_amount, invoice) = match &action {
                    Action::PayInvoice => {
                        // For PayInvoice, extract invoice from PaymentRequest payload
                        match &inner_kind.payload {
                            Some(Payload::PaymentRequest(_, invoice, _)) => {
                                (None, Some(invoice.clone()))
                            }
                            _ => (None, None),
                        }
                    }
                    Action::AddInvoice => {
                        // For AddInvoice, extract sat amount from Order payload
                        match &inner_kind.payload {
                            Some(Payload::Order(order)) => (Some(order.amount), None),
                            _ => (None, None),
                        }
                    }
                    _ => (None, None),
                };
                // Check if this is a new message for this order_id
                // Find the latest message for this order_id (if any exists)
                let existing_message = messages_lock
                    .iter()
                    .filter(|m| m.order_id == Some(*order_id))
                    .max_by_key(|m| m.timestamp);

                // Only increment pending notifications if this is a truly new message
                let is_new_message = match existing_message {
                    None => {
                        // No message exists for this order_id - this is new
                        true
                    }
                    Some(existing) => {
                        // Check if the new message is newer than what we already have
                        // Also check action to avoid counting exact duplicates
                        let existing_action =
                            existing.message.get_inner_message_kind().action.clone();
                        timestamp > existing.timestamp
                            || (timestamp == existing.timestamp && action != existing_action)
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
                    order_id: Some(*order_id),
                    trade_index: *trade_index,
                    read: false, // New messages are unread by default
                    sat_amount,
                    buyer_invoice: invoice.clone(),
                    auto_popup_shown: false,
                };

                // Add to messages list
                messages_lock.push(order_message.clone());
                // Sort by time
                messages_lock.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
                // Remove duplicates with dedup
                messages_lock.dedup_by_key(|a| a.order_id.unwrap());

                // Create notification
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

                let notification = crate::ui::MessageNotification {
                    order_id: Some(*order_id),
                    message_preview: action_str.to_string(),
                    timestamp,
                    action,
                    sat_amount,
                    invoice,
                };

                // Send notification (ignore errors if channel is closed)
                let _ = message_notification_tx.send(notification);
            }
        }
    }
}
