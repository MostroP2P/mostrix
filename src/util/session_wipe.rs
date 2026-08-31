//! Full local session wipe for seed import and factory reset.
//!
//! Clears SQLite session tables, on-disk chat transcripts, downloaded attachments,
//! and the user Lightning address in settings. Identity rotation (`nsec_privkey`,
//! new mnemonic) is left to the caller.

use anyhow::{Context, Result};
use sqlx::sqlite::SqlitePool;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{AdminDispute, Order, User};
use crate::settings::{
    load_settings_from_disk, replace_settings_file_atomically, settings_file_path, Settings,
};

/// Subdirectories under the Mostrix data dir removed on session wipe.
pub const SESSION_WIPE_DATA_SUBDIRS: &[&str] = &[
    "orders_chat",
    "user_disputes_chat",
    "disputes_chat",
    "downloads",
];

#[cfg(test)]
type InjectionHook = Option<fn() -> Result<()>>;

#[cfg(test)]
fn inject_settings_replace_fail() -> Result<()> {
    Err(anyhow::anyhow!("injected settings replace failure"))
}

#[cfg(test)]
fn inject_db_commit_fail() -> Result<()> {
    Err(anyhow::anyhow!("injected db commit failure"))
}

#[cfg(not(test))]
fn replace_settings_atomically_checked(path: &Path, toml_string: &str) -> Result<()> {
    replace_settings_file_atomically(path, toml_string)
}

#[cfg(test)]
fn replace_settings_atomically_checked_inner(
    path: &Path,
    toml_string: &str,
    inject_fail: InjectionHook,
) -> Result<()> {
    if let Some(hook) = inject_fail {
        hook()?;
    }
    replace_settings_file_atomically(path, toml_string)
}

#[cfg(test)]
fn replace_settings_atomically_checked(path: &Path, toml_string: &str) -> Result<()> {
    replace_settings_atomically_checked_inner(path, toml_string, None)
}

#[cfg(not(test))]
fn maybe_fail_db_commit_injection() -> Result<()> {
    Ok(())
}

#[cfg(test)]
fn maybe_fail_db_commit_inner(inject_fail: InjectionHook) -> Result<()> {
    if let Some(hook) = inject_fail {
        hook()?;
    }
    Ok(())
}

#[cfg(test)]
fn maybe_fail_db_commit_injection() -> Result<()> {
    maybe_fail_db_commit_inner(None)
}

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

/// On-disk copy of `settings.toml` taken before a destructive session mutation.
struct SettingsSnapshot {
    path: PathBuf,
    backup_path: PathBuf,
}

impl SettingsSnapshot {
    fn capture(settings_path: &Path, staging_dir: &Path) -> Result<Option<Self>> {
        if !settings_path.exists() {
            return Ok(None);
        }
        fs::create_dir_all(staging_dir).with_context(|| {
            format!(
                "Failed to create settings staging dir {}",
                staging_dir.display()
            )
        })?;
        let backup_path = staging_dir.join("settings.toml.bak");
        fs::copy(settings_path, &backup_path).with_context(|| {
            format!(
                "Failed to back up settings file {}",
                settings_path.display()
            )
        })?;
        Ok(Some(Self {
            path: settings_path.to_path_buf(),
            backup_path,
        }))
    }

    fn restore(self) -> Result<()> {
        fs::copy(&self.backup_path, &self.path)
            .with_context(|| format!("Failed to restore settings file {}", self.path.display()))?;
        let _ = fs::remove_file(&self.backup_path);
        Ok(())
    }

    fn discard(self) -> Result<()> {
        if self.backup_path.exists() {
            fs::remove_file(&self.backup_path).with_context(|| {
                format!(
                    "Failed to remove settings backup {}",
                    self.backup_path.display()
                )
            })?;
        }
        Ok(())
    }
}

/// Stages session files and settings so DB/settings/files can be rolled back together.
struct SessionMutationScope {
    files: StagedSessionWipe,
    settings: Option<SettingsSnapshot>,
    staging_dir: PathBuf,
}

