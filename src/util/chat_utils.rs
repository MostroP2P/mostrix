use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use mostro_core::prelude::{Action, DisputeStatus, Message, Payload};
use nostr_sdk::prelude::*;

use crate::models::AdminDispute;
use crate::ui::{AdminChatLastSeen, AdminChatUpdate, ChatParty};
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;

/// Messages grouped by (dispute_id, party); value is (content, timestamp, sender_pubkey).
type AdminChatByKey = HashMap<(String, ChatParty), Vec<(String, i64, PublicKey)>>;

// ---------------------------------------------------------------------------
// Shared-key helpers (ECDH derivation, hex conversion)
// ---------------------------------------------------------------------------

/// Derive a shared key from the admin's secret key and a counterparty public key
/// using ECDH via `nostr_sdk::util::generate_shared_key`, then wrap the result
/// in a `Keys` instance (mirroring the mostro-chat model).
///
/// Returns `None` if either argument is missing or derivation fails.
pub fn derive_shared_keys(
    admin_keys: Option<&Keys>,
    counterparty_pubkey: Option<&PublicKey>,
) -> Option<Keys> {
    let admin = admin_keys?;
    let cp_pk = counterparty_pubkey?;
    let shared_bytes = nostr_sdk::util::generate_shared_key(admin.secret_key(), cp_pk).ok()?;
    let secret = SecretKey::from_slice(&shared_bytes).ok()?;
    Some(Keys::new(secret))
}

/// Convenience wrapper: derive a shared key and return its secret as a hex string
/// suitable for DB persistence. Returns `None` when derivation is not possible.
pub fn derive_shared_key_hex(
    admin_keys: Option<&Keys>,
    counterparty_pubkey_str: Option<&str>,
) -> Option<String> {
    let cp_pk = counterparty_pubkey_str.and_then(|s| PublicKey::parse(s).ok());
    let keys = derive_shared_keys(admin_keys, cp_pk.as_ref())?;
    Some(keys.secret_key().to_secret_hex())
}

/// Rebuild a `Keys` from a stored shared-key hex string.
pub fn keys_from_shared_hex(hex: &str) -> Option<Keys> {
    let secret = SecretKey::from_str(hex).ok()?;
    Some(Keys::new(secret))
}

/// Build a NIP-59 gift wrap event to a recipient pubkey (e.g. trade pubkey).
/// Rumor content is Mostro protocol format: JSON of (Message, Option<String>) with
/// Message::Dm(SendDm, TextMessage(...)) so mostro-cli and mostro daemon can parse it.
async fn build_chat_giftwrap_event_to_pubkey(
    sender: &Keys,
    recipient_pubkey: &PublicKey,
    message: &str,
) -> Result<Event> {
    let dm_message = Message::new_dm(
        None,
        None,
        Action::SendDm,
        Some(Payload::TextMessage(message.to_string())),
    );
    let rumor_content = serde_json::to_string(&(dm_message, None::<String>))
        .map_err(|e| anyhow::anyhow!("Failed to serialize admin chat message: {e}"))?;
    let rumor = EventBuilder::text_note(rumor_content).build(sender.public_key());
    let event = EventBuilder::gift_wrap(sender, recipient_pubkey, rumor, [])
        .await
        .map_err(|e| anyhow::anyhow!("Failed to build gift wrap: {e}"))?;
    // Ensure content is valid base64 (NIP-44) so recipients can decrypt.
    if BASE64_STANDARD.decode(event.content.as_bytes()).is_err() {
        return Err(anyhow::anyhow!(
            "Gift wrap content is not valid base64 (NIP-44); refusing to send"
        ));
    }
    Ok(event)
}

