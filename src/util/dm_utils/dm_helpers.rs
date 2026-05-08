// Helper functions for direct message operations
use nostr_sdk::prelude::*;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::ui::{AdminChatLastSeen, AppState, ChatParty};
use crate::util::filters::filter_giftwrap_to_recipient;

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
            let ts = u64::try_from(ts).unwrap_or(Timestamp::now().as_secs());
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
