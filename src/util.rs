// Copied from mostro-cli/src/util/events.rs and adapted for mostrix
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use uuid::Uuid;
use base64::engine::general_purpose;
use base64::Engine;
use nip44::v2::{decrypt_to_bytes, ConversationKey};

pub const FETCH_EVENTS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Clone, Debug)]
pub enum ListKind {
    Orders,
    Disputes,
    DirectMessagesUser,
    DirectMessagesAdmin,
    PrivateDirectMessagesUser,
}

#[derive(Clone, Debug)]
pub enum Event {
    SmallOrder(SmallOrder),
    Dispute(Dispute),
    MessageTuple(Box<(Message, u64, PublicKey)>),
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

fn create_filter(
    list_kind: ListKind,
    pubkey: PublicKey,
    _since: Option<&i64>,
) -> Result<Filter> {
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
            let fetched_events = client
                .fetch_events(filters, FETCH_EVENTS_TIMEOUT)
                .await?;
            let orders = parse_orders_events(fetched_events, currency, status, kind);
            Ok(orders.into_iter().map(Event::SmallOrder).collect())
        }
        _ => Err(anyhow::anyhow!("Unsupported ListKind for mostrix")),
    }
}

/// Send a direct message (encrypted PDM) to a receiver
pub async fn send_dm(
    client: &Client,
    trade_keys: &Keys,
    receiver_pubkey: &PublicKey,
    payload: String,
) -> Result<()> {
    use nip44::v2::{ConversationKey, encrypt_to_bytes};
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    
    let ck = ConversationKey::derive(trade_keys.secret_key(), receiver_pubkey)
        .map_err(|e| anyhow::anyhow!("Failed to derive conversation key: {}", e))?;
    let encrypted = encrypt_to_bytes(&ck, payload.as_bytes())
        .map_err(|e| anyhow::anyhow!("Failed to encrypt message: {}", e))?;
    let b64 = B64.encode(encrypted);
    
    let event = EventBuilder::new(nostr_sdk::Kind::PrivateDirectMessage, b64)
        .tag(Tag::public_key(*receiver_pubkey))
        .sign_with_keys(trade_keys)?;
    
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
    let opts = SubscribeAutoCloseOptions::default()
        .exit_policy(ReqExitPolicy::WaitForEventsAfterEOSE(1));
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
                        log::warn!(
                            "Could not decrypt gift wrap (event {}): {}",
                            dm.id, e
                        );
                        continue;
                    }
                };
                let (message, _): (Message, Option<String>) =
                    match serde_json::from_str(&unwrapped_gift.rumor.content) {
                        Ok(msg) => msg,
                        Err(e) => {
                            log::warn!(
                                "Could not parse message content (event {}): {}",
                                dm.id, e
                            );
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