/// Send a chat message from the admin to a counterparty via the per-dispute
/// shared key (ECDH-derived).  The gift wrap is addressed to the **shared key's
/// public key** so both the admin and the counterparty (who derive the same
/// shared key) can fetch and decrypt the event, mirroring the mostro-chat model.
///
/// `shared_keys` is the `Keys` instance rebuilt from the stored shared-key hex.
pub async fn send_admin_chat_message_via_shared_key(
    client: &Client,
    admin_keys: &Keys,
    shared_keys: &Keys,
    content: &str,
) -> Result<()> {
    let content = content.trim();
    if content.is_empty() {
        return Err(anyhow::anyhow!("Cannot send empty admin chat message"));
    }
    let recipient_pubkey = shared_keys.public_key();
    let event = build_chat_giftwrap_event_to_pubkey(admin_keys, &recipient_pubkey, content).await?;
    client
        .send_event(&event)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send admin chat event: {e}"))?;
    Ok(())
}

/// Extract display text from rumor content. If content is Mostro protocol format
/// (Message::Dm(SendDm, TextMessage(s))), return s; otherwise return content as-is.
fn extract_chat_content_from_rumor(rumor_content: &str) -> String {
    if let Ok((Message::Dm(kind), _)) =
        serde_json::from_str::<(Message, Option<String>)>(rumor_content)
    {
        if let Some(Payload::TextMessage(s)) = kind.payload {
            return s;
        }
    }
    rumor_content.to_string()
}

/// Unwrap a gift wrap event addressed to a shared key (decrypt with the shared key).
/// Returns (content, timestamp, sender_pubkey).
///
/// Tries standard NIP-59 (Rumor in Seal in GW) first via `from_gift_wrap`, then
/// falls back to the simplified mostro-chat format where the gift wrap content
/// decrypts directly to a signed inner event.
pub async fn unwrap_giftwrap_with_shared_key(
    shared_keys: &Keys,
    event: &Event,
) -> Result<(String, i64, PublicKey)> {
    // Standard NIP-59: GW content decrypts to Seal, Seal content decrypts to Rumor
    if let Ok(unwrapped) = nip59::UnwrappedGift::from_gift_wrap(shared_keys, event).await {
        let content = extract_chat_content_from_rumor(&unwrapped.rumor.content);
        return Ok((
            content,
            unwrapped.rumor.created_at.as_u64() as i64,
            unwrapped.sender,
        ));
    }

    // Simplified mostro-chat format: GW content decrypts directly to signed Event
    let decrypted = nip44::decrypt(shared_keys.secret_key(), &event.pubkey, &event.content)
        .map_err(|e| anyhow::anyhow!("Failed to decrypt gift wrap with shared key: {e}"))?;

    let inner_event = Event::from_json(&decrypted)
        .map_err(|e| anyhow::anyhow!("Invalid inner chat event: {e}"))?;

    inner_event
        .verify()
        .map_err(|e| anyhow::anyhow!("Invalid inner chat event signature: {e}"))?;

    let content = extract_chat_content_from_rumor(&inner_event.content);
    Ok((
        content,
        inner_event.created_at.as_u64() as i64,
        inner_event.pubkey,
    ))
}

/// Fetch gift wrap events addressed to a specific shared key's public key,
/// decrypt each with the shared key, and return (content, timestamp, sender_pubkey).
async fn fetch_gift_wraps_for_shared_key(
    client: &Client,
    shared_keys: &Keys,
) -> Result<Vec<(String, i64, PublicKey)>> {
    let now = Timestamp::now().as_u64();
    let seven_days_secs: u64 = 7 * 24 * 60 * 60;
    let wide_since = now.saturating_sub(seven_days_secs);

    let shared_pubkey = shared_keys.public_key();
    let filter = Filter::new()
        .kind(Kind::GiftWrap)
        .pubkey(shared_pubkey)
        .since(Timestamp::from(wide_since))
        .limit(100);

    let events = client
        .fetch_events(filter, FETCH_EVENTS_TIMEOUT)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch admin chat events for shared key: {e}"))?;

    let mut messages = Vec::new();
    for wrapped in events.iter() {
        let to_shared = wrapped.tags.public_keys().any(|pk| *pk == shared_pubkey);
        if !to_shared {
            continue;
        }
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
        if ts <= last_seen {
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
    _admin_keys: &Keys,
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
