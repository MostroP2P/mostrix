use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::sqlite::SqlitePool;

use crate::models::Order;

/// Delete an order row from the local database.
///
/// Used when the local state should be discarded (e.g. taker cancels pre-Active and the order
/// returns to the public book as Pending).
pub async fn delete_order_by_id(pool: &SqlitePool, order_id: &str) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM orders
        WHERE id = ?
        "#,
    )
    .bind(order_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Save an order to the database (ported from mostro-cli).
///
/// `is_maker`: `true` when the user published the order (maker), `false` when they took an order (taker).
/// `full_privacy`: omit identity proof on protocol DMs for this trade.
pub async fn save_order(
    order: SmallOrder,
    trade_keys: &Keys,
    request_id: u64,
    trade_index: i64,
    pool: &SqlitePool,
    is_maker: bool,
    full_privacy: bool,
) -> Result<()> {
    if let Ok(order) = Order::new(
        pool,
        order,
        trade_keys,
        Some(request_id as i64),
        trade_index,
        is_maker,
        full_privacy,
    )
    .await
    {
        if let Some(order_id) = order.id {
            log::info!("Order {} created", order_id);
        } else {
            log::warn!("Warning: The newly created order has no ID.");
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

#[cfg(test)]
mod tests {
    use super::save_order;
    use mostro_core::prelude::{Kind, SmallOrder, Status};
    use nostr_sdk::prelude::Keys;
    use uuid::Uuid;

    #[tokio::test]
    async fn save_order_propagates_invalid_trade_index() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("memory db");
        sqlx::query(
            r#"
            CREATE TABLE orders (
                id TEXT PRIMARY KEY, kind TEXT, status TEXT, amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL, min_amount INTEGER, max_amount INTEGER,
                fiat_amount INTEGER NOT NULL, payment_method TEXT NOT NULL,
                premium INTEGER NOT NULL, trade_keys TEXT, counterparty_pubkey TEXT,
                order_chat_shared_key_hex TEXT, dispute_id TEXT, solver_pubkey TEXT,
                dispute_chat_shared_key_hex TEXT, is_mine INTEGER NOT NULL,
                full_privacy INTEGER NOT NULL DEFAULT 0,
                pending_next_trade_index INTEGER,
                buyer_invoice TEXT, request_id INTEGER, trade_index INTEGER,
                created_at INTEGER, expires_at INTEGER, last_seen_dm_ts INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("orders");

        let err = save_order(
            SmallOrder {
                id: Some(Uuid::new_v4()),
                kind: Some(Kind::Buy),
                status: Some(Status::Pending),
                amount: 1000,
                fiat_code: "USD".into(),
                fiat_amount: 10,
                payment_method: "ln".into(),
                ..Default::default()
            },
            &Keys::generate(),
            1,
            0,
            &pool,
            true,
            false,
        )
        .await
        .expect_err("trade_index 0 must surface as Err, not Ok(())");
        assert!(
            err.to_string().contains("trade_index"),
            "unexpected error: {err}"
        );
    }
}
