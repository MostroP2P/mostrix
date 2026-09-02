use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use mostro_core::prelude::{
    Action, DisputeStatus, Kind as OrderKind, Message, Payload, SmallOrder, Status, Transport,
};
use nostr_sdk::prelude::{Client, Keys, PublicKey};
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use super::order_chat_projection::order_chat_list_item_from_db_order;
use crate::models::{AdminDispute, Order, User};
use crate::ui::{
    AdminChatLastSeen, AdminChatUpdate, AppState, ChatParty, DecodedChatMessage,
    DisputeChatMessage, MessageNotification, OperationResult, OrderChatLastSeen,
    OrderChatStaticHeader, OrderMessage, UserChatChannel, UserChatSender, UserOrderChatMessage,
    UserRole,
};
use crate::util::{
    chat_listener::{track_dispute_chat, track_order_chat, track_user_dispute_chat},
    chat_utils::{
        clamp_chat_since_cursor_now, derive_shared_key_hex, dispute_chat_allowed_signers,
        dispute_chat_role_for_inner_signer, fetch_chat_messages_for_shared_key,
        keys_from_shared_hex, order_chat_allowed_signers, parse_chat_pubkey,
    },
    hydrate_startup_active_order_dm_state, replay_active_trade_dms, seed_admin_chat_last_seen,
};

use super::attachments::{
    build_attachment_toast, legacy_placeholder_matches_filename, try_parse_attachment_message,
};
use super::chat_storage::{
    dispute_chat_inner_id_known, load_chat_from_file, load_order_chat_from_file,
    load_user_dispute_chat_from_file, max_party_timestamps, order_chat_inner_id_known,
    remember_dispute_chat_inner_id, remember_order_chat_inner_id,
    remember_user_dispute_chat_inner_id, rewrite_dispute_chat_messages,
    rewrite_order_chat_messages, save_chat_message, save_order_chat_message,
    save_user_dispute_chat_message, user_dispute_chat_inner_id_known,
    user_dispute_chat_since_from_file,
};

/// Parse `admin_privkey` text and store in [`AppState::admin_keys`].
pub fn hydrate_app_admin_keys_from_privkey(app: &mut AppState, admin_privkey: &str) {
    app.admin_keys = if admin_privkey.trim().is_empty() {
        None
    } else {
        match Keys::parse(admin_privkey.trim()) {
            Ok(keys) => Some(keys),
            Err(e) => {
                log::warn!("Invalid admin_privkey: {e}");
                None
            }
        }
    };
}

/// Admin Nostr keys for shared-key dispute chat send/fetch when in admin mode.
#[must_use]
pub fn admin_chat_keys_clone_for_role(app: &AppState) -> Option<Keys> {
    match app.user_role {
        UserRole::Admin => app.admin_keys.clone(),
        UserRole::User => None,
    }
}

/// Recover chat history from saved files for InProgress disputes.
pub fn recover_admin_chat_from_files(
    admin_disputes_in_progress: &[AdminDispute],
    admin_dispute_chats: &mut HashMap<String, Vec<DisputeChatMessage>>,
    admin_chat_last_seen: &mut HashMap<(String, ChatParty), AdminChatLastSeen>,
) {
    for dispute in admin_disputes_in_progress {
        let is_in_progress = dispute
            .status
            .as_deref()
            .and_then(|s| mostro_core::prelude::DisputeStatus::from_str(s).ok())
            == Some(mostro_core::prelude::DisputeStatus::InProgress);
        if !is_in_progress {
            continue;
        }
        if let Some(msgs) = load_chat_from_file(&dispute.dispute_id) {
            admin_dispute_chats.insert(dispute.dispute_id.clone(), msgs.clone());
            let (buyer_max, seller_max) = max_party_timestamps(&msgs);
            update_last_seen_timestamp(buyer_max, seller_max, dispute, admin_chat_last_seen);
        }
    }
}

fn update_last_seen_timestamp(
    buyer_max_timestamp: i64,
    seller_max_timestamp: i64,
    dispute: &AdminDispute,
    admin_chat_last_seen: &mut HashMap<(String, ChatParty), AdminChatLastSeen>,
) {
    let buyer_entry = admin_chat_last_seen
        .entry((dispute.dispute_id.clone(), ChatParty::Buyer))
        .or_insert_with(|| AdminChatLastSeen {
            last_seen_timestamp: None,
        });
    // Normalize the stored cursor too so a stale future value can't outrank real messages.
    let buyer_existing = buyer_entry
        .last_seen_timestamp
        .map(clamp_chat_since_cursor_now)
        .unwrap_or(0);
    let buyer_new = buyer_existing.max(clamp_chat_since_cursor_now(buyer_max_timestamp));
    if buyer_new > 0 {
        buyer_entry.last_seen_timestamp = Some(buyer_new);
    }

    let seller_entry = admin_chat_last_seen
        .entry((dispute.dispute_id.clone(), ChatParty::Seller))
        .or_insert_with(|| AdminChatLastSeen {
            last_seen_timestamp: None,
        });
    let seller_existing = seller_entry
        .last_seen_timestamp
        .map(clamp_chat_since_cursor_now)
        .unwrap_or(0);
    let seller_new = seller_existing.max(clamp_chat_since_cursor_now(seller_max_timestamp));
    if seller_new > 0 {
        seller_entry.last_seen_timestamp = Some(seller_new);
    }
}

/// Loads admin disputes and restores in-progress chat transcripts from disk.
pub async fn load_admin_disputes_at_startup(pool: &SqlitePool, app: &mut AppState) {
    if app.user_role != UserRole::Admin {
        return;
    }
    let admin_keys_present = app.admin_keys.is_some();
    match AdminDispute::get_all(pool).await {
        Ok(all_disputes) => {
            app.admin_disputes_in_progress = all_disputes;
            if admin_keys_present {
                seed_admin_chat_last_seen(app);
            }
            recover_admin_chat_from_files(
                &app.admin_disputes_in_progress,
                &mut app.admin_dispute_chats,
                &mut app.admin_chat_last_seen,
            );
        }
        Err(e) => {
            log::warn!("Failed to load admin disputes: {}", e);
        }
    }
}

