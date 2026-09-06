//! Shared-key chat subscription router (P2P order chat + admin dispute chat).
//!
//! Live kind-14 subscription (`authors: [pub(K_sign)]`). Outbound chat is
//! always kind 14. GiftWrap (kind 1059) is not subscribed or unwrapped.
//!
//! Live kind-14 traffic is routed **only** by outer author ∈ tracked `pub(K_sign)`
//! (never by `#p` alone). Hydration and last-seen cursors use
//! `min(accepted_ts, local_now)` so a far-future timestamp cannot poison `since`.
//!
//! Incoming events are decrypted with the per-channel ECDH secret (`K_conv` /
//! `K_sign` derived at unwrap). Unwrap requires a non-empty inner-signer
//! allow-list on [`ChatRouterCmd::TrackChatKey`]; messages whose inner event is
//! not signed by an allowed trade/admin key are dropped. Decoded messages are
//! emitted on the existing `admin_chat_updates` / `user_order_chat_updates`
//! channels. Inner-event replay protection uses the **durable** `.inner_ids`
//! set (recorded by the UI only after transcript save). The router must not
//! consume an inner id on emit — a failed persist would otherwise drop later
//! deliveries for the rest of the session. The session outer-id LRU is the same
//! contract: retain an outer event id only after a durable skip (inner already
//! accepted) or a poison unwrap, so a relay redelivery of the same Nostr event
//! can still retry a failed transcript write without restarting the listener.
//!
//! Lifecycle (see also `docs/DM_LISTENER_FLOW.md`): spawned via
//! [`crate::util::spawn_supervised_chat_listener`] at startup (and on client
//! reload/reconnect). The supervisor also respawns this task on panic or unexpected
//! exit without aborting other workers: it publishes a fresh command sender immediately
//! (commands buffer during backoff) and the main loop replays [`crate::ui::helpers::track_startup_chats`].
//! Chat keys are tracked/untracked via the
//! global command channel published by [`set_chat_router_cmd_tx`].

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use nostr_sdk::prelude::*;
use tokio::sync::mpsc::{self, Sender};
use uuid::Uuid;

use crate::models::Order;
use crate::ui::helpers::{
    dispute_chat_inner_id_known, load_dispute_chat_inner_ids, load_order_chat_inner_ids,
    load_user_dispute_chat_inner_ids, order_chat_inner_id_known, order_chat_since_from_file,
    user_dispute_chat_inner_id_known,
};
use crate::ui::{AdminChatUpdate, ChatParty, DecodedChatMessage, OrderChatUpdate, UserChatChannel};
use crate::util::chat_security::{
    try_emit_chat_update, ChatRateLimiters, OuterIdLru, CHAT_SEEN_OUTER_CAP,
};
use crate::util::chat_utils::{
    chat_keys_from_ecdh, clamp_chat_since_cursor_now, derive_shared_key_hex,
    fetch_chat_messages_for_shared_key, keys_from_shared_hex, order_chat_allowed_signers,
    unwrap_chat_envelope,
};
use futures::StreamExt;

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
    /// key's pubkey in the batched live subscription. `allowed_signers` must be
    /// non-empty (empty lists are ignored and the key is not tracked).
    TrackChatKey {
        key_id: ChatKeyId,
        /// Hex of the ECDH shared secret (round-trips via [`keys_from_shared_hex`]).
        shared_key_hex: String,
        /// Local trade pubkey for order chats (used to skip relay echoes of our own sends).
        local_trade_pubkey: Option<PublicKey>,
        /// Inner-signer allow-list (party trade keys, plus admin on dispute channels).
        allowed_signers: Vec<PublicKey>,
        /// Only emit history messages at/after this unix timestamp (last-seen cursor).
        since: Option<i64>,
    },
    /// Stop tracking a shared-key chat (order row deleted / dispute finalized, etc.).
    UntrackChatKey { key_id: ChatKeyId },
}

