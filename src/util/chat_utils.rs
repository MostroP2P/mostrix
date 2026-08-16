use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Result;
use mostro_core::chat::{
    chat_filter, giftwrap_chat_filter, unwrap_chat_message, unwrap_giftwrap_chat_message,
    wrap_chat_message, SharedKey,
};
use mostro_core::prelude::DisputeStatus;
use mostro_core::prelude::SmallOrder;
use nostr_sdk::prelude::*;

use crate::models::{AdminDispute, Order};
use crate::ui::{
    AdminChatLastSeen, AdminChatUpdate, ChatParty, ChatSender, DecodedChatMessage,
    DisputeChatMessage,
};
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use crate::util::mostro_info::MostroInstanceInfo;

/// Messages grouped by (dispute_id, party).
type AdminChatByKey = HashMap<(String, ChatParty), Vec<DecodedChatMessage>>;

/// Dual-read: accept legacy NIP-59 GiftWrap (kind 1059) P2P / dispute chat
/// envelopes until a coordinated deprecation (mostrix#102).
///
/// Outbound chat is always kind 14 (`K_sign` / `K_conv`). Protocol DMs to Mostro
/// are unrelated and keep their own GiftWrap vs kind-14 transport.
pub const CHAT_ACCEPT_LEGACY_GIFTWRAP: bool = true;

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

/// Rebuild `Keys` from a 32-byte secret hex (ECDH IKM **or** disclosed `K_conv`).
pub fn keys_from_shared_hex(hex: &str) -> Option<Keys> {
    SharedKey::from_hex(hex)
        .ok()
        .map(|shared| shared.keys().clone())
}

/// Parse optional Observer `pub(K_sign)` locator (hex or bech32). Empty → `None`.
pub fn parse_optional_sign_pubkey(s: &str) -> Result<Option<PublicKey>, anyhow::Error> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    parse_chat_pubkey(s)
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("pub(K_sign) must be a valid hex or npub pubkey"))
}

/// Kind-14 Observer fetch filter: `authors = [pub(K_sign)]` when known, else `#p = pub(K_conv)`.
pub(crate) fn observer_kind14_filter(
    conv_pubkey: PublicKey,
    sign_pubkey: Option<PublicKey>,
    since: Timestamp,
) -> Filter {
    match sign_pubkey {
        Some(author) => chat_filter(author).since(since).limit(100),
        None => Filter::new()
            .kind(Kind::PrivateDirectMessage)
            .pubkey(conv_pubkey)
            .since(since)
            .limit(100),
    }
}

/// Read-only disclosure for a solver: `K_conv` secret hex and `pub(K_sign)`.
///
/// Never returns the `K_sign` secret. `K_conv` decrypts; it cannot author kind 14.
pub fn conversation_disclosure_from_ecdh(ecdh_keys: &Keys) -> Option<(String, String)> {
    let (conv, sign) = chat_keys_from_ecdh(ecdh_keys)?;
    Some((
        conv.secret_key().to_secret_hex(),
        sign.public_key().to_hex(),
    ))
}

/// Observer disclosure from a stored order (ECDH hex or trade-key ECDH).
pub fn conversation_disclosure_from_order(
    order_chat_shared_key_hex: Option<&str>,
    trade_keys: Option<&Keys>,
    counterparty_pubkey: Option<&str>,
) -> Option<(String, String)> {
    let ecdh = order_chat_shared_key_hex
        .and_then(keys_from_shared_hex)
        .or_else(|| {
            let trade = trade_keys?;
            let pk = parse_chat_pubkey(counterparty_pubkey?)?;
            derive_shared_keys(Some(trade), Some(&pk))
        })?;
    conversation_disclosure_from_ecdh(&ecdh)
}

/// Derive `(K_conv, K_sign)` from ECDH keys rebuilt via [`keys_from_shared_hex`].
pub fn chat_keys_from_ecdh(ecdh_keys: &Keys) -> Option<(Keys, Keys)> {
    SharedKey::from_keys(ecdh_keys.clone()).chat_keys().ok()
}

