//! Full local session wipe for seed import and factory reset.
//!
//! Clears SQLite session tables, on-disk chat transcripts, downloaded attachments,
//! and the user Lightning address in settings. Identity rotation (`nsec_privkey`,
//! new mnemonic) is left to the caller.

use anyhow::{Context, Result};
use sqlx::sqlite::SqlitePool;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{AdminDispute, Order};
use crate::settings::{load_settings_from_disk, save_settings, Settings};

/// Subdirectories under the Mostrix data dir removed on session wipe.
pub const SESSION_WIPE_DATA_SUBDIRS: &[&str] = &[
    "orders_chat",
    "user_disputes_chat",
    "disputes_chat",
    "downloads",
];

/// Session chat/download dirs moved aside during a wipe until the caller commits
/// or rolls back.
pub struct StagedSessionWipe {
    base: PathBuf,
    backup_dir: PathBuf,
}

impl StagedSessionWipe {
    /// Move session directories aside so they can be restored if later steps fail.
    pub fn begin(base: &Path) -> Result<Self> {
        let backup_dir = base.join(format!(".session-wipe-staging-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&backup_dir)
            .with_context(|| format!("Failed to create staging dir {}", backup_dir.display()))?;

        for subdir in SESSION_WIPE_DATA_SUBDIRS {
            let path = base.join(subdir);
            if path.exists() {
                fs::rename(&path, backup_dir.join(subdir)).with_context(|| {
                    format!("Failed to stage {} for session wipe", path.display())
                })?;
            }
        }

        Ok(Self {
            base: base.to_path_buf(),
            backup_dir,
        })
    }

    /// Replace session dirs with fresh empty directories and delete the staging copy.
    pub fn commit(self) -> Result<()> {
        for subdir in SESSION_WIPE_DATA_SUBDIRS {
            let path = self.base.join(subdir);
            if path.exists() {
                fs::remove_dir_all(&path).with_context(|| {
                    format!("Failed to remove staged session dir {}", path.display())
                })?;
            }
            fs::create_dir_all(&path)
                .with_context(|| format!("Failed to recreate session dir {}", path.display()))?;
        }
        if self.backup_dir.exists() {
            fs::remove_dir_all(&self.backup_dir).with_context(|| {
                format!(
                    "Failed to remove session wipe staging dir {}",
                    self.backup_dir.display()
                )
            })?;
        }
        Ok(())
    }

    /// Restore the pre-wipe session directories after a failed import/wipe step.
    pub fn rollback(self) -> Result<()> {
        for subdir in SESSION_WIPE_DATA_SUBDIRS {
            let path = self.base.join(subdir);
            if path.exists() {
                fs::remove_dir_all(&path).ok();
            }
            let backup = self.backup_dir.join(subdir);
            if backup.exists() {
                fs::rename(&backup, &path).with_context(|| {
                    format!(
                        "Failed to restore {} from session wipe staging",
                        path.display()
                    )
                })?;
            }
        }
        if self.backup_dir.exists() {
            fs::remove_dir_all(&self.backup_dir).ok();
        }
        Ok(())
    }
}

/// Resolve `~/.mostrix` (i.e. `~/.{CARGO_PKG_NAME}`).
pub fn mostrix_data_dir() -> Result<PathBuf> {
    let home_dir = dirs::home_dir().context("Could not find home directory")?;
    Ok(home_dir.join(format!(".{}", env!("CARGO_PKG_NAME"))))
}

/// Delete all session-related rows inside an open transaction.
pub async fn clear_session_tables_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<()> {
    AdminDispute::delete_all_in_tx(tx).await?;
    Order::delete_all_in_tx(tx).await?;
    sqlx::query(r#"DELETE FROM users"#)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Remove transcript and download directories under `base`, then recreate them empty.
pub fn clear_session_files_at(base: &Path) -> Result<()> {
    for subdir in SESSION_WIPE_DATA_SUBDIRS {
        let path = base.join(subdir);
        if path.exists() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
        fs::create_dir_all(&path)
            .with_context(|| format!("Failed to recreate {}", path.display()))?;
    }
    Ok(())
}

/// Clear [`Settings::ln_address`] in memory. No-op when already empty.
pub fn clear_ln_address(settings: &mut Settings) {
    settings.ln_address.clear();
}

/// Persist an empty `ln_address` while keeping relays, Mostro pubkey, and admin key.
pub fn clear_ln_address_in_settings() -> Result<()> {
    let mut settings = match load_settings_from_disk() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Skipping ln_address clear — settings not loaded: {e}");
            return Ok(());
        }
    };
    if settings.ln_address.is_empty() {
        return Ok(());
    }
    clear_ln_address(&mut settings);
    save_settings(&settings)?;
    Ok(())
}

/// Wipe all local session state: database rows, chat/download files, and `ln_address`.
///
/// Does **not** rotate identity keys or update `nsec_privkey` — callers insert a new
/// user (e.g. via [`crate::models::User::replace_all_atomic`]) after this returns.
///
/// Session directories are staged first and only replaced after database and settings
/// updates succeed, so a mid-wipe failure can roll back the on-disk session.
pub async fn clear_local_session_state(pool: &SqlitePool) -> Result<()> {
    let base = mostrix_data_dir()?;
    let staged = StagedSessionWipe::begin(&base)?;

    if let Err(e) = (async {
        let mut tx = pool.begin().await?;
        clear_session_tables_in_tx(&mut tx).await?;
        tx.commit().await?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    {
        staged.rollback()?;
        return Err(e);
    }

    if let Err(e) = clear_ln_address_in_settings() {
        staged.rollback()?;
        return Err(e);
    }

    staged.commit()?;

    log::info!("Cleared local session state (database, chat files, downloads, ln_address)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn create_wipe_test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        sqlx::query(
            r#"
            CREATE TABLE orders (
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
                buyer_invoice TEXT,
                request_id INTEGER,
                trade_index INTEGER,
                created_at INTEGER,
                expires_at INTEGER,
                last_seen_dm_ts INTEGER
            );
            CREATE TABLE users (
                i0_pubkey char(64) PRIMARY KEY,
                mnemonic TEXT,
                last_trade_index INTEGER,
                created_at INTEGER
            );
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
        .execute(&pool)
        .await
        .expect("schema");

        sqlx::query(
            r#"INSERT INTO users (i0_pubkey, mnemonic, created_at) VALUES ('aa', 'words', 1)"#,
        )
        .execute(&pool)
        .await
        .expect("user");
        sqlx::query(
            r#"INSERT INTO orders (id, amount, fiat_code, fiat_amount, payment_method, premium, is_mine)
               VALUES ('order-1', 1000, 'USD', 100, 'bank', 0, 1)"#,
        )
        .execute(&pool)
        .await
        .expect("order");
        sqlx::query(
            r#"INSERT INTO admin_disputes (
                id, dispute_id, initiator_pubkey, initiator_full_privacy, counterpart_full_privacy,
                premium, payment_method, amount, fiat_amount, fiat_code, fee, routing_fee,
                taken_at, created_at
            ) VALUES ('d1', 'dispute-uuid', 'initiator', 0, 0, 0, 'bank', 1000, 100, 'USD', 0, 0, 1, 1)"#,
        )
        .execute(&pool)
        .await
        .expect("dispute");

        pool
    }

    #[tokio::test]
    async fn clear_session_tables_in_tx_removes_all_rows() {
        let pool = create_wipe_test_pool().await;
        let mut tx = pool.begin().await.expect("tx");
        clear_session_tables_in_tx(&mut tx)
            .await
            .expect("wipe tables");
        tx.commit().await.expect("commit");

        let (users,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .expect("users count");
        let (orders,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM orders")
            .fetch_one(&pool)
            .await
            .expect("orders count");
        let (disputes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM admin_disputes")
            .fetch_one(&pool)
            .await
            .expect("disputes count");

        assert_eq!(users, 0);
        assert_eq!(orders, 0);
        assert_eq!(disputes, 0);
    }

    #[test]
    fn staged_session_wipe_rollback_restores_files() {
        let base = std::env::temp_dir().join(format!("mostrix-staged-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).expect("base dir");
        let chat_dir = base.join("orders_chat");
        fs::create_dir_all(&chat_dir).expect("chat dir");
        let marker = chat_dir.join("keep.txt");
        fs::write(&marker, "restore-me").expect("marker");

        let staged = StagedSessionWipe::begin(&base).expect("stage");
        assert!(!chat_dir.exists());
        staged.rollback().expect("rollback");

        assert!(marker.is_file());
        assert_eq!(fs::read_to_string(&marker).expect("read"), "restore-me");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn clear_session_files_at_removes_and_recreates_dirs() {
        let base = std::env::temp_dir().join(format!("mostrix-wipe-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).expect("base dir");

        for subdir in SESSION_WIPE_DATA_SUBDIRS {
            let dir = base.join(subdir);
            fs::create_dir_all(&dir).expect("subdir");
            let marker = dir.join("marker.txt");
            fs::write(&marker, "stay-gone").expect("marker");
        }

        clear_session_files_at(&base).expect("clear files");

        for subdir in SESSION_WIPE_DATA_SUBDIRS {
            let dir = base.join(subdir);
            assert!(dir.is_dir(), "{subdir} should exist");
            assert!(
                fs::read_dir(&dir).expect("read dir").next().is_none(),
                "{subdir} should be empty"
            );
        }

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn clear_ln_address_clears_only_ln_address_field() {
        let mut settings = Settings {
            mostro_pubkey: "npub".to_string(),
            nsec_privkey: "nsec".to_string(),
            admin_privkey: "admin".to_string(),
            relays: vec!["wss://relay.example".to_string()],
            log_level: "info".to_string(),
            currencies_filter: vec!["USD".to_string()],
            user_mode: "user".to_string(),
            ln_address: "user@domain.com".to_string(),
            blossom_servers: vec![],
        };

        clear_ln_address(&mut settings);

        assert!(settings.ln_address.is_empty());
        assert_eq!(settings.mostro_pubkey, "npub");
        assert_eq!(settings.relays, vec!["wss://relay.example"]);
    }
}
