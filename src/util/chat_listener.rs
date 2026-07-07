//! Shared-key chat subscription router (P2P order chat + admin dispute chat).
//!
//! Replaces the previous 2-second timed `fetch_events` polling with a single
//! long-lived subscription, mirroring Mostro Mobile's `SubscriptionManager`: one
//! relay subscription whose filter batches **all** active shared-key pubkeys
//! (`kind: 1059`, `#p: [shared pubkeys]`). Incoming gift wraps are routed to the
//! owning chat by matching the event's `p` tag against the tracked pubkey set,
//! decrypted with the per-channel shared `Keys`, and emitted on the existing
//! `admin_chat_updates` / `user_order_chat_updates` channels so the
//! `apply_*_chat_updates` handlers stay unchanged.
//!
//! Lifecycle (see also `docs/DM_LISTENER_FLOW.md`): the task is spawned once at
//! startup and respawned on client reload/reconnect, exactly like the trade DM
//! listener. Chat keys are tracked/untracked via the global command channel
//! published by [`set_chat_router_cmd_tx`].

use std::collections::HashMap;
use std::sync::Mutex;

use nostr_sdk::prelude::*;
use tokio::sync::mpsc::{self, UnboundedSender};
use uuid::Uuid;

use crate::models::Order;
use crate::ui::{AdminChatUpdate, ChatParty, OrderChatUpdate};
use crate::util::chat_utils::{
    derive_shared_key_hex, fetch_gift_wraps_for_shared_key, keys_from_shared_hex,
    unwrap_giftwrap_with_shared_key,
};

/// Identifies which chat a shared key belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatKeyId {
    /// User P2P order chat, keyed by order id (UUID string).
    Order(String),
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
        /// Only emit history messages at/after this unix timestamp (last-seen cursor).
        since: Option<i64>,
    },
    /// Stop tracking a shared-key chat (order row deleted / dispute finalized, etc.).
    UntrackChatKey { key_id: ChatKeyId },
}

/// Per-tracked-key routing metadata.
struct ChatTarget {
    key_id: ChatKeyId,
    shared_keys: Keys,
    /// Order chat only; enables echo-skip in `apply_user_order_chat_updates`.
    local_trade_pubkey: Option<PublicKey>,
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
        since,
    });
}

/// Stop tracking a user P2P order chat (order row removed / terminal cancel).
pub fn untrack_order_chat(order_id: String) {
    send_chat_router_cmd(ChatRouterCmd::UntrackChatKey {
        key_id: ChatKeyId::Order(order_id),
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
/// from `trade_keys` + `counterparty_pubkey`), and emits a track command. Idempotent at the
/// router level: re-tracking an already-tracked key is a cheap no-op, so this is safe to call
/// on every trade DM.
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
        track_order_chat(order_id.to_string(), hex, trade_keys.public_key(), None);
    }
}

/// Emit decrypted chat messages on the appropriate update channel.
///
/// Reuses the same `AdminChatUpdate` / `OrderChatUpdate` shapes as the old
/// polling path so `apply_admin_chat_updates` / `apply_user_order_chat_updates`
/// (which dedupe by timestamp/last-seen) are unchanged.
fn emit_messages(
    target: &ChatTarget,
    messages: Vec<(String, i64, PublicKey)>,
    admin_tx: &UnboundedSender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    user_tx: &UnboundedSender<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
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
            let _ = user_tx.send(Ok(vec![OrderChatUpdate {
                order_id: order_id.clone(),
                local_trade_pubkey,
                messages,
            }]));
        }
        ChatKeyId::Dispute(dispute_id, party) => {
            let _ = admin_tx.send(Ok(vec![AdminChatUpdate {
                dispute_id: dispute_id.clone(),
                party: *party,
                messages,
            }]));
        }
    }
}

