// Helper functions for direct message operations
use mostro_core::prelude::Transport;
use nostr_sdk::prelude::*;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::ui::{AdminChatLastSeen, AppState, ChatParty};
use crate::util::filters::filter_protocol_dm_from_mostro;

/// Subscription behavior for protocol DM filters (signed kind 14).
pub(crate) enum DmSubscriptionMode {
    /// Startup catch-up: request the latest retained event for this pubkey.
    StartupCatchUp,
    /// Startup catch-up from the persisted cursor timestamp.
    StartupSince(i64),
    /// Live-only stream: no backlog replay, only events after subscription.
    LiveOnly,
    /// Stored events since an in-flight waiter registered, then live (reconnect resurrection).
    WaiterCatchUp(Timestamp),
}

/// Metadata/config used when binding a subscription id to an order.
pub(crate) struct DmOrderSubscription {
    pub(crate) order_id: Uuid,
    pub(crate) trade_index: i64,
    pub(crate) error_label: &'static str,
    pub(crate) info_label: Option<&'static str>,
    pub(crate) mode: DmSubscriptionMode,
}

/// Subscribe for protocol DMs on the trade pubkey and remember the returned subscription id.
/// Returns `true` when subscription is active (already subscribed or newly subscribed).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn ensure_order_dm_subscription(
    client: &Client,
    transport: Transport,
    mostro_pubkey: PublicKey,
    subscribed_pubkeys: &mut HashSet<PublicKey>,
    subscription_to_order: &mut HashMap<SubscriptionId, (Uuid, i64)>,
    pubkey_to_subscription: &mut HashMap<PublicKey, SubscriptionId>,
    trade_pubkey: PublicKey,
    options: DmOrderSubscription,
) -> bool {
    if !subscribed_pubkeys.insert(trade_pubkey) {
        // Already subscribed: keep the tracked mapping fresh (e.g. post-take-order TrackOrder
        // rebinding from optimistic order_id -> effective_order_id).
        if let Some(sub_id) = pubkey_to_subscription.get(&trade_pubkey).cloned() {
            subscription_to_order.insert(sub_id, (options.order_id, options.trade_index));
            return true;
        }
        log::warn!(
            "[dm_listener] pubkey {} marked subscribed but missing subscription id; resubscribing to restore mapping",
            trade_pubkey
        );
    }
    let base = filter_protocol_dm_from_mostro(transport, mostro_pubkey, trade_pubkey);
    let filter = protocol_dm_filter_for_mode(base, &options.mode);

    match client.subscribe(filter).await {
        Ok(output) => {
            let sub_id = output.value;
            if let Some(label) = options.info_label {
                log::info!(
                    "{} subscription_id={}, order_id={}, trade_index={}",
                    label,
                    sub_id,
                    options.order_id,
                    options.trade_index
                );
            }
            pubkey_to_subscription.insert(trade_pubkey, sub_id.clone());
            subscription_to_order.insert(sub_id.clone(), (options.order_id, options.trade_index));
            super::register_dm_listener_subscription(sub_id);
            true
        }
        Err(e) => {
            log::warn!(
                "{} {} (index {}): {}",
                options.error_label,
                trade_pubkey,
                options.trade_index,
                e
            );
            subscribed_pubkeys.remove(&trade_pubkey);
            pubkey_to_subscription.remove(&trade_pubkey);
            false
        }
    }
}

fn protocol_dm_filter_for_mode(base: Filter, mode: &DmSubscriptionMode) -> Filter {
    match mode {
        DmSubscriptionMode::StartupCatchUp => base.limit(1),
        DmSubscriptionMode::StartupSince(ts) => {
            let ts = u64::try_from(*ts).unwrap_or(Timestamp::now().as_secs());
            base.since(Timestamp::from(ts))
        }
        // Live-only: match `RegisterWaiter` in `listen_for_order_messages` (`.limit(0)`).
        // `take_order` sends `TrackOrder` before `wait_for_dm`, so this subscription is created
        // first; if we used `.since(now)` here, same-second Mostro responses could be missed and
        // `RegisterWaiter` would not add a second subscription (pubkey already subscribed).
        DmSubscriptionMode::LiveOnly => base.limit(0),
        DmSubscriptionMode::WaiterCatchUp(ts) => base.since(*ts),
    }
}

/// Immediate subscribe attempts for a waiter pubkey (no sleep; GC retries with catch-up).
const WAITER_SUBSCRIBE_ATTEMPTS: u8 = 2;

/// Waiter pubkeys that still need a relay subscription after a failed LiveOnly subscribe.
pub(crate) fn waiter_targets_needing_subscription(
    targets: &[(PublicKey, Timestamp)],
    subscribed_pubkeys: &HashSet<PublicKey>,
    pubkey_to_subscription: &HashMap<PublicKey, SubscriptionId>,
) -> Vec<(PublicKey, Timestamp)> {
    targets
        .iter()
        .copied()
        .filter(|(pk, _)| {
            !subscribed_pubkeys.contains(pk) || !pubkey_to_subscription.contains_key(pk)
        })
        .collect()
}

pub(crate) fn note_waiter_subscribe_failure(
    subscribed_pubkeys: &mut HashSet<PublicKey>,
    pubkey_to_subscription: &mut HashMap<PublicKey, SubscriptionId>,
    trade_pubkey: PublicKey,
) {
    subscribed_pubkeys.remove(&trade_pubkey);
    pubkey_to_subscription.remove(&trade_pubkey);
}

