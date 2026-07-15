use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Result;
use mostro_core::chat::{chat_filter, unwrap_chat_message, wrap_chat_message, SharedKey};
use mostro_core::prelude::DisputeStatus;
use mostro_core::prelude::SmallOrder;
use nostr_sdk::prelude::*;

use crate::models::{AdminDispute, Order};
use crate::ui::{AdminChatLastSeen, AdminChatUpdate, ChatParty, ChatSender, DisputeChatMessage};
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use crate::util::mostro_info::MostroInstanceInfo;

/// Messages grouped by (dispute_id, party); value is (content, timestamp, sender_pubkey).
type AdminChatByKey = HashMap<(String, ChatParty), Vec<(String, i64, PublicKey)>>;

// ---------------------------------------------------------------------------
// Shared-key helpers (ECDH derivation, hex conversion)
// ---------------------------------------------------------------------------

/// Derive the per-channel shared key (ECDH) for a chat counterparty.
///
/// This is **not** a normal identity/trade key: it is the 32-byte ECDH output
/// between the local secret key and the counterparty pubkey (see
/// `mostro_core::chat::SharedKey`). Both peers derive the same value, and the
/// corresponding **shared pubkey** is used as the GiftWrap `p` tag and recipient.
///
/// Mostrix keeps the return type as `Keys` to avoid churn in existing call sites.
///
/// Returns `None` if either argument is missing or derivation fails.
pub fn derive_shared_keys(
    admin_keys: Option<&Keys>,
    counterparty_pubkey: Option<&PublicKey>,
) -> Option<Keys> {
    let admin = admin_keys?;
    let cp_pk = counterparty_pubkey?;
    SharedKey::derive(admin.secret_key(), cp_pk)
        .ok()
        .map(|shared| shared.keys().clone())
}

/// Derive a shared key and return its secret as hex for DB persistence.
///
/// The persisted hex represents the **shared secret**, not a user’s secret key.
/// It round-trips via `keys_from_shared_hex`.
///
/// Returns `None` when derivation is not possible.
pub fn derive_shared_key_hex(
    admin_keys: Option<&Keys>,
    counterparty_pubkey_str: Option<&str>,
) -> Option<String> {
    let cp_pk = counterparty_pubkey_str.and_then(|s| PublicKey::parse(s).ok());
    let admin = admin_keys?;
    let cp_pk = cp_pk.as_ref()?;
    SharedKey::derive(admin.secret_key(), cp_pk)
        .ok()
        .map(|shared| shared.to_hex())
}

/// Rebuild a `Keys` from a stored shared-key hex string.
///
/// The persisted hex is the `SharedKey` secret (ECDH output), not a normal
/// user/trade secret key.
pub fn keys_from_shared_hex(hex: &str) -> Option<Keys> {
    SharedKey::from_hex(hex)
        .ok()
        .map(|shared| shared.keys().clone())
}

/// 32-byte ChaCha20 key for decrypting order-chat attachments (shared ECDH secret).
pub fn order_chat_decryption_key_bytes(order: &Order) -> Option<Vec<u8>> {
    if let Some(hex) = order.order_chat_shared_key_hex.as_deref() {
        if let Some(keys) = keys_from_shared_hex(hex) {
            return Some(keys.secret_key().secret_bytes().to_vec());
        }
    }
    let trade_keys_hex = order.trade_keys.as_deref()?;
    let trade_sk = SecretKey::from_str(trade_keys_hex).ok()?;
    let trade_keys = Keys::new(trade_sk);
    let cp = order.counterparty_pubkey.as_deref()?;
    let cp_pk = PublicKey::parse(cp).ok()?;
    derive_shared_keys(Some(&trade_keys), Some(&cp_pk))
        .map(|k| k.secret_key().secret_bytes().to_vec())
}

