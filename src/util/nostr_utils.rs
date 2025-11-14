// Nostr-related utilities (copied from mostro-cli/src/util/events.rs and adapted for mostrix)
use anyhow::{Error, Result};
use base64::engine::general_purpose;
use base64::Engine;
use mostro_core::prelude::*;
use nip44::v2::{decrypt_to_bytes, ConversationKey};
use nip44::v2::encrypt_to_bytes;
use nostr_sdk::prelude::*;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use uuid::Uuid;

use crate::SETTINGS;
use crate::settings::Settings;

pub const FETCH_EVENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

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
        ListKind::Orders => create_seven_days_filter(Alphabet::Z, "order".to_string(), pubkey),
        _ => Err(anyhow::anyhow!("Unsupported ListKind for mostrix")),
    }
}

/// Parse order from nostr tags (copied from mostro-cli/src/nip33.rs)
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

/// Parse orders from events (copied from mostro-cli/src/parser/orders.rs)
fn parse_orders_events(
    events: Events,
    currency: Option<String>,
    status: Option<Status>,
    kind: Option<mostro_core::order::Kind>,
) -> Vec<SmallOrder> {
    // HashMap to store the latest order by id
    let mut latest_by_id: HashMap<Uuid, SmallOrder> = HashMap::new();

    for event in events.iter() {
        // Get order from tags
        let mut order = match order_from_tags(event.tags.clone()) {
            Ok(o) => o,
            Err(e) => {
                log::error!("{e:?}");
                continue;
            }
        };
        // Get order id
        let order_id = match order.id {
            Some(id) => id,
            None => {
                log::info!("Order ID is none");
                continue;
            }
        };
        // Check if order kind is none
        if order.kind.is_none() {
            log::info!("Order kind is none");
            continue;
        }
        // Set created at
        order.created_at = Some(event.created_at.as_u64() as i64);
        // Update latest order by id
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

    let mut requested: Vec<SmallOrder> = latest_by_id
        .into_values()
        .filter(|o| status.map(|s| o.status == Some(s)).unwrap_or(true))
        .filter(|o| currency.as_ref().map(|c| o.fiat_code == *c).unwrap_or(true))
        .filter(|o| {
            kind.as_ref()
                .map(|k| o.kind.as_ref() == Some(k))
                .unwrap_or(true)
        })
        .collect();

    requested.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    requested
}

/// Fetch events list using the same logic as mostro-cli (adapted for mostrix)
pub async fn fetch_events_list(
    list_kind: ListKind,
    status: Option<Status>,
    currency: Option<String>,
    kind: Option<mostro_core::order::Kind>,
    client: &Client,
    mostro_pubkey: PublicKey,
    _since: Option<&i64>,
) -> Result<Vec<Event>> {
    match list_kind {
        ListKind::Orders => {
            let filters = create_filter(list_kind, mostro_pubkey, None)?;
            let fetched_events = client.fetch_events(filters, FETCH_EVENTS_TIMEOUT).await?;
            let orders = parse_orders_events(fetched_events, currency, status, kind);
            Ok(orders.into_iter().map(Event::SmallOrder).collect())
        }
        _ => Err(anyhow::anyhow!("Unsupported ListKind for mostrix")),
    }
}

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
    // pow is set in the settings
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
        SubscribeAutoCloseOptions::default().exit_policy(ReqExitPolicy::WaitForEventsAfterEOSE(1));
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
        // check if the message is older than the since time if it is, skip it
        if let Some(since_time) = since {
            // Calculate since time from now in minutes subtracting the since time
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

/// Send a new order to Mostro (similar to execute_new_order in mostro-cli)
pub async fn send_new_order(
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    settings: &Settings,
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

    // Wait for Mostro response (subscribes first, then sends message to avoid missing messages)
    let recv_event = wait_for_dm(client, &trade_keys, FETCH_EVENTS_TIMEOUT, async {
        // Send DM inside the future passed to wait_for_dm
        send_dm(client, Some(&identity_keys), &trade_keys, &mostro_pubkey, message_json, None, false).await
    })
    .await?;

    // Parse DM events
    let messages = parse_dm_events(recv_event, &trade_keys, None).await;

    if let Some((response_message, _, _)) = messages.first() {
        let inner_message = response_message.get_inner_message_kind();
        match inner_message.request_id {
            Some(id) => {
                if request_id == id {
                    // Request ID matches, process the response
                    match inner_message.action {
                        mostro_core::prelude::Action::NewOrder => {
                            if let Some(mostro_core::prelude::Payload::Order(order)) =
                                &inner_message.payload
                            {
                                log::info!(
                                    "✅ Order created successfully! Order ID: {:?}",
                                    order.id
                                );

                                // Save order to database
                                if let Err(e) = save_order(
                                    order.clone(),
                                    &trade_keys,
                                    request_id,
                                    next_idx,
                                    pool,
                                )
                                .await
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
                            log::warn!("Received unexpected action: {:?}", inner_message.action);
                            Err(anyhow::anyhow!(
                                "Unexpected action: {:?}",
                                inner_message.action
                            ))
                        }
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
            None if inner_message.action == mostro_core::prelude::Action::RateReceived
                || inner_message.action == mostro_core::prelude::Action::NewOrder =>
            {
                // Some actions don't require request_id matching
                if let Some(mostro_core::prelude::Payload::Order(order)) = &inner_message.payload {
                    // Save order to database
                    if let Err(e) =
                        save_order(order.clone(), &trade_keys, request_id, next_idx, pool).await
                    {
                        log::error!("Failed to save order to database: {}", e);
                        // Continue anyway - we still return success to the UI
                    }

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
            None => {
                log::warn!(
                    "Received response with null request_id. Expected: {}",
                    request_id
                );
                Err(anyhow::anyhow!("Response with null request_id"))
            }
        }
    } else {
        log::error!("No response received from Mostro");
        Err(anyhow::anyhow!("No response received from Mostro"))
    }
}
