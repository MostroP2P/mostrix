//! SQLite persistence for [`SolverDm`]s, so they survive restarts and relay
//! gaps. Rows are keyed by event id: re-delivered events are ignored.

use anyhow::Result;
use sqlx::SqlitePool;

use super::SolverDm;

/// Creates the `solver_dms` table when missing (new and existing databases)
/// and adds `recipient_pubkey` to tables created before it existed.
pub async fn ensure_table(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS solver_dms (
            event_id TEXT PRIMARY KEY,
            sender_pubkey TEXT NOT NULL,
            recipient_pubkey TEXT NOT NULL DEFAULT '',
            dispute_id TEXT,
            subject TEXT NOT NULL,
            text TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await?;
    let (has_recipient,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pragma_table_info('solver_dms') WHERE name = 'recipient_pubkey'",
    )
    .fetch_one(pool)
    .await?;
    if has_recipient == 0 {
        sqlx::query("ALTER TABLE solver_dms ADD COLUMN recipient_pubkey TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_solver_dms_recipient_created
            ON solver_dms (recipient_pubkey, created_at);
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Deletes every stored message (session wipe / restore).
pub async fn delete_all_in_tx(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<()> {
    sqlx::query("DELETE FROM solver_dms")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Stores `dm`; returns `false` when the event was already stored.
pub async fn insert(pool: &SqlitePool, dm: &SolverDm) -> Result<bool> {
    let result = sqlx::query(
        r#"
        INSERT OR IGNORE INTO solver_dms
            (event_id, sender_pubkey, recipient_pubkey, dispute_id, subject, text, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&dm.event_id)
    .bind(&dm.sender_pubkey)
    .bind(&dm.recipient_pubkey)
    .bind(&dm.dispute_id)
    .bind(&dm.subject)
    .bind(&dm.text)
    .bind(dm.created_at)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Messages to `recipient` created at or after `since`, oldest first.
pub async fn load_since(pool: &SqlitePool, recipient: &str, since: i64) -> Result<Vec<SolverDm>> {
    let rows: Vec<(String, String, Option<String>, String, String, i64)> = sqlx::query_as(
        r#"
        SELECT event_id, sender_pubkey, dispute_id, subject, text, created_at
        FROM solver_dms
        WHERE recipient_pubkey = ? AND created_at >= ?
        ORDER BY created_at ASC, event_id ASC
        "#,
    )
    .bind(recipient)
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(event_id, sender_pubkey, dispute_id, subject, text, created_at)| SolverDm {
                event_id,
                sender_pubkey,
                recipient_pubkey: recipient.to_string(),
                dispute_id,
                subject,
                text,
                created_at,
            },
        )
        .collect())
}

/// Where the relay fetch for `recipient` resumes: the oldest of each trusted
/// sender's newest stored message, so every sender is caught up. `None`
/// (fetch the whole history window) when a trusted sender has nothing stored
/// yet, e.g. one just added to `trusted_dm_senders`, or nobody is trusted.
pub async fn resume_cursor(
    pool: &SqlitePool,
    recipient: &str,
    trusted: &[String],
) -> Result<Option<i64>> {
    let newest: Vec<(String, i64)> = sqlx::query_as(
        "SELECT sender_pubkey, MAX(created_at) FROM solver_dms \
         WHERE recipient_pubkey = ? GROUP BY sender_pubkey",
    )
    .bind(recipient)
    .fetch_all(pool)
    .await?;
    let mut cursor: Option<i64> = None;
    for sender in trusted {
        let Some((_, at)) = newest.iter().find(|(s, _)| s == sender) else {
            return Ok(None);
        };
        cursor = Some(cursor.map_or(*at, |c| c.min(*at)));
    }
    Ok(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        ensure_table(&pool).await.unwrap();
        pool
    }

    const ME: &str = "admin-me";

    fn dm(event_id: &str, dispute_id: Option<&str>, created_at: i64) -> SolverDm {
        SolverDm {
            event_id: event_id.to_string(),
            sender_pubkey: "ab".repeat(32),
            recipient_pubkey: ME.to_string(),
            dispute_id: dispute_id.map(str::to_string),
            subject: "taken".to_string(),
            text: "Dispute x · taken".to_string(),
            created_at,
        }
    }

    #[tokio::test]
    async fn a_stored_message_is_loaded_back() {
        let pool = pool().await;
        let original = dm("e1", Some("d1"), 100);

        assert!(insert(&pool, &original).await.unwrap());

        assert_eq!(load_since(&pool, ME, 0).await.unwrap(), vec![original]);
    }

    #[tokio::test]
    async fn the_same_event_is_stored_once() {
        let pool = pool().await;

        assert!(insert(&pool, &dm("e1", None, 100)).await.unwrap());
        assert!(!insert(&pool, &dm("e1", None, 100)).await.unwrap());

        assert_eq!(load_since(&pool, ME, 0).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn loading_skips_older_messages_and_sorts_oldest_first() {
        let pool = pool().await;
        insert(&pool, &dm("late", Some("d1"), 300)).await.unwrap();
        insert(&pool, &dm("old", Some("d1"), 50)).await.unwrap();
        insert(&pool, &dm("mid", Some("d2"), 200)).await.unwrap();

        let ids: Vec<String> = load_since(&pool, ME, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|d| d.event_id)
            .collect();

        assert_eq!(ids, ["mid", "late"]);
    }

    #[tokio::test]
    async fn the_resume_cursor_tracks_the_newest_message() {
        let pool = pool().await;
        assert_eq!(
            resume_cursor(&pool, ME, &["ab".repeat(32)]).await.unwrap(),
            None
        );

        insert(&pool, &dm("a", None, 10)).await.unwrap();
        insert(&pool, &dm("b", None, 30)).await.unwrap();

        assert_eq!(
            resume_cursor(&pool, ME, &["ab".repeat(32)]).await.unwrap(),
            Some(30)
        );
    }

    #[tokio::test]
    async fn ensuring_the_table_twice_keeps_its_rows() {
        let pool = pool().await;
        insert(&pool, &dm("a", None, 10)).await.unwrap();

        ensure_table(&pool).await.unwrap();

        assert_eq!(load_since(&pool, ME, 0).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn another_admin_keys_messages_are_not_loaded_or_resumed_from() {
        let pool = pool().await;
        let old_key = SolverDm {
            recipient_pubkey: "admin-old".to_string(),
            ..dm("old", Some("d1"), 900)
        };
        insert(&pool, &old_key).await.unwrap();
        insert(&pool, &dm("mine", Some("d1"), 100)).await.unwrap();

        let ids: Vec<String> = load_since(&pool, ME, 0)
            .await
            .unwrap()
            .into_iter()
            .map(|d| d.event_id)
            .collect();

        assert_eq!(ids, ["mine"]);
        assert_eq!(
            resume_cursor(&pool, ME, &["ab".repeat(32)]).await.unwrap(),
            Some(100)
        );
    }

    fn from(sender: &str, event_id: &str, created_at: i64) -> SolverDm {
        SolverDm {
            sender_pubkey: sender.to_string(),
            ..dm(event_id, None, created_at)
        }
    }

    #[tokio::test]
    async fn the_resume_cursor_waits_for_the_least_recent_trusted_sender() {
        let pool = pool().await;
        insert(&pool, &from("a", "a1", 500)).await.unwrap();
        insert(&pool, &from("b", "b1", 200)).await.unwrap();
        insert(&pool, &from("gone", "g1", 900)).await.unwrap();

        let trusted = ["a".to_string(), "b".to_string()];
        assert_eq!(resume_cursor(&pool, ME, &trusted).await.unwrap(), Some(200));
    }

    #[tokio::test]
    async fn a_newly_trusted_sender_gets_the_full_history() {
        let pool = pool().await;
        insert(&pool, &from("a", "a1", 500)).await.unwrap();

        let trusted = ["a".to_string(), "new".to_string()];
        assert_eq!(resume_cursor(&pool, ME, &trusted).await.unwrap(), None);
        assert_eq!(resume_cursor(&pool, ME, &[]).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_table_without_recipients_gains_the_column() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE solver_dms (event_id TEXT PRIMARY KEY, sender_pubkey TEXT NOT NULL, \
             dispute_id TEXT, subject TEXT NOT NULL, text TEXT NOT NULL, created_at INTEGER NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        ensure_table(&pool).await.unwrap();
        insert(&pool, &dm("a", None, 10)).await.unwrap();

        assert_eq!(load_since(&pool, ME, 0).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_session_wipe_deletes_every_message() {
        let pool = pool().await;
        insert(&pool, &dm("a", None, 10)).await.unwrap();

        let mut tx = pool.begin().await.unwrap();
        delete_all_in_tx(&mut tx).await.unwrap();
        tx.commit().await.unwrap();

        assert!(load_since(&pool, ME, 0).await.unwrap().is_empty());
    }
}
