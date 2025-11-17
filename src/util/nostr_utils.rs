// Nostr-related utilities (copied from mostro-cli/src/util/events.rs and adapted for mostrix)
use anyhow::Result;
use base64::engine::general_purpose;
use base64::Engine;
use mostro_core::prelude::*;
use nip44::v2::{decrypt_to_bytes, ConversationKey};
use nostr_sdk::prelude::*;
use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration as StdDuration;
use uuid::Uuid;

use crate::settings::Settings;

pub const FETCH_EVENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Convert CantDoReason to user-friendly description
fn get_cant_do_description(reason: &CantDoReason) -> String {
    match reason {
        CantDoReason::InvalidSignature => "Invalid signature - authentication failed".to_string(),
        CantDoReason::InvalidTradeIndex => "Invalid trade index - please try again".to_string(),
        CantDoReason::InvalidAmount => "Invalid amount - check your order values".to_string(),
        CantDoReason::InvalidInvoice => {
            "Invalid invoice - please provide a valid lightning invoice".to_string()
        }
        CantDoReason::InvalidPaymentRequest => "Invalid payment request".to_string(),
        CantDoReason::InvalidPeer => "Invalid peer information".to_string(),
        CantDoReason::InvalidRating => "Invalid rating value".to_string(),
        CantDoReason::InvalidTextMessage => "Invalid text message".to_string(),
        CantDoReason::InvalidOrderKind => {
            "Invalid order kind - must be 'buy' or 'sell'".to_string()
        }
        CantDoReason::InvalidOrderStatus => "Invalid order status".to_string(),
        CantDoReason::InvalidPubkey => "Invalid public key".to_string(),
        CantDoReason::InvalidParameters => {
            "Invalid parameters - check your order details".to_string()
        }
        CantDoReason::OrderAlreadyCanceled => "Order is already canceled".to_string(),
        CantDoReason::CantCreateUser => "Cannot create user - please contact support".to_string(),
        CantDoReason::IsNotYourOrder => "This is not your order".to_string(),
        CantDoReason::NotAllowedByStatus => {
            "Action not allowed - order status prevents this operation".to_string()
        }
        CantDoReason::OutOfRangeFiatAmount => "Fiat amount is out of acceptable range".to_string(),
        CantDoReason::OutOfRangeSatsAmount => {
            "Satoshis amount is out of acceptable range".to_string()
        }
        CantDoReason::IsNotYourDispute => "This is not your dispute".to_string(),
        CantDoReason::DisputeTakenByAdmin => {
            "Dispute has been taken over by an administrator".to_string()
        }
        CantDoReason::DisputeCreationError => "Cannot create dispute for this order".to_string(),
        CantDoReason::NotFound => "Resource not found".to_string(),
        CantDoReason::InvalidDisputeStatus => "Invalid dispute status".to_string(),
        CantDoReason::InvalidAction => "Invalid action for current state".to_string(),
        CantDoReason::PendingOrderExists => {
            "You already have a pending order - please complete or cancel it first".to_string()
        }
        CantDoReason::InvalidFiatCurrency => {
            "Invalid fiat currency - currency not supported or specify a fixed rate".to_string()
        }
        CantDoReason::TooManyRequests => {
            "Too many requests - please wait and try again".to_string()
        }
    }
}

#[derive(Clone, Debug)]
pub enum ListKind {
    Orders,
    Disputes,
    DirectMessagesUser,
    DirectMessagesAdmin,
    PrivateDirectMessagesUser,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum MessageType {
    PrivateDirectMessage,
    PrivateGiftWrap,
    SignedGiftWrap,
}

#[derive(Clone, Debug)]
pub enum Event {
    SmallOrder(SmallOrder),
    Dispute(Dispute),
    MessageTuple(Box<(Message, u64, PublicKey)>),
}

fn create_expiration_tags(expiration: Option<Timestamp>) -> Tags {
    let mut tags: Vec<Tag> = Vec::with_capacity(1 + usize::from(expiration.is_some()));
    if let Some(timestamp) = expiration {
        tags.push(Tag::expiration(timestamp));
    }
    Tags::from_list(tags)
}

fn create_seven_days_filter(letter: Alphabet, value: String, pubkey: PublicKey) -> Result<Filter> {
    let since_time = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(7))
        .ok_or(anyhow::anyhow!("Failed to get since days ago"))?
        .timestamp() as u64;
    let timestamp = Timestamp::from(since_time);
    Ok(Filter::new()
        .author(pubkey)
        .limit(50)
        .since(timestamp)
        .custom_tag(SingleLetterTag::lowercase(letter), value)
        .kind(nostr_sdk::Kind::Custom(NOSTR_REPLACEABLE_EVENT_KIND)))
}

