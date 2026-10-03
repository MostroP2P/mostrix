//! Relay subscription for [`SolverDm`]s addressed to the admin key.
//!
//! Kept apart from the Mostro DM router: authors are the configured trusted
//! senders, not the Mostro node, so the Mostro author checks stay untouched.
//! Subscribe first, then backfill from the newest stored message, so nothing
//! published during the fetch is missed; the store drops duplicates.

use std::time::Duration;

use futures::StreamExt;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use super::{parse_solver_dm, store, SolverDm};
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;

/// How far back the first fetch looks when nothing is stored yet.
pub const HISTORY_WINDOW_SECS: i64 = 7 * 24 * 3600;
/// Re-read this much before the newest stored message, for relays that
/// received a message late.
const RESUME_OVERLAP_SECS: i64 = 3600;
/// Pause before resubscribing after the notification stream ends or fails.
const RETRY_DELAY: Duration = Duration::from_secs(5);
/// Fixed relay subscription id, so a respawn replaces the previous one and
/// [`unsubscribe`] can drop it after an abort.
const SUBSCRIPTION_ID: &str = "mostrix-solver-dms";

fn subscription_id() -> SubscriptionId {
    SubscriptionId::new(SUBSCRIPTION_ID)
}

/// Drops the listener's relay subscription (after aborting its task).
pub async fn unsubscribe(client: &Client) {
    if let Err(e) = client.unsubscribe(&subscription_id()).await {
        log::debug!("[solver_dms] unsubscribe failed: {e}");
    }
}

/// Kind-14 messages from `trusted` to `admin`.
pub fn solver_dm_filter(trusted: &[PublicKey], admin: PublicKey) -> Filter {
    Filter::new()
        .kind(Kind::PrivateDirectMessage)
        .authors(trusted.iter().copied())
        .pubkey(admin)
}

/// Start of the backfill window: just before the newest stored message, or
/// [`HISTORY_WINDOW_SECS`] ago when none is stored.
pub fn history_since(latest_stored: Option<i64>, now: i64) -> i64 {
    match latest_stored {
        Some(latest) => latest.saturating_sub(RESUME_OVERLAP_SECS),
        None => now.saturating_sub(HISTORY_WINDOW_SECS),
    }
}

