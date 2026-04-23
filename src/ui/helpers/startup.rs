use std::collections::HashMap;
use std::str::FromStr;

use mostro_core::prelude::{Action, Kind as OrderKind, Message, Payload, SmallOrder, Status};
use nostr_sdk::prelude::{Client, Keys, PublicKey};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::{AdminDispute, Order, User};
use crate::ui::{
    AdminChatLastSeen, AdminChatUpdate, AppState, ChatParty, ChatSender, DisputeChatMessage,
    OrderChatLastSeen, OrderMessage, UserChatSender, UserOrderChatMessage, UserRole,
};
use crate::util::{chat_utils::fetch_user_order_chat_updates, seed_admin_chat_last_seen};

use super::attachments::{build_attachment_toast, try_parse_attachment_message};
use super::chat_storage::{
    load_chat_from_file, load_order_chat_from_file, max_party_timestamps, save_chat_message,
    save_order_chat_message,
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
    if buyer_max_timestamp > buyer_entry.last_seen_timestamp.unwrap_or(0) {
        buyer_entry.last_seen_timestamp = Some(buyer_max_timestamp);
    }

    let seller_entry = admin_chat_last_seen
        .entry((dispute.dispute_id.clone(), ChatParty::Seller))
        .or_insert_with(|| AdminChatLastSeen {
            last_seen_timestamp: None,
        });
    if seller_max_timestamp > seller_entry.last_seen_timestamp.unwrap_or(0) {
        seller_entry.last_seen_timestamp = Some(seller_max_timestamp);
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

/// Load user order chat at startup.
pub async fn load_user_order_chats_at_startup(
    client: &Client,
    pool: &SqlitePool,
    app: &mut AppState,
) {
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
                    last_seen_timestamp: Some(max_ts),
                },
            );
        }
    }

    let updates = fetch_user_order_chat_updates(client, pool, &app.order_chat_last_seen)
        .await
        .unwrap_or_default();
    apply_user_order_chat_updates(app, updates);
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

    // Use non-NewOrder actions for history projection so My Trades can be DB-fed
    // without polluting Messages-tab rows that are intentionally stripped.
    let action = match kind {
        Some(OrderKind::Buy) => Action::TakeBuy,
        Some(OrderKind::Sell) => Action::TakeSell,
        None => Action::WaitingSellerToPay,
    };

    let mut payload_order = SmallOrder::default();
    payload_order.id = Some(order_id);
    payload_order.kind = kind;
    payload_order.status = status;
    payload_order.amount = order.amount;
    payload_order.fiat_code = order.fiat_code.clone();
    payload_order.min_amount = order.min_amount;
    payload_order.max_amount = order.max_amount;
    payload_order.fiat_amount = order.fiat_amount;
    payload_order.payment_method = order.payment_method.clone();
    payload_order.premium = order.premium;
    payload_order.buyer_invoice = order.buyer_invoice.clone();
    payload_order.created_at = order.created_at;
    payload_order.expires_at = order.expires_at;

    let request_id = order.request_id.and_then(|id| u64::try_from(id).ok());
    let message = Message::new_order(
        Some(order_id),
        request_id,
        Some(trade_index),
        action,
        Some(Payload::Order(payload_order)),
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
        read: true,
        auto_popup_shown: true,
    };
    Some(history_message)
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
    history_messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    match app.messages.lock() {
        Ok(mut messages) => {
            for msg in history_messages {
                messages.retain(|m| m.order_id != msg.order_id);
                messages.push(msg);
            }
            messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        }
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
        }
    }
}

