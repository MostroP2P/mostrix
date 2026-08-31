use anyhow::Result;
use chrono::Utc;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::sqlite::SqlitePool;
use sqlx::{QueryBuilder, Sqlite};

#[derive(Debug, Default, Clone, sqlx::FromRow)]
pub struct User {
    pub i0_pubkey: String,
    pub mnemonic: String,
    pub last_trade_index: Option<i64>,
    pub created_at: i64,
}

impl User {
    pub fn from_mnemonic(mnemonic: String) -> Result<Self> {
        let mut user = User::default();
        let account: u32 = NOSTR_ORDER_EVENT_KIND as u32;
        let i0_keys =
            Keys::from_mnemonic_advanced(&mnemonic, None, Some(account), Some(0), Some(0))?;
        user.i0_pubkey = i0_keys.public_key().to_string();
        user.created_at = Utc::now().timestamp();
        user.mnemonic = mnemonic;
        Ok(user)
    }

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

    pub async fn delete_all(pool: &SqlitePool) -> Result<()> {
        sqlx::query(r#"DELETE FROM users"#).execute(pool).await?;
        Ok(())
    }

    /// Atomically replace the single local user row with a new mnemonic-derived identity.
    ///
    /// This wraps DELETE + INSERT in one SQL transaction so failures do not leave
    /// the users table empty.
    pub async fn replace_all_atomic(mnemonic: String, pool: &SqlitePool) -> Result<Self> {
        let user = User::from_mnemonic(mnemonic)?;

        let mut tx = pool.begin().await?;
        Self::replace_all_in_tx(&user, &mut tx).await?;
        Order::delete_all_in_tx(&mut tx).await?;
        tx.commit().await?;

        Ok(user)
    }

    pub async fn replace_all_in_tx(
        user: &User,
        tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<()> {
        sqlx::query(r#"DELETE FROM users"#)
            .execute(&mut **tx)
            .await?;
        sqlx::query(
            r#"
                  INSERT INTO users (i0_pubkey, mnemonic, created_at)
                  VALUES (?, ?, ?)
                "#,
        )
        .bind(&user.i0_pubkey)
        .bind(&user.mnemonic)
        .bind(user.created_at)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    /// Raise `last_trade_index` to at least `idx` without decreasing the counter.
    ///
    /// Prefer [`Self::reserve_next_trade_index`] before starting a trade; trade flows
    /// already commit the index at reservation time and must not write a stale value
    /// after a delayed order save.
    pub async fn update_last_trade_index(pool: &SqlitePool, idx: i64) -> Result<()> {
        sqlx::query(
            r#"UPDATE users SET last_trade_index = MAX(COALESCE(last_trade_index, 0), ?) WHERE i0_pubkey = (SELECT i0_pubkey FROM users LIMIT 1)"#,
        )
        .bind(idx)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Atomically increment `last_trade_index` and derive keys for the new index.
    ///
    /// Runs read-modify-write inside a transaction so a failed commit (e.g. `SQLITE_BUSY`)
    /// surfaces as an error instead of silently reusing an index. `none_base` is the value
    /// treated as the previous index when `last_trade_index` is NULL (order flows use `1`,
    /// range-order `NextTrade` uses `0`).
    ///
    /// # Errors
    ///
    /// Returns an error if the transaction cannot be started, the user row cannot be read,
    /// the index update fails, or key derivation fails.
    pub async fn reserve_next_trade_index(
        pool: &SqlitePool,
        none_base: i64,
    ) -> Result<(i64, Keys)> {
        let mut tx = pool.begin().await?;
        let user: User = sqlx::query_as(
            r#"SELECT i0_pubkey, mnemonic, last_trade_index, created_at FROM users LIMIT 1"#,
        )
        .fetch_one(&mut *tx)
        .await?;
        let next_idx = user.last_trade_index.unwrap_or(none_base) + 1;
        sqlx::query(r#"UPDATE users SET last_trade_index = ? WHERE i0_pubkey = ?"#)
            .bind(next_idx)
            .bind(&user.i0_pubkey)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let trade_keys = user.derive_trade_keys(next_idx)?;
        Ok((next_idx, trade_keys))
    }

    pub async fn get_identity_keys(pool: &SqlitePool) -> Result<Keys> {
        let user = User::get(pool).await?;
        let account = NOSTR_ORDER_EVENT_KIND as u32;
        let keys =
            Keys::from_mnemonic_advanced(&user.mnemonic, None, Some(account), Some(0), Some(0))?;

        Ok(keys)
    }

    /// Derive the trade key at `m/44'/1237'/38383'/0/{trade_index}` (NIP-06).
    pub fn derive_trade_keys(&self, trade_index: i64) -> Result<Keys> {
        if trade_index < 0 || trade_index > u32::MAX as i64 {
            anyhow::bail!(
                "Invalid trade_index {} for key derivation; expected 0..={}",
                trade_index,
                u32::MAX
            );
        }
        let account: u32 = NOSTR_ORDER_EVENT_KIND as u32;
        let keys = Keys::from_mnemonic_advanced(
            &self.mnemonic,
            None,
            Some(account),
            Some(0),
            Some(trade_index as u32),
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
    /// ECDH shared secret for P2P order chat (hex), derived once when both trade pubkeys are known.
    pub order_chat_shared_key_hex: Option<String>,
    /// Dispute UUID assigned by Mostro for this order.
    pub dispute_id: Option<String>,
    /// Trade pubkey of the solver that took the dispute.
    pub solver_pubkey: Option<String>,
    /// ECDH shared secret for the user-to-solver dispute chat.
    pub dispute_chat_shared_key_hex: Option<String>,
    /// Maker (`true`) vs taker (`false`). Matches `orders.is_mine` INTEGER NOT NULL (0/1).
    pub is_mine: bool,
    pub buyer_invoice: Option<String>,
    pub request_id: Option<i64>,
    pub trade_index: Option<i64>,
    pub created_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub last_seen_dm_ts: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StartupActiveOrderRecord {
    pub id: String,
    pub status: Option<String>,
    pub trade_index: Option<i64>,
    pub trade_keys: Option<String>,
    pub last_seen_dm_ts: Option<i64>,
}

/// Kebab-case order status strings excluded from startup DM hydration ([`Order::get_startup_active_orders`]).
///
/// **`success` is intentionally omitted** so post-success trade DMs still hydrate. Must match
/// [`mostro_core::order::Status`] `Display` for each variant listed; see
/// `terminal_dm_statuses_match_mostro_core_display` in tests for drift coverage.
pub const TERMINAL_DM_STATUSES: &[&str] = &[
    "canceled",
    "canceled-by-admin",
    "settled-by-admin",
    "completed-by-admin",
    "expired",
    "cooperatively-canceled",
];

/// Kebab-case terminal statuses for order-history retention/cleanup operations.
pub const TERMINAL_ORDER_HISTORY_STATUSES: &[&str] = &[
    "success",
    "canceled",
    "canceled-by-admin",
    "settled-by-admin",
    "completed-by-admin",
    "expired",
    "cooperatively-canceled",
];

/// Kebab-case statuses eligible for bulk user cleanup in My Trades.
pub const ORDER_HISTORY_BULK_DELETE_STATUSES: &[&str] = &["success", "canceled"];

impl Order {
    /// Delete every row in `orders`. Used when rotating the user mnemonic so persisted trade keys
    /// cannot reference the previous seed.
    pub async fn delete_all_in_tx(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(r#"DELETE FROM orders"#)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Create a new order from SmallOrder and save it to the database.
    ///
    /// `is_maker`: `true` if the local user **created** the order (maker); `false` if they **took**
    /// an existing order from the book (taker). Stored as `is_mine`.
    pub async fn new(
        pool: &SqlitePool,
        order: mostro_core::prelude::SmallOrder,
        trade_keys: &nostr_sdk::prelude::Keys,
        _request_id: Option<i64>,
        trade_index: i64,
        is_maker: bool,
    ) -> Result<Self> {
        if trade_index <= 0 {
            anyhow::bail!(
                "Invalid trade_index {} while persisting order; expected positive index",
                trade_index
            );
        }
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
            order_chat_shared_key_hex: None,
            dispute_id: None,
            solver_pubkey: None,
            dispute_chat_shared_key_hex: None,
            is_mine: is_maker,
            buyer_invoice: order.buyer_invoice,
            request_id: _request_id,
            trade_index: Some(trade_index),
            created_at: Some(chrono::Utc::now().timestamp()),
            expires_at: order.expires_at,
            last_seen_dm_ts: None,
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
            trade_keys, counterparty_pubkey, order_chat_shared_key_hex,
            dispute_id, solver_pubkey, dispute_chat_shared_key_hex,
            buyer_invoice, request_id, trade_index, created_at, expires_at, last_seen_dm_ts)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(&self.order_chat_shared_key_hex)
        .bind(&self.dispute_id)
        .bind(&self.solver_pubkey)
        .bind(&self.dispute_chat_shared_key_hex)
        .bind(&self.buyer_invoice)
        .bind(self.request_id)
        .bind(self.trade_index)
        .bind(self.created_at)
        .bind(self.expires_at)
        .bind(self.last_seen_dm_ts)
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
                is_mine = ?, trade_keys = ?, counterparty_pubkey = ?, order_chat_shared_key_hex = ?,
                dispute_id = ?, solver_pubkey = ?, dispute_chat_shared_key_hex = ?, buyer_invoice = ?,
                request_id = ?, trade_index = ?, created_at = ?, expires_at = ?, last_seen_dm_ts = ?
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
        .bind(&self.order_chat_shared_key_hex)
        .bind(&self.dispute_id)
        .bind(&self.solver_pubkey)
        .bind(&self.dispute_chat_shared_key_hex)
        .bind(&self.buyer_invoice)
        .bind(self.request_id)
        .bind(self.trade_index)
        .bind(self.created_at)
        .bind(self.expires_at)
        .bind(self.last_seen_dm_ts)
        .bind(&self.id)
        .execute(pool)
        .await?;
        Ok(())
    }

    fn build_order_from_small_order(
        id: String,
        small_order: &mostro_core::prelude::SmallOrder,
        trade_keys: &nostr_sdk::prelude::Keys,
        existing: Option<&Order>,
        message_request_id: Option<i64>,
    ) -> Order {
        let (counterparty_pubkey, order_chat_shared_key_hex) =
            match crate::util::chat_utils::order_chat_counterparty_and_shared_hex(
                trade_keys,
                small_order,
            ) {
                Some((cp, sk)) => (Some(cp), Some(sk)),
                None => (
                    existing.and_then(|e| e.counterparty_pubkey.clone()),
                    existing.and_then(|e| e.order_chat_shared_key_hex.clone()),
                ),
            };

        Order {
            id: Some(id),
            kind: small_order.kind.as_ref().map(|k| k.to_string()),
            status: small_order.status.as_ref().map(|s| s.to_string()),
            amount: small_order.amount,
            fiat_code: small_order.fiat_code.clone(),
            min_amount: small_order.min_amount,
            max_amount: small_order.max_amount,
            fiat_amount: small_order.fiat_amount,
            payment_method: small_order.payment_method.clone(),
            premium: small_order.premium,
            // Security invariant: trade_keys ownership is bound to the persisted order row.
            // DM hydration must never rotate/swap it from the decrypting trade context.
            trade_keys: existing.and_then(|e| e.trade_keys.clone()),
            counterparty_pubkey,
            order_chat_shared_key_hex,
            dispute_id: existing.and_then(|e| e.dispute_id.clone()),
            solver_pubkey: existing.and_then(|e| e.solver_pubkey.clone()),
            dispute_chat_shared_key_hex: existing
                .and_then(|e| e.dispute_chat_shared_key_hex.clone()),
            is_mine: existing.map(|e| e.is_mine).unwrap_or(true),
            buyer_invoice: small_order.buyer_invoice.clone(),
            request_id: message_request_id.or_else(|| existing.and_then(|e| e.request_id)),
            trade_index: existing.and_then(|e| e.trade_index),
            created_at: existing
                .and_then(|e| e.created_at)
                .or_else(|| Some(Utc::now().timestamp())),
            expires_at: small_order.expires_at,
            last_seen_dm_ts: existing.and_then(|e| e.last_seen_dm_ts),
        }
    }

    /// Insert or update an order from a trade DM (e.g. `AddInvoice` with `waiting-buyer-invoice`).
    ///
    /// Does not update `users.last_trade_index` (unlike [`crate::util::db_utils::save_order`]).
    /// Preserves `created_at` and selected fields when a row already exists.
    ///
    /// `is_mine` is taken from an existing row when present; otherwise defaults to `true` (maker)
    /// for a brand-new DM-only insert (rare race before [`save_order`]).
    pub async fn upsert_from_small_order_dm(
        pool: &SqlitePool,
        order_id_fallback: uuid::Uuid,
        mut small_order: mostro_core::prelude::SmallOrder,
        trade_keys: &nostr_sdk::prelude::Keys,
        message_request_id: Option<i64>,
    ) -> Result<Self> {
        if let Some(payload_id) = small_order.id {
            if payload_id != order_id_fallback {
                anyhow::bail!(
                    "Rejected DM order upsert: payload id {} does not match routed order id {}",
                    payload_id,
                    order_id_fallback
                );
            }
        }
        let resolved_id = order_id_fallback;
        small_order.id = Some(resolved_id);
        let id_str = resolved_id.to_string();

        let existing = Self::get_by_id(pool, &id_str).await.ok();
        let order_row = Self::build_order_from_small_order(
            id_str.clone(),
            &small_order,
            trade_keys,
            existing.as_ref(),
            message_request_id,
        );

        if existing.is_some() {
            order_row.update_db(pool).await?;
            return Ok(order_row);
        }
        if order_row.trade_index.is_none() {
            anyhow::bail!(
                "Cannot insert order {} from DM without persisted trade_index; this indicates an inconsistent local state.",
                id_str
            );
        }

        match order_row.insert_db(pool).await {
            Ok(()) => Ok(order_row),
            Err(e) => {
                let is_unique_violation = match e.as_database_error() {
                    Some(db_err) => {
                        let code = db_err.code().map(|c| c.to_string()).unwrap_or_default();
                        code == "1555" || code == "2067"
                    }
                    None => false,
                };
                if is_unique_violation {
                    let ex = Self::get_by_id(pool, &id_str).await?;
                    let updated = Self::build_order_from_small_order(
                        id_str,
                        &small_order,
                        trade_keys,
                        Some(&ex),
                        message_request_id,
                    );
                    updated.update_db(pool).await?;
                    Ok(updated)
                } else {
                    Err(e.into())
                }
            }
        }
    }

    /// Load an order when present; missing rows are `Ok(None)` (not an error).
    pub async fn try_get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<Order>> {
        let order = sqlx::query_as::<_, Order>(
            r#"
            SELECT * FROM orders WHERE id = ?
            LIMIT 1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(order.filter(|o| o.id.is_some()))
    }

    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Order> {
        match Self::try_get_by_id(pool, id).await? {
            Some(order) => Ok(order),
            None => Err(anyhow::anyhow!("Order not found")),
        }
    }

    /// Update only the status field of an existing order by id.
    /// The caller is responsible for providing a valid Mostro `Status`.
    pub async fn update_status(
        pool: &SqlitePool,
        order_id: &str,
        new_status: mostro_core::prelude::Status,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE orders
            SET status = ?
            WHERE id = ?
            "#,
        )
        .bind(new_status.to_string())
        .bind(order_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn update_last_seen_dm_ts(pool: &SqlitePool, order_id: &str, ts: i64) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE orders
            SET last_seen_dm_ts = CASE
                WHEN last_seen_dm_ts IS NULL OR ? > last_seen_dm_ts THEN ?
                ELSE last_seen_dm_ts
            END
            WHERE id = ?
            "#,
        )
        .bind(ts)
        .bind(ts)
        .bind(order_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Persist the dispute id announced by Mostro for a user order.
    pub async fn update_dispute_id(
        pool: &SqlitePool,
        order_id: &str,
        dispute_id: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE orders SET dispute_id = ? WHERE id = ?")
            .bind(dispute_id)
            .bind(order_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Persist the assigned solver and the derived user-to-solver chat secret.
    pub async fn update_solver_chat(
        pool: &SqlitePool,
        order_id: &str,
        solver_pubkey: &str,
        shared_key_hex: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE orders SET solver_pubkey = ?, dispute_chat_shared_key_hex = ? WHERE id = ?",
        )
        .bind(solver_pubkey)
        .bind(shared_key_hex)
        .bind(order_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_startup_active_orders(
        pool: &SqlitePool,
    ) -> Result<Vec<StartupActiveOrderRecord>> {
        let mut qb: QueryBuilder<'_, Sqlite> = QueryBuilder::new(
            "SELECT id, status, trade_index, trade_keys, last_seen_dm_ts FROM orders \
             WHERE trade_keys IS NOT NULL AND trade_keys != '' \
             AND (status IS NULL OR lower(status) NOT IN (",
        );
        {
            let mut separated = qb.separated(", ");
            for s in TERMINAL_DM_STATUSES {
                separated.push_bind(*s);
            }
        }
        qb.push("))");
        let rows = qb
            .build_query_as::<StartupActiveOrderRecord>()
            .fetch_all(pool)
            .await?;
        Ok(rows)
    }

    /// Order UUIDs (as strings) that may still need a relay terminal status, for targeted reconcile.
    ///
    /// Rows must have persisted trade keys. Status is compared case-insensitively against
    /// [`TERMINAL_ORDER_HISTORY_STATUSES`] (includes `success`, unlike [`TERMINAL_DM_STATUSES`]).
    pub async fn list_ids_for_targeted_relay_reconcile(pool: &SqlitePool) -> Result<Vec<String>> {
        let mut qb: QueryBuilder<'_, Sqlite> = QueryBuilder::new(
            "SELECT id FROM orders WHERE trade_keys IS NOT NULL AND trade_keys != '' \
             AND id IS NOT NULL AND id != '' \
             AND (status IS NULL OR lower(status) NOT IN (",
        );
        {
            let mut separated = qb.separated(", ");
            for s in TERMINAL_ORDER_HISTORY_STATUSES {
                separated.push_bind(*s);
            }
        }
        qb.push(")) ORDER BY id");
        let rows = qb.build_query_scalar::<String>().fetch_all(pool).await?;
        Ok(rows)
    }

    /// Fetches all locally-known user trade rows (maker + taker history) with persisted trade keys.
    pub async fn get_user_history_orders(pool: &SqlitePool) -> Result<Vec<Order>> {
        let rows = sqlx::query_as::<_, Order>(
            r#"
            SELECT * FROM orders
            WHERE trade_keys IS NOT NULL
              AND trade_keys != ''
            ORDER BY COALESCE(last_seen_dm_ts, created_at, 0) DESC, id DESC
            "#,
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Deletes one order row when it is in a terminal status.
    /// Returns number of deleted rows (0 when order is non-terminal or missing).
    pub async fn delete_terminal_order_by_id(pool: &SqlitePool, order_id: &str) -> Result<u64> {
        let mut qb: QueryBuilder<'_, Sqlite> = QueryBuilder::new("DELETE FROM orders WHERE id = ");
        qb.push_bind(order_id);
        qb.push(" AND lower(COALESCE(status, '')) IN (");
        {
            let mut separated = qb.separated(", ");
            for s in TERMINAL_ORDER_HISTORY_STATUSES {
                separated.push_bind(*s);
            }
        }
        qb.push(")");
        let result = qb.build().execute(pool).await?;
        Ok(result.rows_affected())
    }

    /// Deletes only success/canceled rows for user bulk cleanup.
    /// Returns number of deleted rows.
    pub async fn delete_bulk_history_cleanup_orders(pool: &SqlitePool) -> Result<u64> {
        let mut qb: QueryBuilder<'_, Sqlite> =
            QueryBuilder::new("DELETE FROM orders WHERE lower(COALESCE(status, '')) IN (");
        {
            let mut separated = qb.separated(", ");
            for s in ORDER_HISTORY_BULK_DELETE_STATUSES {
                separated.push_bind(*s);
            }
        }
        qb.push(")");
        let result = qb.build().execute(pool).await?;
        Ok(result.rows_affected())
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
    pub buyer_shared_key_hex: Option<String>,
    pub seller_shared_key_hex: Option<String>,
}

impl AdminDispute {
    /// Create a new admin dispute from SolverDisputeInfo and save it to the database.
    ///
    /// When `admin_keys` is provided, per-dispute ECDH shared secrets are eagerly
    /// derived (`derive_shared_key_hex`) and persisted as hex. At chat send/receive
    /// time Mostrix derives `K_conv` / `K_sign` from that ECDH IKM (kind-14 wrap),
    /// matching the mostro-chat model. No schema change is required for the keys.
    pub async fn new(
        pool: &SqlitePool,
        dispute_info: SolverDisputeInfo,
        dispute_id: String,
        fiat_code_from_relay: Option<String>,
        admin_keys: Option<&Keys>,
    ) -> Result<Self> {
        // Validate required fields
        if dispute_info.buyer_pubkey.is_none() || dispute_info.seller_pubkey.is_none() {
            return Err(anyhow::anyhow!(
                "Invalid dispute data: buyer_pubkey and seller_pubkey are required fields. \
                 The database entry cannot be saved without these fields."
            ));
        }

        // Eagerly derive per-party shared keys when admin_keys is available
        let buyer_shared_key_hex = crate::util::chat_utils::derive_shared_key_hex(
            admin_keys,
            dispute_info.buyer_pubkey.as_deref(),
        );
        let seller_shared_key_hex = crate::util::chat_utils::derive_shared_key_hex(
            admin_keys,
            dispute_info.seller_pubkey.as_deref(),
        );

        if buyer_shared_key_hex.is_some() {
            log::info!("Derived buyer shared key for dispute {}", dispute_id,);
        }
        if seller_shared_key_hex.is_some() {
            log::info!("Derived seller shared key for dispute {}", dispute_id,);
        }
        // Sanity check: different counterparties must yield different shared keys for chat
        if let (Some(buyer_pk), Some(seller_pk), Some(ref b_hex), Some(ref s_hex)) = (
            dispute_info.buyer_pubkey.as_deref(),
            dispute_info.seller_pubkey.as_deref(),
            &buyer_shared_key_hex,
            &seller_shared_key_hex,
        ) {
            if buyer_pk != seller_pk && b_hex == s_hex {
                log::error!(
                    "Shared keys for dispute {} are identical for different buyer/seller pubkeys; chat may be broken",
                    dispute_id
                );
            }
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

        // Resolve fiat_code from relay (admin never has the user's order in local DB); default USD if missing
        let fiat_code = fiat_code_from_relay
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "USD".to_string());

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
            buyer_shared_key_hex,
            seller_shared_key_hex,
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
                let existing = Self::get_by_id(pool, &dispute.id).await?;
                if existing.is_finalized() {
                    return Err(anyhow::anyhow!(
                        "Refusing to overwrite finalized admin dispute for order {} (status: {})",
                        dispute.id,
                        existing.status.as_deref().unwrap_or("unknown")
                    ));
                }
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
                buyer_chat_last_seen, seller_chat_last_seen,
                buyer_shared_key_hex, seller_shared_key_hex
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .bind(&self.buyer_shared_key_hex)
        .bind(&self.seller_shared_key_hex)
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
                taken_at = ?, created_at = ?, buyer_chat_last_seen = ?, seller_chat_last_seen = ?,
                buyer_shared_key_hex = ?, seller_shared_key_hex = ?
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
        .bind(&self.buyer_shared_key_hex)
        .bind(&self.seller_shared_key_hex)
        .bind(&self.id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Get all admin disputes from the database
    pub async fn get_all(pool: &SqlitePool) -> Result<Vec<AdminDispute>> {
        let mut disputes = sqlx::query_as::<_, AdminDispute>(
            r#"SELECT * FROM admin_disputes ORDER BY taken_at DESC"#,
        )
        .fetch_all(pool)
        .await?;

        // Deserialize UserInfo from JSON
        for dispute in &mut disputes {
            dispute.deserialize_user_info();
        }

        Ok(disputes)
    }

    /// Get a dispute by order ID when present.
    pub async fn try_get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<AdminDispute>> {
        let mut dispute = sqlx::query_as::<_, AdminDispute>(
            r#"SELECT * FROM admin_disputes WHERE id = ? LIMIT 1"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        if let Some(ref mut dispute) = dispute {
            dispute.deserialize_user_info();
        }

        Ok(dispute)
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

    /// Get a taken dispute by its Mostro dispute UUID (`admin_disputes.dispute_id`).
    pub async fn get_by_dispute_id(
        pool: &SqlitePool,
        dispute_id: &str,
    ) -> Result<Option<AdminDispute>> {
        let mut dispute = sqlx::query_as::<_, AdminDispute>(
            r#"SELECT * FROM admin_disputes WHERE dispute_id = ? LIMIT 1"#,
        )
        .bind(dispute_id)
        .fetch_optional(pool)
        .await?;

        if let Some(ref mut dispute) = dispute {
            dispute.deserialize_user_info();
        }

        Ok(dispute)
    }

    /// Dispute UUIDs still locally `in-progress` (targeted kind-38386 reconcile).
    pub async fn list_in_progress_dispute_ids(pool: &SqlitePool) -> Result<Vec<String>> {
        let rows = sqlx::query_as::<_, (String,)>(
            r#"SELECT dispute_id FROM admin_disputes
               WHERE status = ?
                 AND dispute_id IS NOT NULL
                 AND dispute_id != ''
               ORDER BY dispute_id"#,
        )
        .bind(DisputeStatus::InProgress.to_string())
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Delete every row in `admin_disputes`. Used when wiping local session state
    /// before seed import or a full factory reset.
    pub async fn delete_all_in_tx(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(r#"DELETE FROM admin_disputes"#)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Delete one local `admin_disputes` row by Mostro dispute UUID.
    ///
    /// Local-only: does not cancel/settle on Mostro. Returns rows affected (0 if missing).
    pub async fn delete_by_dispute_id(pool: &SqlitePool, dispute_id: &str) -> Result<u64> {
        let result = sqlx::query(r#"DELETE FROM admin_disputes WHERE dispute_id = ?"#)
            .bind(dispute_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
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

    /// Update chat last_seen timestamp (unix seconds) for buyer or seller using dispute_id.
    /// Returns the number of rows affected (0 if dispute_id not found).
    ///
    /// # Arguments
    /// * `is_buyer` - true to update buyer_chat_last_seen, false for seller_chat_last_seen
    pub async fn update_chat_last_seen_by_dispute_id(
        pool: &SqlitePool,
        dispute_id: &str,
        ts: i64,
        is_buyer: bool,
    ) -> Result<u64> {
        let sql = if is_buyer {
            "UPDATE admin_disputes SET buyer_chat_last_seen = ? WHERE dispute_id = ?"
        } else {
            "UPDATE admin_disputes SET seller_chat_last_seen = ? WHERE dispute_id = ?"
        };
        let result = sqlx::query(sql)
            .bind(ts)
            .bind(dispute_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
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

    /// Advance a taken dispute to `status` by Mostro dispute UUID.
    ///
    /// No-op (returns `false`) when the row is missing or already finalized, so a
    /// later `seller-refunded` event cannot overwrite `settled`.
    pub async fn set_status_by_dispute_id(
        pool: &SqlitePool,
        dispute_id: &str,
        status: DisputeStatus,
    ) -> Result<bool> {
        let result = sqlx::query(
            r#"UPDATE admin_disputes SET status = ?
               WHERE dispute_id = ?
                 AND (status IS NULL OR status NOT IN (?, ?, ?))"#,
        )
        .bind(status.to_string())
        .bind(dispute_id)
        .bind(DisputeStatus::Settled.to_string())
        .bind(DisputeStatus::SellerRefunded.to_string())
        .bind(DisputeStatus::Released.to_string())
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Whether a kebab-case dispute status is terminal (Settled, SellerRefunded, Released).
    pub fn is_terminal_status(status: &str) -> bool {
        use std::str::FromStr;
        DisputeStatus::from_str(status)
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

    /// Check if the dispute is finalized (Settled, SellerRefunded, or Released)
    ///
    /// A finalized dispute cannot have further actions taken on it.
    pub fn is_finalized(&self) -> bool {
        self.status.as_deref().is_some_and(Self::is_terminal_status)
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

#[cfg(test)]
mod derive_trade_keys_tests {
    use super::User;

    /// Same mnemonic as mostro-webtool trade-key API tests.
    const SAMPLE_MNEMONIC: &str =
        "leader monkey parrot ring guide accident before fence cannon height naive bean";

    /// Independent cross-client vectors for `m/44'/1237'/38383'/0/{index}`.
    const TRADE_KEY_VECTORS: &[(i64, &str, &str)] = &[
        (
            1,
            "1c71f0a29c9d14198781f36897c6d6c08e2c4f905ba69fc5f8e67d375d705a8a",
            "977b9da8056e6a83a991cb31ac507b0a618fbbee355e36e107e0e348686ab833",
        ),
        (
            2,
            "5011d0e7a57ae27627ab76962537072c6809ef3b9e24c2e3db4e672c61624eef",
            "03c78114b80a2db782bb86be47c8062388d78a0585dfa79b8594e105740758c3",
        ),
        (
            5,
            "088bb3266bde36a0d7080b8ae655e86f5c12ac13ec59fa12754f277cdc2c970b",
            "542fe37421c128dd421289d45f498cd99a06701706f33a1a74926e6d2d32182d",
        ),
    ];

    /// Pre-fix swapped-path pubkey for index 2 (`m/44'/1237'/38383'/2/0`).
    const WRONG_PATH_INDEX_TWO_PUBKEY: &str =
        "7da91a7dec95859f279866bc0bb3a86ff3d67a43da3f43cd722d1dbc948e0d02";

    #[test]
    fn derive_trade_keys_matches_mostro_derivation_path() {
        let user = User::from_mnemonic(SAMPLE_MNEMONIC.to_string()).expect("user from mnemonic");

        for &(trade_index, expected_pubkey_hex, expected_secret_hex) in TRADE_KEY_VECTORS {
            let derived = user
                .derive_trade_keys(trade_index)
                .expect("derive_trade_keys");
            assert_eq!(
                derived.public_key().to_hex(),
                expected_pubkey_hex,
                "trade index {trade_index} pubkey"
            );
            assert_eq!(
                derived.secret_key().to_secret_hex(),
                expected_secret_hex,
                "trade index {trade_index} secret"
            );
        }
    }

    #[test]
    fn derive_trade_keys_index_two_matches_mostro_webtool_vector() {
        let user = User::from_mnemonic(SAMPLE_MNEMONIC.to_string()).expect("user from mnemonic");
        let keys = user.derive_trade_keys(2).expect("trade index 2");

        assert_eq!(
            keys.public_key().to_hex(),
            "5011d0e7a57ae27627ab76962537072c6809ef3b9e24c2e3db4e672c61624eef"
        );
        assert_ne!(keys.public_key().to_hex(), WRONG_PATH_INDEX_TWO_PUBKEY);
    }

    #[test]
    fn derive_trade_keys_rejects_out_of_range_indices() {
        let user = User::from_mnemonic(SAMPLE_MNEMONIC.to_string()).expect("user from mnemonic");

        for invalid in [-1_i64, i64::from(u32::MAX) + 1] {
            let err = user
                .derive_trade_keys(invalid)
                .expect_err("out-of-range trade_index must fail");
            assert!(
                err.to_string().contains("Invalid trade_index"),
                "unexpected error: {err}"
            );
        }
    }
}

#[cfg(test)]
mod terminal_dm_status_tests {
    use super::TERMINAL_DM_STATUSES;
    use mostro_core::prelude::Status;

    #[test]
    fn terminal_dm_statuses_match_mostro_core_display() {
        let variants = [
            Status::Canceled,
            Status::CanceledByAdmin,
            Status::SettledByAdmin,
            Status::CompletedByAdmin,
            Status::Expired,
            Status::CooperativelyCanceled,
        ];
        let expected: Vec<String> = variants
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let actual: Vec<String> = TERMINAL_DM_STATUSES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(
            expected, actual,
            "TERMINAL_DM_STATUSES must match mostro_core::order::Status::to_string()"
        );
    }
}

#[cfg(test)]
mod upsert_from_small_order_dm_tests {
    use super::Order;
    use mostro_core::prelude::{Kind, SmallOrder, Status};
    use nostr_sdk::prelude::Keys;
    use uuid::Uuid;

    async fn create_test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        sqlx::query(
            r#"
            CREATE TABLE orders (
                id TEXT PRIMARY KEY, kind TEXT, status TEXT, amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL, min_amount INTEGER, max_amount INTEGER,
                fiat_amount INTEGER NOT NULL, payment_method TEXT NOT NULL,
                premium INTEGER NOT NULL, trade_keys TEXT, counterparty_pubkey TEXT,
                order_chat_shared_key_hex TEXT, dispute_id TEXT, solver_pubkey TEXT,
                dispute_chat_shared_key_hex TEXT, is_mine INTEGER NOT NULL,
                buyer_invoice TEXT, request_id INTEGER, trade_index INTEGER,
                created_at INTEGER, expires_at INTEGER, last_seen_dm_ts INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("orders table");
        pool
    }

    fn sample_small_order(id: Uuid, amount: i64) -> SmallOrder {
        SmallOrder {
            id: Some(id),
            kind: Some(Kind::Buy),
            status: Some(Status::Active),
            amount,
            fiat_code: "USD".to_string(),
            min_amount: None,
            max_amount: None,
            fiat_amount: 100,
            payment_method: "bank".to_string(),
            premium: 0,
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            buyer_invoice: None,
            created_at: None,
            expires_at: None,
        }
    }

    #[tokio::test]
    async fn rejects_payload_id_mismatch_against_routed_order_id() {
        let pool = create_test_pool().await;
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let keys_b = Keys::generate();
        sqlx::query(
            "INSERT INTO orders (id, kind, status, amount, fiat_code, fiat_amount, payment_method, premium, trade_keys, is_mine, trade_index) VALUES (?, 'buy', 'active', 1000, 'USD', 10, 'bank', 0, ?, 1, 7)",
        )
        .bind(id_b.to_string())
        .bind(keys_b.secret_key().to_secret_hex())
        .execute(&pool)
        .await
        .expect("seed order b");

        let err = Order::upsert_from_small_order_dm(
            &pool,
            id_a,
            sample_small_order(id_b, 1),
            &Keys::generate(),
            Some(1),
        )
        .await
        .expect_err("mismatched id must fail");
        assert!(err.to_string().contains("payload id"));

        let untouched = Order::get_by_id(&pool, &id_b.to_string())
            .await
            .expect("order b still present");
        assert_eq!(untouched.amount, 1000);
        assert_eq!(
            untouched.trade_keys.as_deref(),
            Some(keys_b.secret_key().to_secret_hex().as_str())
        );
    }

    #[tokio::test]
    async fn preserves_existing_trade_keys_on_dm_upsert() {
        let pool = create_test_pool().await;
        let id = Uuid::new_v4();
        let stored_keys = Keys::generate();
        let decrypting_keys = Keys::generate();
        sqlx::query(
            "INSERT INTO orders (id, kind, status, amount, fiat_code, fiat_amount, payment_method, premium, trade_keys, is_mine, trade_index) VALUES (?, 'buy', 'active', 1000, 'USD', 10, 'bank', 0, ?, 1, 3)",
        )
        .bind(id.to_string())
        .bind(stored_keys.secret_key().to_secret_hex())
        .execute(&pool)
        .await
        .expect("seed order");

        Order::upsert_from_small_order_dm(
            &pool,
            id,
            sample_small_order(id, 777),
            &decrypting_keys,
            Some(2),
        )
        .await
        .expect("upsert succeeds");

        let updated = Order::get_by_id(&pool, &id.to_string())
            .await
            .expect("updated order");
        assert_eq!(updated.amount, 777);
        assert_eq!(
            updated.trade_keys.as_deref(),
            Some(stored_keys.secret_key().to_secret_hex().as_str())
        );
    }
}

#[cfg(test)]
mod admin_dispute_new_tests {
    use super::AdminDispute;
    use mostro_core::prelude::{DisputeStatus, SolverDisputeInfo};
    use uuid::Uuid;

    async fn create_admin_disputes_table(pool: &sqlx::SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE admin_disputes (
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
        .execute(pool)
        .await
        .expect("admin_disputes table");
    }

    fn solver_dispute_info(
        order_id: Uuid,
        status: &str,
        payment_method: &str,
    ) -> SolverDisputeInfo {
        SolverDisputeInfo {
            id: order_id,
            kind: "sell".to_string(),
            status: status.to_string(),
            hash: Some("hash-abc".to_string()),
            preimage: None,
            order_previous_status: "active".to_string(),
            initiator_pubkey: "02".repeat(32),
            buyer_pubkey: Some("03".repeat(32)),
            seller_pubkey: Some("04".repeat(32)),
            initiator_full_privacy: false,
            counterpart_full_privacy: false,
            initiator_info: None,
            counterpart_info: None,
            premium: 1,
            payment_method: payment_method.to_string(),
            amount: 50_000,
            fiat_amount: 75,
            fee: 250,
            routing_fee: 3,
            buyer_invoice: None,
            invoice_held_at: 1_700_000_000,
            taken_at: 1_700_000_100,
            created_at: 1_700_000_000,
        }
    }

    #[tokio::test]
    async fn refuses_upsert_over_finalized_dispute() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        create_admin_disputes_table(&pool).await;

        let order_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO admin_disputes (
                id, dispute_id, kind, status, initiator_pubkey, initiator_full_privacy,
                counterpart_full_privacy, premium, payment_method, amount, fiat_amount,
                fiat_code, fee, routing_fee, buyer_invoice, invoice_held_at, taken_at, created_at
            ) VALUES (?, 'dispute-final-1', 'sell', 'settled', ?, 0, 0, 1,
                'ORIGINAL zelle victim@example.com', 50000, 75, 'USD', 250, 3,
                'lnbc-settled-buyer-invoice', 1700000000, 1700000100, 1700000000)"#,
        )
        .bind(order_id.to_string())
        .bind("02".repeat(32))
        .execute(&pool)
        .await
        .expect("insert settled row");

        let err = AdminDispute::new(
            &pool,
            solver_dispute_info(
                order_id,
                &DisputeStatus::InProgress.to_string(),
                "REPLACED pm attacker@evil.example",
            ),
            "dispute-reattack-2".to_string(),
            Some("USD".to_string()),
            None,
        )
        .await
        .expect_err("must refuse to overwrite finalized row");
        assert!(
            err.to_string().contains("finalized"),
            "unexpected error: {err}"
        );

        let unchanged = AdminDispute::get_by_id(&pool, &order_id.to_string())
            .await
            .expect("row still present");
        assert!(unchanged.is_finalized());
        assert_eq!(unchanged.dispute_id, "dispute-final-1");
        assert_eq!(
            unchanged.payment_method,
            "ORIGINAL zelle victim@example.com"
        );
        assert_eq!(
            unchanged.buyer_invoice.as_deref(),
            Some("lnbc-settled-buyer-invoice")
        );
    }

    #[tokio::test]
    async fn allows_upsert_over_in_progress_dispute() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        create_admin_disputes_table(&pool).await;

        let order_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO admin_disputes (
                id, dispute_id, kind, status, initiator_pubkey, initiator_full_privacy,
                counterpart_full_privacy, premium, payment_method, amount, fiat_amount,
                fiat_code, fee, routing_fee, taken_at, created_at
            ) VALUES (?, 'dispute-old', 'sell', 'in-progress', ?, 0, 0, 1,
                'old pm', 50000, 75, 'USD', 250, 3, 1700000100, 1700000000)"#,
        )
        .bind(order_id.to_string())
        .bind("02".repeat(32))
        .execute(&pool)
        .await
        .expect("insert in-progress row");

        AdminDispute::new(
            &pool,
            solver_dispute_info(
                order_id,
                &DisputeStatus::InProgress.to_string(),
                "updated pm",
            ),
            "dispute-new".to_string(),
            Some("EUR".to_string()),
            None,
        )
        .await
        .expect("in-progress upsert succeeds");

        let updated = AdminDispute::get_by_id(&pool, &order_id.to_string())
            .await
            .expect("row updated");
        assert_eq!(updated.status.as_deref(), Some("in-progress"));
        assert_eq!(updated.dispute_id, "dispute-new");
        assert_eq!(updated.payment_method, "updated pm");
        assert_eq!(updated.fiat_code, "EUR");
    }

    #[tokio::test]
    async fn delete_by_dispute_id_removes_local_row() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        create_admin_disputes_table(&pool).await;

        let order_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO admin_disputes (
                id, dispute_id, kind, status, initiator_pubkey, initiator_full_privacy,
                counterpart_full_privacy, premium, payment_method, amount, fiat_amount,
                fiat_code, fee, routing_fee, buyer_invoice, invoice_held_at, taken_at, created_at
            ) VALUES (?, 'dispute-to-delete', 'sell', 'in-progress', ?, 0, 0, 1,
                'pm', 50000, 75, 'USD', 250, 3, NULL, 1700000000, 1700000100, 1700000000)"#,
        )
        .bind(order_id.to_string())
        .bind("02".repeat(32))
        .execute(&pool)
        .await
        .expect("insert row");

        let affected = AdminDispute::delete_by_dispute_id(&pool, "dispute-to-delete")
            .await
            .expect("delete");
        assert_eq!(affected, 1);
        assert!(AdminDispute::get_by_dispute_id(&pool, "dispute-to-delete")
            .await
            .expect("query")
            .is_none());
        assert_eq!(
            AdminDispute::delete_by_dispute_id(&pool, "dispute-to-delete")
                .await
                .expect("idempotent"),
            0
        );
    }
}