fn create_filter(list_kind: ListKind, pubkey: PublicKey, _since: Option<&i64>) -> Result<Filter> {
    match list_kind {
        ListKind::Orders => {
            // Use "order" tag (letter Z) like mostro-cli, not status
            let letter = Alphabet::Z;
            let value = "order".to_string();
            create_seven_days_filter(letter, value, pubkey)
        }
        ListKind::Disputes => {
            let letter = Alphabet::Y;
            let value = "dispute".to_string();
            create_seven_days_filter(letter, value, pubkey)
        }
        ListKind::DirectMessagesUser => Ok(Filter::new()
            .pubkey(pubkey)
            .kind(nostr_sdk::Kind::GiftWrap)
            .limit(20)),
        ListKind::DirectMessagesAdmin => Ok(Filter::new()
            .pubkey(pubkey)
            .kind(nostr_sdk::Kind::GiftWrap)
            .limit(20)),
        ListKind::PrivateDirectMessagesUser => Ok(Filter::new()
            .pubkey(pubkey)
            .kind(nostr_sdk::Kind::PrivateDirectMessage)
            .limit(20)),
    }
}

pub async fn send_gift_wrap_dm(
    client: &Client,
    trade_keys: &Keys,
    receiver_pubkey: &PublicKey,
    message_json: String,
) -> Result<()> {
    let rumor = EventBuilder::text_note(message_json).build(trade_keys.public_key());
    let event = EventBuilder::gift_wrap(trade_keys, receiver_pubkey, rumor, Tags::new()).await?;
    client.send_event(&event).await?;
    Ok(())
}

async fn create_gift_wrap_event(
    trade_keys: &Keys,
    identity_keys: Option<&Keys>,
    receiver_pubkey: &PublicKey,
    payload: String,
    pow: u8,
    expiration: Option<Timestamp>,
    signed: bool,
) -> Result<nostr_sdk::Event> {
    // Parse the message from JSON
    let message = Message::from_json(&payload)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize message: {e}"))?;

    // Format as (Message, Option<Signature>) tuple
    let content = if signed {
        // Sign the message using trade_keys
        let sig = Message::sign(payload, trade_keys);
        serde_json::to_string(&(message, sig))
            .map_err(|e| anyhow::anyhow!("Failed to serialize message: {e}"))?
    } else {
        // For unsigned messages, use (Message, None)
        let content: (Message, Option<Signature>) = (message, None);
        serde_json::to_string(&content)
            .map_err(|e| anyhow::anyhow!("Failed to serialize message: {e}"))?
    };

    // Create the rumor with the properly formatted content
    let rumor = EventBuilder::text_note(content)
        .pow(pow)
        .build(trade_keys.public_key());

    // Create expiration tags if needed
    let mut tags = Tags::new();
    if let Some(timestamp) = expiration {
        tags.push(Tag::expiration(timestamp));
    }

    // Determine signer keys
    let signer_keys = if signed {
        identity_keys
            .ok_or_else(|| anyhow::anyhow!("identity_keys required for signed messages"))?
    } else {
        trade_keys
    };

    Ok(EventBuilder::gift_wrap(signer_keys, receiver_pubkey, rumor, tags).await?)
}

fn determine_message_type(to_user: bool, private: bool) -> MessageType {
    match (to_user, private) {
        (true, _) => MessageType::PrivateDirectMessage,
        (false, true) => MessageType::PrivateGiftWrap,
        (false, false) => MessageType::SignedGiftWrap,
    }
}

