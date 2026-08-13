use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Result;
use mostro_core::chat::{
    chat_filter, unwrap_chat_message, unwrap_giftwrap_chat_message, wrap_chat_message, SharedKey,
};
use mostro_core::prelude::DisputeStatus;
use mostro_core::prelude::SmallOrder;
use nostr_sdk::nips::nip44;
use nostr_sdk::prelude::*;

use crate::models::{AdminDispute, Order};
use crate::ui::{AdminChatLastSeen, AdminChatUpdate, ChatParty, ChatSender, DisputeChatMessage};
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use crate::util::mostro_info::MostroInstanceInfo;

/// Messages grouped by (dispute_id, party); value is (content, timestamp, sender_pubkey).
type AdminChatByKey = HashMap<(String, ChatParty), Vec<(String, i64, PublicKey)>>;

// ---------------------------------------------------------------------------
// Shared-key helpers (ECDH IKM + K_conv / K_sign)
// ---------------------------------------------------------------------------

/// Derive the per-channel ECDH shared secret for a chat counterparty.
///
/// This is the 32-byte ECDH output used as HKDF IKM for `K_conv` / `K_sign`
/// (see `mostro_core::chat`). Mostrix persists that secret as hex; wire
/// addressing uses `pub(K_conv)` / `pub(K_sign)` from [`chat_keys_from_ecdh`].
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

/// Clamp a chat `since` cursor to the client's clock.
///
/// Protocol requirement: persisted `since` MUST be `min(accepted_ts, local_now)`.
/// Without this, a counterparty can date a message far in the future, poison the
/// cursor, and permanently silence the conversation until that date.
pub fn clamp_chat_since_cursor(accepted_ts: i64, local_now: i64) -> i64 {
    accepted_ts.min(local_now)
}

/// [`clamp_chat_since_cursor`] against [`Timestamp::now`].
pub fn clamp_chat_since_cursor_now(accepted_ts: i64) -> i64 {
    clamp_chat_since_cursor(accepted_ts, Timestamp::now().as_secs() as i64)
}

/// Derive a shared ECDH secret and return it as hex for DB persistence.
///
/// The persisted hex is the ECDH IKM, not `K_conv` / `K_sign`. It round-trips
/// via [`keys_from_shared_hex`] then [`chat_keys_from_ecdh`].
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

/// Rebuild ECDH `Keys` from a stored shared-key hex string.
pub fn keys_from_shared_hex(hex: &str) -> Option<Keys> {
    SharedKey::from_hex(hex)
        .ok()
        .map(|shared| shared.keys().clone())
}

/// Derive `(K_conv, K_sign)` from ECDH keys rebuilt via [`keys_from_shared_hex`].
pub fn chat_keys_from_ecdh(ecdh_keys: &Keys) -> Option<(Keys, Keys)> {
    SharedKey::from_keys(ecdh_keys.clone()).chat_keys().ok()
}

/// Derive `(K_conv, K_sign)` from a persisted ECDH hex string.
pub fn chat_keys_from_shared_hex(hex: &str) -> Option<(Keys, Keys)> {
    SharedKey::from_hex(hex).ok()?.chat_keys().ok()
}

/// 32-byte ChaCha20 key for decrypting order-chat attachments (shared ECDH secret).
pub fn order_chat_decryption_key_bytes(order: &Order) -> Option<Vec<u8>> {
    if let Some(hex) = order.order_chat_shared_key_hex.as_deref() {
        if let Some(keys) = keys_from_shared_hex(hex) {
            return Some(keys.secret_key().to_secret_bytes().to_vec());
        }
    }
    let trade_keys_hex = order.trade_keys.as_deref()?;
    let trade_sk = SecretKey::from_str(trade_keys_hex).ok()?;
    let trade_keys = Keys::new(trade_sk);
    let cp = order.counterparty_pubkey.as_deref()?;
    let cp_pk = PublicKey::parse(cp).ok()?;
    derive_shared_keys(Some(&trade_keys), Some(&cp_pk))
        .map(|k| k.secret_key().to_secret_bytes().to_vec())
}

/// Resolve the order-chat counterparty pubkey and the ECDH shared-key hex.
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