/// Emit initial chat-router track commands for the active set (option B).
///
/// - **User**: every [`Order::get_startup_active_orders`] row (active states + `success`;
///   excludes [`crate::models::TERMINAL_DM_STATUSES`]) with a resolvable shared key —
///   persisted `order_chat_shared_key_hex`, else ECDH from `trade_keys` + `counterparty_pubkey`.
///   Rows without a counterparty trade pubkey are skipped (no inner-signer allow-list).
/// - **Admin**: each InProgress dispute's buyer/seller shared key, tracked with
///   that party's trade pubkey plus the admin pubkey when configured. Parties
///   missing a pubkey are skipped.
///
/// Commands are buffered on the router's channel until the task starts consuming them, so this
/// is safe to call before the chat router task is spawned. History for each key is hydrated by
/// the router on `TrackChatKey` using the passed `since` (last-seen) cursor. After
/// [`OperationResult::SessionRestored`], call again once peer-chat transcripts are on disk
/// ([`OperationResult::PostRestorePeerChatReplayCompleted`]) so live subscriptions cover rebuilt orders.
pub async fn track_startup_chats(pool: &SqlitePool, app: &AppState) {
    match app.user_role {
        UserRole::User => {
            let Ok(rows) = Order::get_startup_active_orders(pool).await else {
                return;
            };
            for row in rows {
                let Ok(order) = Order::get_by_id(pool, &row.id).await else {
                    continue;
                };
                let Some(trade_keys_hex) = order.trade_keys.as_deref().filter(|v| !v.is_empty())
                else {
                    continue;
                };
                let Ok(trade_keys) = Keys::parse(trade_keys_hex) else {
                    continue;
                };
                let shared_hex = order.order_chat_shared_key_hex.clone().or_else(|| {
                    derive_shared_key_hex(Some(&trade_keys), order.counterparty_pubkey.as_deref())
                });
                if let Some(shared_hex) = shared_hex {
                    if let Some(allowed) = order_chat_allowed_signers(
                        trade_keys.public_key(),
                        order.counterparty_pubkey.as_deref(),
                    ) {
                        let since = app
                            .order_chat_last_seen
                            .get(&row.id)
                            .and_then(|s| s.last_seen_timestamp)
                            .map(clamp_chat_since_cursor_now);
                        track_order_chat(
                            row.id.clone(),
                            shared_hex,
                            trade_keys.public_key(),
                            allowed,
                            since,
                        );
                    } else {
                        log::warn!(
                            "startup: order {} missing counterparty pubkey; not tracking chat",
                            row.id
                        );
                    }
                }

                if let (Some(shared_hex), Some(solver)) = (
                    order.dispute_chat_shared_key_hex.clone(),
                    order
                        .solver_pubkey
                        .as_deref()
                        .and_then(|value| PublicKey::parse(value).ok()),
                ) {
                    track_user_dispute_chat(
                        row.id.clone(),
                        shared_hex,
                        trade_keys.public_key(),
                        solver,
                        user_dispute_chat_since_from_file(&row.id),
                    );
                }
            }
        }
        UserRole::Admin => {
            let admin_pk = app.admin_keys.as_ref().map(|k| k.public_key());
            for dispute in &app.admin_disputes_in_progress {
                let is_in_progress = dispute
                    .status
                    .as_deref()
                    .and_then(|s| DisputeStatus::from_str(s).ok())
                    == Some(DisputeStatus::InProgress);
                if !is_in_progress {
                    continue;
                }
                for (party, hex, party_pk) in [
                    (
                        ChatParty::Buyer,
                        dispute.buyer_shared_key_hex.as_deref(),
                        dispute.buyer_pubkey.as_deref(),
                    ),
                    (
                        ChatParty::Seller,
                        dispute.seller_shared_key_hex.as_deref(),
                        dispute.seller_pubkey.as_deref(),
                    ),
                ] {
                    let Some(hex) = hex else {
                        continue;
                    };
                    let Some(allowed) = dispute_chat_allowed_signers(admin_pk.as_ref(), party_pk)
                    else {
                        log::warn!(
                            "startup: dispute {} {party} missing party pubkey; not tracking chat",
                            dispute.dispute_id
                        );
                        continue;
                    };
                    let since = app
                        .admin_chat_last_seen
                        .get(&(dispute.dispute_id.clone(), party))
                        .and_then(|s| s.last_seen_timestamp)
                        .map(clamp_chat_since_cursor_now);
                    track_dispute_chat(
                        dispute.dispute_id.clone(),
                        party,
                        hex.to_string(),
                        allowed,
                        since,
                    );
                }
            }
        }
    }
}

/// Load user order chat at startup from on-disk transcripts.
///
/// Relay history is **not** polled here — [`track_startup_chats`] seeds the shared-key chat
/// router, which hydrates once per key on `TrackChatKey` (avoids a duplicate fetch). After
/// [`OperationResult::SessionRestored`], peer transcripts are rebuilt from relay via
/// [`spawn_post_restore_peer_chat_hydrate`] instead of relying on this path alone.
pub async fn load_user_order_chats_at_startup(pool: &SqlitePool, app: &mut AppState) {
    if app.user_role != UserRole::User {
        return;
    }
    sync_user_order_history_messages_from_db(pool, app).await;
    let Ok(rows) = Order::get_startup_active_orders(pool).await else {
        return;
    };

    for row in rows {
        let order_id = row.id.clone();
        if let Some(messages) = load_order_chat_from_file(&order_id) {
            let max_ts = messages.iter().map(|m| m.timestamp).max().unwrap_or(0);
            app.order_chats.insert(order_id.clone(), messages);
            app.order_chat_last_seen.insert(
                order_id.clone(),
                OrderChatLastSeen {
                    last_seen_timestamp: Some(clamp_chat_since_cursor_now(max_ts)),
                },
            );
        }
        if let Some(messages) = load_user_dispute_chat_from_file(&order_id) {
            let max_ts = messages.iter().map(|m| m.timestamp).max().unwrap_or(0);
            app.user_dispute_chats.insert(order_id.clone(), messages);
            app.user_dispute_chat_last_seen.insert(
                order_id,
                OrderChatLastSeen {
                    last_seen_timestamp: Some(clamp_chat_since_cursor_now(max_ts)),
                },
            );
        }
    }

    refresh_my_trades_maker_book_cache(pool, app).await;
}

/// Rebuild [`crate::ui::AppState::my_trades_maker_book`] from SQLite (maker + `pending` only).
pub async fn refresh_my_trades_maker_book_cache(pool: &SqlitePool, app: &mut AppState) {
    let rows = match Order::get_user_history_orders(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            log::warn!(
                "Failed to load orders for My Trades maker-book cache: {}",
                e
            );
            return;
        }
    };
    app.my_trades_maker_book = rows
        .iter()
        .filter_map(order_chat_list_item_from_db_order)
        .collect();
}

