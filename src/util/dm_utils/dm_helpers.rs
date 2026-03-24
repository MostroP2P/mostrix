// Helper functions for direct message operations
use anyhow::{Error, Result};
use base64::engine::general_purpose;
use base64::Engine;
use mostro_core::prelude::*;
use nip44::v2::encrypt_to_bytes;
use nip44::v2::ConversationKey;
use nostr_sdk::prelude::*;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::util::types::create_expiration_tags;

/// Subscription behavior for GiftWrap filters.
pub(crate) enum GiftWrapSubscriptionMode {
    /// Startup catch-up: request the latest retained event for this pubkey.
    StartupCatchUp,
    /// Live-only stream: no backlog replay, only events after subscription.
    LiveOnly,
}

/// Metadata/config used when binding a subscription id to an order.
pub(crate) struct GiftWrapOrderSubscription {
    pub(crate) order_id: Uuid,
    pub(crate) trade_index: i64,
    pub(crate) error_label: &'static str,
    pub(crate) info_label: Option<&'static str>,
    pub(crate) mode: GiftWrapSubscriptionMode,
}

/// Create a private direct message event
pub(crate) async fn create_private_dm_event(
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

/// Create a gift wrap event (private or signed)
pub(crate) async fn create_gift_wrap_event(
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

/// Subscribe GiftWrap for a trade pubkey and remember the returned subscription id.
/// Returns `true` when subscription is active (already subscribed or newly subscribed).
pub(crate) async fn ensure_order_giftwrap_subscription(
    client: &Client,
    subscribed_pubkeys: &mut HashSet<PublicKey>,
    subscription_to_order: &mut HashMap<SubscriptionId, (Uuid, i64)>,
    pubkey: PublicKey,
    options: GiftWrapOrderSubscription,
) -> bool {
    if !subscribed_pubkeys.insert(pubkey) {
        return true;
    }
    // get the limit for the subscription
    let limit = match options.mode {
        GiftWrapSubscriptionMode::StartupCatchUp => 1,
        GiftWrapSubscriptionMode::LiveOnly => 0,
    };

    let filter = Filter::new()
        .pubkey(pubkey)
        .kind(nostr_sdk::Kind::GiftWrap)
        .limit(limit);

    match client.subscribe(filter, None).await {
        Ok(output) => {
            if let Some(label) = options.info_label {
                log::info!(
                    "{} subscription_id={}, order_id={}, trade_index={}",
                    label,
                    output.val,
                    options.order_id,
                    options.trade_index
                );
            }
            subscription_to_order.insert(output.val, (options.order_id, options.trade_index));
            true
        }
        Err(e) => {
            log::warn!(
                "{} {} (index {}): {}",
                options.error_label,
                pubkey,
                options.trade_index,
                e
            );
            subscribed_pubkeys.remove(&pubkey);
            false
        }
    }
}