/// Derive `(K_conv, K_sign)` from a persisted ECDH hex string.
pub fn chat_keys_from_shared_hex(hex: &str) -> Option<(Keys, Keys)> {
    SharedKey::from_hex(hex).ok()?.chat_keys().ok()
}

/// Parse a hex or bech32 Nostr pubkey used as a chat inner signer.
pub fn parse_chat_pubkey(s: &str) -> Option<PublicKey> {
    PublicKey::parse(s).ok()
}

/// Inner signers for user P2P order chat: local trade key and counterparty trade key.
///
/// Returns `None` when the counterparty is missing or equal to the local key.
pub fn order_chat_allowed_signers(
    local_trade_pubkey: PublicKey,
    counterparty_pubkey: Option<&str>,
) -> Option<Vec<PublicKey>> {
    let counterparty = parse_chat_pubkey(counterparty_pubkey?)?;
    if counterparty == local_trade_pubkey {
        return None;
    }
    Some(vec![local_trade_pubkey, counterparty])
}

/// Inner signers for one admin↔party dispute channel.
///
/// Always includes the party trade key; includes the admin pubkey when known.
/// Returns `None` when the party pubkey is missing.
pub fn dispute_chat_allowed_signers(
    admin_pubkey: Option<&PublicKey>,
    party_pubkey: Option<&str>,
) -> Option<Vec<PublicKey>> {
    let party = parse_chat_pubkey(party_pubkey?)?;
    let mut allowed = vec![party];
    if let Some(admin) = admin_pubkey {
        if *admin != party {
            allowed.push(*admin);
        }
    }
    Some(allowed)
}

/// Role map for observer chat: admin key plus buyer/seller trade keys from taken disputes.
///
/// Unknown inner signers must not be guessed (no arrival-order / Admin fallback).
pub fn observer_known_signer_roles(
    admin_pubkey: Option<&PublicKey>,
    disputes: &[AdminDispute],
) -> HashMap<PublicKey, ChatSender> {
    let mut roles = HashMap::new();
    if let Some(admin) = admin_pubkey {
        roles.insert(*admin, ChatSender::Admin);
    }
    for dispute in disputes {
        if let Some(pk) = dispute.buyer_pubkey.as_deref().and_then(parse_chat_pubkey) {
            roles.entry(pk).or_insert(ChatSender::Buyer);
        }
        if let Some(pk) = dispute.seller_pubkey.as_deref().and_then(parse_chat_pubkey) {
            roles.entry(pk).or_insert(ChatSender::Seller);
        }
    }
    roles
}