/// Resolve the order-chat counterparty pubkey and the shared-key hex used for chat GiftWraps.
///
/// This is only possible once `SmallOrder` includes both `buyer_trade_pubkey` and
/// `seller_trade_pubkey`, and the local `trade_keys` matches one of them.
pub fn order_chat_counterparty_and_shared_hex(
    trade_keys: &Keys,
    small_order: &SmallOrder,
) -> Option<(String, String)> {
    let buyer_s = small_order.buyer_trade_pubkey.as_deref()?;
    let seller_s = small_order.seller_trade_pubkey.as_deref()?;
    if buyer_s.is_empty() || seller_s.is_empty() {
        return None;
    }
    let my_pk = trade_keys.public_key();
    let buyer_pk = PublicKey::parse(buyer_s).ok()?;
    let seller_pk = PublicKey::parse(seller_s).ok()?;
    let counterparty_str = if my_pk == buyer_pk {
        seller_s.to_string()
    } else if my_pk == seller_pk {
        buyer_s.to_string()
    } else {
        log::warn!(
            "Order chat: trade key pubkey {} matches neither buyer nor seller trade pubkey for order {:?}",
            my_pk,
            small_order.id
        );
        return None;
    };
    let shared_hex = derive_shared_key_hex(Some(trade_keys), Some(counterparty_str.as_str()))?;
    Some((counterparty_str, shared_hex))
}

