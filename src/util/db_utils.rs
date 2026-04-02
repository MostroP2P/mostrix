use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::sqlite::SqlitePool;

use crate::models::{Order, User};

/// Save an order to the database (ported from mostro-cli).
///
/// `is_maker`: `true` when the user published the order (maker), `false` when they took an order (taker).
pub async fn save_order(
    order: SmallOrder,
    trade_keys: &Keys,
    request_id: u64,
    trade_index: i64,
    pool: &SqlitePool,
    is_maker: bool,
) -> Result<()> {
    if let Ok(order) = Order::new(
        pool,
        order,
        trade_keys,
        Some(request_id as i64),
        trade_index,
        is_maker,
    )
    .await
    {
        if let Some(order_id) = order.id {
            log::info!("Order {} created", order_id);
        } else {
            log::warn!("Warning: The newly created order has no ID.");
        }

        match User::get(pool).await {
            Ok(_user) => {
                if let Err(e) = User::update_last_trade_index(pool, trade_index).await {
                    log::error!("Failed to update user: {}", e);
                }
            }
            Err(e) => log::error!("Failed to get user: {}", e),
        }
    }
    Ok(())
}

/// Update the status for an existing order in the local database.
/// This is a thin wrapper over `Order::update_status` that logs failures.
pub async fn update_order_status(pool: &SqlitePool, order_id: &str, status: Status) -> Result<()> {
    match Order::update_status(pool, order_id, status).await {
        Ok(()) => {
            log::info!("Updated status for order {} to {:?}", order_id, status);
            Ok(())
        }
        Err(e) => {
            log::error!(
                "Failed to update status for order {} to {:?}: {}",
                order_id,
                status,
                e
            );
            Err(e)
        }
    }
}

/// Best-effort helper to sync the local DB status from a `SmallOrder` that was
/// fetched from relays (e.g. via `order_from_tags`), when an order row already
/// exists locally.
pub async fn refresh_order_status_from_small_order(
    pool: &SqlitePool,
    small_order: &SmallOrder,
) -> Result<()> {
    if let (Some(order_id), Some(status)) = (small_order.id, small_order.status) {
        // Ignore errors here; callers typically run this as a background refresh.
        let _ = update_order_status(pool, &order_id.to_string(), status).await;
    }
    Ok(())
}