/// Send one admin dispute chat message via the per-dispute ECDH shared secret.
///
/// Wraps with the gift-wrap-free envelope: kind 14 signed by `K_sign`, encrypted
/// under `K_conv`. `shared_keys` is the ECDH `Keys` from stored hex.
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
    let (conv, sign) = chat_keys_from_ecdh(shared_keys)
        .ok_or_else(|| anyhow::anyhow!("Failed to derive K_conv / K_sign from shared key"))?;
    let event = wrap_chat_message(admin_keys, &conv, &sign, content)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to wrap admin chat message: {e}"))?;
    client
        .send_event(&event)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send admin chat event: {e}"))?;
    Ok(())
}

/// Unwrap a chat envelope addressed via the channel ECDH secret.
///
/// Accepts the new kind-14 form and, during migration, legacy GiftWrap.
/// When `allowed_signers` is `None`, any verified inner signer is accepted
/// (observer / hydration paths that do not yet know both party pubkeys).
pub async fn unwrap_giftwrap_with_shared_key(
    shared_keys: &Keys,
    event: &Event,
    allowed_signers: Option<&[PublicKey]>,
) -> Result<(String, i64, PublicKey)> {
    if event.kind == Kind::GiftWrap {
        let msg = unwrap_giftwrap_chat_message(shared_keys, event)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to unwrap chat gift wrap: {e}"))?;
        if let Some(allowed) = allowed_signers {
            if !allowed.contains(&msg.sender) {
                return Err(anyhow::anyhow!(
                    "inner gift-wrap signer is not a party to this conversation"
                ));
            }
        }
        return Ok((msg.content, msg.created_at.as_secs() as i64, msg.sender));
    }

    let (conv, sign) = chat_keys_from_ecdh(shared_keys)
        .ok_or_else(|| anyhow::anyhow!("Failed to derive K_conv / K_sign from shared key"))?;
    let sign_pk = sign.public_key();
    let now = Timestamp::now();

    let allowed_owned: Vec<PublicKey>;
    let allowed: &[PublicKey] = match allowed_signers {
        Some(s) => s,
        None => {
            // Learn the inner signer cheaply, then run the full normative unwrap.
            let decrypted = nip44::decrypt(conv.secret_key(), &conv.public_key(), &event.content)
                .map_err(|e| anyhow::anyhow!("K_conv decrypt failed: {e}"))?;
            let inner = Event::from_json(&decrypted)
                .map_err(|e| anyhow::anyhow!("malformed inner chat event: {e}"))?;
            allowed_owned = vec![inner.pubkey];
            &allowed_owned
        }
    };

    let msg = unwrap_chat_message(&conv, &sign_pk, allowed, event, now)
        .map_err(|e| anyhow::anyhow!("Failed to unwrap chat event: {e}"))?;
    Ok((msg.content, msg.created_at.as_secs() as i64, msg.sender))
}

/// Fetch recent chat events for a shared ECDH key and return decoded messages.
///
/// Subscribes by `authors = [pub(K_sign)]` (kind 14). Also tries the legacy
/// gift-wrap `#p` filter so dual-read hydration still works during migration.
pub async fn fetch_gift_wraps_for_shared_key(
    client: &Client,
    shared_keys: &Keys,
) -> Result<Vec<(String, i64, PublicKey)>> {
    fetch_chat_messages_for_shared_key(client, shared_keys, None, None).await
}