/// Buyer vs seller for mapping persisted status to a Messages-tab [`Action`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TradeSide {
    Buyer,
    Seller,
}

/// Infer buyer/seller from maker/taker flag and order kind (SQLite rows lack trade pubkeys).
fn trade_side_for_db_order(is_mine: bool, kind: Option<OrderKind>) -> TradeSide {
    match (is_mine, kind) {
        (true, Some(OrderKind::Sell)) => TradeSide::Seller,
        (true, Some(OrderKind::Buy)) => TradeSide::Buyer,
        (false, Some(OrderKind::Sell)) => TradeSide::Buyer,
        (false, Some(OrderKind::Buy)) => TradeSide::Seller,
        (true, None) => TradeSide::Seller,
        (false, None) => TradeSide::Buyer,
    }
}

/// Fallback when `orders.status` is missing or unparsable.
fn history_action_without_status(is_mine: bool, kind: Option<OrderKind>) -> Action {
    if is_mine {
        Action::NewOrder
    } else {
        match kind {
            Some(OrderKind::Buy) => Action::TakeBuy,
            Some(OrderKind::Sell) => Action::TakeSell,
            None => Action::WaitingSellerToPay,
        }
    }
}

/// Map persisted order status + role to a Messages-tab [`Action`].
///
/// Aligned with the mobile restore path (`RestoreService._getActionFromStatus`) so
/// startup/restore DB sync shows the correct trade phase before relay DM replay.
/// Trade-DM hydration may still replace the row when a fresher rumor arrives.
pub(crate) fn history_action_for_db_order(order: &Order) -> Action {
    let kind = order
        .kind
        .as_deref()
        .and_then(|k| OrderKind::from_str(k).ok());
    let status = order
        .status
        .as_deref()
        .and_then(|s| Status::from_str(s).ok());
    let side = trade_side_for_db_order(order.is_mine, kind);

    let Some(status) = status else {
        return history_action_without_status(order.is_mine, kind);
    };

    match status {
        Status::Pending => Action::NewOrder,
        Status::WaitingMakerBond | Status::WaitingTakerBond => Action::PayBondInvoice,
        Status::WaitingBuyerInvoice => match side {
            TradeSide::Buyer => Action::AddInvoice,
            TradeSide::Seller => Action::WaitingBuyerInvoice,
        },
        Status::WaitingPayment => match side {
            TradeSide::Seller => Action::PayInvoice,
            TradeSide::Buyer => Action::WaitingSellerToPay,
        },
        Status::InProgress => match side {
            TradeSide::Buyer => Action::HoldInvoicePaymentAccepted,
            TradeSide::Seller => Action::BuyerTookOrder,
        },
        Status::Active => {
            let matched = order
                .counterparty_pubkey
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty());
            // Unmatched maker listing stays NewOrder (hidden in Messages); matched trades use side.
            if order.is_mine && !matched {
                Action::NewOrder
            } else {
                match side {
                    TradeSide::Buyer => Action::HoldInvoicePaymentAccepted,
                    TradeSide::Seller => Action::BuyerTookOrder,
                }
            }
        }
        Status::FiatSent => Action::FiatSentOk,
        Status::SettledHoldInvoice => match side {
            TradeSide::Buyer => Action::Released,
            TradeSide::Seller => Action::HoldInvoicePaymentSettled,
        },
        Status::Success => Action::PurchaseCompleted,
        Status::Canceled | Status::Expired => Action::Canceled,
        Status::CanceledByAdmin => Action::AdminCanceled,
        Status::SettledByAdmin | Status::CompletedByAdmin => Action::AdminSettled,
        Status::Dispute => Action::DisputeInitiatedByPeer,
        Status::CooperativelyCanceled => Action::CooperativeCancelInitiatedByPeer,
    }
}

fn db_order_to_history_message(order: &Order, sender: PublicKey) -> Option<OrderMessage> {
    let order_id_str = order.id.as_deref()?;
    let order_id = Uuid::parse_str(order_id_str).ok()?;
    let trade_index = order.trade_index?;
    let status = order
        .status
        .as_deref()
        .and_then(|s| Status::from_str(s).ok());
    let kind = order
        .kind
        .as_deref()
        .and_then(|k| OrderKind::from_str(k).ok());

    let action = history_action_for_db_order(order);

    let payload_order = SmallOrder {
        id: Some(order_id),
        kind,
        status,
        amount: order.amount,
        fiat_code: order.fiat_code.clone(),
        min_amount: order.min_amount,
        max_amount: order.max_amount,
        fiat_amount: order.fiat_amount,
        payment_method: order.payment_method.clone(),
        premium: order.premium,
        buyer_invoice: order.buyer_invoice.clone(),
        created_at: order.created_at,
        expires_at: order.expires_at,
        ..Default::default()
    };

    let request_id = order.request_id.and_then(|id| u64::try_from(id).ok());
    let message = Message::new_order(
        Some(order_id),
        request_id,
        Some(trade_index),
        action,
        Some(Payload::Order(payload_order.clone())),
    );

    let history_message = OrderMessage {
        message,
        timestamp: order
            .last_seen_dm_ts
            .or(order.created_at)
            .unwrap_or_else(|| chrono::Utc::now().timestamp()),
        sender,
        order_id: Some(order_id),
        trade_index,
        sat_amount: None,
        buyer_invoice: order.buyer_invoice.clone(),
        order_kind: kind,
        is_mine: Some(order.is_mine),
        order_status: status,
        order_snapshot: Some(payload_order),
        read: true,
        auto_popup_shown: !matches!(
            status,
            Some(Status::WaitingBuyerInvoice | Status::WaitingMakerBond | Status::WaitingTakerBond)
        ),
    };
    Some(history_message)
}

fn order_chat_static_from_db_order(row: &Order) -> Option<OrderChatStaticHeader> {
    let id_str = row.id.as_deref()?;
    let order_id = Uuid::parse_str(id_str).ok()?;
    let kind = row
        .kind
        .as_deref()
        .and_then(|s| OrderKind::from_str(s).ok());
    let trade_index = row.trade_index?;
    let keys_hex = row.trade_keys.as_deref()?;
    let trade_keys = Keys::parse(keys_hex).ok()?;
    Some(OrderChatStaticHeader {
        order_id,
        kind,
        created_at: row.created_at,
        trade_index,
        initiator_trade_pubkey: trade_keys.public_key().to_string(),
        is_mine: row.is_mine,
        solver_pubkey: row.solver_pubkey.clone(),
        dispute_id: row.dispute_id.clone(),
    })
}

