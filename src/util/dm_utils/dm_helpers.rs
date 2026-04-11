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

use crate::ui::{AdminChatLastSeen, AppState, ChatParty};
use crate::util::filters::filter_giftwrap_to_recipient;
use crate::util::types::create_expiration_tags;

// nip59::RANGE_RANDOM_TIMESTAMP_TWEAK — you already use `nip59::` elsewhere in the project
fn gift_wrap_from_seal_with_pow(
    receiver: &PublicKey,
    seal: &Event,
    extra_tags: impl IntoIterator<Item = Tag>,
    pow: u8,
) -> Result<Event> {
    // or map to anyhow in create_gift_wrap_event
    if seal.kind != nostr_sdk::Kind::Seal {
        // same validation as upstream, or map_err to anyhow
        return Err(anyhow::anyhow!("Invalid kind"));
    }

    let ephem = Keys::generate();
    let content = nip44::encrypt(
        ephem.secret_key(),
        receiver,
        seal.as_json(),
        nip44::Version::default(),
    )?;

    let mut tags: Vec<Tag> = extra_tags.into_iter().collect();
    tags.push(Tag::public_key(*receiver));

    EventBuilder::new(nostr_sdk::Kind::GiftWrap, content)
        .tags(tags)
        .custom_created_at(Timestamp::tweaked(nip59::RANGE_RANDOM_TIMESTAMP_TWEAK))
        .pow(pow) // <-- this is what the SDK’s gift_wrap path does NOT do
        .sign_with_keys(&ephem)
        .map_err(|e| anyhow::anyhow!("Failed to sign gift wrap: {e}"))
}

/// Subscription behavior for GiftWrap filters.
pub(crate) enum GiftWrapSubscriptionMode {
    /// Startup catch-up: request the latest retained event for this pubkey.
    StartupCatchUp,
    /// Startup catch-up from the persisted cursor timestamp.
    StartupSince(i64),
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

    let seal: Event = EventBuilder::seal(signer_keys, receiver_pubkey, rumor)
        .await?
        .sign(signer_keys)
        .await?;

    gift_wrap_from_seal_with_pow(receiver_pubkey, &seal, tags, pow)
}

/// Subscribe GiftWrap for a trade pubkey and remember the returned subscription id.
/// Returns `true` when subscription is active (already subscribed or newly subscribed).
pub(crate) async fn ensure_order_giftwrap_subscription(
    client: &Client,
    subscribed_pubkeys: &mut HashSet<PublicKey>,
    subscription_to_order: &mut HashMap<SubscriptionId, (Uuid, i64)>,
    pubkey_to_subscription: &mut HashMap<PublicKey, SubscriptionId>,
    pubkey: PublicKey,
    options: GiftWrapOrderSubscription,
) -> bool {
    if !subscribed_pubkeys.insert(pubkey) {
        // Already subscribed: keep the tracked mapping fresh (e.g. post-take-order TrackOrder
        // rebinding from optimistic order_id -> effective_order_id).
        if let Some(sub_id) = pubkey_to_subscription.get(&pubkey).cloned() {
            subscription_to_order.insert(sub_id, (options.order_id, options.trade_index));
            return true;
        }
        log::warn!(
            "[dm_listener] pubkey {} marked subscribed but missing subscription id; resubscribing to restore mapping",
            pubkey
        );
    }
    let filter = match options.mode {
        GiftWrapSubscriptionMode::StartupCatchUp => filter_giftwrap_to_recipient(pubkey).limit(1),
        GiftWrapSubscriptionMode::StartupSince(ts) => {
            let ts = u64::try_from(ts).unwrap_or(Timestamp::now().as_u64());
            let since_ts = ts.saturating_sub(super::STARTUP_GIFTWRAP_ENVELOPE_SKEW_SECS);
            filter_giftwrap_to_recipient(pubkey).since(Timestamp::from(since_ts))
        }
        // Live-only: match `RegisterWaiter` in `listen_for_order_messages` (`.limit(0)`).
        // `take_order` sends `TrackOrder` before `wait_for_dm`, so this subscription is created
        // first; if we used `.since(now)` here, same-second Mostro responses could be missed and
        // `RegisterWaiter` would not add a second subscription (pubkey already subscribed).
        GiftWrapSubscriptionMode::LiveOnly => filter_giftwrap_to_recipient(pubkey).limit(0),
    };

    match client.subscribe(filter, None).await {
        Ok(output) => {
            let sub_id = output.val;
            if let Some(label) = options.info_label {
                log::info!(
                    "{} subscription_id={}, order_id={}, trade_index={}",
                    label,
                    sub_id,
                    options.order_id,
                    options.trade_index
                );
            }
            pubkey_to_subscription.insert(pubkey, sub_id.clone());
            subscription_to_order.insert(sub_id, (options.order_id, options.trade_index));
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
            pubkey_to_subscription.remove(&pubkey);
            false
        }
    }
}

/// Seed `app.admin_chat_last_seen` with last_seen timestamps per (dispute, party)
/// from the list of admin disputes (DB fields buyer_chat_last_seen / seller_chat_last_seen).
pub fn seed_admin_chat_last_seen(app: &mut AppState) {
    for dispute in &app.admin_disputes_in_progress {
        if dispute.buyer_pubkey.is_some() {
            app.admin_chat_last_seen.insert(
                (dispute.dispute_id.clone(), ChatParty::Buyer),
                AdminChatLastSeen {
                    last_seen_timestamp: dispute.buyer_chat_last_seen,
                },
            );
        }
        if dispute.seller_pubkey.is_some() {
            app.admin_chat_last_seen.insert(
                (dispute.dispute_id.clone(), ChatParty::Seller),
                AdminChatLastSeen {
                    last_seen_timestamp: dispute.seller_chat_last_seen,
                },
            );
        }
    }
}
