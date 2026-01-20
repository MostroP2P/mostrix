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
            CREATE TABLE IF NOT EXISTS admin_disputes (
                id TEXT PRIMARY KEY,
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
                fee INTEGER NOT NULL,
                routing_fee INTEGER NOT NULL,
                buyer_invoice TEXT,
                invoice_held_at INTEGER,
                taken_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL
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
    let result = sqlx::query(
        r#"
        SELECT initiator_info FROM admin_disputes LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await;

    // If the query fails, the column doesn't exist, so add it
    if result.is_err() {
        log::info!("Running migration: Adding initiator_info and counterpart_info columns to admin_disputes table");
        sqlx::query(
            r#"
            ALTER TABLE admin_disputes ADD COLUMN initiator_info TEXT;
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            ALTER TABLE admin_disputes ADD COLUMN counterpart_info TEXT;
            "#,
        )
        .execute(pool)
        .await?;

        log::info!("Migration completed successfully");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