pub async fn sync_user_order_history_messages_from_db(pool: &SqlitePool, app: &mut AppState) {
    let identity_keys = match User::get_identity_keys(pool).await {
        Ok(k) => k,
        Err(e) => {
            log::warn!(
                "Failed to derive identity keys for DB history sender attribution: {}",
                e
            );
            return;
        }
    };
    let sender = identity_keys.public_key();
    let rows = match Order::get_user_history_orders(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            log::warn!("Failed to load user order history rows at startup: {}", e);
            return;
        }
    };
    let mut history_messages: Vec<OrderMessage> = rows
        .iter()
        .filter_map(|row| db_order_to_history_message(row, sender))
        .collect();
    history_messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));

    match app.messages.lock() {
        Ok(mut messages) => {
            for msg in history_messages {
                messages.retain(|m| m.order_id != msg.order_id);
                messages.push(msg);
            }
            messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
        }
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
        }
    }
    for row in &rows {
        if let Some(h) = order_chat_static_from_db_order(row) {
            app.order_chat_static.insert(h.order_id, h);
        }
    }
}

/// Clear in-memory peer/solver chat transcripts and relay cursors.
///
/// After a session wipe or before post-restore hydrate, stale `order_chat_last_seen`
/// values from the prior identity would bound relay fetches incorrectly and echo-skip
/// logic would drop the user's own messages when the on-disk transcript is empty.
pub fn clear_session_chat_projection(app: &mut AppState) {
    app.order_chats.clear();
    app.user_dispute_chats.clear();
    app.order_chat_last_seen.clear();
    app.user_dispute_chat_last_seen.clear();
    app.order_chat_static.clear();
    app.startup_popup_floor_ts.clear();
    app.buyer_invoice_preference.clear();
    app.orders_needing_replacement_invoice.clear();
    app.my_trades_maker_book.clear();
    app.pending_order_attachment_sends.clear();
    app.sending_attachment_order_id = None;
    app.selected_order_chat_idx = 0;
    app.order_chat_input.clear();
    app.order_chat_input_enabled = false;
    app.order_chat_selected_message_idx = None;
    app.order_chat_line_starts.clear();
    app.order_chat_scroll_tracker = None;
    if let Ok(mut dropped) = app.dropped_user_history_order_ids.lock() {
        dropped.clear();
    }
}

/// Snapshot of app/DB state needed for a background post-restore trade-DM replay.
pub struct PostRestoreTradeDmReplayJob {
    transport: Transport,
    startup_active_orders: HashMap<Uuid, i64>,
    order_last_seen_dm_ts: HashMap<Uuid, i64>,
    messages: Arc<Mutex<Vec<OrderMessage>>>,
    pending_notifications: Arc<Mutex<usize>>,
    active_order_trade_indices: Arc<Mutex<HashMap<Uuid, i64>>>,
    dropped_user_history_order_ids: Arc<Mutex<std::collections::HashSet<Uuid>>>,
}

/// Hydrate DM-router indices on `app` and return relay-fetch inputs for a background replay.
pub async fn prepare_post_restore_trade_dm_replay(
    pool: &SqlitePool,
    app: &mut AppState,
) -> Option<PostRestoreTradeDmReplayJob> {
    if app.user_role != UserRole::User {
        return None;
    }

    let startup_dm = match hydrate_startup_active_order_dm_state(pool).await {
        Ok(h) => h,
        Err(e) => {
            log::warn!("Post-restore: failed to load active-order DM map: {e}");
            return None;
        }
    };

    if let Ok(mut indices) = app.active_order_trade_indices.lock() {
        *indices = startup_dm.active_order_trade_indices.clone();
    }
    for (order_id, ts) in &startup_dm.order_last_seen_dm_ts {
        app.startup_popup_floor_ts.entry(*order_id).or_insert(*ts);
    }

    Some(PostRestoreTradeDmReplayJob {
        transport: app.transport,
        startup_active_orders: startup_dm.active_order_trade_indices,
        order_last_seen_dm_ts: startup_dm.order_last_seen_dm_ts,
        messages: Arc::clone(&app.messages),
        pending_notifications: Arc::clone(&app.pending_notifications),
        active_order_trade_indices: Arc::clone(&app.active_order_trade_indices),
        dropped_user_history_order_ids: Arc::clone(&app.dropped_user_history_order_ids),
    })
}

/// Spawn relay fetch + hydrate without blocking the UI loop; completion is reported on
/// `order_result_tx` (silent [`OperationResult::PostRestoreTradeDmReplayCompleted`]).
pub fn spawn_post_restore_trade_dm_replay(
    job: PostRestoreTradeDmReplayJob,
    pool: SqlitePool,
    client: Client,
    mostro_pubkey: PublicKey,
    message_notification_tx: UnboundedSender<MessageNotification>,
    order_result_tx: UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        let user = match User::get(&pool).await {
            Ok(u) => u,
            Err(e) => {
                log::warn!("Post-restore: failed to load user for trade DM replay: {e}");
                let _ = order_result_tx.send(OperationResult::PostRestoreTradeDmReplayCompleted);
                return;
            }
        };

        let summary = replay_active_trade_dms(
            &client,
            mostro_pubkey,
            job.transport,
            &pool,
            &user,
            job.messages,
            job.pending_notifications,
            message_notification_tx,
            job.active_order_trade_indices,
            job.dropped_user_history_order_ids,
            job.startup_active_orders,
            job.order_last_seen_dm_ts,
        )
        .await;

        log::info!(
            "Post-restore trade DM replay: attempted={} hydrated={} empty={} fetch_failed={} parse_failed={} skipped_no_keys={}",
            summary.attempted,
            summary.hydrated,
            summary.empty,
            summary.fetch_failed,
            summary.parse_failed,
            summary.skipped_no_keys
        );

        let _ = order_result_tx.send(OperationResult::PostRestoreTradeDmReplayCompleted);
    });
}

/// Outcome of relay rebuild for peer order chats after session restore.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PeerOrderChatRestoreSummary {
    pub attempted: usize,
    pub hydrated: usize,
    pub skipped_no_key: usize,
    pub fetch_failed: usize,
    pub empty: usize,
}