/// Runs the listener until aborted, resubscribing after failures. Returns
/// `None` (nothing spawned) when there is nobody to trust.
pub fn spawn_solver_dm_listener(
    client: Client,
    admin_keys: Keys,
    trusted: Vec<PublicKey>,
    pool: SqlitePool,
    tx: UnboundedSender<SolverDm>,
) -> Option<JoinHandle<()>> {
    if trusted.is_empty() {
        return None;
    }
    Some(tokio::spawn(async move {
        loop {
            if let Err(e) = listen_once(&client, &admin_keys, &trusted, &pool, &tx).await {
                log::warn!("[solver_dms] listener stopped: {e}; retrying");
            }
            if tx.is_closed() {
                return;
            }
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }))
}

async fn listen_once(
    client: &Client,
    admin_keys: &Keys,
    trusted: &[PublicKey],
    pool: &SqlitePool,
    tx: &UnboundedSender<SolverDm>,
) -> anyhow::Result<()> {
    let filter = solver_dm_filter(trusted, admin_keys.public_key());
    let notifications = client.notifications();
    client
        .subscribe(filter.clone().limit(0))
        .with_id(subscription_id())
        .await?;
    // Whatever happens after subscribing, drop the subscription before
    // returning so a retry never leaves it behind.
    let result =
        backfill_then_follow(client, filter, notifications, admin_keys, trusted, pool, tx).await;
    unsubscribe(client).await;
    result
}

async fn backfill_then_follow(
    client: &Client,
    filter: Filter,
    mut notifications: impl futures::Stream<Item = ClientNotification> + Unpin,
    admin_keys: &Keys,
    trusted: &[PublicKey],
    pool: &SqlitePool,
    tx: &UnboundedSender<SolverDm>,
) -> anyhow::Result<()> {
    let since = history_since(
        store::resume_cursor(
            pool,
            &admin_keys.public_key().to_hex(),
            &trusted.iter().map(PublicKey::to_hex).collect::<Vec<_>>(),
        )
        .await?,
        Timestamp::now().as_secs() as i64,
    );
    let history = client
        .fetch_events(filter.since(Timestamp::from_secs(since.max(0) as u64)))
        .timeout(FETCH_EVENTS_TIMEOUT)
        .await;
    match history {
        Ok(events) => {
            for event in events.into_iter() {
                accept(&event, admin_keys, trusted, pool, tx).await;
            }
        }
        Err(e) => log::warn!("[solver_dms] history fetch failed: {e}"),
    }

    while let Some(notification) = notifications.next().await {
        if let ClientNotification::Event { event, .. } = notification {
            accept(&event, admin_keys, trusted, pool, tx).await;
        }
        if tx.is_closed() {
            break;
        }
    }
    Ok(())
}

/// Stores a solver DM and forwards it to the UI the first time it is seen.
async fn accept(
    event: &Event,
    admin_keys: &Keys,
    trusted: &[PublicKey],
    pool: &SqlitePool,
    tx: &UnboundedSender<SolverDm>,
) {
    let Some(dm) = parse_solver_dm(event, admin_keys, trusted) else {
        return;
    };
    match store::insert(pool, &dm).await {
        Ok(true) => {
            // Already persisted: if the UI is gone, the next start loads it.
            let _ = tx.send(dm);
        }
        Ok(false) => {}
        Err(e) => log::warn!("[solver_dms] cannot store {}: {e}", dm.event_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_fetch_reads_the_history_window() {
        assert_eq!(
            history_since(None, 1_000_000),
            1_000_000 - HISTORY_WINDOW_SECS
        );
    }

    #[test]
    fn later_fetches_resume_just_before_the_newest_message() {
        assert_eq!(
            history_since(Some(500_000), 1_000_000),
            500_000 - RESUME_OVERLAP_SECS
        );
    }

    #[test]
    fn the_filter_reads_only_trusted_authors_writing_to_the_admin() {
        let trusted = Keys::generate().public_key();
        let admin = Keys::generate().public_key();
        let filter = solver_dm_filter(&[trusted], admin);

        let json = filter.as_json();
        assert!(json.contains(&trusted.to_hex()), "{json}");
        assert!(json.contains(&admin.to_hex()), "{json}");
        assert!(json.contains("\"kinds\":[14]"), "{json}");
    }

    #[tokio::test]
    async fn nothing_is_spawned_without_trusted_senders() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let handle =
            spawn_solver_dm_listener(Client::default(), Keys::generate(), vec![], pool, tx);

        assert!(handle.is_none());
    }

    #[tokio::test]
    async fn a_new_trusted_message_is_stored_and_forwarded_once() {
        use mostro_core::prelude::*;
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        store::ensure_table(&pool).await.unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let serbero = Keys::generate();
        let admin = Keys::generate();
        let message = Message::new_dm(
            None,
            None,
            Action::SendDm,
            Some(Payload::TextMessage("Dispute d · taken".into())),
        );
        let event = mostro_core::transport::wrap_message_nip44(
            &message,
            &serbero,
            &serbero,
            admin.public_key(),
            mostro_core::prelude::WrapOptions::default(),
        )
        .unwrap();

        accept(&event, &admin, &[serbero.public_key()], &pool, &tx).await;
        accept(&event, &admin, &[serbero.public_key()], &pool, &tx).await;

        assert_eq!(rx.try_recv().unwrap().subject, "taken");
        assert!(
            rx.try_recv().is_err(),
            "a re-delivered event is not forwarded"
        );
    }
}