/// Merge fetched user order chat updates into app state and persist them to file.
pub fn apply_user_order_chat_updates(app: &mut AppState, updates: Vec<crate::ui::OrderChatUpdate>) {
    for update in updates {
        let order_id = update.order_id.clone();
        let messages_vec = app.order_chats.entry(order_id.clone()).or_default();
        let mut max_ts = app
            .order_chat_last_seen
            .get(&order_id)
            .and_then(|s| s.last_seen_timestamp)
            .unwrap_or(0);
        for (content, ts, _sender_pubkey) in update.messages {
            let msg = UserOrderChatMessage {
                sender: UserChatSender::Peer,
                content,
                timestamp: ts,
                attachment: None,
            };
            let duplicated = messages_vec
                .iter()
                .any(|m| m.timestamp == msg.timestamp && m.content == msg.content);
            if duplicated {
                continue;
            }
            save_order_chat_message(&order_id, &msg);
            messages_vec.push(msg);
            if ts > max_ts {
                max_ts = ts;
            }
        }
        app.order_chat_last_seen.insert(
            order_id,
            OrderChatLastSeen {
                last_seen_timestamp: Some(max_ts),
            },
        );
    }
}

/// Apply fetched admin chat updates back into the UI state and persist
/// last_seen timestamps to the database.
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

        for (content, ts, sender_pubkey) in update.messages {
            if let Some(admin_pk) = admin_chat_pubkey {
                if &sender_pubkey == admin_pk {
                    if ts > max_ts {
                        max_ts = ts;
                    }
                    continue;
                }
            }

            let (sender, target_party) = app
                .admin_disputes_in_progress
                .iter()
                .find(|d| d.dispute_id == dispute_key)
                .map(|dispute| {
                    let buyer_pk = dispute
                        .buyer_pubkey
                        .as_deref()
                        .and_then(|s| PublicKey::from_str(s).ok());
                    let seller_pk = dispute
                        .seller_pubkey
                        .as_deref()
                        .and_then(|s| PublicKey::from_str(s).ok());
                    if buyer_pk.as_ref() == Some(&sender_pubkey) {
                        (ChatSender::Buyer, None)
                    } else if seller_pk.as_ref() == Some(&sender_pubkey) {
                        (ChatSender::Seller, None)
                    } else {
                        (ChatSender::Admin, Some(party))
                    }
                })
                .unwrap_or((
                    match party {
                        ChatParty::Buyer => ChatSender::Buyer,
                        ChatParty::Seller => ChatSender::Seller,
                    },
                    None,
                ));

            let (msg_content, attachment_opt) = match try_parse_attachment_message(&content) {
                Some((attachment, display)) => {
                    let filename = attachment.filename.clone();
                    (display, Some((attachment, filename)))
                }
                None => (content.clone(), None),
            };

            let is_duplicate = messages_vec.iter().any(|m: &DisputeChatMessage| {
                m.timestamp == ts && m.sender == sender && m.content == msg_content
            });
            if is_duplicate {
                if ts > max_ts {
                    max_ts = ts;
                }
                continue;
            }

            if let Some((_, filename_for_toast)) = &attachment_opt {
                app.attachment_toast = Some(build_attachment_toast(filename_for_toast));
                if let Some(idx) = app
                    .admin_disputes_in_progress
                    .iter()
                    .position(|d| d.dispute_id == dispute_key)
                {
                    app.selected_in_progress_idx = idx;
                    app.active_chat_party = party;
                }
            }
            let msg = match attachment_opt {
                Some((attachment, _)) => DisputeChatMessage {
                    sender,
                    content: msg_content,
                    timestamp: ts,
                    target_party,
                    attachment: Some(attachment),
                },
                None => DisputeChatMessage {
                    sender,
                    content: msg_content,
                    timestamp: ts,
                    target_party,
                    attachment: None,
                },
            };
            save_chat_message(&dispute_key, &msg);
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
        if max_ts > entry.last_seen_timestamp.unwrap_or(0) {
            entry.last_seen_timestamp = Some(max_ts);
        }

        if max_ts > 0 {
            if let Err(e) = AdminDispute::update_chat_last_seen_by_dispute_id(
                pool,
                &dispute_key,
                max_ts,
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
