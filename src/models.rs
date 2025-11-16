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

    // Applying changes to the database
    pub async fn save(&self, pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
              UPDATE users 
              SET mnemonic = ?, last_trade_index = ?
              WHERE i0_pubkey = ?
              "#,
        )
        .bind(&self.mnemonic)
        .bind(self.last_trade_index)
        .bind(&self.i0_pubkey)
        .execute(pool)
        .await?;

        Ok(())
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

    pub async fn get_identity_keys(pool: &SqlitePool) -> Result<Keys> {
        let user = User::get(pool).await?;
        let account = NOSTR_REPLACEABLE_EVENT_KIND as u32;
        let keys =
            Keys::from_mnemonic_advanced(&user.mnemonic, None, Some(account), Some(0), Some(0))?;

        Ok(keys)
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

impl Order {
    /// Create a new order from SmallOrder and save it to the database
    pub async fn new(
        pool: &SqlitePool,
        order: mostro_core::prelude::SmallOrder,
        _trade_keys: &nostr_sdk::prelude::Keys,
        _request_id: Option<i64>,
    ) -> Result<Self> {
        let id = match order.id {
            Some(id) => id.to_string(),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let order = Order {
            id: Some(id.clone()),
            kind: order.kind.as_ref().map(|k| k.to_string()),
            status: order.status.as_ref().map(|s| s.to_string()),
            amount: order.amount,
            fiat_code: order.fiat_code,
            min_amount: order.min_amount,
            max_amount: order.max_amount,
            fiat_amount: order.fiat_amount,
            payment_method: order.payment_method,
            premium: order.premium,
            is_mine: true,
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            created_at: Some(chrono::Utc::now().timestamp()),
            expires_at: order.expires_at,
        };

        // Try insert; if id already exists, perform an update instead
        let insert_result = order.insert_db(pool).await;

        if let Err(e) = insert_result {
            // If the error is due to unique constraint (id already present), update instead
            let is_unique_violation = match e.as_database_error() {
                Some(db_err) => {
                    let code = db_err.code().map(|c| c.to_string()).unwrap_or_default();
                    code == "1555" || code == "2067"
                }
                None => false,
            };

            if is_unique_violation {
                order.update_db(pool).await?;
            } else {
                return Err(e.into());
            }
        }

        Ok(order)
    }

    async fn insert_db(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO orders (id, kind, status, amount, min_amount, max_amount,
            fiat_code, fiat_amount, payment_method, premium, is_mine,
            buyer_trade_pubkey, seller_trade_pubkey, created_at, expires_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&self.id)
        .bind(&self.kind)
        .bind(&self.status)
        .bind(self.amount)
        .bind(self.min_amount)
        .bind(self.max_amount)
        .bind(&self.fiat_code)
        .bind(self.fiat_amount)
        .bind(&self.payment_method)
        .bind(self.premium)
        .bind(if self.is_mine { 1 } else { 0 })
        .bind(&self.buyer_trade_pubkey)
        .bind(&self.seller_trade_pubkey)
        .bind(self.created_at)
        .bind(self.expires_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    async fn update_db(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE orders 
            SET kind = ?, status = ?, amount = ?, min_amount = ?, max_amount = ?,
                fiat_code = ?, fiat_amount = ?, payment_method = ?, premium = ?,
                is_mine = ?, buyer_trade_pubkey = ?, seller_trade_pubkey = ?,
                created_at = ?, expires_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&self.kind)
        .bind(&self.status)
        .bind(self.amount)
        .bind(self.min_amount)
        .bind(self.max_amount)
        .bind(&self.fiat_code)
        .bind(self.fiat_amount)
        .bind(&self.payment_method)
        .bind(self.premium)
        .bind(if self.is_mine { 1 } else { 0 })
        .bind(&self.buyer_trade_pubkey)
        .bind(&self.seller_trade_pubkey)
        .bind(self.created_at)
        .bind(self.expires_at)
        .bind(&self.id)
        .execute(pool)
        .await?;
        Ok(())
    }
}