fn peer_chat_sender_for_decode(
    local_trade_pubkey: &PublicKey,
    inner_sender: &PublicKey,
) -> UserChatSender {
    if inner_sender == local_trade_pubkey {
        UserChatSender::You
    } else {
        UserChatSender::Peer
    }
}

/// Build a sorted peer-chat transcript from relay-decoded rows (deduped by inner event id).
pub fn peer_order_chat_transcript_from_decoded(
    decoded_messages: Vec<DecodedChatMessage>,
    local_trade_pubkey: PublicKey,
) -> Vec<UserOrderChatMessage> {
    use std::collections::HashSet;

    let mut seen_inner = HashSet::new();
    let mut transcript = Vec::new();
    for decoded in decoded_messages {
        if !seen_inner.insert(decoded.inner_event_id) {
            continue;
        }
        let (content, attachment) = match try_parse_attachment_message(&decoded.content) {
            Some((attachment, display)) => (display, Some(attachment)),
            None => (decoded.content, None),
        };
        transcript.push(UserOrderChatMessage {
            sender: peer_chat_sender_for_decode(&local_trade_pubkey, &decoded.sender),
            content,
            timestamp: decoded.timestamp,
            attachment,
        });
    }
    transcript.sort_by_key(|m| m.timestamp);
    transcript
}

/// Fetch peer order chat from relays, persist transcript, record inner ids for dedupe.
async fn rebuild_peer_order_chat_transcript(
    client: &Client,
    order: &Order,
) -> Result<Option<Vec<UserOrderChatMessage>>, anyhow::Error> {
    let order_id = order
        .id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("order row missing id"))?;
    let trade_keys_hex = order
        .trade_keys
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("order {order_id} missing trade keys"))?;
    let trade_keys = Keys::parse(trade_keys_hex)?;
    let local_trade_pubkey = trade_keys.public_key();

    let shared_hex = order
        .order_chat_shared_key_hex
        .clone()
        .or_else(|| derive_shared_key_hex(Some(&trade_keys), order.counterparty_pubkey.as_deref()));
    let Some(shared_hex) = shared_hex else {
        return Ok(None);
    };
    let Some(shared_keys) = keys_from_shared_hex(&shared_hex) else {
        return Err(anyhow::anyhow!(
            "invalid shared key hex for order {order_id}"
        ));
    };
    let Some(allowed) =
        order_chat_allowed_signers(local_trade_pubkey, order.counterparty_pubkey.as_deref())
    else {
        return Ok(None);
    };

    let decoded = fetch_chat_messages_for_shared_key(client, &shared_keys, &allowed, None).await?;
    if decoded.is_empty() {
        return Ok(None);
    }

    let mut seen_inner = std::collections::HashSet::new();
    let mut deduped = Vec::new();
    for msg in decoded {
        if seen_inner.insert(msg.inner_event_id) {
            deduped.push(msg);
        }
    }
    for inner in &deduped {
        let _ = remember_order_chat_inner_id(order_id, &inner.inner_event_id);
    }

    let transcript = peer_order_chat_transcript_from_decoded(deduped, local_trade_pubkey);
    if transcript.is_empty() {
        return Ok(None);
    }

    if !rewrite_order_chat_messages(order_id, &transcript) {
        return Err(anyhow::anyhow!(
            "failed to persist peer chat transcript for order {order_id}"
        ));
    }

    Ok(Some(transcript))
}

/// Relay-fetch peer chats for active restored orders (awaitable).
pub async fn rebuild_peer_order_chats_after_restore(
    client: &Client,
    pool: &SqlitePool,
    order_ids: &[String],
) -> (PeerOrderChatRestoreSummary, Vec<String>) {
    let mut summary = PeerOrderChatRestoreSummary::default();
    let mut hydrated_ids = Vec::new();

    for order_id in order_ids {
        summary.attempted += 1;
        let order = match Order::get_by_id(pool, order_id).await {
            Ok(o) => o,
            Err(e) => {
                log::warn!("Post-restore peer chat: order {order_id} not in DB: {e}");
                summary.skipped_no_key += 1;
                continue;
            }
        };
        if order
            .order_chat_shared_key_hex
            .as_deref()
            .filter(|s| !s.is_empty())
            .is_none()
            && order.counterparty_pubkey.is_none()
        {
            summary.skipped_no_key += 1;
            continue;
        }

        match rebuild_peer_order_chat_transcript(client, &order).await {
            Ok(Some(_)) => {
                summary.hydrated += 1;
                hydrated_ids.push(order_id.clone());
            }
            Ok(None) => summary.empty += 1,
            Err(e) => {
                log::warn!("Post-restore peer chat: rebuild failed for {order_id}: {e}");
                summary.fetch_failed += 1;
            }
        }
    }

    (summary, hydrated_ids)
}

/// Load rebuilt peer-chat transcripts from disk into `app` (after background restore).
pub fn apply_restored_peer_order_chats_from_disk(app: &mut AppState, order_ids: &[String]) {
    for order_id in order_ids {
        let Some(messages) = load_order_chat_from_file(order_id) else {
            continue;
        };
        let max_ts = messages.iter().map(|m| m.timestamp).max().unwrap_or(0);
        app.order_chats.insert(order_id.clone(), messages);
        app.order_chat_last_seen.insert(
            order_id.clone(),
            OrderChatLastSeen {
                last_seen_timestamp: Some(clamp_chat_since_cursor_now(max_ts)),
            },
        );
    }
}

/// Active order ids eligible for post-restore peer-chat relay rebuild.
pub async fn active_peer_chat_order_ids_for_restore(pool: &SqlitePool) -> Vec<String> {
    match Order::get_startup_active_orders(pool).await {
        Ok(rows) => rows.into_iter().map(|r| r.id).collect(),
        Err(e) => {
            log::warn!("Post-restore peer chat: failed to list active orders: {e}");
            Vec::new()
        }
    }
}

/// Spawn peer-chat relay rebuild without blocking the UI loop; reports
/// [`OperationResult::PostRestorePeerChatReplayCompleted`] with hydrated `order_ids`.
pub fn spawn_post_restore_peer_chat_hydrate(
    pool: SqlitePool,
    client: Client,
    order_ids: Vec<String>,
    order_result_tx: UnboundedSender<OperationResult>,
) {
    if order_ids.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let (summary, hydrated_ids) =
            rebuild_peer_order_chats_after_restore(&client, &pool, &order_ids).await;

        log::info!(
            "Post-restore peer chat rebuild: attempted={} hydrated={} empty={} fetch_failed={} skipped_no_key={}",
            summary.attempted,
            summary.hydrated,
            summary.empty,
            summary.fetch_failed,
            summary.skipped_no_key
        );

        let _ = order_result_tx.send(OperationResult::PostRestorePeerChatReplayCompleted {
            order_ids: hydrated_ids,
        });
    });
}