/// Rebuild the single batched live subscription from the current tracked set.
///
/// Unsubscribes the previous subscription and, if any keys remain, subscribes to
/// `kind: 1059` gift wraps addressed to all tracked shared pubkeys. Uses
/// `.limit(0)` (live-only, same as the DM listener's `LiveOnly` mode); startup
/// history is hydrated separately per key on [`ChatRouterCmd::TrackChatKey`].
async fn resubscribe(
    client: &Client,
    targets: &HashMap<PublicKey, ChatTarget>,
    current_sub: &mut Option<SubscriptionId>,
) {
    if let Some(id) = current_sub.take() {
        client.unsubscribe(&id).await;
    }
    if targets.is_empty() {
        return;
    }
    let pubkeys: Vec<PublicKey> = targets.keys().copied().collect();
    let filter = Filter::new().kind(Kind::GiftWrap).pubkeys(pubkeys).limit(0);
    match client.subscribe(filter, None).await {
        Ok(output) => {
            log::debug!(
                "[chat_live] subscribed to {} shared-key chat(s) subscription_id={}",
                targets.len(),
                output.val
            );
            *current_sub = Some(output.val);
        }
        Err(e) => log::warn!("[chat_live] failed to subscribe shared-key chats: {e}"),
    }
}

/// Single background router for all shared-key chats (user order + admin dispute).
///
/// Spawned once at startup and respawned on client reload/reconnect (mirrors
/// `listen_for_order_messages`). Consumes [`ChatRouterCmd`] for track/untrack and
/// routes live `kind: 1059` gift wraps by `p` tag to the owning chat.
pub async fn listen_for_chat_messages(
    client: Client,
    admin_chat_updates_tx: UnboundedSender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    user_order_chat_updates_tx: UnboundedSender<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
    mut cmd_rx: mpsc::UnboundedReceiver<ChatRouterCmd>,
) {
    // Create the notification receiver BEFORE subscribing so no live event is missed.
    let mut notifications = client.notifications();
    let mut targets: HashMap<PublicKey, ChatTarget> = HashMap::new();
    let mut current_sub: Option<SubscriptionId> = None;

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    // Sender dropped (respawn/shutdown): unsubscribe and exit.
                    if let Some(id) = current_sub.take() {
                        client.unsubscribe(&id).await;
                    }
                    break;
                };
                match cmd {
                    ChatRouterCmd::TrackChatKey { key_id, shared_key_hex, local_trade_pubkey, since } => {
                        let Some(shared_keys) = keys_from_shared_hex(&shared_key_hex) else {
                            log::warn!("[chat_live] invalid shared key hex for {key_id:?}; not tracking");
                            continue;
                        };
                        let target_pubkey = shared_keys.public_key();
                        // Idempotent: skip redundant history fetch + resubscribe if already tracked.
                        if targets.get(&target_pubkey).is_some_and(|t| t.key_id == key_id) {
                            continue;
                        }
                        let target = ChatTarget {
                            key_id: key_id.clone(),
                            shared_keys: shared_keys.clone(),
                            local_trade_pubkey,
                        };

                        // One-shot history hydration (relay subscriptions alone don't replay history).
                        match fetch_gift_wraps_for_shared_key(&client, &shared_keys).await {
                            Ok(messages) => {
                                let cutoff = since.unwrap_or(0);
                                let history: Vec<(String, i64, PublicKey)> = messages
                                    .into_iter()
                                    .filter(|(_, ts, _)| *ts >= cutoff)
                                    .collect();
                                emit_messages(&target, history, &admin_chat_updates_tx, &user_order_chat_updates_tx);
                            }
                            Err(e) => log::warn!("[chat_live] history fetch failed for {key_id:?}: {e}"),
                        }

                        targets.insert(target_pubkey, target);
                        resubscribe(&client, &targets, &mut current_sub).await;
                    }
                    ChatRouterCmd::UntrackChatKey { key_id } => {
                        let before = targets.len();
                        targets.retain(|_, t| t.key_id != key_id);
                        if targets.len() != before {
                            resubscribe(&client, &targets, &mut current_sub).await;
                        }
                    }
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
                if event.kind != Kind::GiftWrap {
                    continue;
                }
                // Gift wrap recipient is the shared-key pubkey in the `p` tag.
                let Some(target_pubkey) = event.tags.public_keys().next().copied() else {
                    continue;
                };
                let Some(target) = targets.get(&target_pubkey) else {
                    continue;
                };
                match unwrap_giftwrap_with_shared_key(&target.shared_keys, &event).await {
                    Ok((content, ts, sender)) => {
                        emit_messages(
                            target,
                            vec![(content, ts, sender)],
                            &admin_chat_updates_tx,
                            &user_order_chat_updates_tx,
                        );
                    }
                    Err(e) => log::warn!(
                        "[chat_live] failed to unwrap chat gift wrap {}: {e}",
                        event.id
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
