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
            is_mine INTEGER NOT NULL,
            buyer_invoice TEXT,
            request_id INTEGER,
            created_at INTEGER,
            expires_at INTEGER
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
