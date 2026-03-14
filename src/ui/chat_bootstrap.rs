use nostr_sdk::prelude::Keys;

use crate::models::Order;
use crate::ui::{AdminChatLastSeen, AppState, ChatParty};
use crate::ui::helpers::recover_user_chat_from_files;

/// Seed `app.admin_chat_last_seen` with last_seen timestamps per (dispute, party)
/// from the list of admin disputes (DB fields buyer_chat_last_seen / seller_chat_last_seen).
pub fn seed_admin_chat_last_seen(app: &mut AppState, _admin_chat_keys: &Keys) {
    let disputes = app.admin_disputes_in_progress.clone();

    for dispute in &disputes {
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

/// Load user trade orders and seed in-memory state for MyTrades chat.
///
/// This mirrors `seed_admin_chat_last_seen` for user-mode chat bootstrap:
/// - caches trade orders for sidebar/header rendering
/// - rebuilds per-order trade pubkeys from stored trade secret keys
/// - recovers persisted chat transcripts + last-seen cursors
pub async fn seed_user_trade_chat_state(app: &mut AppState, pool: &sqlx::SqlitePool) {
    match load_user_trade_orders(pool).await {
        Ok(orders) => {
            apply_user_trade_orders_state(app, &orders, true);
        }
        Err(e) => {
            log::warn!("Failed to load user trade orders: {}", e);
        }
    }
}

/// Load active user trade orders from the database.
pub async fn load_user_trade_orders(pool: &sqlx::SqlitePool) -> Result<Vec<Order>, anyhow::Error> {
    Ok(Order::get_user_trade_orders(pool).await?)
}

/// Apply user trade orders into runtime chat state.
///
/// - caches orders for sidebar/header rendering
/// - refreshes `user_trade_keys_by_order`
/// - seeds `user_chat_last_seen` from persisted DB cursors when missing
/// - optionally recovers transcripts from local files (startup path)
pub fn apply_user_trade_orders_state(
    app: &mut AppState,
    orders: &[Order],
    recover_from_files: bool,
) {
    app.my_trade_orders = orders.to_vec();

    // Rebuild key cache from current orders to avoid stale order->pubkey mappings.
    app.user_trade_keys_by_order.clear();

    for order in orders {
        if let Some(order_id) = order.id.as_deref() {
            app.user_chat_last_seen
                .entry(order_id.to_string())
                .or_insert_with(|| crate::ui::UserChatLastSeen {
                    last_seen_timestamp: order.chat_last_seen,
                });
        }

        if let (Some(order_id), Some(trade_hex)) = (order.id.as_deref(), order.trade_keys.as_deref())
        {
            if let Ok(keys) = Keys::parse(trade_hex) {
                app.user_trade_keys_by_order
                    .insert(order_id.to_string(), keys.public_key());
            }
        }
    }

    if recover_from_files {
        recover_user_chat_from_files(
            orders,
            &mut app.user_trade_chats,
            &mut app.user_chat_last_seen,
        );
    }
}
