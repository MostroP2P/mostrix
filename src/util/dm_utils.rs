// Direct message utilities for Nostr
use anyhow::{Error, Result};
use base64::engine::general_purpose;
use base64::Engine;
use mostro_core::prelude::*;
use nip44::v2::encrypt_to_bytes;
use nip44::v2::{decrypt_to_bytes, ConversationKey};
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::util::types::{create_expiration_tags, determine_message_type, MessageType};
use crate::SETTINGS;

pub const FETCH_EVENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Create a private direct message event
async fn create_private_dm_event(
    trade_keys: &Keys,
    receiver_pubkey: &PublicKey,
    payload: String,
    pow: u8,
) -> Result<nostr_sdk::Event> {
    let ck = ConversationKey::derive(trade_keys.secret_key(), receiver_pubkey)?;
    let encrypted_content = encrypt_to_bytes(&ck, payload.as_bytes())?;
    let b64decoded_content = general_purpose::STANDARD.encode(encrypted_content);
    Ok(
        EventBuilder::new(nostr_sdk::Kind::PrivateDirectMessage, b64decoded_content)
            .pow(pow)
            .tag(Tag::public_key(*receiver_pubkey))
            .sign_with_keys(trade_keys)?,
    )
}

/// Create a gift wrap event (private or signed)
async fn create_gift_wrap_event(
    trade_keys: &Keys,
    identity_keys: Option<&Keys>,
    receiver_pubkey: &PublicKey,
    payload: String,
    pow: u8,
    expiration: Option<Timestamp>,
    signed: bool,
) -> Result<nostr_sdk::Event> {
    let message = Message::from_json(&payload)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize message: {e}"))?;

    let content = if signed {
        let _identity_keys = identity_keys
            .ok_or_else(|| Error::msg("identity_keys required for signed messages"))?;
        let sig = Message::sign(payload, trade_keys);
        serde_json::to_string(&(message, sig))
            .map_err(|e| anyhow::anyhow!("Failed to serialize message: {e}"))?
    } else {
        let content: (Message, Option<Signature>) = (message, None);
        serde_json::to_string(&content)
            .map_err(|e| anyhow::anyhow!("Failed to serialize message: {e}"))?
    };

    let rumor = EventBuilder::text_note(content)
        .pow(pow)
        .build(trade_keys.public_key());

    let tags = create_expiration_tags(expiration);

    let signer_keys = if signed {
        identity_keys.ok_or_else(|| Error::msg("identity_keys required for signed messages"))?
    } else {
        trade_keys
    };

    Ok(EventBuilder::gift_wrap(signer_keys, receiver_pubkey, rumor, tags).await?)
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
    let pow: u8 = SETTINGS.get().unwrap().pow;
    let message_type = determine_message_type(to_user, false);

    let event = match message_type {
        MessageType::PrivateDirectMessage => {
            create_private_dm_event(trade_keys, receiver_pubkey, payload, pow).await?
        }
        MessageType::PrivateGiftWrap => {
            create_gift_wrap_event(
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
            create_gift_wrap_event(
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
) {
    use crate::models::User;

    let mut refresh_interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

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
            let user = match User::get(&pool).await {
                Ok(u) => u,
                Err(e) => {
                    log::error!("Failed to get user: {}", e);
                    continue;
                }
            };

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
                .limit(10);

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

            // Check if we have new messages
            let mut messages_lock = messages.lock().unwrap();
            let existing_timestamps: HashSet<u64> = messages_lock
                .iter()
                .filter(|m| m.order_id == Some(*order_id))
                .map(|m| m.timestamp)
                .collect();

            for (message, timestamp, sender) in parsed_messages {
                // Only add if it's a new message
                if !existing_timestamps.contains(&timestamp) {
                    let inner_kind = message.get_inner_message_kind();
                    let action = inner_kind.action.clone();

                    // Extract invoice and sat_amount from payload based on action type
                    // PayInvoice: PaymentRequest payload contains invoice
                    // AddInvoice: Order payload contains sat amount
                    let (sat_amount, buyer_invoice) = match &action {
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

                    let order_message = crate::ui::OrderMessage {
                        message: message.clone(),
                        timestamp,
                        sender,
                        order_id: Some(*order_id),
                        trade_index: *trade_index,
                        read: false, // New messages are unread by default
                        sat_amount,
                        buyer_invoice: buyer_invoice.clone(),
                    };

                    // Add to messages list
                    messages_lock.push(order_message.clone());

                    // Create notification
                    let action_str = match &action {
                        mostro_core::prelude::Action::AddInvoice => "Invoice Request",
                        mostro_core::prelude::Action::PayInvoice => "Payment Request",
                        mostro_core::prelude::Action::FiatSent => "Fiat Sent",
                        mostro_core::prelude::Action::FiatSentOk => "Fiat Received",
                        mostro_core::prelude::Action::Release
                        | mostro_core::prelude::Action::Released => "Release",
                        mostro_core::prelude::Action::Dispute
                        | mostro_core::prelude::Action::DisputeInitiatedByYou => "Dispute",
                        _ => "New Message",
                    };

                    let notification = crate::ui::MessageNotification {
                        order_id: Some(*order_id),
                        message_preview: action_str.to_string(),
                        timestamp,
                        action,
                        sat_amount,
                        buyer_invoice,
                    };

                    // Send notification (ignore errors if channel is closed)
                    let _ = message_notification_tx.send(notification);
                }
            }
        }
    }
}
