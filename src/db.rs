// src/db.rs
use crate::models::User;
use anyhow::Result;
use bip39::Mnemonic;
use sqlx::{ SqlitePool};
use std::fs::File;
use std::path::Path;

pub async fn init_db() -> Result<SqlitePool> {
    let pool: SqlitePool;
    let name = env!("CARGO_PKG_NAME");
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Unable to get home directory"))?;
    let app_dir = home_dir.join(format!(".{}", name));
    let db_path = app_dir.join(format!("{}.db", name));
    let db_url = format!("sqlite://{}", db_path.to_str().ok_or_else(|| anyhow::anyhow!("Invalid db path"))?);
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
                is_mine INTEGER NOT NULL,
                buyer_trade_pubkey TEXT,
                seller_trade_pubkey TEXT,
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
    }

    Ok(pool)
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