/// Send one admin dispute chat message via the per-dispute shared key.
///
/// The GiftWrap is addressed to the **shared pubkey** (`SharedKey.public_key()`),
/// allowing both sides (admin and counterparty) to fetch the same events and
/// decrypt them by deriving/rebuilding the same shared secret.
///
/// `shared_keys` is the `Keys` instance rebuilt from the stored shared-key hex.
pub async fn send_admin_chat_message_via_shared_key(
    client: &Client,
    admin_keys: &Keys,
    shared_keys: &Keys,
    content: &str,
    _mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<()> {
    let content = content.trim();
    if content.is_empty() {
        return Err(anyhow::anyhow!("Cannot send empty admin chat message"));
    }
    let shared_pubkey = shared_keys.public_key();
    let event = wrap_chat_message(admin_keys, &shared_pubkey, content)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to wrap admin chat message: {e}"))?;
    // Send the event to the relay
    client
        .send_event(&event)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send admin chat event: {e}"))?;
    Ok(())
}

/// Unwrap a Mostro P2P chat GiftWrap addressed to a shared key.
///
/// Uses `mostro_core::chat::unwrap_chat_message`, which decrypts and verifies the
/// inner kind-1 signature so the returned sender pubkey can be trusted.
pub async fn unwrap_giftwrap_with_shared_key(
    shared_keys: &Keys,
    event: &Event,
) -> Result<(String, i64, PublicKey)> {
    let msg = unwrap_chat_message(shared_keys, event)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to unwrap chat gift wrap: {e}"))?;
    Ok((msg.content, msg.created_at.as_secs() as i64, msg.sender))
}

/// Fetch recent chat GiftWrap events for a shared key and return decoded messages.
///
/// This uses a wide (7 day) lookback for resiliency on restart / relay lag, and
/// then unwraps each event with `unwrap_chat_message`.
pub async fn fetch_gift_wraps_for_shared_key(
    client: &Client,
    shared_keys: &Keys,
) -> Result<Vec<(String, i64, PublicKey)>> {
    let now = Timestamp::now().as_secs();
    let seven_days_secs: u64 = 7 * 24 * 60 * 60;
    let wide_since = now.saturating_sub(seven_days_secs);

    let shared_pubkey = shared_keys.public_key();
    let filter = chat_filter(shared_pubkey)
        .since(Timestamp::from(wide_since))
        .limit(100);

    let events = client
        .fetch_events(filter, FETCH_EVENTS_TIMEOUT)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch admin chat events for shared key: {e}"))?;

    let mut messages = Vec::new();
    for wrapped in events.iter() {
        match unwrap_giftwrap_with_shared_key(shared_keys, wrapped).await {
            Ok((content, ts, sender_pubkey)) => {
                messages.push((content, ts, sender_pubkey));
            }
            Err(e) => {
                log::warn!(
                    "Failed to unwrap gift wrap for shared key {}: {}",
                    wrapped.id,
                    e
                );
            }
        }
    }
    messages.sort_by_key(|(_, ts, _)| *ts);
    Ok(messages)
}

/// Fetch and collect new messages for a single (dispute, party) shared key.
///
/// Rebuilds `Keys` from the stored hex, fetches gift wraps addressed to that
/// shared key's pubkey, filters by `last_seen`, and appends results to `by_key`.
async fn fetch_party_messages(
    client: &Client,
    dispute_id: &str,
    party: ChatParty,
    shared_key_hex: Option<&str>,
    last_seen: i64,
    by_key: &mut AdminChatByKey,
) {
    let Some(hex) = shared_key_hex else { return };
    let Some(shared_keys) = keys_from_shared_hex(hex) else {
        return;
    };

    let Ok(messages) = fetch_gift_wraps_for_shared_key(client, &shared_keys).await else {
        return;
    };

    for (content, ts, sender_pubkey) in messages {
        if ts < last_seen {
            continue;
        }
        by_key
            .entry((dispute_id.to_string(), party))
            .or_default()
            .push((content, ts, sender_pubkey));
    }
}

/// Fetch admin chat updates for all active disputes using per-dispute shared keys.
///
/// For each (dispute, party) that has a stored `shared_key_hex`, we rebuild the
/// `Keys`, fetch gift wrap events addressed to the shared key's public key, and
/// apply `last_seen_timestamp` filtering. Messages are grouped into
/// `AdminChatUpdate` results the same way as before.
pub async fn fetch_admin_chat_updates(
    client: &Client,
    disputes: &[AdminDispute],
    admin_chat_last_seen: &HashMap<(String, ChatParty), AdminChatLastSeen>,
) -> Result<Vec<AdminChatUpdate>, anyhow::Error> {
    let mut by_key: AdminChatByKey = HashMap::new();

    for d in disputes {
        // Only fetch for InProgress disputes
        let is_in_progress = d
            .status
            .as_deref()
            .and_then(|s| DisputeStatus::from_str(s).ok())
            == Some(DisputeStatus::InProgress);
        if !is_in_progress {
            continue;
        }

        for (party, hex) in [
            (ChatParty::Buyer, d.buyer_shared_key_hex.as_deref()),
            (ChatParty::Seller, d.seller_shared_key_hex.as_deref()),
        ] {
            let last_seen = admin_chat_last_seen
                .get(&(d.dispute_id.clone(), party))
                .and_then(|s| s.last_seen_timestamp)
                .unwrap_or(0);

            fetch_party_messages(client, &d.dispute_id, party, hex, last_seen, &mut by_key).await;
        }
    }

    let updates: Vec<AdminChatUpdate> = by_key
        .into_iter()
        .filter(|(_, msgs)| !msgs.is_empty())
        .map(|((dispute_id, party), messages)| AdminChatUpdate {
            dispute_id,
            party,
            messages,
        })
        .collect();

    Ok(updates)
}

/// Fetch chat messages for the Observer tab using a pasted shared key hex.
///
/// Converts the hex to `Keys`, fetches gift wraps from the last 7 days,
/// decrypts them, assigns Buyer/Seller/Admin roles by pubkey order, and
/// returns `DisputeChatMessage` items ready for display.
pub async fn fetch_observer_chat(
    client: &Client,
    shared_key_hex: &str,
    admin_pubkey: Option<&PublicKey>,
) -> Result<Vec<DisputeChatMessage>> {
    use std::collections::HashMap;

    use crate::ui::helpers::try_parse_attachment_message;

    let shared_keys = keys_from_shared_hex(shared_key_hex)
        .ok_or_else(|| anyhow::anyhow!("Invalid shared key hex"))?;

    let raw = fetch_gift_wraps_for_shared_key(client, &shared_keys).await?;

    // Map pubkeys to roles: admin (if known) → Admin, first unknown → Buyer, second → Seller
    let mut role_map: HashMap<PublicKey, ChatSender> = HashMap::new();
    if let Some(apk) = admin_pubkey {
        role_map.insert(*apk, ChatSender::Admin);
    }

    for (_, _, pk) in &raw {
        if role_map.contains_key(pk) {
            continue;
        }
        let has_buyer = role_map.values().any(|s| *s == ChatSender::Buyer);
        let has_seller = role_map.values().any(|s| *s == ChatSender::Seller);
        if !has_buyer {
            role_map.insert(*pk, ChatSender::Buyer);
        } else if !has_seller {
            role_map.insert(*pk, ChatSender::Seller);
        } else {
            role_map.insert(*pk, ChatSender::Admin);
        }
    }

    let mut messages = Vec::with_capacity(raw.len());
    for (content, ts, pk) in raw {
        let sender = role_map.get(&pk).copied().unwrap_or(ChatSender::Admin);

        let (msg_content, attachment) = match try_parse_attachment_message(&content) {
            Some((att, display)) => (display, Some(att)),
            None => (content, None),
        };

        messages.push(DisputeChatMessage {
            sender,
            content: msg_content,
            timestamp: ts,
            target_party: None,
            attachment,
        });
    }

    Ok(messages)
}

/// Send one user order chat message using shared-key wrapping.
pub async fn send_user_order_chat_message_via_shared_key(
    client: &Client,
    trade_keys: &Keys,
    shared_keys: &Keys,
    content: &str,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<()> {
    send_admin_chat_message_via_shared_key(
        client,
        trade_keys,
        shared_keys,
        content,
        mostro_instance,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Different counterparty pubkeys must produce different shared keys (ECDH output is unique per peer).
    #[test]
    fn derive_shared_key_hex_different_users_different_keys() {
        let admin = Keys::generate();
        let buyer = Keys::generate();
        let seller = Keys::generate();
        assert_ne!(
            buyer.public_key().to_string(),
            seller.public_key().to_string(),
            "test setup: buyer and seller must be different"
        );

        let buyer_hex =
            derive_shared_key_hex(Some(&admin), Some(buyer.public_key().to_string().as_str()));
        let seller_hex =
            derive_shared_key_hex(Some(&admin), Some(seller.public_key().to_string().as_str()));

        assert!(buyer_hex.is_some(), "buyer shared key should derive");
        assert!(seller_hex.is_some(), "seller shared key should derive");
        assert_ne!(
            buyer_hex.as_deref(),
            seller_hex.as_deref(),
            "shared keys for different users must differ"
        );
    }

    #[test]
    fn order_chat_counterparty_is_other_trade_side() {
        let buyer = Keys::generate();
        let seller = Keys::generate();
        let buyer_hex = buyer.public_key().to_string();
        let seller_hex = seller.public_key().to_string();
        let small_order = SmallOrder {
            buyer_trade_pubkey: Some(buyer_hex.clone()),
            seller_trade_pubkey: Some(seller_hex.clone()),
            ..Default::default()
        };

        let (cp_from_buyer, sk_buyer) =
            order_chat_counterparty_and_shared_hex(&buyer, &small_order).expect("buyer side");
        assert_eq!(cp_from_buyer, seller_hex);
        let (cp_from_seller, sk_seller) =
            order_chat_counterparty_and_shared_hex(&seller, &small_order).expect("seller side");
        assert_eq!(cp_from_seller, buyer_hex);
        assert_eq!(
            sk_buyer, sk_seller,
            "ECDH shared secret matches for both peers"
        );
    }

    #[test]
    fn order_chat_counterparty_none_when_trade_key_unknown() {
        let buyer = Keys::generate();
        let seller = Keys::generate();
        let other = Keys::generate();
        let small_order = SmallOrder {
            buyer_trade_pubkey: Some(buyer.public_key().to_string()),
            seller_trade_pubkey: Some(seller.public_key().to_string()),
            ..Default::default()
        };
        assert!(order_chat_counterparty_and_shared_hex(&other, &small_order).is_none());
    }

    #[tokio::test]
    async fn chat_wrap_unwrap_roundtrip_preserves_sender_and_content() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let shared = SharedKey::derive(sender.secret_key(), &receiver.public_key())
            .expect("shared key derives");
        let shared_pubkey = shared.public_key();

        let content = "hello from test";
        let wrapped = wrap_chat_message(&sender, &shared_pubkey, content)
            .await
            .expect("wrap_chat_message succeeds");

        let unwrapped = unwrap_chat_message(shared.keys(), &wrapped)
            .await
            .expect("unwrap_chat_message succeeds");

        assert_eq!(unwrapped.sender, sender.public_key());
        assert_eq!(unwrapped.content, content);
    }
}
