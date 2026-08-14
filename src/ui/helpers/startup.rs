use std::collections::HashMap;
use std::str::FromStr;

use mostro_core::prelude::{
    Action, DisputeStatus, Kind as OrderKind, Message, Payload, SmallOrder, Status,
};
use nostr_sdk::prelude::{Keys, PublicKey};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::order_chat_projection::order_chat_list_item_from_db_order;
use crate::models::{AdminDispute, Order, User};
use crate::ui::{
    AdminChatLastSeen, AdminChatUpdate, AppState, ChatParty, DisputeChatMessage, OrderChatLastSeen,
    OrderChatStaticHeader, OrderMessage, UserChatChannel, UserChatSender, UserOrderChatMessage,
    UserRole,
};
use crate::util::{
    chat_listener::{track_dispute_chat, track_order_chat, track_user_dispute_chat},
    chat_utils::{
        clamp_chat_since_cursor_now, derive_shared_key_hex, dispute_chat_allowed_signers,
        dispute_chat_role_for_inner_signer, order_chat_allowed_signers, parse_chat_pubkey,
    },
    seed_admin_chat_last_seen,
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
/// the router on `TrackChatKey` using the passed `since` (last-seen) cursor.
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
/// router, which hydrates once per key on `TrackChatKey` (avoids a duplicate fetch).
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

    let action = if order.is_mine {
        match status {
            Some(Status::WaitingMakerBond) => Action::PayBondInvoice,
            _ => Action::NewOrder,
        }
    } else {
        match kind {
            Some(OrderKind::Buy) => Action::TakeBuy,
            Some(OrderKind::Sell) => Action::TakeSell,
            None => Action::WaitingSellerToPay,
        }
    };

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

/// Merge fetched user order chat updates into app state and persist them to file.
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

            // Skip relay echoes of messages we already added locally on send (mirror admin chat).
            if sender_pubkey == update.local_trade_pubkey {
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

            let msg = UserOrderChatMessage {
                sender: UserChatSender::Peer,
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
