//! Shared-key chat subscription router (P2P order chat + admin dispute chat).
//!
//! Long-lived dual-read subscriptions (migration window):
//! * legacy GiftWrap (`kind: 1059`, `#p: [ECDH pubkeys]`)
//! * gift-wrap-free kind 14 (`authors: [pub(K_sign)]`)
//!
//! Live kind-14 traffic is routed **only** by outer author ∈ tracked `pub(K_sign)`
//! (never by `#p` alone). GiftWrap dual-read still matches `#p` = ECDH pubkey.
//! Hydration and last-seen cursors use `min(accepted_ts, local_now)` so a
//! far-future timestamp cannot poison `since`.
//!
//! Incoming events are decrypted with the per-channel ECDH secret (`K_conv` /
//! `K_sign` derived at unwrap), and emitted on the existing
//! `admin_chat_updates` / `user_order_chat_updates` channels so the
//! `apply_*_chat_updates` handlers stay unchanged.
//!
//! Lifecycle (see also `docs/DM_LISTENER_FLOW.md`): the task is spawned once at
//! startup and respawned on client reload/reconnect, exactly like the trade DM
//! listener. Chat keys are tracked/untracked via the global command channel
//! published by [`set_chat_router_cmd_tx`].

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use nostr_sdk::prelude::*;
use tokio::sync::mpsc::{self, Sender};
use uuid::Uuid;

use crate::models::Order;
use crate::ui::helpers::{
    load_dispute_chat_inner_ids, load_order_chat_inner_ids, load_user_dispute_chat_inner_ids,
    order_chat_since_from_file,
};
use crate::ui::{AdminChatUpdate, ChatParty, DecodedChatMessage, OrderChatUpdate, UserChatChannel};
use crate::util::chat_security::{
    try_emit_chat_update, ChatRateLimiters, OuterIdLru, CHAT_SEEN_OUTER_CAP,
};
use crate::util::chat_utils::{
    chat_keys_from_ecdh, clamp_chat_since_cursor_now, derive_shared_key_hex,
    fetch_chat_messages_for_shared_key, keys_from_shared_hex, unwrap_giftwrap_with_shared_key,
};

/// Identifies which chat a shared key belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatKeyId {
    /// User P2P order chat, keyed by order id (UUID string).
    Order(String),
    /// User-to-solver dispute chat, keyed by parent order id.
    UserDispute(String),
    /// Admin dispute chat, keyed by dispute id + party (buyer/seller).
    Dispute(String, ChatParty),
}

/// Commands consumed by [`listen_for_chat_messages`].
pub enum ChatRouterCmd {
    /// Start tracking a shared-key chat: hydrate history once, then include the
    /// key's pubkey in the batched live subscription.
    TrackChatKey {
        key_id: ChatKeyId,
        /// Hex of the ECDH shared secret (round-trips via [`keys_from_shared_hex`]).
        shared_key_hex: String,
        /// Local trade pubkey for order chats (used to skip relay echoes of our own sends).
        local_trade_pubkey: Option<PublicKey>,
        /// Verified inner signers accepted for this conversation.
        allowed_signers: Option<Vec<PublicKey>>,
        /// Only emit history messages at/after this unix timestamp (last-seen cursor).
        since: Option<i64>,
    },
    /// Stop tracking a shared-key chat (order row deleted / dispute finalized, etc.).
    UntrackChatKey { key_id: ChatKeyId },
}

/// Per-tracked-key routing metadata.
struct ChatTarget {
    key_id: ChatKeyId,
    /// ECDH IKM keys (persisted hex); GiftWrap `#p` lookup uses [`Keys::public_key`].
    shared_keys: Keys,
    /// `pub(K_sign)` — kind-14 outer author used for live `authors` filter / routing.
    sign_pubkey: PublicKey,
    /// Order chat only; enables echo-skip in `apply_user_order_chat_updates`.
    local_trade_pubkey: Option<PublicKey>,
    allowed_signers: Option<Vec<PublicKey>>,
}

