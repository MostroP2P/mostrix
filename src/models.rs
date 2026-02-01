use anyhow::Result;
use chrono::Utc;
use mostro_core::prelude::*;
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
        let account: u32 = NOSTR_ORDER_EVENT_KIND as u32;
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
        let account = NOSTR_ORDER_EVENT_KIND as u32;
        let keys =
            Keys::from_mnemonic_advanced(&user.mnemonic, None, Some(account), Some(0), Some(0))?;

        Ok(keys)
    }

    pub fn derive_trade_keys(&self, trade_index: i64) -> Result<Keys> {
        let account: u32 = NOSTR_ORDER_EVENT_KIND as u32;
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

#[derive(Debug, Default, Clone, sqlx::FromRow)]
pub struct Order {
    pub id: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub amount: i64,
    pub fiat_code: String,
    pub min_amount: Option<i64>,
    pub max_amount: Option<i64>,
    pub fiat_amount: i64,
    pub payment_method: String,
    pub premium: i64,
    pub trade_keys: Option<String>,
    pub counterparty_pubkey: Option<String>,
    pub is_mine: Option<bool>,
    pub buyer_invoice: Option<String>,
    pub request_id: Option<i64>,
    pub created_at: Option<i64>,
    pub expires_at: Option<i64>,
}

impl Order {
    /// Create a new order from SmallOrder and save it to the database
    pub async fn new(
        pool: &SqlitePool,
        order: mostro_core::prelude::SmallOrder,
        trade_keys: &nostr_sdk::prelude::Keys,
        _request_id: Option<i64>,
    ) -> Result<Self> {
        let trade_keys_hex = trade_keys.secret_key().to_secret_hex();

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
            trade_keys: Some(trade_keys_hex),
            counterparty_pubkey: None,
            is_mine: Some(true),
            buyer_invoice: order.buyer_invoice,
            request_id: _request_id,
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
            trade_keys, counterparty_pubkey, buyer_invoice, created_at, expires_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(self.is_mine)
        .bind(&self.trade_keys)
        .bind(&self.counterparty_pubkey)
        .bind(&self.buyer_invoice)
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
                is_mine = ?, trade_keys = ?, counterparty_pubkey = ?, buyer_invoice = ?,
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
        .bind(self.is_mine)
        .bind(&self.trade_keys)
        .bind(&self.counterparty_pubkey)
        .bind(&self.buyer_invoice)
        .bind(self.created_at)
        .bind(self.expires_at)
        .bind(&self.id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Order> {
        let order = sqlx::query_as::<_, Order>(
            r#"
            SELECT * FROM orders WHERE id = ?
            LIMIT 1
            "#,
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        if order.id.is_none() {
            return Err(anyhow::anyhow!("Order not found"));
        }

        Ok(order)
    }
}

/// Admin dispute model for storing SolverDisputeInfo
#[derive(Debug, Default, Clone, sqlx::FromRow)]
pub struct AdminDispute {
    pub id: String,         // Order ID (from dispute_info.id)
    pub dispute_id: String, // Actual dispute ID (from AdminTakeDispute)
    pub kind: Option<String>,
    pub status: Option<String>,
    pub hash: Option<String>,
    pub preimage: Option<String>,
    pub order_previous_status: Option<String>,
    pub initiator_pubkey: String,
    pub buyer_pubkey: Option<String>,
    pub seller_pubkey: Option<String>,
    pub initiator_full_privacy: bool,
    pub counterpart_full_privacy: bool,
    #[sqlx(skip)]
    pub initiator_info_data: Option<mostro_core::prelude::UserInfo>,
    #[sqlx(skip)]
    pub counterpart_info_data: Option<mostro_core::prelude::UserInfo>,
    pub initiator_info: Option<String>,   // JSON serialized
    pub counterpart_info: Option<String>, // JSON serialized
    pub premium: i64,
    pub payment_method: String,
    pub amount: i64,
    pub fiat_amount: i64,
    pub fiat_code: String,
    pub fee: i64,
    pub routing_fee: i64,
    pub buyer_invoice: Option<String>,
    pub invoice_held_at: Option<i64>,
    pub taken_at: i64,
    pub created_at: i64,
    pub buyer_chat_last_seen: Option<i64>,
    pub seller_chat_last_seen: Option<i64>,
}

impl AdminDispute {
    /// Create a new admin dispute from SolverDisputeInfo and save it to the database
    pub async fn new(
        pool: &SqlitePool,
        dispute_info: SolverDisputeInfo,
        dispute_id: String,
    ) -> Result<Self> {
        // Validate required fields
        if dispute_info.buyer_pubkey.is_none() || dispute_info.seller_pubkey.is_none() {
            return Err(anyhow::anyhow!(
                "Invalid dispute data: buyer_pubkey and seller_pubkey are required fields. \
                 The database entry cannot be saved without these fields."
            ));
        }

        // Serialize UserInfo to JSON
        let initiator_info_json = dispute_info
            .initiator_info
            .as_ref()
            .and_then(|info| serde_json::to_string(info).ok());
        let counterpart_info_json = dispute_info
            .counterpart_info
            .as_ref()
            .and_then(|info| serde_json::to_string(info).ok());

        // Try to get fiat_code from the related order using dispute ID
        // In Mostro, the dispute ID typically matches the order ID
        let fiat_code = match Order::get_by_id(pool, &dispute_info.id.to_string()).await {
            Ok(order) => order.fiat_code,
            Err(_) => {
                // Order not found, use default
                log::debug!(
                    "Order not found for dispute {}, using default fiat_code",
                    dispute_info.id
                );
                "USD".to_string()
            }
        };

        let dispute = AdminDispute {
            id: dispute_info.id.to_string(), // Order ID
            dispute_id,                      // Actual dispute ID (from AdminTakeDispute)
            kind: Some(dispute_info.kind),
            status: Some(dispute_info.status),
            hash: dispute_info.hash,
            preimage: dispute_info.preimage,
            order_previous_status: Some(dispute_info.order_previous_status),
            initiator_pubkey: dispute_info.initiator_pubkey,
            buyer_pubkey: dispute_info.buyer_pubkey,
            seller_pubkey: dispute_info.seller_pubkey,
            initiator_full_privacy: dispute_info.initiator_full_privacy,
            counterpart_full_privacy: dispute_info.counterpart_full_privacy,
            initiator_info_data: dispute_info.initiator_info.clone(),
            counterpart_info_data: dispute_info.counterpart_info.clone(),
            initiator_info: initiator_info_json,
            counterpart_info: counterpart_info_json,
            premium: dispute_info.premium,
            payment_method: dispute_info.payment_method,
            amount: dispute_info.amount,
            fiat_amount: dispute_info.fiat_amount,
            fiat_code,
            fee: dispute_info.fee,
            routing_fee: dispute_info.routing_fee,
            buyer_invoice: dispute_info.buyer_invoice,
            invoice_held_at: Some(dispute_info.invoice_held_at),
            taken_at: dispute_info.taken_at,
            created_at: dispute_info.created_at,
            buyer_chat_last_seen: None,
            seller_chat_last_seen: None,
        };

        // Try insert; if id already exists, perform an update instead
        let insert_result = dispute.insert_db(pool).await;

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
                dispute.update_db(pool).await?;
            } else {
                return Err(e.into());
            }
        }

        Ok(dispute)
    }

    async fn insert_db(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO admin_disputes (
                id, dispute_id, kind, status, hash, preimage, order_previous_status,
                initiator_pubkey, buyer_pubkey, seller_pubkey,
                initiator_full_privacy, counterpart_full_privacy,
                initiator_info, counterpart_info,
                premium, payment_method, amount, fiat_amount, fiat_code, fee, routing_fee,
                buyer_invoice, invoice_held_at, taken_at, created_at,
                buyer_chat_last_seen, seller_chat_last_seen
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&self.id)
        .bind(&self.dispute_id)
        .bind(&self.kind)
        .bind(&self.status)
        .bind(&self.hash)
        .bind(&self.preimage)
        .bind(&self.order_previous_status)
        .bind(&self.initiator_pubkey)
        .bind(&self.buyer_pubkey)
        .bind(&self.seller_pubkey)
        .bind(self.initiator_full_privacy)
        .bind(self.counterpart_full_privacy)
        .bind(&self.initiator_info)
        .bind(&self.counterpart_info)
        .bind(self.premium)
        .bind(&self.payment_method)
        .bind(self.amount)
        .bind(self.fiat_amount)
        .bind(&self.fiat_code)
        .bind(self.fee)
        .bind(self.routing_fee)
        .bind(&self.buyer_invoice)
        .bind(self.invoice_held_at)
        .bind(self.taken_at)
        .bind(self.created_at)
        .bind(self.buyer_chat_last_seen)
        .bind(self.seller_chat_last_seen)
        .execute(pool)
        .await?;
        Ok(())
    }

    async fn update_db(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE admin_disputes 
            SET dispute_id = ?, kind = ?, status = ?, hash = ?, preimage = ?, order_previous_status = ?,
                initiator_pubkey = ?, buyer_pubkey = ?, seller_pubkey = ?,
                initiator_full_privacy = ?, counterpart_full_privacy = ?,
                initiator_info = ?, counterpart_info = ?,
                premium = ?, payment_method = ?, amount = ?, fiat_amount = ?, fiat_code = ?,
                fee = ?, routing_fee = ?, buyer_invoice = ?, invoice_held_at = ?,
                taken_at = ?, created_at = ?, buyer_chat_last_seen = ?, seller_chat_last_seen = ?
            WHERE id = ?
            "#,
        )
        .bind(&self.dispute_id)
        .bind(&self.kind)
        .bind(&self.status)
        .bind(&self.hash)
        .bind(&self.preimage)
        .bind(&self.order_previous_status)
        .bind(&self.initiator_pubkey)
        .bind(&self.buyer_pubkey)
        .bind(&self.seller_pubkey)
        .bind(self.initiator_full_privacy)
        .bind(self.counterpart_full_privacy)
        .bind(&self.initiator_info)
        .bind(&self.counterpart_info)
        .bind(self.premium)
        .bind(&self.payment_method)
        .bind(self.amount)
        .bind(self.fiat_amount)
        .bind(&self.fiat_code)
        .bind(self.fee)
        .bind(self.routing_fee)
        .bind(&self.buyer_invoice)
        .bind(self.invoice_held_at)
        .bind(self.taken_at)
        .bind(self.created_at)
        .bind(self.buyer_chat_last_seen)
        .bind(self.seller_chat_last_seen)
        .bind(&self.id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Get all admin disputes from the database
    pub async fn get_all(pool: &SqlitePool) -> Result<Vec<AdminDispute>> {
        let mut disputes = sqlx::query_as::<_, AdminDispute>(
            r#"SELECT * FROM admin_disputes WHERE status = ? ORDER BY taken_at DESC"#,
        )
        .bind(DisputeStatus::InProgress.to_string())
        .fetch_all(pool)
        .await?;

        // Deserialize UserInfo from JSON
        for dispute in &mut disputes {
            dispute.deserialize_user_info();
        }

        Ok(disputes)
    }

    /// Get a dispute by ID
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<AdminDispute> {
        let mut dispute = sqlx::query_as::<_, AdminDispute>(
            r#"SELECT * FROM admin_disputes WHERE id = ? LIMIT 1"#,
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        // Deserialize UserInfo from JSON
        dispute.deserialize_user_info();

        Ok(dispute)
    }

    /// Helper method to deserialize JSON UserInfo fields
    fn deserialize_user_info(&mut self) {
        if let Some(ref json_str) = self.initiator_info {
            self.initiator_info_data = serde_json::from_str(json_str).ok();
        }
        if let Some(ref json_str) = self.counterpart_info {
            self.counterpart_info_data = serde_json::from_str(json_str).ok();
        }
    }

    /// Check if there is an active dispute in InProgress state
    ///
    /// Returns `Ok(Some(dispute_id))` if an InProgress dispute exists,
    /// `Ok(None)` if no InProgress dispute exists, or an error if the query fails.
    pub async fn has_in_progress_dispute(pool: &SqlitePool) -> Result<Option<String>> {
        let result = sqlx::query_as::<_, (String,)>(
            r#"SELECT id FROM admin_disputes WHERE status = ? LIMIT 1"#,
        )
        .bind(DisputeStatus::InProgress.to_string())
        .fetch_optional(pool)
        .await?;
        Ok(result.map(|(id,)| id))
    }

    /// Update buyer chat last_seen timestamp (unix seconds) using dispute_id
    pub async fn update_buyer_chat_last_seen_by_dispute_id(
        pool: &SqlitePool,
        dispute_id: &str,
        ts: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE admin_disputes
            SET buyer_chat_last_seen = ?
            WHERE dispute_id = ?
            "#,
        )
        .bind(ts)
        .bind(dispute_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update seller chat last_seen timestamp (unix seconds) using dispute_id
    pub async fn update_seller_chat_last_seen_by_dispute_id(
        pool: &SqlitePool,
        dispute_id: &str,
        ts: i64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE admin_disputes
            SET seller_chat_last_seen = ?
            WHERE dispute_id = ?
            "#,
        )
        .bind(ts)
        .bind(dispute_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update the status of a dispute to Settled
    ///
    /// This is called when an admin settles a dispute in favor of the buyer.
    /// Updates by id (the order ID, which is the primary key).
    pub async fn set_status_settled(pool: &SqlitePool, order_id: &str) -> Result<()> {
        sqlx::query(r#"UPDATE admin_disputes SET status = ? WHERE id = ?"#)
            .bind(DisputeStatus::Settled.to_string())
            .bind(order_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Update the status of a dispute to SellerRefunded
    ///
    /// This is called when an admin cancels a dispute and refunds the seller.
    /// Updates by id (the order ID, which is the primary key).
    pub async fn set_status_seller_refunded(pool: &SqlitePool, order_id: &str) -> Result<()> {
        sqlx::query(r#"UPDATE admin_disputes SET status = ? WHERE id = ?"#)
            .bind(DisputeStatus::SellerRefunded.to_string())
            .bind(order_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Check if the dispute is finalized (Settled, SellerRefunded, or Released)
    ///
    /// A finalized dispute cannot have further actions taken on it.
    pub fn is_finalized(&self) -> bool {
        use std::str::FromStr;
        self.status
            .as_deref()
            .and_then(|s| DisputeStatus::from_str(s).ok())
            .map(|s| {
                matches!(
                    s,
                    DisputeStatus::Settled
                        | DisputeStatus::SellerRefunded
                        | DisputeStatus::Released
                )
            })
            .unwrap_or(false)
    }

    /// Check if AdminSettle action can be performed on this dispute
    ///
    /// Returns true if the dispute is not finalized and can be settled.
    pub fn can_settle(&self) -> bool {
        !self.is_finalized()
    }

    /// Check if AdminCancel action can be performed on this dispute
    ///
    /// Returns true if the dispute is not finalized and can be canceled.
    pub fn can_cancel(&self) -> bool {
        !self.is_finalized()
    }
}
