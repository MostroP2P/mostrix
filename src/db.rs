// src/db.rs
use crate::models::User;
use anyhow::Result;
use bip39::Mnemonic;
use sqlx::SqlitePool;
use std::fs::File;
use std::path::Path;

pub async fn init_db() -> Result<SqlitePool> {
    let pool: SqlitePool;
    let name = env!("CARGO_PKG_NAME");
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Unable to get home directory"))?;
    let app_dir = home_dir.join(format!(".{}", name));
    let db_path = app_dir.join(format!("{}.db", name));
    let db_url = format!(
        "sqlite://{}",
        db_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid db path"))?
    );
    if !app_dir.exists() {
        std::fs::create_dir_all(&app_dir)?;
    }

    if !Path::exists(Path::new(&db_path)) {
        if let Err(res) = File::create(&db_path) {
            println!("Error in creating db file: {}", res);
            return Err(res.into());
        }

        pool = SqlitePool::connect(&db_url).await?;

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
                is_mine INTEGER NOT NULL,
                buyer_invoice TEXT,
                request_id INTEGER,
                trade_index INTEGER,
                created_at INTEGER,
                expires_at INTEGER,
                last_seen_dm_ts INTEGER
            );
            CREATE TABLE IF NOT EXISTS users (
                i0_pubkey char(64) PRIMARY KEY,
                mnemonic TEXT,
                last_trade_index INTEGER,
                created_at INTEGER
            );
            CREATE TABLE IF NOT EXISTS admin_disputes (
                id TEXT PRIMARY KEY,
                dispute_id TEXT NOT NULL,
                kind TEXT,
                status TEXT,
                hash TEXT,
                preimage TEXT,
                order_previous_status TEXT,
                initiator_pubkey TEXT NOT NULL,
                buyer_pubkey TEXT,
                seller_pubkey TEXT,
                initiator_full_privacy INTEGER NOT NULL,
                counterpart_full_privacy INTEGER NOT NULL,
                initiator_info TEXT,
                counterpart_info TEXT,
                premium INTEGER NOT NULL,
                payment_method TEXT NOT NULL,
                amount INTEGER NOT NULL,
                fiat_amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL,
                fee INTEGER NOT NULL,
                routing_fee INTEGER NOT NULL,
                buyer_invoice TEXT,
                invoice_held_at INTEGER,
                taken_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                buyer_chat_last_seen INTEGER,
                seller_chat_last_seen INTEGER,
                buyer_shared_key_hex TEXT,
                seller_shared_key_hex TEXT
            );
            "#,
        )
        .execute(&pool)
        .await?;

        // Check if a user exists, if not, create one
        let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await?;
        if user_count.0 == 0 {
            let mnemonic = Mnemonic::generate(12)?.to_string();
            User::new(mnemonic, &pool).await?;
        }
    } else {
        pool = SqlitePool::connect(&db_url).await?;

        // Run migrations for existing databases
        migrate_db(&pool).await?;
    }

    Ok(pool)
}

