use mostro_core::NOSTR_REPLACEABLE_EVENT_KIND;
use sqlx::sqlite::SqlitePool;
use anyhow::Result;
use chrono::Utc;
use nostr_sdk::prelude::*;
use nip06::FromMnemonic;

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
        let account = NOSTR_REPLACEABLE_EVENT_KIND as u32;
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