/// Per-tracked-key routing metadata.
struct ChatTarget {
    key_id: ChatKeyId,
    /// ECDH IKM keys (persisted hex); HashMap key is the ECDH pubkey.
    shared_keys: Keys,
    /// `pub(K_sign)` — kind-14 outer author used for live `authors` filter / routing.
    sign_pubkey: PublicKey,
    /// Order chat only; enables echo-skip in `apply_user_order_chat_updates`.
    local_trade_pubkey: Option<PublicKey>,
    /// Accepted inner signers for unwrap (must be non-empty).
    allowed_signers: Vec<PublicKey>,
}

/// Live kind-14 subscription id.
#[derive(Default)]
struct LiveSubs {
    kind14: Option<SubscriptionId>,
}

impl LiveSubs {
    async fn clear(&mut self, client: &Client) {
        if let Some(id) = self.kind14.take() {
            if let Err(e) = client.unsubscribe(&id).await {
                log::debug!("unsubscribe failed: {e}");
            }
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
///
/// `allowed_signers` must be the local trade pubkey and the counterparty trade
/// pubkey (see [`order_chat_allowed_signers`]). An empty list is not tracked.
pub fn track_order_chat(
    order_id: String,
    shared_key_hex: String,
    local_trade_pubkey: PublicKey,
    allowed_signers: Vec<PublicKey>,
    since: Option<i64>,
) {
    send_chat_router_cmd(ChatRouterCmd::TrackChatKey {
        key_id: ChatKeyId::Order(order_id),
        shared_key_hex,
        local_trade_pubkey: Some(local_trade_pubkey),
        allowed_signers,
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
        allowed_signers: vec![local_trade_pubkey, solver_pubkey],
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
///
/// `allowed_signers` must include that party's trade pubkey and, when known,
/// the admin pubkey (see [`crate::util::chat_utils::dispute_chat_allowed_signers`]).
/// An empty list is not tracked.
pub fn track_dispute_chat(
    dispute_id: String,
    party: ChatParty,
    shared_key_hex: String,
    allowed_signers: Vec<PublicKey>,
    since: Option<i64>,
) {
    send_chat_router_cmd(ChatRouterCmd::TrackChatKey {
        key_id: ChatKeyId::Dispute(dispute_id, party),
        shared_key_hex,
        local_trade_pubkey: None,
        allowed_signers,
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
/// Skips tracking when the counterparty trade pubkey is missing (no inner-signer allow-list).
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
    let Some(allowed) = order_chat_allowed_signers(
        trade_keys.public_key(),
        order.counterparty_pubkey.as_deref(),
    ) else {
        log::warn!(
            "order chat {order_id}: missing counterparty pubkey; not tracking shared-key chat"
        );
        return;
    };
    if let Some(hex) = shared_hex {
        let since = order_chat_since_from_file(&order_id.to_string());
        track_order_chat(
            order_id.to_string(),
            hex,
            trade_keys.public_key(),
            allowed,
            since,
        );
    }
}

/// Emit decrypted chat messages on the appropriate update channel.
///
/// Reuses the same `AdminChatUpdate` / `OrderChatUpdate` shapes as the old
/// polling path so `apply_admin_chat_updates` / `apply_user_order_chat_updates`
/// can merge, persist, and dedupe by durable inner-event id. This helper does
/// not record inner ids.
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

/// Result of one live kind-14 notification after the event has been routed to a
/// tracked chat. Outer-id LRU insertion is a durable/poison ack, not a
/// pre-emit reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveChatDisposition {
    /// Outer id already retained (post-persist skip or poison unwrap).
    DuplicateOuter,
    /// Token bucket denied; outer id is left retryable.
    RateLimited,
    /// Decrypt/allow-list failed; outer id retained (retrying will not help).
    UnwrapFailed,
    /// Inner id already in the durable set; outer id retained.
    AlreadyPersisted,
    /// Emitted to the UI; outer id is **not** retained until persist records the inner id.
    Emitted,
}

/// Spec order: outer-id lookup, then rate limit, then decrypt.
///
/// Unlike a consume-on-sight LRU, this only **inserts** `event.id` when the
/// event is a poison unwrap or the inner id is already durably accepted.
/// Emitted events stay retryable so a failed `save_order_chat_message` /
/// `save_chat_message` can still be retried on the same outer redelivery.
fn handle_live_chat_event(
    event: &Event,
    target: &ChatTarget,
    seen_outer: &mut OuterIdLru,
    rate_limiters: &mut ChatRateLimiters<ChatKeyId>,
    admin_tx: &Sender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    user_tx: &Sender<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
) -> LiveChatDisposition {
    if seen_outer.contains(&event.id) {
        return LiveChatDisposition::DuplicateOuter;
    }
    if !rate_limiters.allow(&target.key_id) {
        log::debug!(
            "[chat_live] rate-limited chat event for {:?}",
            target.key_id
        );
        return LiveChatDisposition::RateLimited;
    }
    match unwrap_chat_envelope(&target.shared_keys, event, &target.allowed_signers) {
        Ok(msg) => {
            if chat_inner_id_accepted(&target.key_id, &msg.inner_event_id) {
                let _ = seen_outer.insert(event.id);
                return LiveChatDisposition::AlreadyPersisted;
            }
            emit_messages(target, vec![msg], admin_tx, user_tx);
            LiveChatDisposition::Emitted
        }
        Err(e) => {
            log::warn!("[chat_live] failed to unwrap chat event {}: {e}", event.id);
            let _ = seen_outer.insert(event.id);
            LiveChatDisposition::UnwrapFailed
        }
    }
}

/// Rebuild live subscriptions from the current tracked set.
///
/// Kind-14 `authors = [pub(K_sign)]`. Make-before-break: only drop the previous
/// subscription after the replacement is live. Uses `.limit(0)` (live-only);
/// history is hydrated separately.
async fn resubscribe(
    client: &Client,
    targets: &HashMap<PublicKey, ChatTarget>,
    current_subs: &mut LiveSubs,
) {
    if targets.is_empty() {
        current_subs.clear(client).await;
        return;
    }

    let sign_pubkeys: Vec<PublicKey> = targets.values().map(|t| t.sign_pubkey).collect();
    let kind14_filter = live_chat_filters(&sign_pubkeys);

    let new_kind14 = match subscribe_chat_filter(client, kind14_filter, "kind-14").await {
        Some(id) => id,
        None => {
            log::error!(
                "[chat_live] kind-14 subscribe failed; keeping previous subscription alive"
            );
            return;
        }
    };

    log::debug!(
        "[chat_live] subscribed to {} chat(s) kind14={:?}",
        targets.len(),
        new_kind14
    );

    replace_live_sub(client, &mut current_subs.kind14, Some(new_kind14)).await;
}

/// Live kind-14 filter for the tracked set (`authors = [pub(K_sign)]`).
fn live_chat_filters(sign_pubkeys: &[PublicKey]) -> Filter {
    Filter::new()
        .kind(Kind::PrivateDirectMessage)
        .authors(sign_pubkeys.iter().copied())
        .limit(0)
}

async fn subscribe_chat_filter(
    client: &Client,
    filter: Filter,
    label: &str,
) -> Option<SubscriptionId> {
    const MAX_ATTEMPTS: u32 = 3;
    for attempt in 1..=MAX_ATTEMPTS {
        match client.subscribe(filter.clone()).await {
            Ok(output) => return Some(output.value),
            Err(e) => {
                if attempt == MAX_ATTEMPTS {
                    log::error!(
                        "[chat_live] failed to subscribe {label} chats after {MAX_ATTEMPTS} attempts: {e}"
                    );
                    return None;
                }
                let delay_ms = 250u64 * (1 << (attempt - 1));
                log::warn!(
                    "[chat_live] {label} subscribe attempt {attempt}/{MAX_ATTEMPTS} failed: {e}; retrying in {delay_ms}ms"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
        }
    }
    None
}

async fn replace_live_sub(
    client: &Client,
    slot: &mut Option<SubscriptionId>,
    new_id: Option<SubscriptionId>,
) {
    let old_id = match new_id {
        Some(id) => slot.replace(id),
        None => slot.take(),
    };
    if let Some(old_id) = old_id {
        if let Err(e) = client.unsubscribe(&old_id).await {
            log::debug!("unsubscribe failed: {e}");
        }
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
    /// Key that was newly tracked (warm durable inner-id cache).
    tracked: Option<ChatKeyId>,
    /// Key that was removed (drop the per-key rate limiter).
    untracked: Option<ChatKeyId>,
}

/// Apply one track/untrack command to the tracked set.
///
/// This only mutates `targets`; history hydration is deferred to the caller and
/// runs **after** [`resubscribe`] so the live filter already covers the new key
/// (see [`PendingHydration`]). An empty `allowed_signers` list is a no-op (the
/// key is not tracked). Returns whether the live subscription must be rebuilt
/// and, for a new track, the hydration job to run afterwards.
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
            if allowed_signers.is_empty() {
                log::warn!(
                    "[chat_live] empty inner-signer allow-list for {key_id:?}; not tracking"
                );
                return CmdOutcome {
                    needs_resubscribe: false,
                    hydrate: None,
                    tracked: None,
                    untracked: None,
                };
            }
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

/// True when this inner id is already in the durable `.inner_ids` set (UI records
/// it only after a successful transcript save). Used to skip replay; must not be
/// treated as "emitted this session".
fn chat_inner_id_accepted(key_id: &ChatKeyId, id: &EventId) -> bool {
    match key_id {
        ChatKeyId::Order(order_id) => order_chat_inner_id_known(order_id, id),
        ChatKeyId::UserDispute(order_id) => user_dispute_chat_inner_id_known(order_id, id),
        ChatKeyId::Dispute(dispute_id, party) => {
            dispute_chat_inner_id_known(dispute_id, *party, id)
        }
    }
}

/// Drop history rows already durably accepted, plus duplicates inside this fetch.
/// Does **not** record new inner ids — a failed UI persist must still be retryable.
fn filter_unpersisted_chat_history(
    messages: Vec<DecodedChatMessage>,
    cutoff: i64,
    inner_already_accepted: impl Fn(&EventId) -> bool,
) -> Vec<DecodedChatMessage> {
    let mut batch_seen = HashSet::new();
    messages
        .into_iter()
        .filter(|m| m.timestamp >= cutoff)
        .filter(|m| !inner_already_accepted(&m.inner_event_id))
        .filter(|m| batch_seen.insert(m.inner_event_id))
        .collect()
}

/// Backfill one newly tracked chat's history **after** the live subscription is
/// active (relay subscriptions alone don't replay history). Unwrap uses the
/// target's inner-signer allow-list. Because the live filter already covers this
/// key, any message published during the fetch is also delivered live; the UI
/// dedupes by durable inner id after persist. No-op if the key was untracked
/// within the same command burst.
async fn hydrate_history(
    client: &Client,
    targets: &HashMap<PublicKey, ChatTarget>,
    pending: &PendingHydration,
    admin_tx: &Sender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    user_tx: &Sender<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
) {
    let Some(target) = targets.get(&pending.target_pubkey) else {
        return;
    };
    match fetch_chat_messages_for_shared_key(
        client,
        &pending.shared_keys,
        &target.allowed_signers,
        pending.since,
    )
    .await
    {
        Ok(messages) => {
            let cutoff = pending.since.unwrap_or(0);
            let history = filter_unpersisted_chat_history(messages, cutoff, |id| {
                chat_inner_id_accepted(&target.key_id, id)
            });
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
/// Spawned via [`crate::util::spawn_supervised_chat_listener`] at startup and on
/// client reload/reconnect (mirrors the trade DM listener). The supervisor also
/// respawns this task on panic or unexpected exit. Consumes [`ChatRouterCmd`] for
/// track/untrack and routes live kind-14 (`authors = pub(K_sign)`) events.
/// Live unwrap and hydration require the per-key inner-signer allow-list from
/// [`ChatRouterCmd::TrackChatKey`]. Inner-event replay protection uses the
/// durable `.inner_ids` set (UI records an id only after transcript save);
/// emit does not consume ids. The outer-id LRU likewise retains an event id
/// only after a durable inner-id skip or a poison unwrap, so the same Nostr
/// event can be retried after a failed transcript write.
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
                        // Warm the durable inner-id cache; do not snapshot it into a
                        // session set that would outrun transcript persist.
                        let _ = load_inner_ids_for_key(&key_id);
                    }
                    if let Some(key_id) = outcome.untracked {
                        rate_limiters.remove(&key_id);
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
                        &admin_chat_updates_tx,
                        &user_order_chat_updates_tx,
                    )
                    .await;
                }
            }
            notification = notifications.next() => {
                let Some(notification) = notification else {
                    log::debug!("[chat_live] notification stream ended");
                    break;
                };
                let ClientNotification::Event { event, .. } = notification else {
                    continue;
                };
                let event = *event;
                let Some(target) = resolve_chat_target(&targets, &event) else {
                    continue;
                };
                handle_live_chat_event(
                    &event,
                    target,
                    &mut seen_outer,
                    &mut rate_limiters,
                    &admin_chat_updates_tx,
                    &user_order_chat_updates_tx,
                );
            }
        }
    }
}

/// Route a live event to its tracked chat (kind 14 by outer author).
fn resolve_chat_target<'a>(
    targets: &'a HashMap<PublicKey, ChatTarget>,
    event: &Event,
) -> Option<&'a ChatTarget> {
    match event.kind {
        Kind::PrivateDirectMessage => targets.values().find(|t| t.sign_pubkey == event.pubkey),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::helpers::{
        apply_user_order_chat_updates, force_next_chat_save_failure, install_test_chat_home,
        order_chat_inner_id_known,
    };
    use crate::ui::{AppState, UserRole};
    use mostro_core::chat::{wrap_chat_message, SharedKey};

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

    fn sample_allowed_signers() -> Vec<PublicKey> {
        vec![Keys::generate().public_key(), Keys::generate().public_key()]
    }

    fn track_order_cmd(shared_hex: String, since: Option<i64>) -> ChatRouterCmd {
        ChatRouterCmd::TrackChatKey {
            key_id: ChatKeyId::Order("order-1".to_string()),
            shared_key_hex: shared_hex,
            local_trade_pubkey: None,
            allowed_signers: sample_allowed_signers(),
            since,
        }
    }

    /// Tracking a new key must defer history hydration (return a `PendingHydration`)
    /// and request a resubscribe, so the live filter is rebuilt *before* the backfill
    /// runs — closing the miss window flagged in review.
    #[test]
    fn track_new_key_defers_hydration_and_requests_resubscribe() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let expected_pubkey = keys_from_shared_hex(&shared_hex).unwrap().public_key();

        let outcome = apply_chat_router_cmd(track_order_cmd(shared_hex, Some(42)), &mut targets);

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
        let track = || track_order_cmd(shared_hex.clone(), None);

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
        let _ = apply_chat_router_cmd(track_order_cmd(shared_hex, None), &mut targets);

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
        let _ = apply_chat_router_cmd(track_order_cmd(shared_hex, None), &mut targets);
        let target = targets.values().next().expect("tracked");
        let (_conv, sign) = chat_keys_from_ecdh(&target.shared_keys).expect("chat keys");
        // Outer kind-14 authored by K_sign
        let event = EventBuilder::new(Kind::PrivateDirectMessage, "ciphertext")
            .tag(Tag::public_key(Keys::generate().public_key()))
            .finalize(&sign)
            .expect("sign");

        let resolved = resolve_chat_target(&targets, &event).expect("route kind14");
        assert_eq!(resolved.key_id, ChatKeyId::Order("order-1".to_string()));
        assert_eq!(resolved.sign_pubkey, event.pubkey);
    }

    #[test]
    fn resolve_chat_target_ignores_giftwrap() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let _ = apply_chat_router_cmd(track_order_cmd(shared_hex, None), &mut targets);
        let target_pubkey = *targets.keys().next().expect("tracked");
        let ephemeral = Keys::generate();
        let event = EventBuilder::new(Kind::GiftWrap, "ciphertext")
            .tag(Tag::public_key(target_pubkey))
            .finalize(&ephemeral)
            .expect("sign");

        assert!(resolve_chat_target(&targets, &event).is_none());
    }

    #[test]
    fn live_chat_filters_kind14_uses_authors_not_p_tags() {
        let sign = vec![Keys::generate().public_key()];
        let kind14 = live_chat_filters(&sign);
        let json = serde_json::to_value(&kind14).expect("filter json");
        let authors = json.get("authors").expect("authors");
        assert!(authors
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some(&sign[0].to_hex())));
        assert!(json.get("#p").is_none());
        assert_eq!(
            json.get("kinds")
                .and_then(|k| k.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_u64()),
            Some(u64::from(Kind::PrivateDirectMessage.as_u16()))
        );
    }

    #[test]
    fn resolve_chat_target_ignores_kind14_from_unknown_author() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let _ = apply_chat_router_cmd(track_order_cmd(shared_hex, None), &mut targets);
        // Tag `#p` with the tracked target key so this fails if routing ever fell back
        // to `#p`; the unknown outer author must still be rejected.
        let target_pubkey = *targets.keys().next().expect("tracked target");
        let junk = Keys::generate();
        let event = EventBuilder::new(Kind::PrivateDirectMessage, "ciphertext")
            .tag(Tag::public_key(target_pubkey))
            .finalize(&junk)
            .expect("sign");

        assert!(resolve_chat_target(&targets, &event).is_none());
    }

    #[test]
    fn track_clamps_future_since_cursor() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let shared_hex = sample_shared_hex();
        let future = Timestamp::now().as_secs() as i64 + 86_400;
        let outcome =
            apply_chat_router_cmd(track_order_cmd(shared_hex, Some(future)), &mut targets);
        let hydrate = outcome.hydrate.expect("hydrate");
        let since = hydrate.since.expect("since");
        assert!(since <= Timestamp::now().as_secs() as i64);
        assert!(since < future);
    }

    #[test]
    fn track_rejects_empty_allow_list() {
        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let outcome = apply_chat_router_cmd(
            ChatRouterCmd::TrackChatKey {
                key_id: ChatKeyId::Order("order-1".to_string()),
                shared_key_hex: sample_shared_hex(),
                local_trade_pubkey: None,
                allowed_signers: Vec::new(),
                since: None,
            },
            &mut targets,
        );
        assert!(!outcome.needs_resubscribe);
        assert!(outcome.hydrate.is_none());
        assert!(targets.is_empty());
    }

    fn fake_inner_id(byte: u8) -> EventId {
        EventId::from_byte_array({
            let mut hex = [0u8; 32];
            hex[31] = byte;
            hex
        })
    }

    fn fake_decoded(ts: i64, id_byte: u8) -> DecodedChatMessage {
        DecodedChatMessage {
            content: format!("msg-{id_byte}"),
            timestamp: ts,
            sender: Keys::generate().public_key(),
            inner_event_id: fake_inner_id(id_byte),
        }
    }

    /// Hydrate must skip durably accepted ids without recording newly emitted ones,
    /// so a failed transcript save can still be retried on a later delivery.
    #[test]
    fn hydrate_filter_skips_accepted_ids_without_consuming_new_ones() {
        let accepted = HashSet::from([fake_inner_id(1)]);
        let messages = vec![
            fake_decoded(10, 1),
            fake_decoded(11, 2),
            fake_decoded(11, 2),
            fake_decoded(5, 3),
        ];
        let out = filter_unpersisted_chat_history(messages, 10, |id| accepted.contains(id));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].inner_event_id, fake_inner_id(2));
        assert!(!accepted.contains(&fake_inner_id(2)));
    }

    #[test]
    fn hydrate_filter_emits_again_when_inner_id_was_never_accepted() {
        let messages = vec![fake_decoded(10, 7), fake_decoded(10, 7)];
        let first = filter_unpersisted_chat_history(messages.clone(), 0, |_| false);
        assert_eq!(first.len(), 1);
        // Simulate persist failure: durable set still empty, so a later delivery
        // (second hydrate / live overlap) is not dropped at the router.
        let second = filter_unpersisted_chat_history(messages, 0, |_| false);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].inner_event_id, fake_inner_id(7));
    }

    /// Live path: same outer event, forced transcript save failure, then redelivery
    /// must emit again and persist/display without restarting the listener.
    #[tokio::test]
    async fn live_redelivery_retries_after_transcript_save_failure() {
        let dir = std::env::temp_dir().join(format!("mostrix-chat-home-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp home");
        let _home = install_test_chat_home(dir.clone());
        let order_id = uuid::Uuid::new_v4().to_string();

        let local = Keys::generate();
        let peer = Keys::generate();
        let shared = SharedKey::derive(local.secret_key(), &peer.public_key()).expect("ecdh");
        let (conv, sign) = shared.chat_keys().expect("chat keys");
        let event = wrap_chat_message(&peer, &conv, &sign, "retry me")
            .await
            .expect("wrap");

        let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
        let _ = apply_chat_router_cmd(
            ChatRouterCmd::TrackChatKey {
                key_id: ChatKeyId::Order(order_id.clone()),
                shared_key_hex: shared.to_hex(),
                local_trade_pubkey: Some(local.public_key()),
                allowed_signers: vec![local.public_key(), peer.public_key()],
                since: None,
            },
            &mut targets,
        );
        let target = targets.values().next().expect("tracked");

        let mut seen_outer = OuterIdLru::new(CHAT_SEEN_OUTER_CAP);
        let mut rate_limiters: ChatRateLimiters<ChatKeyId> = ChatRateLimiters::default();
        let (admin_tx, _admin_rx) = mpsc::channel(8);
        let (user_tx, mut user_rx) = mpsc::channel(8);
        let mut app = AppState::new(UserRole::User);

        force_next_chat_save_failure();
        assert_eq!(
            handle_live_chat_event(
                &event,
                target,
                &mut seen_outer,
                &mut rate_limiters,
                &admin_tx,
                &user_tx,
            ),
            LiveChatDisposition::Emitted
        );
        assert!(
            !seen_outer.contains(&event.id),
            "emitted outer id must stay retryable until persist succeeds"
        );

        let first = user_rx.try_recv().expect("first emit").expect("ok");
        let inner_id = first[0].messages[0].inner_event_id;
        apply_user_order_chat_updates(&mut app, first);
        assert!(
            app.order_chats
                .get(&order_id)
                .is_none_or(|msgs| msgs.is_empty()),
            "failed save must not display the message"
        );
        assert!(!order_chat_inner_id_known(&order_id, &inner_id));

        // Same listener LRU / rate-limiter state — no restart, no re-track.
        assert_eq!(
            handle_live_chat_event(
                &event,
                target,
                &mut seen_outer,
                &mut rate_limiters,
                &admin_tx,
                &user_tx,
            ),
            LiveChatDisposition::Emitted
        );
        let second = user_rx.try_recv().expect("redelivery emit").expect("ok");
        apply_user_order_chat_updates(&mut app, second);
        let displayed = app.order_chats.get(&order_id).expect("chat row");
        assert_eq!(displayed.len(), 1);
        assert_eq!(displayed[0].content, "retry me");
        assert!(order_chat_inner_id_known(&order_id, &inner_id));

        assert_eq!(
            handle_live_chat_event(
                &event,
                target,
                &mut seen_outer,
                &mut rate_limiters,
                &admin_tx,
                &user_tx,
            ),
            LiveChatDisposition::AlreadyPersisted
        );
        assert!(seen_outer.contains(&event.id));
        assert!(user_rx.try_recv().is_err());

        assert_eq!(
            handle_live_chat_event(
                &event,
                target,
                &mut seen_outer,
                &mut rate_limiters,
                &admin_tx,
                &user_tx,
            ),
            LiveChatDisposition::DuplicateOuter
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