/// Merge fetched user order chat updates into app state and persist them to file.
///
/// On [`UserChatChannel::Peer`], relay rows from the local trade key are stored as **You**
/// unless the inner event id is already known or an optimistic local line exists at the same
/// timestamp (live-send echo). The solver channel still skips all local-trade-key rows.
///
/// Durable inner-event ids are recorded only after a successful transcript
/// [`save_order_chat_message`] / [`rewrite_order_chat_messages`]. On write
/// failure the id is left unrecorded so a later delivery can retry.
pub fn apply_user_order_chat_updates(app: &mut AppState, updates: Vec<crate::ui::OrderChatUpdate>) {
    for update in updates {
        let order_id = update.order_id.clone();
        let messages_vec = match update.channel {
            UserChatChannel::Peer => app.order_chats.entry(order_id.clone()).or_default(),
            UserChatChannel::Solver => app.user_dispute_chats.entry(order_id.clone()).or_default(),
        };
        let last_seen_map = match update.channel {
            UserChatChannel::Peer => &mut app.order_chat_last_seen,
            UserChatChannel::Solver => &mut app.user_dispute_chat_last_seen,
        };
        let mut max_ts = last_seen_map
            .get(&order_id)
            .and_then(|s| s.last_seen_timestamp)
            .unwrap_or(0);
        for msg in update.messages {
            let content = msg.content;
            let ts = msg.timestamp;
            let sender_pubkey = msg.sender;
            let inner_id = msg.inner_event_id;

            if update.channel == UserChatChannel::Peer && sender_pubkey == update.local_trade_pubkey
            {
                if order_chat_inner_id_known(&order_id, &inner_id) {
                    if ts > max_ts {
                        max_ts = ts;
                    }
                    continue;
                }
                let optimistic_echo = messages_vec.iter().any(|m| {
                    m.sender == UserChatSender::You && m.timestamp == ts && m.content == content
                });
                if optimistic_echo {
                    let _ = remember_order_chat_inner_id(&order_id, &inner_id);
                    if ts > max_ts {
                        max_ts = ts;
                    }
                    continue;
                }
            } else if update.channel == UserChatChannel::Solver
                && sender_pubkey == update.local_trade_pubkey
            {
                if ts > max_ts {
                    max_ts = ts;
                }
                continue;
            }

            // Durable replay guard: skip if already accepted (do not write again).
            let inner_id_known = match update.channel {
                UserChatChannel::Peer => order_chat_inner_id_known(&order_id, &inner_id),
                UserChatChannel::Solver => user_dispute_chat_inner_id_known(&order_id, &inner_id),
            };
            if inner_id_known {
                if ts > max_ts {
                    max_ts = ts;
                }
                continue;
            }

            let (msg_content, attachment) = match update.channel {
                UserChatChannel::Peer => match try_parse_attachment_message(&content) {
                    Some((attachment, display)) => (display, Some(attachment)),
                    None => (content.clone(), None),
                },
                UserChatChannel::Solver => (content.clone(), None),
            };

            if let Some(ref att) = attachment {
                if let Some(idx) = messages_vec.iter().position(|m| {
                    m.timestamp == ts
                        && m.attachment.is_none()
                        && legacy_placeholder_matches_filename(&m.content, &att.filename)
                }) {
                    let previous = messages_vec[idx].clone();
                    let sender = previous.sender;
                    messages_vec[idx] = UserOrderChatMessage {
                        sender,
                        content: msg_content.clone(),
                        timestamp: ts,
                        attachment: Some(att.clone()),
                    };
                    if !rewrite_order_chat_messages(&order_id, messages_vec) {
                        messages_vec[idx] = previous;
                        log::warn!(
                            "Failed to persist order chat attachment upgrade for {order_id}; leaving inner id unrecorded"
                        );
                        continue;
                    }
                    let _ = remember_order_chat_inner_id(&order_id, &inner_id);
                    if ts > max_ts {
                        max_ts = ts;
                    }
                    continue;
                }
            }

            // Relay rows are always Peer; only dedupe against existing Peer messages so an
            // optimistic local You line cannot suppress a real counterparty message at the same second.
            let is_duplicate = messages_vec.iter().any(|m| {
                if m.sender != UserChatSender::Peer || m.timestamp != ts {
                    return false;
                }
                if m.content == msg_content {
                    return true;
                }
                if let Some(att) = attachment.as_ref() {
                    if m.attachment
                        .as_ref()
                        .is_some_and(|a| a.blossom_url == att.blossom_url)
                    {
                        return true;
                    }
                    if legacy_placeholder_matches_filename(&m.content, &att.filename) {
                        return true;
                    }
                }
                false
            });
            if is_duplicate {
                // Content already in the transcript; still record the inner id.
                match update.channel {
                    UserChatChannel::Peer => {
                        let _ = remember_order_chat_inner_id(&order_id, &inner_id);
                    }
                    UserChatChannel::Solver => {
                        let _ = remember_user_dispute_chat_inner_id(&order_id, &inner_id);
                    }
                }
                if ts > max_ts {
                    max_ts = ts;
                }
                continue;
            }

            if let Some(att) = &attachment {
                app.attachment_toast = Some(build_attachment_toast(&att.filename));
            }

            // Relay rows: map inner signer to You/Peer on peer channel (solver stays Peer).
            let sender_label = match update.channel {
                UserChatChannel::Peer if sender_pubkey == update.local_trade_pubkey => {
                    UserChatSender::You
                }
                UserChatChannel::Peer => UserChatSender::Peer,
                UserChatChannel::Solver => UserChatSender::Peer,
            };

            let msg = UserOrderChatMessage {
                sender: sender_label,
                content: msg_content,
                timestamp: ts,
                attachment,
            };
            let saved = match update.channel {
                UserChatChannel::Peer => save_order_chat_message(&order_id, &msg),
                UserChatChannel::Solver => save_user_dispute_chat_message(&order_id, &msg),
            };
            if !saved {
                log::warn!(
                    "Failed to persist {} chat message for {order_id}; leaving inner id unrecorded",
                    update.channel
                );
                continue;
            }
            match update.channel {
                UserChatChannel::Peer => {
                    let _ = remember_order_chat_inner_id(&order_id, &inner_id);
                }
                UserChatChannel::Solver => {
                    let _ = remember_user_dispute_chat_inner_id(&order_id, &inner_id);
                }
            }
            messages_vec.push(msg);
            if ts > max_ts {
                max_ts = ts;
            }
        }
        last_seen_map.insert(
            order_id,
            OrderChatLastSeen {
                last_seen_timestamp: Some(clamp_chat_since_cursor_now(max_ts)),
            },
        );
    }
}

