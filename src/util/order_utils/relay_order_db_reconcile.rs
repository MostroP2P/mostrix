//! Align local `orders` rows with terminal statuses seen on Mostro nostr order events.

use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use crate::models::Order;

use super::helper::{
    aggregate_latest_orders_by_id, fetch_mostro_order_events, is_terminal_trade_status,
    should_apply_status_transition,
};

/// Fetch latest order snapshots from relays and apply [`reconcile_one_order_if_terminal`] for each entry.
pub async fn reconcile_terminal_order_statuses_from_relay(
    pool: &SqlitePool,
    relay_latest: &HashMap<Uuid, SmallOrder>,
) -> Result<()> {
    for relay_order in relay_latest.values() {
        reconcile_one_order_if_terminal(pool, relay_order).await;
    }
    Ok(())
}

/// One relay snapshot fetch plus DB reconcile; shared by periodic scheduler and startup.
pub async fn run_relay_order_db_reconcile_once(
    client: &Client,
    pool: &SqlitePool,
    mostro_pubkey: PublicKey,
) -> Result<()> {
    let events = fetch_mostro_order_events(client, mostro_pubkey).await?;
    let latest = aggregate_latest_orders_by_id(&events);
    reconcile_terminal_order_statuses_from_relay(pool, &latest).await
}

/// If `relay_order` carries a terminal status and the local row exists, update SQLite when allowed.
pub async fn reconcile_one_order_if_terminal(pool: &SqlitePool, relay_order: &SmallOrder) {
    let Some(candidate_status) = relay_order.status else {
        return;
    };
    if !is_terminal_trade_status(candidate_status) {
        return;
    }
    let Some(order_id) = relay_order.id else {
        return;
    };

    let row = match Order::get_by_id(pool, &order_id.to_string()).await {
        Ok(row) => row,
        Err(e) => {
            log::warn!("Failed to get order by id: {}", e);
            return;
        }
    };

    let current = row.status.as_deref().and_then(|s| Status::from_str(s).ok());
    let kind = relay_order.kind.or_else(|| {
        row.kind
            .as_deref()
            .and_then(|k| mostro_core::order::Kind::from_str(k).ok())
    });
    if !should_apply_status_transition(current, candidate_status, kind) {
        return;
    }
    if let Err(e) = Order::update_status(pool, &order_id.to_string(), candidate_status).await {
        log::warn!(
            "Relay reconcile: failed to update order {} to {}: {}",
            order_id,
            candidate_status,
            e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

    async fn test_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool")
    }

    #[tokio::test]
    async fn reconcile_updates_pending_row_when_relay_expired() {
        let pool = test_pool().await;
        sqlx::query(
            r#"
            CREATE TABLE orders (
                id TEXT PRIMARY KEY,
                kind TEXT,
                status TEXT,
                amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL,
                min_amount INTEGER,
                max_amount INTEGER,
                fiat_amount INTEGER NOT NULL,
                payment_method TEXT NOT NULL,
                premium INTEGER NOT NULL,
                trade_keys TEXT,
                counterparty_pubkey TEXT,
                order_chat_shared_key_hex TEXT,
                is_mine INTEGER NOT NULL,
                buyer_invoice TEXT,
                request_id INTEGER,
                trade_index INTEGER,
                created_at INTEGER,
                expires_at INTEGER,
                last_seen_dm_ts INTEGER
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let trade_keys = Keys::generate();
        let oid = Uuid::new_v4();
        let small_pending = SmallOrder {
            id: Some(oid),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::Pending),
            amount: 10_000,
            fiat_code: "EUR".to_string(),
            fiat_amount: 50,
            payment_method: "sepa".to_string(),
            premium: 0,
            ..Default::default()
        };

        Order::new(&pool, small_pending, &trade_keys, None, 1, true)
            .await
            .unwrap();

        let mut relay_latest: HashMap<Uuid, SmallOrder> = HashMap::new();
        relay_latest.insert(
            oid,
            SmallOrder {
                id: Some(oid),
                kind: Some(mostro_core::order::Kind::Sell),
                status: Some(Status::Expired),
                amount: 10_000,
                fiat_code: "EUR".to_string(),
                fiat_amount: 50,
                payment_method: "sepa".to_string(),
                premium: 0,
                ..Default::default()
            },
        );

        reconcile_terminal_order_statuses_from_relay(&pool, &relay_latest)
            .await
            .unwrap();

        let row = Order::get_by_id(&pool, &oid.to_string()).await.unwrap();
        assert_eq!(
            row.status.as_deref().and_then(|s| Status::from_str(s).ok()),
            Some(Status::Expired)
        );
    }

    #[tokio::test]
    async fn reconcile_skips_when_no_local_row() {
        let pool = test_pool().await;
        sqlx::query(
            r#"CREATE TABLE orders (
                id TEXT PRIMARY KEY,
                kind TEXT,
                status TEXT,
                amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL,
                min_amount INTEGER,
                max_amount INTEGER,
                fiat_amount INTEGER NOT NULL,
                payment_method TEXT NOT NULL,
                premium INTEGER NOT NULL,
                trade_keys TEXT,
                counterparty_pubkey TEXT,
                order_chat_shared_key_hex TEXT,
                is_mine INTEGER NOT NULL,
                buyer_invoice TEXT,
                request_id INTEGER,
                trade_index INTEGER,
                created_at INTEGER,
                expires_at INTEGER,
                last_seen_dm_ts INTEGER
            );"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let oid = Uuid::new_v4();
        let mut relay_latest: HashMap<Uuid, SmallOrder> = HashMap::new();
        relay_latest.insert(
            oid,
            SmallOrder {
                id: Some(oid),
                kind: Some(mostro_core::order::Kind::Buy),
                status: Some(Status::Canceled),
                amount: 1,
                fiat_code: "USD".to_string(),
                fiat_amount: 1,
                payment_method: "x".to_string(),
                premium: 0,
                ..Default::default()
            },
        );

        reconcile_terminal_order_statuses_from_relay(&pool, &relay_latest)
            .await
            .unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM orders WHERE id = ?")
            .bind(oid.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }
}
