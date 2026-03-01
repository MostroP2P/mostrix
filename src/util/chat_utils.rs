use std::collections::HashMap;
use std::str::FromStr;

use anyhow::Result;
use mostro_core::prelude::DisputeStatus;
use nostr_sdk::prelude::*;

use crate::models::AdminDispute;
use crate::ui::{AdminChatLastSeen, AdminChatUpdate, ChatParty};
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use crate::SETTINGS;

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

/// Build a NIP-59 gift wrap event to a recipient pubkey (e.g. shared key pubkey).
/// The inner content is a simple text note, not Mostro protocol format, as this is
/// used for admin chat messages which are plain text communications.
async fn build_custom_wrap_event(
    sender: &Keys,
    recipient_pubkey: &PublicKey,
    message: &str,
) -> Result<Event> {
    // Message is just sent inside rumor as per https://mostro.network/protocol/chat.html please check that.
    let inner_message = EventBuilder::text_note(message)
        .build(sender.public_key())
        .sign(sender)
        .await?;
    // Ephemeral key for the custom wrap
    let ephem_key = Keys::generate();
    // Encrypt the inner message with the ephemeral key using NIP-44
    let encrypted_content = nip44::encrypt(
        ephem_key.secret_key(),
        recipient_pubkey,
        inner_message.as_json(),
        nip44::Version::V2,
    )?;

    // Build tags for the wrapper event, the recipient pubkey is the shared key pubkey
    let tag = Tag::public_key(*recipient_pubkey);

    // Get the pow from the settings
    let pow: u8 = SETTINGS
        .get()
        .ok_or_else(|| {
            anyhow::anyhow!("Settings not initialized. Please restart the application.")
        })?
        .pow;
    // Build the wrapped event
    let wrapped_event = EventBuilder::new(Kind::GiftWrap, encrypted_content)
        .tag(tag)
        .custom_created_at(Timestamp::tweaked(nip59::RANGE_RANDOM_TIMESTAMP_TWEAK))
        .pow(pow)
        .sign_with_keys(&ephem_key)?;

    // Return the wrapped event
    Ok(wrapped_event)
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
    let event = build_custom_wrap_event(admin_keys, &recipient_pubkey, content).await?;
    // Send the event to the relay
    client
        .send_event(&event)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send admin chat event: {e}"))?;
    Ok(())
}

/// Unwrap a custom Mostro P2P giftwrap addressed to a shared key.
/// Decrypts with the shared key using NIP-44 and returns (content, timestamp, sender_pubkey).
pub async fn unwrap_giftwrap_with_shared_key(
    shared_keys: &Keys,
    event: &Event,
) -> Result<(String, i64, PublicKey)> {
    let decrypted = nip44::decrypt(shared_keys.secret_key(), &event.pubkey, &event.content)
        .map_err(|e| anyhow::anyhow!("Failed to decrypt gift wrap with shared key: {e}"))?;

    let inner_event = Event::from_json(&decrypted)
        .map_err(|e| anyhow::anyhow!("Invalid inner chat event: {e}"))?;

    inner_event
        .verify()
        .map_err(|e| anyhow::anyhow!("Invalid inner chat event signature: {e}"))?;

    Ok((
        inner_event.content,
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
}
