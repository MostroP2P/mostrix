use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::sqlite::SqlitePool;

use crate::models::{Order, User};

/// Save an order to the database (ported from mostro-cli)
pub async fn save_order(
    order: SmallOrder,
    trade_keys: &Keys,
    request_id: u64,
    trade_index: i64,
    pool: &SqlitePool,
) -> Result<()> {
    if let Ok(order) = Order::new(pool, order, trade_keys, Some(request_id as i64)).await {
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