/// Like [`fetch_gift_wraps_for_shared_key`], with optional inner-signer allow-list
/// and optional last-seen `since` cursor (clamped to local now; lookback capped
/// at seven days when the cursor is older or absent).
pub async fn fetch_chat_messages_for_shared_key(
    client: &Client,
    shared_keys: &Keys,
    allowed_signers: Option<&[PublicKey]>,
    since: Option<i64>,
) -> Result<Vec<(String, i64, PublicKey)>> {
    let local_now = Timestamp::now().as_secs() as i64;
    let seven_days_secs: i64 = 7 * 24 * 60 * 60;
    let lookback_floor = local_now.saturating_sub(seven_days_secs);
    let since_secs = match since {
        Some(ts) => clamp_chat_since_cursor(ts, local_now).max(lookback_floor),
        None => lookback_floor,
    } as u64;
    let since_ts = Timestamp::from(since_secs);

    let (_conv, sign) = chat_keys_from_ecdh(shared_keys)
        .ok_or_else(|| anyhow::anyhow!("Failed to derive K_conv / K_sign from shared key"))?;

    let kind14_filter = chat_filter(sign.public_key()).since(since_ts).limit(100);
    // Dual-read: legacy gift wraps addressed to the ECDH pubkey (superseded p tag).
    // Outer 1059 created_at is randomized into the past, so tightening to the cursor
    // would drop still-new messages; keep the wide floor and let callers re-filter on
    // the canonical inner timestamp after unwrap.
    let giftwrap_since = Timestamp::from(lookback_floor.max(0) as u64);
    let giftwrap_filter = mostro_core::chat::giftwrap_chat_filter(shared_keys.public_key())
        .since(giftwrap_since)
        .limit(100);

    // Run both fetches independently so a transient failure of either query does
    // not hide history still available on the other envelope during migration.
    let kind14_result = client
        .fetch_events(kind14_filter, FETCH_EVENTS_TIMEOUT)
        .await;
    let legacy_result = client
        .fetch_events(giftwrap_filter, FETCH_EVENTS_TIMEOUT)
        .await;

    let events = merge_dual_read_events(kind14_result, legacy_result)?;

    let mut messages = Vec::new();
    for wrapped in events.iter() {
        match unwrap_giftwrap_with_shared_key(shared_keys, wrapped, allowed_signers).await {
            Ok((content, ts, sender_pubkey)) => {
                messages.push((content, ts, sender_pubkey));
            }
            Err(e) => {
                log::warn!("Failed to unwrap chat event {}: {}", wrapped.id, e);
            }
        }
    }
    messages.sort_by_key(|(_, ts, _)| *ts);
    Ok(messages)
}

/// Merge kind-14 and legacy GiftWrap fetch results for dual-read hydration.
///
/// Succeeds if either source returns events; errors only when both fetches fail.
/// Event ids are deduplicated (kind-14 first, then legacy).
pub(crate) fn merge_dual_read_events(
    kind14: Result<impl IntoIterator<Item = Event>, impl std::fmt::Display>,
    legacy: Result<impl IntoIterator<Item = Event>, impl std::fmt::Display>,
) -> Result<Vec<Event>> {
    match (kind14, legacy) {
        (Ok(a), Ok(b)) => {
            let mut events: Vec<Event> = a.into_iter().collect();
            for ev in b {
                if events.iter().all(|e| e.id != ev.id) {
                    events.push(ev);
                }
            }
            Ok(events)
        }
        (Ok(a), Err(e)) => {
            log::warn!("legacy GiftWrap chat fetch failed (using kind-14 only): {e}");
            Ok(a.into_iter().collect())
        }
        (Err(e), Ok(b)) => {
            log::warn!("kind-14 chat fetch failed (using legacy GiftWrap only): {e}");
            Ok(b.into_iter().collect())
        }
        (Err(e14), Err(e_legacy)) => Err(anyhow::anyhow!(
            "Failed to fetch chat events (kind-14: {e14}; giftwrap: {e_legacy})"
        )),
    }
}

