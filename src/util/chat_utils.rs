use std::collections::HashMap;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use mostro_core::prelude::{Action, Message, Payload};
use nostr_sdk::prelude::*;

use crate::models::AdminDispute;
use crate::ui::{AdminChatLastSeen, AdminChatUpdate, ChatParty};
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use crate::SETTINGS;

/// Messages grouped by (dispute_id, party); value is (content, timestamp, sender_pubkey).
type AdminChatByKey = HashMap<(String, ChatParty), Vec<(String, u64, PublicKey)>>;

/// Shared key information used for admin ↔ user chat.
///
/// The shared key is derived using ECDH between the admin's private key and the
/// counterparty public key. The resulting secret is turned into a `Keys` pair
/// and used as the receiver for NIP-59 gift wrapped events, following the
/// simplified scheme from the `mostro-chat` project.
#[derive(Clone, Debug)]
pub struct SharedChatKeys {
    /// Keys built from the shared secret. The public key is used in filters and
    /// as the receiver of gift wrap events; the secret key is used to decrypt.
    pub shared_keys: Keys,
}

/// Derive a shared chat key between the admin identity and a counterparty.
///
/// This uses `nostr_sdk::util::generate_shared_key` to perform ECDH between
/// the admin secret key and the counterparty public key. The resulting bytes
/// are turned into a Nostr `SecretKey` and wrapped in a `Keys` struct.
pub fn derive_shared_chat_keys(
    admin_keys: &Keys,
    counterparty: &PublicKey,
) -> Result<SharedChatKeys> {
    let shared_bytes = nostr_sdk::util::generate_shared_key(admin_keys.secret_key(), counterparty)
        .map_err(|e| anyhow::anyhow!("Failed to generate shared chat key: {e}"))?;
    let shared_secret = SecretKey::from_slice(&shared_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to build SecretKey from shared bytes: {e}"))?;
    let shared_keys = Keys::new(shared_secret);
    Ok(SharedChatKeys { shared_keys })
}

/// Wrap a plain text message into a simplified NIP-59 gift wrap event suitable
/// for admin ↔ user chat.
///
/// - The inner event is a `Kind::TextNote` signed by `sender` (admin identity).
/// - The content is encrypted using NIP-44 with an ephemeral key and the shared
///   chat public key (derived via ECDH).
/// - The outer event is a `Kind::GiftWrap` with PoW difficulty configured from
///   settings.
async fn build_chat_giftwrap_event(
    sender: &Keys,
    shared_keys: &Keys,
    message: &str,
) -> Result<Event> {
    // Inner event: text note signed by admin identity
    let inner_event = EventBuilder::text_note(message)
        .build(sender.public_key())
        .sign(sender)
        .await?;

    // Ephemeral keys for encryption and signing the wrapper
    let ephemeral = Keys::generate();

    // Encrypt inner event JSON using NIP-44 (V2) with ephemeral secret and shared chat pubkey
    let encrypted_content = nip44::encrypt(
        ephemeral.secret_key(),
        &shared_keys.public_key(),
        inner_event.as_json(),
        nip44::Version::V2,
    )
    .map_err(|e| anyhow::anyhow!("Failed to encrypt chat message: {e}"))?;

    // Load PoW difficulty from settings (same field used for other NIP-59 messages)
    let pow: u8 = SETTINGS
        .get()
        .ok_or_else(|| {
            anyhow::anyhow!("Settings not initialized. Please restart the application.")
        })?
        .pow;

    // Basic tag to indicate receiver (shared chat pubkey)
    let tags = vec![Tag::public_key(shared_keys.public_key())];

    let wrapped_event = EventBuilder::new(Kind::GiftWrap, encrypted_content)
        .pow(pow)
        .tags(tags)
        .custom_created_at(Timestamp::tweaked(nip59::RANGE_RANDOM_TIMESTAMP_TWEAK))
        .sign_with_keys(&ephemeral)?;

    Ok(wrapped_event)
}

/// Send a chat message from the admin identity to a counterparty using the
/// provided shared chat keys.
pub async fn send_admin_chat_message(
    client: &Client,
    admin_keys: &Keys,
    shared_chat: &SharedChatKeys,
    content: &str,
) -> Result<()> {
    let event = build_chat_giftwrap_event(admin_keys, &shared_chat.shared_keys, content).await?;
    client
        .send_event(&event)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send admin chat event: {e}"))?;
    Ok(())
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

/// Send a chat message from the admin identity to a counterparty's trade pubkey.
/// Used for admin chat in disputes (buyer_pubkey / seller_pubkey from admin_dispute).
/// Message is trimmed to avoid leading/trailing whitespace affecting encoding.
pub async fn send_admin_chat_message_to_pubkey(
    client: &Client,
    admin_keys: &Keys,
    recipient_pubkey: &PublicKey,
    content: &str,
) -> Result<()> {
    let content = content.trim();
    if content.is_empty() {
        return Err(anyhow::anyhow!("Cannot send empty admin chat message"));
    }
    let event = build_chat_giftwrap_event_to_pubkey(admin_keys, recipient_pubkey, content).await?;
    client
        .send_event(&event)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send admin chat event: {e}"))?;
    Ok(())
}

/// Extract display text from rumor content. If content is Mostro protocol format
/// (Message::Dm(SendDm, TextMessage(s))), return s; otherwise return content as-is.
fn extract_chat_content_from_rumor(rumor_content: &str) -> String {
    if let Ok((msg, _)) = serde_json::from_str::<(Message, Option<String>)>(rumor_content) {
        if let Message::Dm(kind) = msg {
            if kind.action == Action::SendDm {
                if let Some(Payload::TextMessage(s)) = kind.payload {
                    return s;
                }
            }
        }
    }
    rumor_content.to_string()
}

/// Unwrap a gift wrap event addressed to the admin (decrypt with admin keys).
/// Returns (content, timestamp, sender_pubkey). Tries standard NIP-59 (Rumor in Seal in GW) first,
/// then falls back to legacy format (signed event directly in GW) for backward compatibility.
pub async fn unwrap_giftwrap_to_admin(
    admin_keys: &Keys,
    event: &Event,
) -> Result<(String, u64, PublicKey)> {
    // Standard NIP-59: GW content decrypts to Seal, Seal content decrypts to Rumor
    if let Ok(unwrapped) = nip59::UnwrappedGift::from_gift_wrap(admin_keys, event).await {
        let content = extract_chat_content_from_rumor(&unwrapped.rumor.content);
        return Ok((
            content,
            unwrapped.rumor.created_at.as_u64(),
            unwrapped.sender,
        ));
    }

    // Legacy: GW content decrypts directly to signed Event (old Mostrix format)
    let decrypted = nip44::decrypt(admin_keys.secret_key(), &event.pubkey, &event.content)
        .map_err(|e| anyhow::anyhow!("Failed to decrypt gift wrap to admin: {e}"))?;

    let inner_event = Event::from_json(&decrypted)
        .map_err(|e| anyhow::anyhow!("Invalid inner chat event: {e}"))?;

    inner_event
        .verify()
        .map_err(|e| anyhow::anyhow!("Invalid inner chat event signature: {e}"))?;

    Ok((
        inner_event.content,
        inner_event.created_at.as_u64(),
        inner_event.pubkey,
    ))
}

/// Fetch all gift wrap events addressed to the admin, decrypt with admin keys.
/// Returns (content, timestamp, sender_pubkey) for each message. Caller routes by sender_pubkey to (dispute_id, party).
pub async fn fetch_gift_wraps_to_admin(
    client: &Client,
    admin_keys: &Keys,
) -> Result<Vec<(String, u64, PublicKey)>> {
    let now = Timestamp::now().as_u64();
    let seven_days_secs: u64 = 7 * 24 * 60 * 60;
    let wide_since = now.saturating_sub(seven_days_secs);

    let admin_pubkey = admin_keys.public_key();
    // Fetch gift wraps in window; relay filter cannot target p tag in all SDKs, so we filter in code
    let filter = Filter::new()
        .kind(Kind::GiftWrap)
        .since(Timestamp::from(wide_since))
        .limit(200);

    let events = client
        .fetch_events(filter, FETCH_EVENTS_TIMEOUT)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch admin chat events: {e}"))?;

    let mut messages = Vec::new();
    for wrapped in events.iter() {
        // Only process events addressed to admin (p tag = admin_pubkey)
        let to_admin = wrapped.tags.public_keys().any(|pk| *pk == admin_pubkey);
        if !to_admin {
            continue;
        }
        match unwrap_giftwrap_to_admin(admin_keys, wrapped).await {
            Ok((content, ts, sender_pubkey)) => {
                messages.push((content, ts, sender_pubkey));
            }
            Err(e) => {
                log::warn!("Failed to unwrap gift wrap to admin {}: {}", wrapped.id, e);
            }
        }
    }
    messages.sort_by_key(|(_, ts, _)| *ts);
    Ok(messages)
}

/// Unwrap a chat gift wrap event for the given shared chat keys and return the
/// decrypted inner event (text note).
pub async fn unwrap_admin_chat_event(shared_chat: &SharedChatKeys, event: &Event) -> Result<Event> {
    let decrypted = nip44::decrypt(
        shared_chat.shared_keys.secret_key(),
        &event.pubkey,
        &event.content,
    )
    .map_err(|e| anyhow::anyhow!("Failed to decrypt chat gift wrap: {e}"))?;

    let inner_event = Event::from_json(&decrypted)
        .map_err(|e| anyhow::anyhow!("Invalid inner chat event: {e}"))?;

    inner_event
        .verify()
        .map_err(|e| anyhow::anyhow!("Invalid inner chat event signature: {e}"))?;

    Ok(inner_event)
}

/// Convenience helper that derives the shared chat keys for a given counterparty
/// (using the configured admin private key), sends a chat message, and returns
/// the derived shared keys for caching in UI state.
pub async fn derive_and_send_admin_chat_message(
    client: &Client,
    counterparty_pubkey_str: &str,
    content: &str,
) -> Result<SharedChatKeys> {
    let settings = SETTINGS.get().ok_or_else(|| {
        anyhow::anyhow!("Settings not initialized. Please restart the application.")
    })?;

    if settings.admin_privkey.is_empty() {
        return Err(anyhow::anyhow!(
            "Admin private key not configured. Set admin_privkey in settings."
        ));
    }

    let admin_keys = Keys::parse(&settings.admin_privkey)
        .map_err(|e| anyhow::anyhow!("Invalid admin private key: {e}"))?;

    let counterparty_pubkey = PublicKey::parse(counterparty_pubkey_str)
        .map_err(|e| anyhow::anyhow!("Invalid counterparty public key: {e}"))?;

    let shared = derive_shared_chat_keys(&admin_keys, &counterparty_pubkey)?;

    // Log shared key for testing/verification with mostro-chat
    log::info!(
        "🔑 Shared chat key derived:\n  Admin pubkey (npub): {}\n  Admin pubkey (hex): {}\n  Counterparty pubkey (hex): {}\n  Shared key secret (hex): {}\n  Shared key pubkey (hex): {}",
        admin_keys.public_key().to_bech32().unwrap_or_else(|_| "invalid".to_string()),
        admin_keys.public_key().to_hex(),
        counterparty_pubkey.to_hex(),
        shared.shared_keys.secret_key().to_secret_hex(),
        shared.shared_keys.public_key().to_hex()
    );

    send_admin_chat_message(client, &admin_keys, &shared, content).await?;

    Ok(shared)
}

/// Fetch and decrypt chat messages for a given shared chat key.
///
/// Returns a list of (content, timestamp, sender_pubkey) tuples, ordered by
/// timestamp ascending. If `since` is provided, only messages whose inner
/// (canonical) `created_at` > since are returned.
///
/// The relay filter always uses a wide time window (7 days). NIP-59 requires
/// gift wrap layer timestamps to be tweaked to the past, so filtering by
/// outer `created_at` would drop new messages; we filter by inner timestamp
/// after unwrapping instead.
pub async fn fetch_chat_messages_for_shared_key(
    client: &Client,
    shared_chat: &SharedChatKeys,
    since: Option<u64>,
) -> Result<Vec<(String, u64, PublicKey)>> {
    let now = Timestamp::now().as_u64();
    let seven_days_secs: u64 = 7 * 24 * 60 * 60;
    let wide_since = now.saturating_sub(seven_days_secs);

    let filter = Filter::new()
        .kind(Kind::GiftWrap)
        .pubkey(shared_chat.shared_keys.public_key())
        .since(Timestamp::from(wide_since))
        .limit(50);

    let events = client
        .fetch_events(filter, FETCH_EVENTS_TIMEOUT)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch admin chat events: {e}"))?;

    let mut messages: Vec<(String, u64, PublicKey)> = Vec::new();

    for wrapped in events.iter() {
        match unwrap_admin_chat_event(shared_chat, wrapped).await {
            Ok(inner) => {
                let ts = inner.created_at.as_u64();
                if let Some(since_ts) = since {
                    if ts <= since_ts {
                        continue;
                    }
                }
                messages.push((inner.content.clone(), ts, inner.pubkey));
            }
            Err(e) => {
                log::warn!("Failed to unwrap admin chat event {}: {}", wrapped.id, e);
                continue;
            }
        }
    }

    messages.sort_by_key(|(_, ts, _)| *ts);
    Ok(messages)
}

/// Fetch gift wraps to admin, route by sender_pubkey to (dispute_id, party), filter by last_seen.
pub async fn fetch_admin_chat_updates(
    client: &Client,
    admin_keys: &Keys,
    disputes: &[AdminDispute],
    admin_chat_last_seen: &HashMap<(String, ChatParty), AdminChatLastSeen>,
) -> Result<Vec<AdminChatUpdate>, anyhow::Error> {
    let all_messages = fetch_gift_wraps_to_admin(client, admin_keys).await?;

    // Build map: sender_pubkey (trade key) -> (dispute_id, party)
    let mut pubkey_to_dispute_party: Vec<(PublicKey, String, ChatParty)> = Vec::new();
    for d in disputes {
        if let Some(ref pk_str) = d.buyer_pubkey {
            if let Ok(pk) = PublicKey::parse(pk_str) {
                pubkey_to_dispute_party.push((pk, d.dispute_id.clone(), ChatParty::Buyer));
            }
        }
        if let Some(ref pk_str) = d.seller_pubkey {
            if let Ok(pk) = PublicKey::parse(pk_str) {
                pubkey_to_dispute_party.push((pk, d.dispute_id.clone(), ChatParty::Seller));
            }
        }
    }

    // Group messages by (dispute_id, party), filtering by last_seen
    let mut by_key: AdminChatByKey = HashMap::new();

    for (content, ts, sender_pubkey) in all_messages {
        let key = pubkey_to_dispute_party
            .iter()
            .find(|(pk, _, _)| pk == &sender_pubkey)
            .map(|(_, id, party)| (id.clone(), *party));
        let (dispute_id, party) = match key {
            Some(k) => k,
            None => continue,
        };
        let last_seen = admin_chat_last_seen
            .get(&(dispute_id.clone(), party))
            .and_then(|s| s.last_seen_timestamp)
            .unwrap_or(0);
        if ts <= last_seen {
            continue;
        }
        by_key
            .entry((dispute_id, party))
            .or_default()
            .push((content, ts, sender_pubkey));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::SETTINGS;

    #[test]
    fn derive_shared_chat_keys_is_symmetric() {
        let admin_keys = Keys::generate();
        let user_keys = Keys::generate();

        let shared_from_admin =
            derive_shared_chat_keys(&admin_keys, &user_keys.public_key()).unwrap();

        let shared_from_user =
            nostr_sdk::util::generate_shared_key(user_keys.secret_key(), &admin_keys.public_key())
                .expect("generate_shared_key should succeed");

        assert_eq!(
            shared_from_admin
                .shared_keys
                .secret_key()
                .secret_bytes()
                .as_slice(),
            shared_from_user.as_slice(),
            "ECDH shared secret should be symmetric"
        );
    }

    #[tokio::test]
    async fn build_and_unwrap_chat_event_roundtrip() {
        // Ensure SETTINGS is initialized with a reasonable PoW value
        let _ = SETTINGS.set(Settings {
            pow: 0, // disable PoW for tests
            ..Settings::default()
        });

        let admin_keys = Keys::generate();
        let counterparty_keys = Keys::generate();

        let shared = derive_shared_chat_keys(&admin_keys, &counterparty_keys.public_key()).unwrap();

        let message = "hello from admin chat";

        let wrapped = build_chat_giftwrap_event(&admin_keys, &shared.shared_keys, message)
            .await
            .unwrap();

        let inner = unwrap_admin_chat_event(&shared, &wrapped).await.unwrap();

        assert_eq!(inner.content, message);
        assert_eq!(inner.pubkey, admin_keys.public_key());
    }
}
