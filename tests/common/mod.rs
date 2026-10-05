// Shared test utilities for Mostrix tests
use anyhow::Result;
use sqlx::sqlite::SqlitePool;

/// Create an in-memory SQLite database for testing
pub async fn create_test_db() -> Result<SqlitePool> {
    let pool = SqlitePool::connect("sqlite::memory:").await?;

    // Create tables matching the production schema
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS orders (
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
            dispute_id TEXT,
            solver_pubkey TEXT,
            dispute_chat_shared_key_hex TEXT,
            is_mine INTEGER NOT NULL,
            full_privacy INTEGER NOT NULL DEFAULT 0,
            pending_next_trade_index INTEGER,
            buyer_invoice TEXT,
            request_id INTEGER,
            trade_index INTEGER,
            created_at INTEGER,
            expires_at INTEGER,
            last_seen_dm_ts INTEGER,
            bond_invoice TEXT
        );
        CREATE TABLE IF NOT EXISTS pending_next_trades (
            child_trade_index INTEGER PRIMARY KEY,
            parent_order_id TEXT NOT NULL,
            full_privacy INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS users (
            i0_pubkey char(64) PRIMARY KEY,
            mnemonic TEXT,
            last_trade_index INTEGER,
            created_at INTEGER
        );
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

/// Generate a test mnemonic for testing
pub fn test_mnemonic() -> String {
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        .to_string()
}