/// Fetch and collect new messages for a single (dispute, party) shared key.
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
    // Normalize a possibly-future stored cursor before it gates the fetch and filter.
    let last_seen = clamp_chat_since_cursor_now(last_seen);

    let Ok(messages) =
        fetch_chat_messages_for_shared_key(client, &shared_keys, None, Some(last_seen)).await
    else {
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
pub async fn fetch_admin_chat_updates(
    client: &Client,
    disputes: &[AdminDispute],
    admin_chat_last_seen: &HashMap<(String, ChatParty), AdminChatLastSeen>,
) -> Result<Vec<AdminChatUpdate>, anyhow::Error> {
    let mut by_key: AdminChatByKey = HashMap::new();

    for d in disputes {
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

/// Fetch chat messages for the Observer tab using a pasted ECDH shared key hex.
///
/// Derives `K_conv` / `K_sign` for fetch/decrypt. Observer UX that accepts
/// `K_conv`-only disclosure is Step 6; this path still expects the ECDH secret
/// Mostrix stores today (which can derive both keys).
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
        let (conv, sign) = shared.chat_keys().expect("chat keys");

        let content = "hello from test";
        let wrapped = wrap_chat_message(&sender, &conv, &sign, content)
            .await
            .expect("wrap_chat_message succeeds");

        assert_eq!(wrapped.kind, Kind::PrivateDirectMessage);
        assert_eq!(wrapped.pubkey, sign.public_key());

        let allowed = [sender.public_key(), receiver.public_key()];
        let unwrapped = unwrap_giftwrap_with_shared_key(shared.keys(), &wrapped, Some(&allowed))
            .await
            .expect("unwrap succeeds");

        assert_eq!(unwrapped.2, sender.public_key());
        assert_eq!(unwrapped.0, content);
    }

    #[tokio::test]
    async fn user_solver_chat_roundtrip_accepts_only_conversation_parties() {
        let user = Keys::generate();
        let solver = Keys::generate();
        let shared =
            SharedKey::derive(user.secret_key(), &solver.public_key()).expect("shared key derives");
        let (conv, sign) = shared.chat_keys().expect("chat keys derive");
        let event = wrap_chat_message(&user, &conv, &sign, "evidence sent")
            .await
            .expect("chat wraps");

        let allowed = [user.public_key(), solver.public_key()];
        let (content, _, sender) =
            unwrap_giftwrap_with_shared_key(shared.keys(), &event, Some(&allowed))
                .await
                .expect("conversation party unwraps");
        assert_eq!(content, "evidence sent");
        assert_eq!(sender, user.public_key());

        let stranger = Keys::generate();
        let stranger_event = wrap_chat_message(&stranger, &conv, &sign, "spoof")
            .await
            .expect("envelope builds");
        assert!(
            unwrap_giftwrap_with_shared_key(shared.keys(), &stranger_event, Some(&allowed))
                .await
                .is_err()
        );
    }

    #[test]
    fn chat_keys_from_shared_hex_matches_derive() {
        let a = Keys::generate();
        let b = Keys::generate();
        let hex = derive_shared_key_hex(Some(&a), Some(b.public_key().to_string().as_str()))
            .expect("hex");
        let (conv, sign) = chat_keys_from_shared_hex(&hex).expect("chat keys");
        let (conv2, sign2) = derive_shared_keys(Some(&a), Some(&b.public_key()))
            .and_then(|ecdh| chat_keys_from_ecdh(&ecdh))
            .expect("from ecdh");
        assert_eq!(conv.public_key(), conv2.public_key());
        assert_eq!(sign.public_key(), sign2.public_key());
    }

    fn sample_text_event(keys: &Keys, content: &str) -> Event {
        EventBuilder::text_note(content)
            .sign_with_keys(keys)
            .expect("sign event")
    }

    #[test]
    fn clamp_chat_since_cursor_caps_future_poison() {
        let now = 1_700_000_000_i64;
        assert_eq!(clamp_chat_since_cursor(now + 86_400, now), now);
        assert_eq!(clamp_chat_since_cursor(now - 10, now), now - 10);
        assert_eq!(clamp_chat_since_cursor(now, now), now);
    }

    #[test]
    fn merge_dual_read_keeps_legacy_when_kind14_fails() {
        let keys = Keys::generate();
        let legacy_ev = sample_text_event(&keys, "legacy");
        let merged = merge_dual_read_events(
            Err::<Vec<Event>, _>("kind14 down"),
            Ok::<Vec<Event>, &str>(vec![legacy_ev.clone()]),
        )
        .expect("legacy alone succeeds");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, legacy_ev.id);
    }

    #[test]
    fn merge_dual_read_keeps_kind14_when_legacy_fails() {
        let keys = Keys::generate();
        let kind14_ev = sample_text_event(&keys, "kind14");
        let merged = merge_dual_read_events(
            Ok::<Vec<Event>, &str>(vec![kind14_ev.clone()]),
            Err::<Vec<Event>, _>("giftwrap down"),
        )
        .expect("kind14 alone succeeds");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, kind14_ev.id);
    }

    #[test]
    fn merge_dual_read_errors_when_both_fail() {
        let err = merge_dual_read_events(
            Err::<Vec<Event>, _>("kind14 down"),
            Err::<Vec<Event>, _>("giftwrap down"),
        )
        .expect_err("both failed");
        let msg = err.to_string();
        assert!(msg.contains("kind14 down"));
        assert!(msg.contains("giftwrap down"));
    }

    #[test]
    fn merge_dual_read_dedupes_by_event_id() {
        let keys = Keys::generate();
        let shared = sample_text_event(&keys, "same");
        let other = sample_text_event(&keys, "other");
        let merged = merge_dual_read_events(
            Ok::<Vec<Event>, &str>(vec![shared.clone()]),
            Ok::<Vec<Event>, &str>(vec![shared.clone(), other.clone()]),
        )
        .expect("merge");
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, shared.id);
        assert_eq!(merged[1].id, other.id);
    }
}