/// Live subscription ids for the dual-read migration window.
#[derive(Default)]
struct LiveSubs {
    giftwrap: Option<SubscriptionId>,
    kind14: Option<SubscriptionId>,
}

impl LiveSubs {
    async fn clear(&mut self, client: &Client) {
        if let Some(id) = self.giftwrap.take() {
            client.unsubscribe(&id).await;
        }
        if let Some(id) = self.kind14.take() {
            client.unsubscribe(&id).await;
        }
    }
}

/// Global sender published for track/untrack helpers, mirroring the DM router's
/// `DM_ROUTER_CMD_TX`. Set at startup and on every chat-router respawn.
static CHAT_ROUTER_CMD_TX: Mutex<Option<mpsc::UnboundedSender<ChatRouterCmd>>> = Mutex::new(None);

/// Publishes the sender consumed by [`listen_for_chat_messages`].
///
/// Returns `Err` if the mutex is poisoned (the sender was **not** updated).
pub fn set_chat_router_cmd_tx(
    tx: mpsc::UnboundedSender<ChatRouterCmd>,
) -> Result<(), &'static str> {
    match CHAT_ROUTER_CMD_TX.lock() {
        Ok(mut guard) => {
            *guard = Some(tx);
            Ok(())
        }
        Err(_) => {
            crate::util::request_fatal_restart(
                "Mostrix encountered an internal error (poisoned chat router lock). Please restart the app."
                    .to_string(),
            );
            Err("CHAT_ROUTER_CMD_TX mutex poisoned")
        }
    }
}

fn send_chat_router_cmd(cmd: ChatRouterCmd) {
    if let Ok(guard) = CHAT_ROUTER_CMD_TX.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(cmd);
        }
    }
}

/// Track a user P2P order chat by its shared key.
pub fn track_order_chat(
    order_id: String,
    shared_key_hex: String,
    local_trade_pubkey: PublicKey,
    since: Option<i64>,
) {
    send_chat_router_cmd(ChatRouterCmd::TrackChatKey {
        key_id: ChatKeyId::Order(order_id),
        shared_key_hex,
        local_trade_pubkey: Some(local_trade_pubkey),
        allowed_signers: None,
        since,
    });
}

/// Track a user-to-solver dispute chat by its derived ECDH secret.
pub fn track_user_dispute_chat(
    order_id: String,
    shared_key_hex: String,
    local_trade_pubkey: PublicKey,
    solver_pubkey: PublicKey,
    since: Option<i64>,
) {
    send_chat_router_cmd(ChatRouterCmd::TrackChatKey {
        key_id: ChatKeyId::UserDispute(order_id),
        shared_key_hex,
        local_trade_pubkey: Some(local_trade_pubkey),
        allowed_signers: Some(vec![local_trade_pubkey, solver_pubkey]),
        since,
    });
}

/// Stop tracking a user P2P order chat (order row removed / terminal cancel).
pub fn untrack_order_chat(order_id: String) {
    send_chat_router_cmd(ChatRouterCmd::UntrackChatKey {
        key_id: ChatKeyId::Order(order_id),
    });
}

/// Stop tracking a user-to-solver dispute chat for an order.
pub fn untrack_user_dispute_chat(order_id: String) {
    send_chat_router_cmd(ChatRouterCmd::UntrackChatKey {
        key_id: ChatKeyId::UserDispute(order_id),
    });
}

/// Track an admin dispute chat party by its shared key.
pub fn track_dispute_chat(
    dispute_id: String,
    party: ChatParty,
    shared_key_hex: String,
    since: Option<i64>,
) {
    send_chat_router_cmd(ChatRouterCmd::TrackChatKey {
        key_id: ChatKeyId::Dispute(dispute_id, party),
        shared_key_hex,
        local_trade_pubkey: None,
        allowed_signers: None,
        since,
    });
}

/// Stop tracking an admin dispute chat party (dispute finalized / no longer InProgress).
pub fn untrack_dispute_chat(dispute_id: String, party: ChatParty) {
    send_chat_router_cmd(ChatRouterCmd::UntrackChatKey {
        key_id: ChatKeyId::Dispute(dispute_id, party),
    });
}

