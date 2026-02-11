use std::collections::HashMap;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use mostro_core::prelude::{Action, Message, Payload};
use nostr_sdk::prelude::*;

use crate::models::AdminDispute;
use crate::ui::{AdminChatLastSeen, AdminChatUpdate, ChatParty};
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;

/// Messages grouped by (dispute_id, party); value is (content, timestamp, sender_pubkey).
type AdminChatByKey = HashMap<(String, ChatParty), Vec<(String, i64, PublicKey)>>;

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
    if let Ok((Message::Dm(kind), _)) =
        serde_json::from_str::<(Message, Option<String>)>(rumor_content)
    {
        if let Some(Payload::TextMessage(s)) = kind.payload {
            return s;
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
) -> Result<(String, i64, PublicKey)> {
    // Standard NIP-59: GW content decrypts to Seal, Seal content decrypts to Rumor
    if let Ok(unwrapped) = nip59::UnwrappedGift::from_gift_wrap(admin_keys, event).await {
        let content = extract_chat_content_from_rumor(&unwrapped.rumor.content);
        return Ok((
            content,
            unwrapped.rumor.created_at.as_u64() as i64,
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
        inner_event.created_at.as_u64() as i64,
        inner_event.pubkey,
    ))
}

/// Fetch all gift wrap events addressed to the admin, decrypt with admin keys.
/// Returns (content, timestamp, sender_pubkey) for each message. Caller routes by sender_pubkey to (dispute_id, party).
pub async fn fetch_gift_wraps_to_admin(
    client: &Client,
    admin_keys: &Keys,
) -> Result<Vec<(String, i64, PublicKey)>> {
    let now = Timestamp::now().as_u64();
    let seven_days_secs: u64 = 7 * 24 * 60 * 60;
    let wide_since = now.saturating_sub(seven_days_secs);

    let admin_pubkey = admin_keys.public_key();
    // Fetch gift wraps in window; relay filter cannot target p tag in all SDKs, so we filter in code
    let filter = Filter::new()
        .kind(Kind::GiftWrap)
        .pubkey(admin_pubkey)
        .since(Timestamp::from(wide_since))
        .limit(100);

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

/// Fetch gift wraps to admin, route by sender_pubkey to (dispute_id, party), filter by last_seen.
pub async fn fetch_admin_chat_updates(
    client: &Client,
    admin_keys: &Keys,
    disputes: &[AdminDispute],
    admin_chat_last_seen: &HashMap<(String, ChatParty), AdminChatLastSeen>,
) -> Result<Vec<AdminChatUpdate>, anyhow::Error> {
    let all_messages = fetch_gift_wraps_to_admin(client, admin_keys).await?;

    // Build map: sender_pubkey (trade key) -> (dispute_id, party) for O(1) lookups
    let mut pubkey_to_dispute_party: HashMap<PublicKey, (String, ChatParty)> = HashMap::new();
    for d in disputes {
        if let Some(ref pk_str) = d.buyer_pubkey {
            if let Ok(pk) = PublicKey::parse(pk_str) {
                pubkey_to_dispute_party.insert(pk, (d.dispute_id.clone(), ChatParty::Buyer));
            }
        }
        if let Some(ref pk_str) = d.seller_pubkey {
            if let Ok(pk) = PublicKey::parse(pk_str) {
                pubkey_to_dispute_party.insert(pk, (d.dispute_id.clone(), ChatParty::Seller));
            }
        }
    }

    // Group messages by (dispute_id, party), filtering by last_seen
    let mut by_key: AdminChatByKey = HashMap::new();

    for (content, ts, sender_pubkey) in all_messages {
        let (dispute_id, party) = match pubkey_to_dispute_party.get(&sender_pubkey) {
            Some((id, party)) => (id.clone(), *party),
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