impl SessionMutationScope {
    fn begin(base: &Path) -> Result<Self> {
        let staging_dir = base.join(format!(".session-mutation-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&staging_dir).with_context(|| {
            format!(
                "Failed to create session mutation staging dir {}",
                staging_dir.display()
            )
        })?;
        let files = StagedSessionWipe::begin(base)?;
        match (|| -> Result<Option<SettingsSnapshot>> {
            let settings_path = settings_file_path()?;
            SettingsSnapshot::capture(&settings_path, &staging_dir)
        })() {
            Ok(settings) => Ok(Self {
                files,
                settings,
                staging_dir,
            }),
            Err(e) => {
                files.rollback()?;
                Err(e)
            }
        }
    }

    #[cfg(test)]
    fn begin_with_settings_path(base: &Path, settings_path: &Path) -> Result<Self> {
        let staging_dir = base.join(format!(".session-mutation-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&staging_dir)?;
        let files = StagedSessionWipe::begin(base)?;
        match SettingsSnapshot::capture(settings_path, &staging_dir) {
            Ok(settings) => Ok(Self {
                files,
                settings,
                staging_dir,
            }),
            Err(e) => {
                files.rollback()?;
                Err(e)
            }
        }
    }

    fn rollback(mut self) -> Result<()> {
        let settings_result = match self.settings.take() {
            Some(settings) => settings.restore(),
            None => Ok(()),
        };
        let files_result = self.files.rollback();
        if self.staging_dir.exists() {
            fs::remove_dir_all(&self.staging_dir).ok();
        }
        settings_result?;
        files_result?;
        Ok(())
    }

    fn commit(mut self) -> Result<()> {
        if let Some(settings) = self.settings.take() {
            settings.discard()?;
        }
        self.files.commit()?;
        if self.staging_dir.exists() {
            fs::remove_dir_all(&self.staging_dir).ok();
        }
        Ok(())
    }

    /// Finalize staged dirs/backups after DB/settings succeeded; log cleanup errors only.
    fn commit_best_effort(self, context: &str) {
        if let Err(e) = self.commit() {
            log::warn!(
                "{context}: staged session cleanup failed after successful state update: {e}"
            );
        }
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
    let settings_path = settings_file_path()?;
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
    let toml_string = toml::to_string_pretty(&settings)
        .map_err(|e| anyhow::anyhow!("Failed to serialize settings: {}", e))?;
    replace_settings_atomically_checked(&settings_path, &toml_string)
}

/// Wipe all local session state: database rows, chat/download files, and `ln_address`.
///
/// Does **not** rotate identity keys or update `nsec_privkey` — callers insert a new
/// user (e.g. via [`crate::models::User::replace_all_atomic`]) after this returns.
///
/// Settings are updated before the database commit; on any failure the settings backup,
/// database transaction, and staged session directories are rolled back together.
pub async fn clear_local_session_state(pool: &SqlitePool) -> Result<()> {
    let base = mostrix_data_dir()?;
    let scope = SessionMutationScope::begin(&base)?;

    let outcome = async {
        clear_ln_address_in_settings()?;
        maybe_fail_db_commit_injection()?;

        let mut tx = pool.begin().await?;
        clear_session_tables_in_tx(&mut tx).await?;
        tx.commit().await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    match outcome {
        Ok(()) => {
            scope.commit_best_effort("clear_local_session_state");
            log::info!("Cleared local session state (database, chat files, downloads, ln_address)");
            Ok(())
        }
        Err(e) => {
            scope.rollback()?;
            Err(e)
        }
    }
}

/// Import a BIP-39 mnemonic: wipe local session tables, insert the new user, and
/// atomically update `nsec_privkey` (clearing `ln_address`) in settings.toml.
///
/// Settings are replaced before the database transaction commits so a late settings
/// failure cannot strand a wiped database behind the old `nsec_privkey`.
pub async fn import_seed_and_wipe_session(
    pool: &SqlitePool,
    mnemonic: String,
    derived_nsec: String,
) -> Result<()> {
    let base = mostrix_data_dir()?;
    let scope = SessionMutationScope::begin(&base)?;

    let outcome = async {
        let new_user = User::from_mnemonic(mnemonic)?;
        let mut tx = pool.begin().await?;
        clear_session_tables_in_tx(&mut tx).await?;
        User::replace_all_in_tx(&new_user, &mut tx).await?;

        let settings_path = settings_file_path()?;
        let mut settings = load_settings_from_disk()?;
        settings.nsec_privkey = derived_nsec;
        clear_ln_address(&mut settings);
        let toml_string = toml::to_string_pretty(&settings)
            .map_err(|e| anyhow::anyhow!("Failed to serialize settings: {}", e))?;
        replace_settings_atomically_checked(&settings_path, &toml_string)?;

        maybe_fail_db_commit_injection()?;
        tx.commit().await?;

        log::info!(
            "Imported seed for identity {}; local session wiped",
            new_user.i0_pubkey
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;

    match outcome {
        Ok(()) => {
            scope.commit_best_effort("import_seed_and_wipe_session");
            Ok(())
        }
        Err(e) => {
            scope.rollback()?;
            Err(e)
        }
    }
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

    async fn count_rows(pool: &SqlitePool, table: &str) -> i64 {
        let query = format!("SELECT COUNT(*) FROM {table}");
        let (count,): (i64,) = sqlx::query_as(&query)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|e| panic!("count {table}: {e}"));
        count
    }

    const SAMPLE_SETTINGS: &str = r#"
mostro_pubkey = "npub-old"
nsec_privkey = "nsec-old"
admin_privkey = ""
relays = ["wss://relay.example.com"]
log_level = "info"
currencies_filter = ["USD"]
user_mode = "user"
ln_address = "user@domain.com"
"#;

    async fn import_seed_with_paths(
        pool: &SqlitePool,
        base: &Path,
        settings_path: &Path,
        derived_nsec: &str,
        settings_inject: InjectionHook,
        db_inject: InjectionHook,
    ) -> Result<()> {
        let scope = SessionMutationScope::begin_with_settings_path(base, settings_path)?;
        let outcome = async {
            let new_user = User::from_mnemonic(
                "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
                    .to_string(),
            )?;
            let mut tx = pool.begin().await?;
            clear_session_tables_in_tx(&mut tx).await?;
            User::replace_all_in_tx(&new_user, &mut tx).await?;

            let mut settings: Settings =
                toml::from_str(&fs::read_to_string(settings_path)?)?;
            settings.nsec_privkey = derived_nsec.to_string();
            clear_ln_address(&mut settings);
            let toml_string = toml::to_string_pretty(&settings)?;
            replace_settings_atomically_checked_inner(
                settings_path,
                &toml_string,
                settings_inject,
            )?;

            maybe_fail_db_commit_inner(db_inject)?;
            tx.commit().await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match outcome {
            Ok(()) => scope.commit(),
            Err(e) => {
                scope.rollback()?;
                Err(e)
            }
        }
    }

    async fn clear_session_with_paths(
        pool: &SqlitePool,
        base: &Path,
        settings_path: &Path,
        settings_inject: InjectionHook,
        db_inject: InjectionHook,
    ) -> Result<()> {
        let scope = SessionMutationScope::begin_with_settings_path(base, settings_path)?;
        let outcome = async {
            let mut settings: Settings =
                toml::from_str(fs::read_to_string(settings_path)?.as_str())?;
            if !settings.ln_address.is_empty() {
                clear_ln_address(&mut settings);
                let toml_string = toml::to_string_pretty(&settings)?;
                replace_settings_atomically_checked_inner(
                    settings_path,
                    &toml_string,
                    settings_inject,
                )?;
            }
            maybe_fail_db_commit_inner(db_inject)?;
            let mut tx = pool.begin().await?;
            clear_session_tables_in_tx(&mut tx).await?;
            tx.commit().await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match outcome {
            Ok(()) => scope.commit(),
            Err(e) => {
                scope.rollback()?;
                Err(e)
            }
        }
    }

    #[tokio::test]
    async fn clear_session_tables_in_tx_removes_all_rows() {
        let pool = create_wipe_test_pool().await;
        let mut tx = pool.begin().await.expect("tx");
        clear_session_tables_in_tx(&mut tx)
            .await
            .expect("wipe tables");
        tx.commit().await.expect("commit");

        assert_eq!(count_rows(&pool, "users").await, 0);
        assert_eq!(count_rows(&pool, "orders").await, 0);
        assert_eq!(count_rows(&pool, "admin_disputes").await, 0);
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

    #[tokio::test]
    async fn import_rollback_keeps_db_and_settings_when_settings_replace_fails() {
        let base = std::env::temp_dir().join(format!("mostrix-import-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).expect("base");
        let settings_path = base.join("settings.toml");
        fs::write(&settings_path, SAMPLE_SETTINGS).expect("settings");

        let pool = create_wipe_test_pool().await;

        let err = import_seed_with_paths(
            &pool,
            &base,
            &settings_path,
            "nsec-new",
            Some(inject_settings_replace_fail),
            None,
        )
        .await
        .expect_err("settings failure should abort import");
        assert!(err
            .to_string()
            .contains("injected settings replace failure"));

        assert_eq!(count_rows(&pool, "users").await, 1);
        assert_eq!(count_rows(&pool, "orders").await, 1);
        let restored = fs::read_to_string(&settings_path).expect("settings");
        assert!(restored.contains("nsec-old"));
        assert!(restored.contains("user@domain.com"));

        let _ = fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn import_rollback_restores_settings_when_db_commit_fails() {
        let base = std::env::temp_dir().join(format!("mostrix-import-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).expect("base");
        let settings_path = base.join("settings.toml");
        fs::write(&settings_path, SAMPLE_SETTINGS).expect("settings");

        let pool = create_wipe_test_pool().await;

        let err = import_seed_with_paths(
            &pool,
            &base,
            &settings_path,
            "nsec-new",
            None,
            Some(inject_db_commit_fail),
        )
        .await
        .expect_err("db failure should abort import");
        assert!(err.to_string().contains("injected db commit failure"));

        assert_eq!(count_rows(&pool, "users").await, 1);
        assert_eq!(count_rows(&pool, "orders").await, 1);
        let restored = fs::read_to_string(&settings_path).expect("settings");
        assert!(restored.contains("nsec-old"));
        assert!(restored.contains("user@domain.com"));

        let _ = fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn clear_session_rollback_restores_settings_when_db_commit_fails() {
        let base = std::env::temp_dir().join(format!("mostrix-wipe-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).expect("base");
        let settings_path = base.join("settings.toml");
        fs::write(&settings_path, SAMPLE_SETTINGS).expect("settings");

        let pool = create_wipe_test_pool().await;

        let err = clear_session_with_paths(
            &pool,
            &base,
            &settings_path,
            None,
            Some(inject_db_commit_fail),
        )
        .await
        .expect_err("db failure should abort wipe");
        assert!(err.to_string().contains("injected db commit failure"));

        assert_eq!(count_rows(&pool, "users").await, 1);
        let restored = fs::read_to_string(&settings_path).expect("settings");
        assert!(restored.contains("user@domain.com"));

        let _ = fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn clear_session_rollback_restores_settings_when_settings_replace_fails() {
        let base = std::env::temp_dir().join(format!("mostrix-wipe-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).expect("base");
        let settings_path = base.join("settings.toml");
        fs::write(&settings_path, SAMPLE_SETTINGS).expect("settings");

        let pool = create_wipe_test_pool().await;

        let err = clear_session_with_paths(
            &pool,
            &base,
            &settings_path,
            Some(inject_settings_replace_fail),
            None,
        )
        .await
        .expect_err("settings failure should abort wipe");
        assert!(err
            .to_string()
            .contains("injected settings replace failure"));

        assert_eq!(count_rows(&pool, "users").await, 1);
        let restored = fs::read_to_string(&settings_path).expect("settings");
        assert!(restored.contains("user@domain.com"));

        let _ = fs::remove_dir_all(&base);
    }
}