pub async fn send_dm(
    client: &Client,
    identity_keys: Option<&Keys>,
    trade_keys: &Keys,
    receiver_pubkey: &PublicKey,
    payload: String,
    expiration: Option<Timestamp>,
    to_user: bool,
) -> Result<()> {
    let pow: u8 = 0; // Default POW, can be made configurable later
    let private = false; // Default to signed gift wrap for Mostro communication

    let message_type = determine_message_type(to_user, private);

    let event = match message_type {
        MessageType::PrivateDirectMessage => {
            // For private DMs, we'd need to implement NIP-44 encryption
            // For now, we'll use gift wrap for Mostro communication
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

/// Wait for a direct message response from Mostro (adapted from mostro-cli)
pub async fn wait_for_dm(
    client: &Client,
    trade_keys: &Keys,
    request_id: u64,
    send_future: impl std::future::Future<Output = Result<()>> + Send + 'static,
) -> Result<Message> {
    // Subscribe to gift wrap events - ONLY NEW ONES WITH LIMIT 0
    let subscription = Filter::new()
        .pubkey(trade_keys.public_key())
        .kind(nostr_sdk::Kind::GiftWrap)
        .limit(0);

    let opts = SubscribeAutoCloseOptions::default().exit_policy(ReqExitPolicy::WaitForEvents(1));
    client.subscribe(subscription, Some(opts)).await?;

    // Spawn a task to send the DM
    tokio::spawn(async move {
        let _ = send_future.await;
    });

    // Wait for incoming gift wraps using notifications
    let mut notifications = client.notifications();

    match tokio::time::timeout(FETCH_EVENTS_TIMEOUT, async move {
        while let Ok(notification) = notifications.recv().await {
            if let RelayPoolNotification::Event { event, .. } = notification {
                if event.kind == nostr_sdk::Kind::GiftWrap {
                    let gift = match nip59::extract_rumor(trade_keys, &event).await {
                        Ok(gift) => gift,
                        Err(e) => {
                            log::warn!("Failed to extract rumor: {}", e);
                            continue;
                        }
                    };
                    let (message, _): (Message, Option<String>) =
                        match serde_json::from_str(&gift.rumor.content) {
                            Ok(msg) => msg,
                            Err(e) => {
                                log::warn!("Failed to deserialize message: {}", e);
                                continue;
                            }
                        };
                    let inner_message = message.get_inner_message_kind();
                    if inner_message.request_id == Some(request_id) {
                        return Ok(message);
                    }
                }
            }
        }
        Err(anyhow::anyhow!("No matching message found"))
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!("Timeout waiting for DM or gift wrap event")),
    }
}

/// Parse order from NIP-33 event tags (from mostro-cli)
fn order_from_tags(tags: Tags) -> Result<SmallOrder> {
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

pub async fn fetch_events_list(
    list_kind: ListKind,
    status: Option<Status>,
    order_id: Option<Uuid>,
    dispute_id: Option<Uuid>,
    client: &Client,
    pubkey: PublicKey,
    _since: Option<&i64>,
) -> Result<Vec<Event>> {
    let filter = create_filter(list_kind.clone(), pubkey, None)?;
    let events = client
        .fetch_events(filter, StdDuration::from_secs(10))
        .await?;

    // Parse orders from NIP-33 event tags (like mostro-cli)
    let events = match list_kind {
        ListKind::Orders => {
            use std::collections::HashMap;
            // HashMap to store the latest order by id (deduplicate replaceable events)
            let mut latest_by_id: HashMap<Uuid, SmallOrder> = HashMap::new();

            for event in events.iter() {
                // Parse order from tags
                let mut order = match order_from_tags(event.tags.clone()) {
                    Ok(o) => o,
                    Err(e) => {
                        log::warn!("Failed to parse order from tags: {}", e);
                        continue;
                    }
                };

                // Get order id
                let order_id_from_event = match order.id {
                    Some(id) => id,
                    None => {
                        log::warn!("Order ID is none, skipping");
                        continue;
                    }
                };

                // Check if order kind is none
                if order.kind.is_none() {
                    log::warn!("Order kind is none, skipping");
                    continue;
                }

                // Set created_at from event timestamp
                order.created_at = Some(event.created_at.as_u64() as i64);

                // Update latest order by id (keep the most recent version)
                latest_by_id
                    .entry(order_id_from_event)
                    .and_modify(|existing| {
                        let new_ts = order.created_at.unwrap_or(0);
                        let old_ts = existing.created_at.unwrap_or(0);
                        if new_ts > old_ts {
                            *existing = order.clone();
                        }
                    })
                    .or_insert(order);
            }

            // Convert to Event::SmallOrder and apply filters
            latest_by_id
                .into_values()
                .filter(|o| status.map(|s| o.status == Some(s)).unwrap_or(true))
                .filter(|o| {
                    order_id
                        .as_ref()
                        .map(|oid| o.id.as_ref() == Some(oid))
                        .unwrap_or(true)
                })
                .map(Event::SmallOrder)
                .collect::<Vec<Event>>()
        }
        ListKind::Disputes => events
            .into_iter()
            .filter_map(|event| {
                // Try to parse dispute from JSON content (disputes might use different format)
                let dispute: Dispute = serde_json::from_str(&event.content).ok()?;
                if let Some(ref did) = dispute_id {
                    if dispute.id != *did {
                        return None;
                    }
                }
                Some(Event::Dispute(dispute))
            })
            .collect::<Vec<Event>>(),
        _ => Vec::new(),
    };

    Ok(events)
}

pub mod db_utils {
    use super::*;
    use crate::models::{Order, User};
    use sqlx::SqlitePool;

    pub async fn save_order(
        order: SmallOrder,
        trade_keys: &Keys,
        _request_id: u64,
        trade_index: i64,
        pool: &SqlitePool,
    ) -> Result<()> {
        User::update_last_trade_index(pool, trade_index).await?;
        Order::new(pool, order, trade_keys, None).await?;

        Ok(())
    }
}

/// Parse DM events to extract Messages (similar to mostro-cli)
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

        // Time-based filtering (only process messages from last 30 minutes)
        if let Some(since_timestamp) = since {
            if created_at.as_u64() < (*since_timestamp as u64) {
                continue;
            }
        } else {
            let since_time =
                match chrono::Utc::now().checked_sub_signed(chrono::Duration::minutes(30)) {
                    Some(dt) => dt.timestamp(),
                    None => {
                        log::warn!("Error: Unable to calculate time 30 minutes ago");
                        continue;
                    }
                };
            if (created_at.as_u64() as i64) < since_time {
                continue;
            }
        }

        direct_messages.push((message, created_at.as_u64(), sender));
    }
    direct_messages.sort_by(|a, b| a.1.cmp(&b.1));
    direct_messages
}

/// Send a new order to Mostro (similar to execute_new_order in mostro-cli)
pub async fn send_new_order(
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    _settings: &Settings,
    mostro_pubkey: PublicKey,
    form: &crate::ui::FormState,
) -> Result<crate::ui::OrderResult, anyhow::Error> {
    use crate::models::User;
    use crate::util::db_utils::save_order;
    use std::collections::HashMap;

    // Parse form data
    let kind_str = if form.kind.trim().is_empty() {
        "buy".to_string()
    } else {
        form.kind.trim().to_lowercase()
    };
    let fiat_code = if form.fiat_code.trim().is_empty() {
        "USD".to_string()
    } else {
        form.fiat_code.trim().to_uppercase()
    };

    let amount: i64 = form.amount.trim().parse().unwrap_or(0);

    // Check if fiat currency is available on Yadio if amount is 0
    if amount == 0 {
        let api_req_string = "https://api.yadio.io/currencies".to_string();
        let fiat_list_check = reqwest::get(api_req_string)
            .await?
            .json::<HashMap<String, String>>()
            .await?
            .contains_key(&fiat_code);
        if !fiat_list_check {
            return Err(anyhow::anyhow!("{} is not present in the fiat market, please specify an amount with -a flag to fix the rate", fiat_code));
        }
    }

    let kind_checked = mostro_core::order::Kind::from_str(&kind_str)
        .map_err(|_| anyhow::anyhow!("Invalid order kind"))?;

    let expiration_days: i64 = form.expiration_days.trim().parse().unwrap_or(0);
    let expires_at = match expiration_days {
        0 => None,
        _ => {
            let now = chrono::Utc::now();
            let expires_at = now + chrono::Duration::days(expiration_days);
            Some(expires_at.timestamp())
        }
    };

    // Handle fiat amount (single or range)
    let (fiat_amount, min_amount, max_amount) =
        if form.use_range && !form.fiat_amount_max.trim().is_empty() {
            let min: i64 = form.fiat_amount.trim().parse().unwrap_or(0);
            let max: i64 = form.fiat_amount_max.trim().parse().unwrap_or(0);
            (0, Some(min), Some(max))
        } else {
            let fiat: i64 = form.fiat_amount.trim().parse().unwrap_or(0);
            (fiat, None, None)
        };

    let payment_method = form.payment_method.trim().to_string();
    let premium: i64 = form.premium.trim().parse().unwrap_or(0);
    let invoice = if form.invoice.trim().is_empty() {
        None
    } else {
        Some(form.invoice.trim().to_string())
    };

    // Get user and trade keys
    let user = User::get(pool).await?;
    let next_idx = user.last_trade_index.unwrap_or(1) + 1;
    let trade_keys = user.derive_trade_keys(next_idx)?;
    let _ = User::update_last_trade_index(pool, next_idx).await;

    // Create SmallOrder
    let small_order = mostro_core::prelude::SmallOrder::new(
        None,
        Some(kind_checked),
        Some(mostro_core::prelude::Status::Pending),
        amount,
        fiat_code.clone(),
        min_amount,
        max_amount,
        fiat_amount,
        payment_method.clone(),
        premium,
        None,
        None,
        invoice.clone(),
        Some(0),
        expires_at,
    );

    // Create message
    let request_id = uuid::Uuid::new_v4().as_u128() as u64;
    let order_content = mostro_core::prelude::Payload::Order(small_order);
    let message = mostro_core::prelude::Message::new_order(
        None,
        Some(request_id),
        Some(next_idx),
        mostro_core::prelude::Action::NewOrder,
        Some(order_content),
    );

    // Serialize message
    let message_json = message
        .as_json()
        .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    log::info!(
        "Sending new order via DM with trade index {} and request_id {}",
        next_idx,
        request_id
    );

    let identity_keys = User::get_identity_keys(pool).await?;

    // Clone values for the async future
    let client_clone = client.clone();
    let identity_keys_clone = identity_keys.clone();
    let trade_keys_clone = trade_keys.clone();
    let mostro_pubkey_clone = mostro_pubkey;
    let message_json_clone = message_json.clone();

    let new_order_messge = async move {
        send_dm(
            &client_clone,
            Some(&identity_keys_clone),
            &trade_keys_clone,
            &mostro_pubkey_clone,
            message_json_clone,
            None,
            false,
        )
        .await
    };

    // Wait for Mostro response (subscribes first, then sends message to avoid missing messages)
    let message = wait_for_dm(client, &trade_keys, request_id, new_order_messge).await?;

    let inner_message = message.get_inner_message_kind();

    // Check for CantDo payload first (error response)
    if let Some(Payload::CantDo(reason)) = &inner_message.payload {
        let error_msg = match reason {
            Some(r) => get_cant_do_description(r),
            None => "Unknown error - Mostro couldn't process your request".to_string(),
        };
        log::error!("Received CantDo error: {}", error_msg);
        return Err(anyhow::anyhow!(error_msg));
    }

    // Process the response based on action
    match inner_message.action {
        Action::NewOrder => {
            if let Some(Payload::Order(order)) = &inner_message.payload {
                log::info!("✅ Order created successfully! Order ID: {:?}", order.id);

                // Save order to database
                if let Err(e) =
                    save_order(order.clone(), &trade_keys, request_id, next_idx, pool).await
                {
                    log::error!("Failed to save order to database: {}", e);
                    // Continue anyway - we still return success to the UI
                }

                // Return success with order details
                Ok(crate::ui::OrderResult::Success {
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
                })
            } else {
                // Response without order details - return what we sent
                log::warn!("Received NewOrder action but no order payload");
                Ok(crate::ui::OrderResult::Success {
                    order_id: None,
                    kind: Some(kind_checked),
                    amount,
                    fiat_code: fiat_code.clone(),
                    fiat_amount,
                    min_amount,
                    max_amount,
                    payment_method: payment_method.clone(),
                    premium,
                    status: Some(mostro_core::prelude::Status::Pending),
                })
            }
        }
        _ => {
            log::warn!(
                "Received unexpected action: {:?}, payload: {:?}",
                inner_message.action,
                inner_message.payload
            );
            Err(anyhow::anyhow!(
                "Unexpected action: {:?}",
                inner_message.action
            ))
        }
    }
}

/// Take an order from the order book (similar to execute_take_order in mostro-cli)
pub async fn take_order(
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    _settings: &Settings,
    mostro_pubkey: PublicKey,
    order: &SmallOrder,
    amount: Option<i64>,
    invoice: Option<String>,
) -> Result<crate::ui::OrderResult, anyhow::Error> {
    use crate::models::User;
    use crate::util::db_utils::save_order;

    // Determine action based on order kind
    let (action, payload) = match order.kind {
        Some(mostro_core::order::Kind::Buy) => {
            // Taking a Buy order = Selling (need invoice)
            let inv =
                invoice.ok_or_else(|| anyhow::anyhow!("Invoice required for taking buy orders"))?;
            let payload = if let Some(amt) = amount {
                Payload::PaymentRequest(None, inv, Some(amt))
            } else {
                Payload::PaymentRequest(None, inv, None)
            };
            (Action::TakeSell, Some(payload))
        }
        Some(mostro_core::order::Kind::Sell) => {
            // Taking a Sell order = Buying (provide amount if range)
            let payload = amount.map(Payload::Amount);
            (Action::TakeBuy, payload)
        }
        None => {
            return Err(anyhow::anyhow!("Order kind is not specified"));
        }
    };

    let order_id = order
        .id
        .ok_or_else(|| anyhow::anyhow!("Order ID is missing"))?;

    // Get user and trade keys
    let user = User::get(pool).await?;
    let next_idx = user.last_trade_index.unwrap_or(1) + 1;
    let trade_keys = user.derive_trade_keys(next_idx)?;
    let _ = User::update_last_trade_index(pool, next_idx).await;

    // Create message
    let request_id = uuid::Uuid::new_v4().as_u128() as u64;
    let take_message = Message::new_order(
        Some(order_id),
        Some(request_id),
        Some(next_idx),
        action,
        payload,
    );

    // Serialize message
    let message_json = take_message
        .as_json()
        .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    log::info!(
        "Taking order {} with trade index {} and request_id {}",
        order_id,
        next_idx,
        request_id
    );

    let identity_keys = User::get_identity_keys(pool).await?;

    // Clone values for the async future
    let client_clone = client.clone();
    let identity_keys_clone = identity_keys.clone();
    let trade_keys_clone = trade_keys.clone();
    let mostro_pubkey_clone = mostro_pubkey;
    let message_json_clone = message_json.clone();

    let take_order_msg = async move {
        send_dm(
            &client_clone,
            Some(&identity_keys_clone),
            &trade_keys_clone,
            &mostro_pubkey_clone,
            message_json_clone,
            None,
            false,
        )
        .await
    };

    // Wait for Mostro response
    let message = wait_for_dm(client, &trade_keys, request_id, take_order_msg).await?;

    let inner_message = message.get_inner_message_kind();

    // Check for CantDo payload first (error response)
    if let Some(Payload::CantDo(reason)) = &inner_message.payload {
        let error_msg = match reason {
            Some(r) => get_cant_do_description(r),
            None => "Unknown error - Mostro couldn't process your request".to_string(),
        };
        log::error!("Received CantDo error: {}", error_msg);
        return Err(anyhow::anyhow!(error_msg));
    }

    match inner_message.request_id {
        Some(id) => {
            if request_id == id {
                // Request ID matches, process the response
                if let Some(Payload::Order(returned_order)) = &inner_message.payload {
                    log::info!(
                        "✅ Order taken successfully! Order ID: {:?}",
                        returned_order.id
                    );

                    // Save order to database
                    if let Err(e) = save_order(
                        returned_order.clone(),
                        &trade_keys,
                        request_id,
                        next_idx,
                        pool,
                    )
                    .await
                    {
                        log::error!("Failed to save order to database: {}", e);
                    }

                    // Return success with order details
                    Ok(crate::ui::OrderResult::Success {
                        order_id: returned_order.id,
                        kind: returned_order.kind,
                        amount: returned_order.amount,
                        fiat_code: returned_order.fiat_code.clone(),
                        fiat_amount: returned_order.fiat_amount,
                        min_amount: returned_order.min_amount,
                        max_amount: returned_order.max_amount,
                        payment_method: returned_order.payment_method.clone(),
                        premium: returned_order.premium,
                        status: returned_order.status,
                    })
                } else {
                    Ok(crate::ui::OrderResult::Success {
                        order_id: Some(order_id),
                        kind: order.kind,
                        amount: order.amount,
                        fiat_code: order.fiat_code.clone(),
                        fiat_amount: order.fiat_amount,
                        min_amount: order.min_amount,
                        max_amount: order.max_amount,
                        payment_method: order.payment_method.clone(),
                        premium: order.premium,
                        status: Some(mostro_core::prelude::Status::Active),
                    })
                }
            } else {
                log::warn!(
                    "Received response with mismatched request_id. Expected: {}, Got: {}",
                    request_id,
                    id
                );
                Err(anyhow::anyhow!("Mismatched request_id"))
            }
        }
        None => {
            log::warn!(
                "Received response with null request_id. Expected: {}",
                request_id
            );
            Err(anyhow::anyhow!("Response with null request_id"))
        }
    }
}