/// Map a verified inner signer to a dispute-chat role.
///
/// Returns `None` for an unknown signer so callers fail closed instead of
/// labeling the message as Admin.
pub fn dispute_chat_role_for_inner_signer(
    sender: &PublicKey,
    buyer: Option<&PublicKey>,
    seller: Option<&PublicKey>,
) -> Option<(ChatSender, Option<ChatParty>)> {
    if buyer == Some(sender) {
        Some((ChatSender::Buyer, None))
    } else if seller == Some(sender) {
        Some((ChatSender::Seller, None))
    } else {
        None
    }
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
/// Accepts kind 14, and legacy GiftWrap while [`CHAT_ACCEPT_LEGACY_GIFTWRAP`]
/// is true. `allowed_signers` is the inner-signer allow-list (party trade keys,
/// plus the admin pubkey on dispute channels). An empty list is rejected so
/// callers cannot accidentally accept an arbitrary inner signer.
pub async fn unwrap_giftwrap_with_shared_key(
    shared_keys: &Keys,
    event: &Event,
    allowed_signers: &[PublicKey],
) -> Result<DecodedChatMessage> {
    unwrap_chat_envelope(
        shared_keys,
        event,
        allowed_signers,
        CHAT_ACCEPT_LEGACY_GIFTWRAP,
    )
    .await
}

/// Like [`unwrap_giftwrap_with_shared_key`], with an explicit dual-read switch
/// so tests can exercise the post-cutover GiftWrap rejection path.
async fn unwrap_chat_envelope(
    shared_keys: &Keys,
    event: &Event,
    allowed_signers: &[PublicKey],
    accept_legacy: bool,
) -> Result<DecodedChatMessage> {
    if allowed_signers.is_empty() {
        return Err(anyhow::anyhow!(
            "chat unwrap requires a non-empty inner-signer allow-list"
        ));
    }

    if event.kind == Kind::GiftWrap {
        if !accept_legacy {
            return Err(anyhow::anyhow!(
                "legacy GiftWrap chat envelopes are no longer accepted"
            ));
        }
        let msg = unwrap_giftwrap_chat_message(shared_keys, event)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to unwrap chat gift wrap: {e}"))?;
        if !allowed_signers.contains(&msg.sender) {
            return Err(anyhow::anyhow!(
                "inner gift-wrap signer is not a party to this conversation"
            ));
        }
        return Ok(DecodedChatMessage {
            content: msg.content,
            timestamp: msg.created_at.as_secs() as i64,
            sender: msg.sender,
            inner_event_id: msg.inner_event_id,
        });
    }

    let (conv, sign) = chat_keys_from_ecdh(shared_keys)
        .ok_or_else(|| anyhow::anyhow!("Failed to derive K_conv / K_sign from shared key"))?;
    let sign_pk = sign.public_key();
    let now = Timestamp::now();

    let msg = unwrap_chat_message(&conv, &sign_pk, allowed_signers, event, now)
        .map_err(|e| anyhow::anyhow!("Failed to unwrap chat event: {e}"))?;
    Ok(DecodedChatMessage {
        content: msg.content,
        timestamp: msg.created_at.as_secs() as i64,
        sender: msg.sender,
        inner_event_id: msg.inner_event_id,
    })
}

/// Fetch recent chat events for a shared ECDH key and return decoded messages.
///
/// Subscribes by `authors = [pub(K_sign)]` (kind 14). While
/// [`CHAT_ACCEPT_LEGACY_GIFTWRAP`] is true, also tries the legacy gift-wrap `#p`
/// filter. Inner events whose signer is not in `allowed_signers` are dropped.
pub async fn fetch_gift_wraps_for_shared_key(
    client: &Client,
    shared_keys: &Keys,
    allowed_signers: &[PublicKey],
) -> Result<Vec<DecodedChatMessage>> {
    fetch_chat_messages_for_shared_key(client, shared_keys, allowed_signers, None).await
}

/// Like [`fetch_gift_wraps_for_shared_key`], with a last-seen `since` cursor
/// (clamped to local now; lookback capped at seven days when the cursor is older
/// or absent). Inner events whose signer is not in `allowed_signers` are dropped.
pub async fn fetch_chat_messages_for_shared_key(
    client: &Client,
    shared_keys: &Keys,
    allowed_signers: &[PublicKey],
    since: Option<i64>,
) -> Result<Vec<DecodedChatMessage>> {
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
    let kind14_result = client
        .fetch_events(kind14_filter)
        .timeout(FETCH_EVENTS_TIMEOUT)
        .await;

    let events = if let Some(giftwrap_filter) = legacy_giftwrap_hydrate_filter(
        shared_keys.public_key(),
        lookback_floor,
        CHAT_ACCEPT_LEGACY_GIFTWRAP,
    ) {
        // Dual-read: a transient failure of either query must not hide history
        // still available on the other envelope during the migration window.
        let legacy_result = client
            .fetch_events(giftwrap_filter)
            .timeout(FETCH_EVENTS_TIMEOUT)
            .await;
        merge_dual_read_events(kind14_result, legacy_result)?
    } else {
        kind14_result
            .map_err(|e| anyhow::anyhow!("Failed to fetch chat events: {e}"))?
            .into_iter()
            .collect()
    };

    let mut messages = Vec::new();
    for wrapped in events.iter() {
        match unwrap_giftwrap_with_shared_key(shared_keys, wrapped, allowed_signers).await {
            Ok(msg) => {
                messages.push(msg);
            }
            Err(e) => {
                log::warn!("Failed to unwrap chat event {}: {}", wrapped.id, e);
            }
        }
    }
    messages.sort_by_key(|m| m.timestamp);
    Ok(messages)
}

/// Legacy GiftWrap hydrate filter, or `None` after dual-read cutover.
///
/// Outer 1059 `created_at` is randomized into the past, so the filter uses the
/// wide lookback floor rather than the kind-14 `since` cursor. Callers re-filter
/// on the canonical inner timestamp after unwrap.
pub(crate) fn legacy_giftwrap_hydrate_filter(
    ecdh_pubkey: PublicKey,
    lookback_floor: i64,
    accept_legacy: bool,
) -> Option<Filter> {
    if !accept_legacy {
        return None;
    }
    let giftwrap_since = Timestamp::from(lookback_floor.max(0) as u64);
    Some(
        giftwrap_chat_filter(ecdh_pubkey)
            .since(giftwrap_since)
            .limit(100),
    )
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
/// Skips the fetch when `allowed_signers` is empty.
async fn fetch_party_messages(
    client: &Client,
    dispute_id: &str,
    party: ChatParty,
    shared_key_hex: Option<&str>,
    allowed_signers: &[PublicKey],
    last_seen: i64,
    by_key: &mut AdminChatByKey,
) {
    let Some(hex) = shared_key_hex else { return };
    if allowed_signers.is_empty() {
        log::warn!(
            "skipping dispute {dispute_id} {party} chat fetch: empty inner-signer allow-list"
        );
        return;
    }
    let Some(shared_keys) = keys_from_shared_hex(hex) else {
        return;
    };
    // Normalize a possibly-future stored cursor before it gates the fetch and filter.
    let last_seen = clamp_chat_since_cursor_now(last_seen);

    let Ok(messages) =
        fetch_chat_messages_for_shared_key(client, &shared_keys, allowed_signers, Some(last_seen))
            .await
    else {
        return;
    };

    for msg in messages {
        if msg.timestamp < last_seen {
            continue;
        }
        by_key
            .entry((dispute_id.to_string(), party))
            .or_default()
            .push(msg);
    }
}

/// Fetch admin chat updates for all active disputes using per-dispute shared keys.
///
/// Each party channel is unwrapped with [`dispute_chat_allowed_signers`] (party
/// trade key plus `admin_pubkey` when present). Channels whose party pubkey is
/// missing are skipped.
pub async fn fetch_admin_chat_updates(
    client: &Client,
    disputes: &[AdminDispute],
    admin_chat_last_seen: &HashMap<(String, ChatParty), AdminChatLastSeen>,
    admin_pubkey: Option<&PublicKey>,
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

        for (party, hex, party_pk) in [
            (
                ChatParty::Buyer,
                d.buyer_shared_key_hex.as_deref(),
                d.buyer_pubkey.as_deref(),
            ),
            (
                ChatParty::Seller,
                d.seller_shared_key_hex.as_deref(),
                d.seller_pubkey.as_deref(),
            ),
        ] {
            let last_seen = admin_chat_last_seen
                .get(&(d.dispute_id.clone(), party))
                .and_then(|s| s.last_seen_timestamp)
                .unwrap_or(0);
            let Some(allowed) = dispute_chat_allowed_signers(admin_pubkey, party_pk) else {
                log::warn!(
                    "skipping dispute {} {party} chat fetch: missing party pubkey",
                    d.dispute_id
                );
                continue;
            };

            fetch_party_messages(
                client,
                &d.dispute_id,
                party,
                hex,
                &allowed,
                last_seen,
                &mut by_key,
            )
            .await;
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

/// Unwrap a kind-14 Observer event using disclosed `K_conv`.
///
/// When `sign_pubkey` is `Some`, the outer author must match `pub(K_sign)`.
/// When `None`, junk authors are accepted at the filter (`#p`) and dropped when
/// decrypt fails — same as mostro-chat `-k` without `-a`.
pub(crate) fn unwrap_observer_chat_event(
    conv: &Keys,
    sign_pubkey: Option<&PublicKey>,
    event: &Event,
    allowed_signers: &[PublicKey],
) -> Result<DecodedChatMessage> {
    if allowed_signers.is_empty() {
        return Err(anyhow::anyhow!(
            "chat unwrap requires a non-empty inner-signer allow-list"
        ));
    }
    if event.kind == Kind::GiftWrap {
        return Err(anyhow::anyhow!(
            "Observer K_conv cannot unwrap legacy GiftWrap (disclose K_conv from a kind-14 chat)"
        ));
    }
    let expected = sign_pubkey.copied().unwrap_or(event.pubkey);
    let msg = unwrap_chat_message(conv, &expected, allowed_signers, event, Timestamp::now())
        .map_err(|e| anyhow::anyhow!("Failed to unwrap observer chat event: {e}"))?;
    Ok(DecodedChatMessage {
        content: msg.content,
        timestamp: msg.created_at.as_secs() as i64,
        sender: msg.sender,
        inner_event_id: msg.inner_event_id,
    })
}

/// Fetch chat messages for the Observer tab using disclosed `K_conv` hex.
///
/// Optional `sign_pubkey` (`pub(K_sign)`) uses an `authors` filter; without it
/// the query is `#p = pub(K_conv)` (junk arrives; decrypt fails).
///
/// Inner signers not present in `known_roles` are rejected. `known_roles` must
/// be non-empty.
pub async fn fetch_observer_chat(
    client: &Client,
    conv_key_hex: &str,
    sign_pubkey: Option<PublicKey>,
    known_roles: &HashMap<PublicKey, ChatSender>,
) -> Result<Vec<DisputeChatMessage>> {
    use crate::ui::helpers::try_parse_attachment_message;

    if known_roles.is_empty() {
        return Err(anyhow::anyhow!(
            "Cannot verify observer chat signers without admin keys or taken-dispute party pubkeys"
        ));
    }

    let conv =
        keys_from_shared_hex(conv_key_hex).ok_or_else(|| anyhow::anyhow!("Invalid K_conv hex"))?;
    let allowed: Vec<PublicKey> = known_roles.keys().copied().collect();

    let local_now = Timestamp::now().as_secs() as i64;
    let seven_days_secs: i64 = 7 * 24 * 60 * 60;
    let lookback_floor = local_now.saturating_sub(seven_days_secs);
    let since = Timestamp::from(lookback_floor.max(0) as u64);
    let filter = observer_kind14_filter(conv.public_key(), sign_pubkey, since);

    let events = client
        .fetch_events(filter)
        .timeout(FETCH_EVENTS_TIMEOUT)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch observer chat events: {e}"))?;

    let mut messages = Vec::new();
    for wrapped in events.iter() {
        match unwrap_observer_chat_event(&conv, sign_pubkey.as_ref(), wrapped, &allowed) {
            Ok(msg) => {
                let Some(sender) = known_roles.get(&msg.sender).copied() else {
                    log::warn!("observer: dropping chat message from unknown inner signer");
                    continue;
                };

                let (msg_content, attachment) = match try_parse_attachment_message(&msg.content) {
                    Some((att, display)) => (display, Some(att)),
                    None => (msg.content, None),
                };

                messages.push(DisputeChatMessage {
                    sender,
                    content: msg_content,
                    timestamp: msg.timestamp,
                    target_party: None,
                    attachment,
                });
            }
            Err(e) => {
                log::debug!("observer: skipped event {}: {e}", wrapped.id);
            }
        }
    }
    messages.sort_by_key(|m| m.timestamp);
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
    use mostro_core::chat::wrap_giftwrap_chat_message;

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
        let unwrapped = unwrap_giftwrap_with_shared_key(shared.keys(), &wrapped, &allowed)
            .await
            .expect("unwrap succeeds");

        assert_eq!(unwrapped.sender, sender.public_key());
        assert_eq!(unwrapped.content, content);
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
        let unwrapped = unwrap_giftwrap_with_shared_key(shared.keys(), &event, &allowed)
            .await
            .expect("conversation party unwraps");
        assert_eq!(unwrapped.content, "evidence sent");
        assert_eq!(unwrapped.sender, user.public_key());

        let stranger = Keys::generate();
        let stranger_event = wrap_chat_message(&stranger, &conv, &sign, "spoof")
            .await
            .expect("envelope builds");
        assert!(
            unwrap_giftwrap_with_shared_key(shared.keys(), &stranger_event, &allowed)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn dual_read_unwraps_giftwrap_and_kind14_fixtures() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let shared = SharedKey::derive(sender.secret_key(), &receiver.public_key())
            .expect("shared key derives");
        let (conv, sign) = shared.chat_keys().expect("chat keys");
        let allowed = [sender.public_key(), receiver.public_key()];

        let kind14 = wrap_chat_message(&sender, &conv, &sign, "kind14 hello")
            .await
            .expect("wrap kind 14");
        assert_eq!(kind14.kind, Kind::PrivateDirectMessage);
        assert_eq!(kind14.pubkey, sign.public_key());

        let giftwrap = wrap_giftwrap_chat_message(&sender, &shared.public_key(), "giftwrap hello")
            .await
            .expect("wrap giftwrap");
        assert_eq!(giftwrap.kind, Kind::GiftWrap);

        let from_kind14 = unwrap_giftwrap_with_shared_key(shared.keys(), &kind14, &allowed)
            .await
            .expect("unwrap kind 14");
        let from_giftwrap = unwrap_giftwrap_with_shared_key(shared.keys(), &giftwrap, &allowed)
            .await
            .expect("unwrap giftwrap");

        assert_eq!(from_kind14.sender, sender.public_key());
        assert_eq!(from_kind14.content, "kind14 hello");
        assert_eq!(from_giftwrap.sender, sender.public_key());
        assert_eq!(from_giftwrap.content, "giftwrap hello");
    }

    #[tokio::test]
    async fn unwrap_rejects_giftwrap_when_legacy_disabled() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let shared = SharedKey::derive(sender.secret_key(), &receiver.public_key())
            .expect("shared key derives");
        let allowed = [sender.public_key(), receiver.public_key()];
        let giftwrap = wrap_giftwrap_chat_message(&sender, &shared.public_key(), "legacy leftover")
            .await
            .expect("wrap giftwrap");

        let err = unwrap_chat_envelope(shared.keys(), &giftwrap, &allowed, false)
            .await
            .expect_err("GiftWrap must be rejected after cutover");
        assert!(
            err.to_string().contains("no longer accepted"),
            "unexpected unwrap error: {err}"
        );
    }

    #[test]
    fn legacy_hydrate_filter_omitted_after_cutover() {
        let pk = Keys::generate().public_key();
        assert!(legacy_giftwrap_hydrate_filter(pk, 1_700_000_000, true).is_some());
        assert!(legacy_giftwrap_hydrate_filter(pk, 1_700_000_000, false).is_none());
    }

    #[tokio::test]
    async fn unwrap_rejects_inner_signer_outside_allow_list() {
        let buyer = Keys::generate();
        let seller = Keys::generate();
        let imposter = Keys::generate();
        let shared = SharedKey::derive(buyer.secret_key(), &seller.public_key())
            .expect("shared key derives");
        let (conv, sign) = shared.chat_keys().expect("chat keys");

        let wrapped = wrap_chat_message(&imposter, &conv, &sign, "forged admin statement")
            .await
            .expect("wrap with shared K_sign succeeds for any inner key");

        let allowed = [buyer.public_key(), seller.public_key()];
        let err = unwrap_giftwrap_with_shared_key(shared.keys(), &wrapped, &allowed)
            .await
            .expect_err("arbitrary inner signer must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("not a party") || msg.contains("inner event is signed"),
            "unexpected unwrap error: {msg}"
        );
    }

    #[tokio::test]
    async fn unwrap_rejects_empty_allow_list() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let shared = SharedKey::derive(sender.secret_key(), &receiver.public_key())
            .expect("shared key derives");
        let (conv, sign) = shared.chat_keys().expect("chat keys");
        let wrapped = wrap_chat_message(&sender, &conv, &sign, "hello")
            .await
            .expect("wrap");

        let err = unwrap_giftwrap_with_shared_key(shared.keys(), &wrapped, &[])
            .await
            .expect_err("empty allow-list must fail closed");
        assert!(err
            .to_string()
            .contains("non-empty inner-signer allow-list"));
    }

    #[test]
    fn order_chat_allowed_signers_requires_distinct_counterparty() {
        let local = Keys::generate();
        let peer = Keys::generate();
        let allowed =
            order_chat_allowed_signers(local.public_key(), Some(&peer.public_key().to_string()))
                .expect("counterparty present");
        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&local.public_key()));
        assert!(allowed.contains(&peer.public_key()));
        assert!(order_chat_allowed_signers(local.public_key(), None).is_none());
        assert!(order_chat_allowed_signers(
            local.public_key(),
            Some(&local.public_key().to_string())
        )
        .is_none());
    }

    #[test]
    fn dispute_chat_allowed_signers_includes_admin_and_party() {
        let admin = Keys::generate();
        let party = Keys::generate();
        let allowed = dispute_chat_allowed_signers(
            Some(&admin.public_key()),
            Some(&party.public_key().to_string()),
        )
        .expect("party present");
        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&admin.public_key()));
        assert!(allowed.contains(&party.public_key()));
        assert!(dispute_chat_allowed_signers(Some(&admin.public_key()), None).is_none());
        let party_only =
            dispute_chat_allowed_signers(None, Some(&party.public_key().to_string())).expect("ok");
        assert_eq!(party_only, vec![party.public_key()]);
    }

    #[test]
    fn dispute_chat_role_for_inner_signer_fails_closed_on_unknown() {
        let buyer = Keys::generate().public_key();
        let seller = Keys::generate().public_key();
        let unknown = Keys::generate().public_key();
        assert_eq!(
            dispute_chat_role_for_inner_signer(&buyer, Some(&buyer), Some(&seller)),
            Some((ChatSender::Buyer, None))
        );
        assert_eq!(
            dispute_chat_role_for_inner_signer(&seller, Some(&buyer), Some(&seller)),
            Some((ChatSender::Seller, None))
        );
        assert!(
            dispute_chat_role_for_inner_signer(&unknown, Some(&buyer), Some(&seller)).is_none()
        );
    }

    #[test]
    fn observer_known_signer_roles_does_not_guess_unknown_keys() {
        let admin = Keys::generate().public_key();
        let buyer = Keys::generate();
        let seller = Keys::generate();
        let dispute = AdminDispute {
            buyer_pubkey: Some(buyer.public_key().to_string()),
            seller_pubkey: Some(seller.public_key().to_string()),
            ..Default::default()
        };
        let roles = observer_known_signer_roles(Some(&admin), &[dispute]);
        assert_eq!(roles.get(&admin), Some(&ChatSender::Admin));
        assert_eq!(roles.get(&buyer.public_key()), Some(&ChatSender::Buyer));
        assert_eq!(roles.get(&seller.public_key()), Some(&ChatSender::Seller));
        assert!(!roles.contains_key(&Keys::generate().public_key()));
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
        EventBuilder::new(Kind::TextNote, content)
            .finalize(keys)
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

    #[test]
    fn parse_optional_sign_pubkey_empty_is_none() {
        assert!(parse_optional_sign_pubkey("").unwrap().is_none());
        assert!(parse_optional_sign_pubkey("  ").unwrap().is_none());
        assert!(parse_optional_sign_pubkey("not-a-key").is_err());
        let pk = Keys::generate().public_key();
        assert_eq!(parse_optional_sign_pubkey(&pk.to_hex()).unwrap(), Some(pk));
    }

    #[test]
    fn observer_kind14_filter_authors_when_locator_present() {
        let conv = Keys::generate().public_key();
        let sign = Keys::generate().public_key();
        let since = Timestamp::from(1_700_000_000u64);
        let with_author = observer_kind14_filter(conv, Some(sign), since);
        let json = serde_json::to_value(&with_author).expect("filter json");
        let authors = json.get("authors").expect("authors");
        assert!(authors
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some(&sign.to_hex())));
        assert!(json.get("#p").is_none());

        let p_only = observer_kind14_filter(conv, None, since);
        let json = serde_json::to_value(&p_only).expect("filter json");
        assert!(json.get("authors").is_none());
        let p = json.get("#p").expect("#p");
        assert!(p
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some(&conv.to_hex())));
    }

    #[test]
    fn conversation_disclosure_is_k_conv_not_k_sign_secret() {
        let a = Keys::generate();
        let b = Keys::generate();
        let ecdh = derive_shared_keys(Some(&a), Some(&b.public_key())).expect("ecdh");
        let (conv_hex, sign_pk_hex) = conversation_disclosure_from_ecdh(&ecdh).expect("disclosure");
        let (conv, sign) = chat_keys_from_ecdh(&ecdh).expect("keys");
        assert_eq!(conv_hex, conv.secret_key().to_secret_hex());
        assert_eq!(sign_pk_hex, sign.public_key().to_hex());
        assert_ne!(conv_hex, sign.secret_key().to_secret_hex());
        let via_order = conversation_disclosure_from_order(
            Some(
                &derive_shared_key_hex(Some(&a), Some(b.public_key().to_string().as_str()))
                    .unwrap(),
            ),
            None,
            None,
        )
        .expect("from stored hex");
        assert_eq!(via_order.0, conv_hex);
        assert_eq!(via_order.1, sign_pk_hex);
    }

    #[tokio::test]
    async fn observer_k_conv_only_unwraps_kind14() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let shared = SharedKey::derive(sender.secret_key(), &receiver.public_key())
            .expect("shared key derives");
        let (conv, sign) = shared.chat_keys().expect("chat keys");
        let wrapped = wrap_chat_message(&sender, &conv, &sign, "evidence")
            .await
            .expect("wrap");
        let observer_conv =
            keys_from_shared_hex(&conv.secret_key().to_secret_hex()).expect("observer conv");
        let allowed = [sender.public_key(), receiver.public_key()];
        let msg = unwrap_observer_chat_event(&observer_conv, None, &wrapped, &allowed)
            .expect("observer decrypt");
        assert_eq!(msg.content, "evidence");
        assert_eq!(msg.sender, sender.public_key());

        let with_locator = unwrap_observer_chat_event(
            &observer_conv,
            Some(&sign.public_key()),
            &wrapped,
            &allowed,
        )
        .expect("locator unwrap");
        assert_eq!(with_locator.content, "evidence");
    }

    #[tokio::test]
    async fn observer_cannot_unwrap_legacy_giftwrap_with_k_conv() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let shared =
            SharedKey::derive(sender.secret_key(), &receiver.public_key()).expect("shared");
        let (conv, _sign) = shared.chat_keys().expect("chat keys");
        let giftwrap = wrap_giftwrap_chat_message(&sender, &shared.public_key(), "legacy")
            .await
            .expect("wrap giftwrap");
        let allowed = [sender.public_key(), receiver.public_key()];
        let err = unwrap_observer_chat_event(&conv, None, &giftwrap, &allowed)
            .expect_err("GiftWrap needs ECDH not K_conv");
        assert!(err.to_string().contains("GiftWrap"));
    }

    #[tokio::test]
    async fn observer_wrong_locator_is_rejected() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let shared =
            SharedKey::derive(sender.secret_key(), &receiver.public_key()).expect("shared");
        let (conv, sign) = shared.chat_keys().expect("chat keys");
        let wrapped = wrap_chat_message(&sender, &conv, &sign, "evidence")
            .await
            .expect("wrap");
        let allowed = [sender.public_key(), receiver.public_key()];
        let wrong = Keys::generate().public_key();
        unwrap_observer_chat_event(&conv, Some(&wrong), &wrapped, &allowed)
            .expect_err("locator must match pub(K_sign)");
    }

    #[tokio::test]
    async fn observer_k_conv_cannot_author_kind14() {
        let sender = Keys::generate();
        let receiver = Keys::generate();
        let shared =
            SharedKey::derive(sender.secret_key(), &receiver.public_key()).expect("shared");
        let (conv, sign) = shared.chat_keys().expect("chat keys");
        // Observer only holds K_conv; using it as K_sign authors pub(K_conv), not pub(K_sign).
        let forged = wrap_chat_message(&sender, &conv, &conv, "inject")
            .await
            .expect("wrap with conv as sign");
        assert_eq!(forged.pubkey, conv.public_key());
        let allowed = [sender.public_key(), receiver.public_key()];
        unwrap_observer_chat_event(&conv, Some(&sign.public_key()), &forged, &allowed)
            .expect_err("K_conv cannot produce a valid K_sign author");
    }
}