/// Stop live shared-key chat subscriptions for both buyer and seller parties.
pub fn untrack_dispute_chat_parties(dispute_id: &str) {
    untrack_dispute_chat(dispute_id.to_string(), ChatParty::Buyer);
    untrack_dispute_chat(dispute_id.to_string(), ChatParty::Seller);
}

/// Track a user P2P order chat once its shared key is resolvable (DM router hook).
///
/// Loads the order, resolves the shared key (persisted `order_chat_shared_key_hex`, else ECDH
/// from `trade_keys` + `counterparty_pubkey`), and emits a track command. Hydrate cutoff comes
/// from the on-disk transcript max timestamp when present (same cursor idea as startup).
/// Idempotent at the router level: re-tracking an already-tracked key is a cheap no-op.
pub async fn maybe_track_order_chat(pool: &sqlx::SqlitePool, order_id: Uuid, trade_keys: &Keys) {
    let order = match Order::get_by_id(pool, &order_id.to_string()).await {
        Ok(o) => o,
        Err(_) => return,
    };
    let shared_hex = order
        .order_chat_shared_key_hex
        .clone()
        .or_else(|| derive_shared_key_hex(Some(trade_keys), order.counterparty_pubkey.as_deref()));
    if let Some(hex) = shared_hex {
        let since = order_chat_since_from_file(&order_id.to_string());
        track_order_chat(order_id.to_string(), hex, trade_keys.public_key(), since);
    }
}

/// Emit decrypted chat messages on the appropriate update channel.
///
/// Reuses the same `AdminChatUpdate` / `OrderChatUpdate` shapes as the old
/// polling path so `apply_admin_chat_updates` / `apply_user_order_chat_updates`
/// (which dedupe by timestamp/last-seen) are unchanged.
fn emit_messages(
    target: &ChatTarget,
    messages: Vec<DecodedChatMessage>,
    admin_tx: &Sender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    user_tx: &Sender<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
) {
    if messages.is_empty() {
        return;
    }
    match &target.key_id {
        ChatKeyId::Order(order_id) => {
            let Some(local_trade_pubkey) = target.local_trade_pubkey else {
                log::warn!("Order chat {order_id} missing local trade pubkey; skipping chat emit");
                return;
            };
            let _ = try_emit_chat_update(
                user_tx,
                vec![OrderChatUpdate {
                    order_id: order_id.clone(),
                    channel: UserChatChannel::Peer,
                    local_trade_pubkey,
                    messages,
                }],
                "order-chat",
            );
        }
        ChatKeyId::UserDispute(order_id) => {
            let Some(local_trade_pubkey) = target.local_trade_pubkey else {
                log::warn!("User dispute chat {order_id} missing local trade pubkey");
                return;
            };
            let _ = try_emit_chat_update(
                user_tx,
                vec![OrderChatUpdate {
                    order_id: order_id.clone(),
                    channel: UserChatChannel::Solver,
                    local_trade_pubkey,
                    messages,
                }],
                "solver-chat",
            );
        }
        ChatKeyId::Dispute(dispute_id, party) => {
            let _ = try_emit_chat_update(
                admin_tx,
                vec![AdminChatUpdate {
                    dispute_id: dispute_id.clone(),
                    party: *party,
                    messages,
                }],
                "admin-chat",
            );
        }
    }
}

