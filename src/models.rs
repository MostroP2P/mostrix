use anyhow::Result;
use chrono::Utc;
use mostro_core::prelude::NOSTR_REPLACEABLE_EVENT_KIND;
use nip06::FromMnemonic;
use nostr_sdk::prelude::*;
use sqlx::sqlite::SqlitePool;

#[derive(Debug, Default, Clone, sqlx::FromRow)]
pub struct User {
    pub i0_pubkey: String,
    pub mnemonic: String,
    pub last_trade_index: Option<i64>,
    pub created_at: i64,
}

impl User {
    pub async fn new(mnemonic: String, pool: &SqlitePool) -> Result<Self> {
        let mut user = User::default();
        let account: u32 = NOSTR_REPLACEABLE_EVENT_KIND as u32;
        let i0_keys =
            Keys::from_mnemonic_advanced(&mnemonic, None, Some(account), Some(0), Some(0))?;
        user.i0_pubkey = i0_keys.public_key().to_string();
        user.created_at = Utc::now().timestamp();
        user.mnemonic = mnemonic;
        sqlx::query(
            r#"
                  INSERT INTO users (i0_pubkey, mnemonic, created_at)
                  VALUES (?, ?, ?)
                "#,
        )
        .bind(&user.i0_pubkey)
        .bind(&user.mnemonic)
        .bind(user.created_at)
        .execute(pool)
        .await?;

        Ok(user)
    }

    pub async fn get(pool: &SqlitePool) -> Result<Self> {
        let user: User = sqlx::query_as(
            r#"SELECT i0_pubkey, mnemonic, last_trade_index, created_at FROM users LIMIT 1"#,
        )
        .fetch_one(pool)
        .await?;
        Ok(user)
    }

    pub async fn update_last_trade_index(pool: &SqlitePool, idx: i64) -> Result<()> {
        sqlx::query(
            r#"UPDATE users SET last_trade_index = ? WHERE i0_pubkey = (SELECT i0_pubkey FROM users LIMIT 1)"#,
        )
        .bind(idx)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub fn derive_trade_keys(&self, trade_index: i64) -> Result<Keys> {
        let account: u32 = NOSTR_REPLACEABLE_EVENT_KIND as u32;
        let keys = Keys::from_mnemonic_advanced(
            &self.mnemonic,
            None,
            Some(account),
            Some(trade_index as u32),
            Some(0),
        )?;
        Ok(keys)
    }
}

/// Struct representing a Mostro order.
#[derive(Debug, Default, Clone)]
pub struct Order {
    pub id: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub amount: i64,
    pub fiat_code: String,
    pub min_amount: Option<i64>,
    pub max_amount: Option<i64>,
    pub is_mine: bool,
    pub fiat_amount: i64,
    pub payment_method: String,
    pub premium: i64,
    pub buyer_trade_pubkey: Option<String>,
    pub seller_trade_pubkey: Option<String>,
    pub created_at: Option<i64>,
    pub expires_at: Option<i64>,
}