/// Subscribe for a `wait_for_dm` trade pubkey without binding `subscription_id` → order.
///
/// Returns `true` when a subscription is already active or was created. Subscribe
/// failure does **not** drop the waiter — it lives in the process-wide registry until
/// timeout or match. A failed attempt is cleared from the local maps so a later
/// [`WaiterCatchUp`] retry (GC tick or immediate fallback) can subscribe again.
/// Live subscribe is tried up to [`WAITER_SUBSCRIBE_ATTEMPTS`] times without sleeping.
pub(crate) async fn ensure_waiter_dm_subscription(
    client: &Client,
    transport: Transport,
    mostro_pubkey: PublicKey,
    subscribed_pubkeys: &mut HashSet<PublicKey>,
    pubkey_to_subscription: &mut HashMap<PublicKey, SubscriptionId>,
    trade_pubkey: PublicKey,
    mode: DmSubscriptionMode,
) -> bool {
    if !subscribed_pubkeys.insert(trade_pubkey) {
        if pubkey_to_subscription.contains_key(&trade_pubkey) {
            return true;
        }
        log::warn!(
            "[dm_listener] waiter pubkey {} marked subscribed but missing subscription id; retrying",
            trade_pubkey
        );
    }
    let mut last_err: Option<String> = None;
    for _ in 0..WAITER_SUBSCRIBE_ATTEMPTS {
        let base = filter_protocol_dm_from_mostro(transport, mostro_pubkey, trade_pubkey);
        let filter = protocol_dm_filter_for_mode(base, &mode);
        match client.subscribe(filter).await {
            Ok(output) => {
                let sub_id = output.value;
                pubkey_to_subscription.insert(trade_pubkey, sub_id.clone());
                super::register_dm_listener_subscription(sub_id);
                return true;
            }
            Err(e) => {
                last_err = Some(e.to_string());
            }
        }
    }
    note_waiter_subscribe_failure(subscribed_pubkeys, pubkey_to_subscription, trade_pubkey);
    if let Some(e) = last_err {
        log::warn!("Failed to subscribe waiter pubkey {trade_pubkey}: {e}");
    }
    false
}

/// Seed `app.admin_chat_last_seen` with last_seen timestamps per (dispute, party)
/// from the list of admin disputes (DB fields buyer_chat_last_seen / seller_chat_last_seen).
pub fn seed_admin_chat_last_seen(app: &mut AppState) {
    for dispute in &app.admin_disputes_in_progress {
        if dispute.buyer_pubkey.is_some() {
            app.admin_chat_last_seen.insert(
                (dispute.dispute_id.clone(), ChatParty::Buyer),
                AdminChatLastSeen {
                    last_seen_timestamp: dispute
                        .buyer_chat_last_seen
                        .map(crate::util::chat_utils::clamp_chat_since_cursor_now),
                },
            );
        }
        if dispute.seller_pubkey.is_some() {
            app.admin_chat_last_seen.insert(
                (dispute.dispute_id.clone(), ChatParty::Seller),
                AdminChatLastSeen {
                    last_seen_timestamp: dispute
                        .seller_chat_last_seen
                        .map(crate::util::chat_utils::clamp_chat_since_cursor_now),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mostro_core::prelude::Transport;

    #[test]
    fn waiter_catch_up_filter_includes_since() {
        let trade = Keys::generate().public_key();
        let mostro = Keys::generate().public_key();
        let base = filter_protocol_dm_from_mostro(Transport::Nip44Direct, mostro, trade);
        let ts = Timestamp::from(1_700_000_000);
        let filter = protocol_dm_filter_for_mode(base, &DmSubscriptionMode::WaiterCatchUp(ts));
        let json = filter.as_json();
        assert!(json.contains("\"since\":1700000000"));
        assert!(json.contains(&format!("\"#p\":[\"{trade}\"]")));
    }

    #[test]
    fn live_only_waiter_filter_uses_limit_zero() {
        let trade = Keys::generate().public_key();
        let mostro = Keys::generate().public_key();
        let base = filter_protocol_dm_from_mostro(Transport::Nip44Direct, mostro, trade);
        let filter = protocol_dm_filter_for_mode(base, &DmSubscriptionMode::LiveOnly);
        let json = filter.as_json();
        assert!(json.contains("\"limit\":0"));
    }

    #[test]
    fn failed_live_subscribe_is_retried_then_recovers_on_success() {
        let pk = Keys::generate().public_key();
        let since = Timestamp::from(1_700_000_000);
        let mut subscribed = HashSet::new();
        let mut pubkey_to_subscription = HashMap::new();
        let targets = vec![(pk, since)];

        subscribed.insert(pk);
        note_waiter_subscribe_failure(&mut subscribed, &mut pubkey_to_subscription, pk);
        assert!(!subscribed.contains(&pk));
        assert_eq!(
            waiter_targets_needing_subscription(&targets, &subscribed, &pubkey_to_subscription),
            vec![(pk, since)],
            "after relay subscribe failure the waiter must still be eligible for catch-up retry"
        );

        subscribed.insert(pk);
        pubkey_to_subscription.insert(pk, SubscriptionId::generate());
        assert!(
            waiter_targets_needing_subscription(&targets, &subscribed, &pubkey_to_subscription)
                .is_empty(),
            "successful resubscription must stop retrying this pubkey"
        );
    }

    #[test]
    fn missing_subscription_id_still_needs_retry() {
        let pk = Keys::generate().public_key();
        let since = Timestamp::from(1);
        let mut subscribed = HashSet::new();
        subscribed.insert(pk);
        let pubkey_to_subscription = HashMap::new();
        assert_eq!(
            waiter_targets_needing_subscription(
                &[(pk, since)],
                &subscribed,
                &pubkey_to_subscription
            ),
            vec![(pk, since)]
        );
    }
}