/// Run database migrations for existing databases
async fn migrate_db(pool: &SqlitePool) -> Result<()> {
    // Migration: Add initiator_info and counterpart_info columns if they don't exist
    // Check if columns exist by attempting to query them and checking for specific SQLite errors
    async fn check_column_exists(
        pool: &SqlitePool,
        table_name: &str,
        column_name: &str,
    ) -> Result<bool> {
        let result = sqlx::query(&format!(
            "SELECT {} FROM {} LIMIT 1",
            column_name, table_name
        ))
        .fetch_optional(pool)
        .await;

        match result {
            Ok(_) => Ok(true), // Column exists (query succeeded)
            Err(e) => {
                // Check if error is specifically "no such column"
                // If it's a different error (table doesn't exist, connection issue, etc.),
                // we'll propagate it rather than incorrectly assuming the column doesn't exist
                let error_msg = e.to_string().to_lowercase();
                if error_msg.contains("no such column") {
                    Ok(false)
                } else {
                    // Re-propagate non-column-related errors
                    Err(e.into())
                }
            }
        }
    }

    // Check if columns exist
    let has_initiator_info = check_column_exists(pool, "admin_disputes", "initiator_info").await?;
    let has_counterpart_info =
        check_column_exists(pool, "admin_disputes", "counterpart_info").await?;
    let has_fiat_code = check_column_exists(pool, "admin_disputes", "fiat_code").await?;
    let has_dispute_id = check_column_exists(pool, "admin_disputes", "dispute_id").await?;
    let has_buyer_chat_last_seen =
        check_column_exists(pool, "admin_disputes", "buyer_chat_last_seen").await?;
    let has_seller_chat_last_seen =
        check_column_exists(pool, "admin_disputes", "seller_chat_last_seen").await?;
    let has_buyer_shared_key_hex =
        check_column_exists(pool, "admin_disputes", "buyer_shared_key_hex").await?;
    let has_seller_shared_key_hex =
        check_column_exists(pool, "admin_disputes", "seller_shared_key_hex").await?;
    let has_request_id = check_column_exists(pool, "orders", "request_id").await?;
    let has_trade_index = check_column_exists(pool, "orders", "trade_index").await?;
    let has_last_seen_dm_ts = check_column_exists(pool, "orders", "last_seen_dm_ts").await?;
    let has_order_chat_shared_key_hex =
        check_column_exists(pool, "orders", "order_chat_shared_key_hex").await?;

    // Only run migration if at least one column is missing
    if !has_initiator_info
        || !has_counterpart_info
        || !has_fiat_code
        || !has_dispute_id
        || !has_buyer_chat_last_seen
        || !has_seller_chat_last_seen
        || !has_buyer_shared_key_hex
        || !has_seller_shared_key_hex
        || !has_request_id
        || !has_trade_index
        || !has_last_seen_dm_ts
        || !has_order_chat_shared_key_hex
    {
        log::info!("Running migration: Adding missing columns to admin_disputes table");

        // Wrap all ALTER TABLE statements in a transaction for atomicity
        let mut tx = pool.begin().await?;

        if !has_initiator_info {
            sqlx::query(
                r#"
                ALTER TABLE admin_disputes ADD COLUMN initiator_info TEXT;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_counterpart_info {
            sqlx::query(
                r#"
                ALTER TABLE admin_disputes ADD COLUMN counterpart_info TEXT;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_fiat_code {
            sqlx::query(
                r#"
                ALTER TABLE admin_disputes ADD COLUMN fiat_code TEXT DEFAULT 'USD';
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_dispute_id {
            // For existing records, use the order_id (id field) as the dispute_id
            // This ensures backwards compatibility
            sqlx::query(
                r#"
                ALTER TABLE admin_disputes ADD COLUMN dispute_id TEXT NOT NULL DEFAULT '';
                "#,
            )
            .execute(&mut *tx)
            .await?;
            // Update existing records to use order_id as dispute_id
            sqlx::query(
                r#"
                UPDATE admin_disputes SET dispute_id = id WHERE dispute_id = '';
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_buyer_chat_last_seen {
            sqlx::query(
                r#"
                ALTER TABLE admin_disputes ADD COLUMN buyer_chat_last_seen INTEGER;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_seller_chat_last_seen {
            sqlx::query(
                r#"
                ALTER TABLE admin_disputes ADD COLUMN seller_chat_last_seen INTEGER;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_buyer_shared_key_hex {
            sqlx::query(
                r#"
                ALTER TABLE admin_disputes ADD COLUMN buyer_shared_key_hex TEXT;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_seller_shared_key_hex {
            sqlx::query(
                r#"
                ALTER TABLE admin_disputes ADD COLUMN seller_shared_key_hex TEXT;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_request_id {
            sqlx::query(
                r#"
                ALTER TABLE orders ADD COLUMN request_id INTEGER;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_trade_index {
            sqlx::query(
                r#"
                ALTER TABLE orders ADD COLUMN trade_index INTEGER;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_last_seen_dm_ts {
            sqlx::query(
                r#"
                ALTER TABLE orders ADD COLUMN last_seen_dm_ts INTEGER;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        if !has_order_chat_shared_key_hex {
            sqlx::query(
                r#"
                ALTER TABLE orders ADD COLUMN order_chat_shared_key_hex TEXT;
                "#,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        log::info!("Migration completed successfully");
    }

    // Builds that added `suppress_next_new_order_dm` must drop it so `SELECT *` matches `Order`.
    // `ALTER TABLE ... DROP COLUMN` requires SQLite 3.35.0+; older runtimes need a table rebuild.
    if check_column_exists(pool, "orders", "suppress_next_new_order_dm").await? {
        let sqlite_ver = sqlite_runtime_version(pool).await?;
        if sqlite_version_at_least(&sqlite_ver, 3, 35, 0) {
            sqlx::query(r#"ALTER TABLE orders DROP COLUMN suppress_next_new_order_dm"#)
                .execute(pool)
                .await?;
            log::info!("Dropped obsolete column orders.suppress_next_new_order_dm (ALTER TABLE DROP COLUMN)");
        } else {
            log::info!(
                "SQLite {sqlite_ver}: rebuilding orders table to drop obsolete column suppress_next_new_order_dm"
            );
            orders_table_rebuild_without_suppress_column(pool).await?;
            log::info!("Dropped obsolete column orders.suppress_next_new_order_dm (table copy)");
        }
    }

    Ok(())
}

/// `SELECT sqlite_version()` — e.g. `"3.39.4"`.
async fn sqlite_runtime_version(pool: &SqlitePool) -> Result<String> {
    let (ver,): (String,) = sqlx::query_as("SELECT sqlite_version()")
        .fetch_one(pool)
        .await?;
    Ok(ver)
}

fn sqlite_version_at_least(version: &str, min_major: u32, min_minor: u32, min_patch: u32) -> bool {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch) >= (min_major, min_minor, min_patch)
}

/// Pre-3.35.0: recreate `orders` without `suppress_next_new_order_dm` (same columns as `init_db`).
async fn orders_table_rebuild_without_suppress_column(pool: &SqlitePool) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        CREATE TABLE orders_new (
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
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO orders_new (
            id, kind, status, amount, fiat_code, min_amount, max_amount, fiat_amount,
            payment_method, premium, trade_keys, counterparty_pubkey, order_chat_shared_key_hex, is_mine, buyer_invoice,
            request_id, trade_index, created_at, expires_at, last_seen_dm_ts
        )
        SELECT
            id, kind, status, amount, fiat_code, min_amount, max_amount, fiat_amount,
            payment_method, premium, trade_keys, counterparty_pubkey, order_chat_shared_key_hex, is_mine, buyer_invoice,
            request_id, trade_index, created_at, expires_at, last_seen_dm_ts
        FROM orders;
        "#,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(r#"DROP TABLE orders;"#)
        .execute(&mut *tx)
        .await?;
    sqlx::query(r#"ALTER TABLE orders_new RENAME TO orders;"#)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_version_at_least_matches_drop_column_requirement() {
        assert!(sqlite_version_at_least("3.35.0", 3, 35, 0));
        assert!(sqlite_version_at_least("3.40.1", 3, 35, 0));
        assert!(!sqlite_version_at_least("3.34.1", 3, 35, 0));
        assert!(!sqlite_version_at_least("3.34.0", 3, 35, 0));
    }

    #[tokio::test]
    async fn test_init_db() {
        let pool = init_db().await.expect("Failed to initialize database");
        let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .expect("Failed to query user count");
        assert_eq!(user_count.0, 1, "Expected one user to be created");
        pool.close().await;
    }
}