/// Apply fetched admin chat updates back into the UI state and persist
/// last_seen timestamps to the database.
///
/// Inner signers that match neither the buyer nor the seller trade pubkey are
/// dropped (not labeled Admin). Admin echoes are skipped via `admin_chat_pubkey`.
/// Durable inner-event ids are recorded only after a successful transcript
/// [`save_chat_message`] / [`rewrite_dispute_chat_messages`]. On write failure
/// the id is left unrecorded so a later delivery can retry.
pub async fn apply_admin_chat_updates(
    app: &mut AppState,
    updates: Vec<AdminChatUpdate>,
    admin_chat_pubkey: Option<&PublicKey>,
    pool: &sqlx::SqlitePool,
) -> Result<(), anyhow::Error> {
    for update in updates {
        let dispute_key = update.dispute_id.clone();
        let party = update.party;

        let messages_vec = app
            .admin_dispute_chats
            .entry(dispute_key.clone())
            .or_default();
        let mut max_ts = app
            .admin_chat_last_seen
            .get(&(dispute_key.clone(), party))
            .and_then(|s| s.last_seen_timestamp)
            .unwrap_or(0);

        for msg in update.messages {
            let content = msg.content;
            let ts = msg.timestamp;
            let sender_pubkey = msg.sender;
            let inner_id = msg.inner_event_id;

            if let Some(admin_pk) = admin_chat_pubkey {
                if &sender_pubkey == admin_pk {
                    if ts > max_ts {
                        max_ts = ts;
                    }
                    continue;
                }
            }

            if dispute_chat_inner_id_known(&dispute_key, party, &inner_id) {
                if ts > max_ts {
                    max_ts = ts;
                }
                continue;
            }

            let (sender, target_party) = {
                let dispute = app
                    .admin_disputes_in_progress
                    .iter()
                    .find(|d| d.dispute_id == dispute_key);
                let buyer_pk = dispute
                    .and_then(|d| d.buyer_pubkey.as_deref())
                    .and_then(parse_chat_pubkey);
                let seller_pk = dispute
                    .and_then(|d| d.seller_pubkey.as_deref())
                    .and_then(parse_chat_pubkey);
                match dispute_chat_role_for_inner_signer(
                    &sender_pubkey,
                    buyer_pk.as_ref(),
                    seller_pk.as_ref(),
                ) {
                    Some(role) => role,
                    None => {
                        log::warn!(
                            "dropping dispute {dispute_key} chat message from unknown inner signer"
                        );
                        if ts > max_ts {
                            max_ts = ts;
                        }
                        continue;
                    }
                }
            };

            let (msg_content, attachment) = match try_parse_attachment_message(&content) {
                Some((attachment, display)) => (display, Some(attachment)),
                None => (content.clone(), None),
            };

            if let Some(ref att) = attachment {
                if let Some(idx) = messages_vec.iter().position(|m| {
                    m.timestamp == ts
                        && m.sender == sender
                        && m.target_party == target_party
                        && m.attachment.is_none()
                        && legacy_placeholder_matches_filename(&m.content, &att.filename)
                }) {
                    let previous = messages_vec[idx].clone();
                    messages_vec[idx] = DisputeChatMessage {
                        sender,
                        content: msg_content.clone(),
                        timestamp: ts,
                        target_party,
                        attachment: Some(att.clone()),
                    };
                    if !rewrite_dispute_chat_messages(&dispute_key, messages_vec) {
                        messages_vec[idx] = previous;
                        log::warn!(
                            "Failed to persist dispute chat attachment upgrade for {dispute_key}; leaving inner id unrecorded"
                        );
                        continue;
                    }
                    let _ = remember_dispute_chat_inner_id(&dispute_key, party, &inner_id);
                    if ts > max_ts {
                        max_ts = ts;
                    }
                    continue;
                }
            }

            let is_duplicate = messages_vec.iter().any(|m: &DisputeChatMessage| {
                if m.timestamp != ts || m.sender != sender {
                    return false;
                }
                if m.content == msg_content {
                    return true;
                }
                if let Some(att) = attachment.as_ref() {
                    if m.attachment
                        .as_ref()
                        .is_some_and(|a| a.blossom_url == att.blossom_url)
                    {
                        return true;
                    }
                    if legacy_placeholder_matches_filename(&m.content, &att.filename) {
                        return true;
                    }
                }
                false
            });
            if is_duplicate {
                let _ = remember_dispute_chat_inner_id(&dispute_key, party, &inner_id);
                if ts > max_ts {
                    max_ts = ts;
                }
                continue;
            }

            if let Some(att) = &attachment {
                app.attachment_toast = Some(build_attachment_toast(&att.filename));
                if app
                    .admin_disputes_in_progress
                    .iter()
                    .any(|d| d.dispute_id == dispute_key)
                {
                    app.selected_dispute_id = Some(dispute_key.clone());
                    app.active_chat_party = party;
                }
            }
            let msg = DisputeChatMessage {
                sender,
                content: msg_content,
                timestamp: ts,
                target_party,
                attachment,
            };
            if !save_chat_message(&dispute_key, &msg) {
                log::warn!(
                    "Failed to persist dispute chat message for {dispute_key}; leaving inner id unrecorded"
                );
                continue;
            }
            let _ = remember_dispute_chat_inner_id(&dispute_key, party, &inner_id);
            messages_vec.push(msg);
            if ts > max_ts {
                max_ts = ts;
            }
        }

        let entry = app
            .admin_chat_last_seen
            .entry((dispute_key.clone(), party))
            .or_insert_with(|| AdminChatLastSeen {
                last_seen_timestamp: None,
            });
        let clamped_max = clamp_chat_since_cursor_now(max_ts);
        // Normalize the stored cursor so a stale future value can't outrank real messages.
        let existing = entry
            .last_seen_timestamp
            .map(clamp_chat_since_cursor_now)
            .unwrap_or(0);
        let new_last_seen = existing.max(clamped_max);
        if new_last_seen > 0 {
            entry.last_seen_timestamp = Some(new_last_seen);
            if let Err(e) = AdminDispute::update_chat_last_seen_by_dispute_id(
                pool,
                &dispute_key,
                new_last_seen,
                party == ChatParty::Buyer,
            )
            .await
            {
                log::warn!("Failed to update chat last seen: {e}");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod peer_order_chat_restore_tests {
    use super::peer_order_chat_transcript_from_decoded;
    use crate::ui::{DecodedChatMessage, UserChatSender};
    use nostr_sdk::prelude::{EventId, Keys};

    fn decoded(
        ts: i64,
        sender: nostr_sdk::prelude::PublicKey,
        content: &str,
        id_byte: u8,
    ) -> DecodedChatMessage {
        let mut hex = [0u8; 32];
        hex[31] = id_byte;
        DecodedChatMessage {
            content: content.to_string(),
            timestamp: ts,
            sender,
            inner_event_id: EventId::from_byte_array(hex),
        }
    }

    #[test]
    fn transcript_maps_you_and_peer_and_sorts_by_timestamp() {
        let local = Keys::generate();
        let peer = Keys::generate();
        let messages = peer_order_chat_transcript_from_decoded(
            vec![
                decoded(200, peer.public_key(), "from peer", 2),
                decoded(100, local.public_key(), "from me", 1),
            ],
            local.public_key(),
        );
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "from me");
        assert_eq!(messages[0].sender, UserChatSender::You);
        assert_eq!(messages[1].content, "from peer");
        assert_eq!(messages[1].sender, UserChatSender::Peer);
    }
}

#[cfg(test)]
mod clear_session_chat_projection_tests {
    use super::clear_session_chat_projection;
    use crate::ui::{AppState, OrderChatLastSeen, UserChatSender, UserOrderChatMessage, UserRole};
    use uuid::Uuid;

    #[test]
    fn clears_peer_and_solver_chat_maps_and_cursors() {
        let mut app = AppState::new(UserRole::User);
        app.order_chats.insert(
            "order-1".to_string(),
            vec![UserOrderChatMessage {
                sender: UserChatSender::Peer,
                content: "hi".to_string(),
                timestamp: 1,
                attachment: None,
            }],
        );
        app.user_dispute_chats.insert("order-1".to_string(), vec![]);
        app.order_chat_last_seen.insert(
            "order-1".to_string(),
            OrderChatLastSeen {
                last_seen_timestamp: Some(9_999),
            },
        );
        app.user_dispute_chat_last_seen.insert(
            "order-1".to_string(),
            OrderChatLastSeen {
                last_seen_timestamp: Some(8_888),
            },
        );
        app.startup_popup_floor_ts
            .insert(Uuid::new_v4(), 1_700_000_000);
        app.selected_order_chat_idx = 3;
        app.order_chat_input = "draft".to_string();
        app.dropped_user_history_order_ids
            .lock()
            .expect("lock")
            .insert(Uuid::new_v4());

        clear_session_chat_projection(&mut app);

        assert!(app.order_chats.is_empty());
        assert!(app.user_dispute_chats.is_empty());
        assert!(app.order_chat_last_seen.is_empty());
        assert!(app.user_dispute_chat_last_seen.is_empty());
        assert!(app.startup_popup_floor_ts.is_empty());
        assert_eq!(app.selected_order_chat_idx, 0);
        assert!(app.order_chat_input.is_empty());
        assert!(app
            .dropped_user_history_order_ids
            .lock()
            .expect("lock")
            .is_empty());
    }
}

#[cfg(test)]
mod history_action_for_db_order_tests {
    use super::history_action_for_db_order;
    use crate::models::Order;
    use mostro_core::prelude::Action;
    use uuid::Uuid;

    fn sample_order(status: &str, is_mine: bool, kind: &str, counterparty: Option<&str>) -> Order {
        Order {
            id: Some(Uuid::new_v4().to_string()),
            kind: Some(kind.to_string()),
            status: Some(status.to_string()),
            amount: 1_000,
            fiat_code: "USD".to_string(),
            fiat_amount: 50,
            payment_method: "ln".to_string(),
            premium: 0,
            is_mine,
            trade_index: Some(2),
            counterparty_pubkey: counterparty.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn pending_maker_stays_new_order() {
        let order = sample_order("pending", true, "sell", None);
        assert_eq!(history_action_for_db_order(&order), Action::NewOrder);
    }

    #[test]
    fn active_taker_on_sell_uses_hold_invoice_payment_accepted() {
        let peer = "a".repeat(64);
        let order = sample_order("active", false, "sell", Some(peer.as_str()));
        assert_eq!(
            history_action_for_db_order(&order),
            Action::HoldInvoicePaymentAccepted
        );
    }

    #[test]
    fn active_maker_sell_with_counterparty_uses_buyer_took_order() {
        let peer = "b".repeat(64);
        let order = sample_order("active", true, "sell", Some(peer.as_str()));
        assert_eq!(history_action_for_db_order(&order), Action::BuyerTookOrder);
    }

    #[test]
    fn active_maker_buy_with_counterparty_uses_hold_invoice_payment_accepted() {
        let peer = "c".repeat(64);
        let order = sample_order("active", true, "buy", Some(peer.as_str()));
        assert_eq!(
            history_action_for_db_order(&order),
            Action::HoldInvoicePaymentAccepted
        );
    }

    #[test]
    fn active_maker_without_counterparty_stays_new_order() {
        let order = sample_order("active", true, "sell", None);
        assert_eq!(history_action_for_db_order(&order), Action::NewOrder);
    }

    #[test]
    fn in_progress_seller_maker_uses_buyer_took_order() {
        let order = sample_order("in-progress", true, "sell", None);
        assert_eq!(history_action_for_db_order(&order), Action::BuyerTookOrder);
    }

    #[test]
    fn waiting_payment_maps_by_side() {
        let seller = sample_order("waiting-payment", true, "sell", None);
        assert_eq!(history_action_for_db_order(&seller), Action::PayInvoice);

        let buyer = sample_order("waiting-payment", false, "sell", None);
        assert_eq!(
            history_action_for_db_order(&buyer),
            Action::WaitingSellerToPay
        );
    }

    #[test]
    fn fiat_sent_uses_fiat_sent_ok() {
        let order = sample_order("fiat-sent", false, "sell", None);
        assert_eq!(history_action_for_db_order(&order), Action::FiatSentOk);
    }
}