/// Rebuild live subscriptions from the current tracked set.
///
/// Dual-read: GiftWrap `#p` (legacy) and kind-14 `authors = [pub(K_sign)]`.
/// Make-before-break: only drop previous subscriptions after the replacements
/// are live. Uses `.limit(0)` (live-only); history is hydrated separately.
async fn resubscribe(
    client: &Client,
    targets: &HashMap<PublicKey, ChatTarget>,
    current_subs: &mut LiveSubs,
) {
    if targets.is_empty() {
        current_subs.clear(client).await;
        return;
    }

    let ecdh_pubkeys: Vec<PublicKey> = targets.keys().copied().collect();
    let sign_pubkeys: Vec<PublicKey> = targets.values().map(|t| t.sign_pubkey).collect();

    let giftwrap_filter = Filter::new()
        .kind(Kind::GiftWrap)
        .pubkeys(ecdh_pubkeys)
        .limit(0);
    let kind14_filter = Filter::new()
        .kind(Kind::PrivateDirectMessage)
        .authors(sign_pubkeys)
        .limit(0);

    const MAX_ATTEMPTS: u32 = 3;
    let mut new_giftwrap: Option<SubscriptionId> = None;
    let mut new_kind14: Option<SubscriptionId> = None;

    for attempt in 1..=MAX_ATTEMPTS {
        match client.subscribe(giftwrap_filter.clone(), None).await {
            Ok(output) => {
                new_giftwrap = Some(output.val);
                break;
            }
            Err(e) => {
                if attempt == MAX_ATTEMPTS {
                    log::error!(
                        "[chat_live] failed to subscribe GiftWrap chats after {MAX_ATTEMPTS} attempts: {e}; keeping previous subscriptions alive"
                    );
                    return;
                }
                let delay_ms = 250u64 * (1 << (attempt - 1));
                log::warn!(
                    "[chat_live] GiftWrap subscribe attempt {attempt}/{MAX_ATTEMPTS} failed: {e}; retrying in {delay_ms}ms"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
        }
    }

    for attempt in 1..=MAX_ATTEMPTS {
        match client.subscribe(kind14_filter.clone(), None).await {
            Ok(output) => {
                new_kind14 = Some(output.val);
                break;
            }
            Err(e) => {
                if attempt == MAX_ATTEMPTS {
                    log::error!(
                        "[chat_live] failed to subscribe kind-14 chats after {MAX_ATTEMPTS} attempts: {e}; rolling back new GiftWrap sub"
                    );
                    if let Some(id) = new_giftwrap.take() {
                        client.unsubscribe(&id).await;
                    }
                    return;
                }
                let delay_ms = 250u64 * (1 << (attempt - 1));
                log::warn!(
                    "[chat_live] kind-14 subscribe attempt {attempt}/{MAX_ATTEMPTS} failed: {e}; retrying in {delay_ms}ms"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
        }
    }

    log::debug!(
        "[chat_live] subscribed to {} chat(s) giftwrap={:?} kind14={:?}",
        targets.len(),
        new_giftwrap,
        new_kind14
    );

    if let Some(old_id) = current_subs
        .giftwrap
        .replace(new_giftwrap.expect("subscribed"))
    {
        client.unsubscribe(&old_id).await;
    }
    if let Some(old_id) = current_subs.kind14.replace(new_kind14.expect("subscribed")) {
        client.unsubscribe(&old_id).await;
    }
}

/// A newly tracked key whose history must be hydrated **after** the live
/// subscription is (re)built, so messages published during the backfill are
/// captured live instead of falling into a miss window (subscribe-then-hydrate).
struct PendingHydration {
    target_pubkey: PublicKey,
    shared_keys: Keys,
    since: Option<i64>,
}

/// Outcome of applying one [`ChatRouterCmd`] to the tracked set.
struct CmdOutcome {
    /// The live pubkey set changed, so the batched subscription must be rebuilt.
    needs_resubscribe: bool,
    /// A newly tracked key awaiting post-subscribe history hydration.
    hydrate: Option<PendingHydration>,
    /// Key that was newly tracked (seed durable inner-id set).
    tracked: Option<ChatKeyId>,
    /// Key that was removed (drop rate-limit / in-memory inner set).
    untracked: Option<ChatKeyId>,
}

/// Apply one track/untrack command to the tracked set.
///
/// This only mutates `targets`; history hydration is deferred to the caller and
/// runs **after** [`resubscribe`] so the live filter already covers the new key
/// (see [`PendingHydration`]). Returns whether the live subscription must be
/// rebuilt and, for a new track, the hydration job to run afterwards.
fn apply_chat_router_cmd(
    cmd: ChatRouterCmd,
    targets: &mut HashMap<PublicKey, ChatTarget>,
) -> CmdOutcome {
    match cmd {
        ChatRouterCmd::TrackChatKey {
            key_id,
            shared_key_hex,
            local_trade_pubkey,
            allowed_signers,
            since,
        } => {
            let Some(shared_keys) = keys_from_shared_hex(&shared_key_hex) else {
                log::warn!("[chat_live] invalid shared key hex for {key_id:?}; not tracking");
                return CmdOutcome {
                    needs_resubscribe: false,
                    hydrate: None,
                    tracked: None,
                    untracked: None,
                };
            };
            let Some((_conv, sign)) = chat_keys_from_ecdh(&shared_keys) else {
                log::warn!("[chat_live] failed to derive K_sign for {key_id:?}; not tracking");
                return CmdOutcome {
                    needs_resubscribe: false,
                    hydrate: None,
                    tracked: None,
                    untracked: None,
                };
            };
            let target_pubkey = shared_keys.public_key();
            // Idempotent: skip redundant history fetch + resubscribe if already tracked.
            if targets
                .get(&target_pubkey)
                .is_some_and(|t| t.key_id == key_id)
            {
                return CmdOutcome {
                    needs_resubscribe: false,
                    hydrate: None,
                    tracked: None,
                    untracked: None,
                };
            }
            let since = since.map(clamp_chat_since_cursor_now);
            let tracked_id = key_id.clone();
            targets.insert(
                target_pubkey,
                ChatTarget {
                    key_id,
                    shared_keys: shared_keys.clone(),
                    sign_pubkey: sign.public_key(),
                    local_trade_pubkey,
                    allowed_signers,
                },
            );
            CmdOutcome {
                needs_resubscribe: true,
                hydrate: Some(PendingHydration {
                    target_pubkey,
                    shared_keys,
                    since,
                }),
                tracked: Some(tracked_id),
                untracked: None,
            }
        }
        ChatRouterCmd::UntrackChatKey { key_id } => {
            let before = targets.len();
            targets.retain(|_, t| t.key_id != key_id);
            let removed = targets.len() != before;
            CmdOutcome {
                needs_resubscribe: removed,
                hydrate: None,
                tracked: None,
                untracked: removed.then_some(key_id),
            }
        }
    }
}

fn load_inner_ids_for_key(key_id: &ChatKeyId) -> HashSet<EventId> {
    match key_id {
        ChatKeyId::Order(order_id) => load_order_chat_inner_ids(order_id),
        ChatKeyId::UserDispute(order_id) => load_user_dispute_chat_inner_ids(order_id),
        ChatKeyId::Dispute(dispute_id, party) => load_dispute_chat_inner_ids(dispute_id, *party),
    }
}

/// Backfill one newly tracked chat's history **after** the live subscription is
/// active (relay subscriptions alone don't replay history). Because the live
/// filter already covers this key, any message published during the fetch is also
/// delivered live and deduped by the last-seen cursor / inner-id set. No-op if
/// the key was untracked within the same command burst.
async fn hydrate_history(
    client: &Client,
    targets: &HashMap<PublicKey, ChatTarget>,
    pending: &PendingHydration,
    seen_inner: &mut HashMap<ChatKeyId, HashSet<EventId>>,
    admin_tx: &Sender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    user_tx: &Sender<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
) {
    let Some(target) = targets.get(&pending.target_pubkey) else {
        return;
    };
    match fetch_chat_messages_for_shared_key(
        client,
        &pending.shared_keys,
        target.allowed_signers.as_deref(),
        pending.since,
    )
    .await
    {
        Ok(messages) => {
            let cutoff = pending.since.unwrap_or(0);
            let inner = seen_inner.entry(target.key_id.clone()).or_default();
            let history: Vec<DecodedChatMessage> = messages
                .into_iter()
                .filter(|m| m.timestamp >= cutoff)
                .filter(|m| inner.insert(m.inner_event_id))
                .collect();
            emit_messages(target, history, admin_tx, user_tx);
        }
        Err(e) => log::warn!(
            "[chat_live] history fetch failed for {:?}: {e}",
            target.key_id
        ),
    }
}

/// Single background router for all shared-key chats (user order + admin dispute).
///
/// Spawned once at startup and respawned on client reload/reconnect (mirrors
/// `listen_for_order_messages`). Consumes [`ChatRouterCmd`] for track/untrack and
/// routes live GiftWrap (`#p`) and kind-14 (`authors = pub(K_sign)`) events.
///
/// Multiple buffered track/untrack commands are drained and applied before a single
/// [`resubscribe`], so startup bursts (e.g. [`crate::ui::helpers::track_startup_chats`])
/// do not thrash unsubscribe/subscribe on every key.
pub async fn listen_for_chat_messages(
    client: Client,
    admin_chat_updates_tx: Sender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    user_order_chat_updates_tx: Sender<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
    mut cmd_rx: mpsc::UnboundedReceiver<ChatRouterCmd>,
) {
    // Create the notification receiver BEFORE subscribing so no live event is missed.
    let mut notifications = client.notifications();
    let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
    let mut current_subs = LiveSubs::default();
    let mut seen_outer = OuterIdLru::new(CHAT_SEEN_OUTER_CAP);
    let mut rate_limiters: ChatRateLimiters<ChatKeyId> = ChatRateLimiters::default();
    let mut seen_inner: HashMap<ChatKeyId, HashSet<EventId>> = HashMap::new();

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    // Sender dropped (respawn/shutdown): unsubscribe and exit.
                    current_subs.clear(&client).await;
                    break;
                };
                let mut outcomes = vec![apply_chat_router_cmd(cmd, &mut targets)];
                while let Ok(more) = cmd_rx.try_recv() {
                    outcomes.push(apply_chat_router_cmd(more, &mut targets));
                }
                let mut needs_resubscribe = false;
                let mut pending_hydration: Vec<PendingHydration> = Vec::new();
                for outcome in outcomes {
                    needs_resubscribe |= outcome.needs_resubscribe;
                    pending_hydration.extend(outcome.hydrate);
                    if let Some(key_id) = outcome.tracked {
                        seen_inner
                            .entry(key_id.clone())
                            .or_insert_with(|| load_inner_ids_for_key(&key_id));
                    }
                    if let Some(key_id) = outcome.untracked {
                        rate_limiters.remove(&key_id);
                        seen_inner.remove(&key_id);
                    }
                }
                if needs_resubscribe {
                    resubscribe(&client, &targets, &mut current_subs).await;
                }
                // Subscribe-first, then backfill: the live filter now covers the newly
                // tracked keys, so messages published during hydration are captured
                // live instead of being dropped in a miss window.
                for pending in &pending_hydration {
                    hydrate_history(
                        &client,
                        &targets,
                        pending,
                        &mut seen_inner,
                        &admin_chat_updates_tx,
                        &user_order_chat_updates_tx,
                    )
                    .await;
                }
            }
            notification = notifications.recv() => {
                let event = match notification {
                    Ok(RelayPoolNotification::Event { event, .. }) => *event,
                    Ok(_) => continue,
                    // Lagged/closed broadcast: log and keep going (not fatal), like the order/dispute loops.
                    Err(e) => {
                        log::debug!("[chat_live] notification channel: {e}");
                        continue;
                    }
                };
                let Some(target) = resolve_chat_target(&targets, &event) else {
                    continue;
                };
                // Spec order: outer-id LRU, then rate limit — both before decrypt.
                if !seen_outer.insert(event.id) {
                    continue;
                }
                if !rate_limiters.allow(&target.key_id) {
                    log::debug!(
                        "[chat_live] rate-limited chat event for {:?}",
                        target.key_id
                    );
                    continue;
                }
                match unwrap_giftwrap_with_shared_key(
                    &target.shared_keys,
                    &event,
                    target.allowed_signers.as_deref(),
                )
                .await
                {
                    Ok(msg) => {
                        let inner = seen_inner.entry(target.key_id.clone()).or_default();
                        if !inner.insert(msg.inner_event_id) {
                            continue;
                        }
                        emit_messages(
                            target,
                            vec![msg],
                            &admin_chat_updates_tx,
                            &user_order_chat_updates_tx,
                        );
                    }
                    Err(e) => log::warn!(
                        "[chat_live] failed to unwrap chat event {}: {e}",
                        event.id
                    ),
                }
            }
        }
    }
}

