use anyhow::Result;
use nostr_sdk::prelude::*;

use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use crate::SETTINGS;

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
/// timestamp ascending. If `since` is provided, only messages with a
/// `created_at` > since will be returned.
pub async fn fetch_chat_messages_for_shared_key(
    client: &Client,
    shared_chat: &SharedChatKeys,
    since: Option<u64>,
) -> Result<Vec<(String, u64, PublicKey)>> {
    let filter = {
        let mut f = Filter::new()
            .kind(Kind::GiftWrap)
            .pubkey(shared_chat.shared_keys.public_key())
            .limit(20);
        if let Some(since_ts) = since {
            f = f.since(Timestamp::from(since_ts));
        }
        f
    };

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