/// Route a live event to its tracked chat (GiftWrap by `#p`, kind 14 by author).
fn resolve_chat_target<'a>(
    targets: &'a HashMap<PublicKey, ChatTarget>,
    event: &Event,
) -> Option<&'a ChatTarget> {
    match event.kind {
        Kind::GiftWrap => {
            let target_pubkey = event.tags.public_keys().next().copied()?;
            targets.get(&target_pubkey)
        }
        Kind::PrivateDirectMessage => targets.values().find(|t| t.sign_pubkey == event.pubkey),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mostro_core::chat::SharedKey;

    /// `ChatKeyId` equality backs both the untrack retention filter (`t.key_id != key_id`)
    /// and the track idempotency check, so it must distinguish order vs dispute and party.
    #[test]
    fn chat_key_id_equality_distinguishes_targets() {
        assert_eq!(
            ChatKeyId::Order("a".to_string()),
            ChatKeyId::Order("a".to_string())
        );
        assert_ne!(
            ChatKeyId::Order("a".to_string()),
            ChatKeyId::Order("b".to_string())
        );
        assert_eq!(
            ChatKeyId::Dispute("d".to_string(), ChatParty::Buyer),
            ChatKeyId::Dispute("d".to_string(), ChatParty::Buyer)
        );
        assert_ne!(
            ChatKeyId::Dispute("d".to_string(), ChatParty::Buyer),
            ChatKeyId::Dispute("d".to_string(), ChatParty::Seller)
        );
        assert_ne!(
            ChatKeyId::Order("d".to_string()),
            ChatKeyId::Dispute("d".to_string(), ChatParty::Buyer)
        );
    }

    /// Produce a valid shared-key hex (ECDH secret) that round-trips through
    /// [`keys_from_shared_hex`], matching how production shared keys are stored.
    fn sample_shared_hex() -> String {
        let a = Keys::generate();
        let b = Keys::generate();
        SharedKey::derive(a.secret_key(), &b.public_key())
            .expect("derive shared key")
            .to_hex()
    }

    /// Tracking a new key must defer history hydration (return a `PendingHydration`)
    /// and request a resubscribe, so the live filter is rebuilt *before* the backfill
    /// runs — closing the miss window flagged in review.
    #[test]
    fn track_new_key_defers_hydration_and_requests_resubscribe() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let expected_pubkey = keys_from_shared_hex(&shared_hex).unwrap().public_key();

        let outcome = apply_chat_router_cmd(
            ChatRouterCmd::TrackChatKey {
                key_id: ChatKeyId::Order("order-1".to_string()),
                shared_key_hex: shared_hex,
                local_trade_pubkey: None,
                allowed_signers: None,
                since: Some(42),
            },
            &mut targets,
        );

        assert!(outcome.needs_resubscribe);
        let hydrate = outcome.hydrate.expect("track must schedule hydration");
        assert_eq!(hydrate.target_pubkey, expected_pubkey);
        assert_eq!(hydrate.since, Some(42));
        assert!(targets.contains_key(&expected_pubkey));
    }

    /// Re-tracking an already-tracked key is a no-op: no resubscribe, no re-hydration.
    #[test]
    fn retrack_existing_key_is_noop() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let track = || ChatRouterCmd::TrackChatKey {
            key_id: ChatKeyId::Order("order-1".to_string()),
            shared_key_hex: shared_hex.clone(),
            local_trade_pubkey: None,
            allowed_signers: None,
            since: None,
        };

        let _ = apply_chat_router_cmd(track(), &mut targets);
        let outcome = apply_chat_router_cmd(track(), &mut targets);

        assert!(!outcome.needs_resubscribe);
        assert!(outcome.hydrate.is_none());
        assert_eq!(targets.len(), 1);
    }

    /// Untracking removes the key and requests a resubscribe, but never hydrates.
    #[test]
    fn untrack_removes_key_without_hydration() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let _ = apply_chat_router_cmd(
            ChatRouterCmd::TrackChatKey {
                key_id: ChatKeyId::Order("order-1".to_string()),
                shared_key_hex: shared_hex,
                local_trade_pubkey: None,
                allowed_signers: None,
                since: None,
            },
            &mut targets,
        );

        let outcome = apply_chat_router_cmd(
            ChatRouterCmd::UntrackChatKey {
                key_id: ChatKeyId::Order("order-1".to_string()),
            },
            &mut targets,
        );

        assert!(outcome.needs_resubscribe);
        assert!(outcome.hydrate.is_none());
        assert!(targets.is_empty());
    }

    /// Untracking a key that isn't tracked changes nothing (no resubscribe).
    #[test]
    fn untrack_absent_key_is_noop() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let outcome = apply_chat_router_cmd(
            ChatRouterCmd::UntrackChatKey {
                key_id: ChatKeyId::Order("missing".to_string()),
            },
            &mut targets,
        );

        assert!(!outcome.needs_resubscribe);
        assert!(outcome.hydrate.is_none());
    }

    #[test]
    fn resolve_chat_target_routes_kind14_by_sign_author() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let _ = apply_chat_router_cmd(
            ChatRouterCmd::TrackChatKey {
                key_id: ChatKeyId::Order("order-1".to_string()),
                shared_key_hex: shared_hex,
                local_trade_pubkey: None,
                allowed_signers: None,
                since: None,
            },
            &mut targets,
        );
        let target = targets.values().next().expect("tracked");
        let (_conv, sign) = chat_keys_from_ecdh(&target.shared_keys).expect("chat keys");
        // Outer kind-14 authored by K_sign
        let event = EventBuilder::new(Kind::PrivateDirectMessage, "ciphertext")
            .tag(Tag::public_key(Keys::generate().public_key()))
            .sign_with_keys(&sign)
            .expect("sign");

        let resolved = resolve_chat_target(&targets, &event).expect("route kind14");
        assert_eq!(resolved.key_id, ChatKeyId::Order("order-1".to_string()));
        assert_eq!(resolved.sign_pubkey, event.pubkey);
    }

    #[test]
    fn resolve_chat_target_ignores_kind14_from_unknown_author() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let _ = apply_chat_router_cmd(
            ChatRouterCmd::TrackChatKey {
                key_id: ChatKeyId::Order("order-1".to_string()),
                shared_key_hex: shared_hex,
                local_trade_pubkey: None,
                allowed_signers: None,
                since: None,
            },
            &mut targets,
        );
        // Tag `#p` with the tracked target key so this fails if routing ever fell back
        // to `#p`; the unknown outer author must still be rejected.
        let target_pubkey = *targets.keys().next().expect("tracked target");
        let junk = Keys::generate();
        let event = EventBuilder::new(Kind::PrivateDirectMessage, "ciphertext")
            .tag(Tag::public_key(target_pubkey))
            .sign_with_keys(&junk)
            .expect("sign");

        assert!(resolve_chat_target(&targets, &event).is_none());
    }

    #[test]
    fn track_clamps_future_since_cursor() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let future = Timestamp::now().as_secs() as i64 + 86_400;
        let outcome = apply_chat_router_cmd(
            ChatRouterCmd::TrackChatKey {
                key_id: ChatKeyId::Order("order-1".to_string()),
                shared_key_hex: shared_hex,
                local_trade_pubkey: None,
                allowed_signers: None,
                since: Some(future),
            },
            &mut targets,
        );
        let hydrate = outcome.hydrate.expect("hydrate");
        let since = hydrate.since.expect("since");
        assert!(since <= Timestamp::now().as_secs() as i64);
        assert!(since < future);
    }
}
