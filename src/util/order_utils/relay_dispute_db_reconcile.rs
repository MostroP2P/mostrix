//! Align local `admin_disputes` rows with terminal statuses seen on kind-38386 events.
//!
//! Mostro auto-closes a dispute on cooperative cancel (`seller-refunded`) or seller
//! release (`settled`) by publishing a NIP-33 replacement. It does not DM the solver,
//! so taken rows must be advanced from that event.

use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::models::AdminDispute;
use crate::util::chat_listener::untrack_dispute_chat_parties;

use super::helper::fetch_dispute_by_id_from_relay;
use super::relay_order_db_reconcile::TARGETED_RELAY_RECONCILE_MAX_PER_TICK;

/// Copy terminal kind-38386 statuses onto matching in-memory taken rows.
///
/// Returns dispute ids whose local status actually changed (for chat untrack).
pub fn apply_terminal_relay_statuses_to_admin_disputes(
    admin_disputes: &mut [AdminDispute],
    relay_disputes: &[Dispute],
) -> Vec<String> {
    let mut closed = Vec::new();
    for relay in relay_disputes {
        if !AdminDispute::is_terminal_status(&relay.status) {
            continue;
        }
        let relay_id = relay.id.to_string();
        let Some(local) = admin_disputes.iter_mut().find(|d| d.dispute_id == relay_id) else {
            continue;
        };
        if local.is_finalized() {
            continue;
        }
        if local.status.as_deref() == Some(relay.status.as_str()) {
            continue;
        }
        local.status = Some(relay.status.clone());
        closed.push(relay_id);
    }
    closed
}

/// If `relay_dispute` is terminal and a non-finalized local row exists, update SQLite.
pub async fn reconcile_one_admin_dispute_if_terminal(pool: &SqlitePool, relay_dispute: &Dispute) {
    if !AdminDispute::is_terminal_status(&relay_dispute.status) {
        return;
    }
    let Ok(status) = DisputeStatus::from_str(&relay_dispute.status) else {
        return;
    };
    let dispute_id = relay_dispute.id.to_string();
    match AdminDispute::set_status_by_dispute_id(pool, &dispute_id, status).await {
        Ok(true) => {
            log::info!(
                "Relay reconcile: dispute {} advanced to {}",
                dispute_id,
                relay_dispute.status
            );
            untrack_dispute_chat_parties(&dispute_id);
        }
        Ok(false) => {}
        Err(e) => {
            log::warn!(
                "Relay reconcile: failed to update dispute {} to {}: {}",
                dispute_id,
                relay_dispute.status,
                e
            );
        }
    }
}

/// Apply [`reconcile_one_admin_dispute_if_terminal`] for each snapshot row.
pub async fn reconcile_terminal_admin_disputes_from_relay(
    pool: &SqlitePool,
    relay_disputes: &[Dispute],
) {
    for relay_dispute in relay_disputes {
        reconcile_one_admin_dispute_if_terminal(pool, relay_dispute).await;
    }
}

/// Per-dispute `#d` fetch for local InProgress rows missing from the latest snapshot.
///
/// Returns terminal disputes fetched from relays so the caller can feed them into
/// the shared live dispute vec (ensuring the UI stays in sync with DB updates).
pub async fn run_targeted_relay_dispute_db_reconcile_tick(
    client: &Client,
    pool: &SqlitePool,
    mostro_pubkey: PublicKey,
    cursor: &Arc<Mutex<usize>>,
    already_seen: &HashSet<Uuid>,
) -> Result<Vec<Dispute>> {
    let ids = AdminDispute::list_in_progress_dispute_ids(pool).await?;
    let ids: Vec<(Uuid, String)> = ids
        .into_iter()
        .filter_map(|id| {
            let uuid = Uuid::parse_str(&id).ok()?;
            if already_seen.contains(&uuid) {
                None
            } else {
                Some((uuid, id))
            }
        })
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let len = ids.len();
    let (start, take) = {
        let c = cursor
            .lock()
            .map_err(|_| anyhow::anyhow!("targeted dispute reconcile cursor mutex poisoned"))?;
        let start = *c % len;
        let take = TARGETED_RELAY_RECONCILE_MAX_PER_TICK.min(len);
        (start, take)
    };
    let mut resolved = Vec::new();
    for i in 0..take {
        let (dispute_id, _) = &ids[(start + i) % len];
        match fetch_dispute_by_id_from_relay(client, mostro_pubkey, *dispute_id).await {
            Ok(Some(relay_dispute)) => {
                reconcile_one_admin_dispute_if_terminal(pool, &relay_dispute).await;
                if AdminDispute::is_terminal_status(&relay_dispute.status) {
                    resolved.push(relay_dispute);
                }
            }
            Ok(None) => {
                log::debug!(
                    "[disputes_reconcile_targeted] no relay event for dispute_id={}",
                    dispute_id
                );
            }
            Err(e) => {
                log::debug!(
                    "[disputes_reconcile_targeted] fetch failed dispute_id={}: {}",
                    dispute_id,
                    e
                );
            }
        }
    }
    {
        let mut c = cursor
            .lock()
            .map_err(|_| anyhow::anyhow!("targeted dispute reconcile cursor mutex poisoned"))?;
        *c = (start + take) % len;
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

    async fn test_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool")
    }

    async fn create_admin_disputes_table(pool: &SqlitePool) {
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
        .unwrap();
    }

    async fn insert_admin_dispute(pool: &SqlitePool, dispute_id: &str, status: &str) {
        sqlx::query(
            r#"INSERT INTO admin_disputes (
                id, dispute_id, initiator_pubkey, initiator_full_privacy,
                counterpart_full_privacy, premium, payment_method, amount, fiat_amount,
                fiat_code, fee, routing_fee, taken_at, created_at, status
            ) VALUES (?, ?, 'npub1initiator', 0, 0, 0, 'sepa', 0, 0, 'USD', 0, 0, 1, 1, ?)"#,
        )
        .bind(format!("order-{dispute_id}"))
        .bind(dispute_id)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
    }

    fn relay_dispute(id: Uuid, status: &str) -> Dispute {
        Dispute {
            id,
            status: status.to_string(),
            ..Default::default()
        }
    }

    fn admin_row(dispute_id: &str, status: &str) -> AdminDispute {
        AdminDispute {
            dispute_id: dispute_id.to_string(),
            status: Some(status.to_string()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn reconcile_updates_in_progress_row_when_relay_seller_refunded() {
        let pool = test_pool().await;
        create_admin_disputes_table(&pool).await;
        let id = Uuid::new_v4();
        insert_admin_dispute(&pool, &id.to_string(), "in-progress").await;

        reconcile_one_admin_dispute_if_terminal(&pool, &relay_dispute(id, "seller-refunded")).await;

        let row = AdminDispute::get_by_dispute_id(&pool, &id.to_string())
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(row.status.as_deref(), Some("seller-refunded"));
    }

    #[tokio::test]
    async fn reconcile_updates_in_progress_row_when_relay_settled() {
        let pool = test_pool().await;
        create_admin_disputes_table(&pool).await;
        let id = Uuid::new_v4();
        insert_admin_dispute(&pool, &id.to_string(), "in-progress").await;

        reconcile_one_admin_dispute_if_terminal(&pool, &relay_dispute(id, "settled")).await;

        let row = AdminDispute::get_by_dispute_id(&pool, &id.to_string())
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(row.status.as_deref(), Some("settled"));
    }

    #[tokio::test]
    async fn reconcile_does_not_overwrite_already_settled() {
        let pool = test_pool().await;
        create_admin_disputes_table(&pool).await;
        let id = Uuid::new_v4();
        insert_admin_dispute(&pool, &id.to_string(), "settled").await;

        reconcile_one_admin_dispute_if_terminal(&pool, &relay_dispute(id, "seller-refunded")).await;

        let row = AdminDispute::get_by_dispute_id(&pool, &id.to_string())
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(row.status.as_deref(), Some("settled"));
    }

    #[tokio::test]
    async fn reconcile_is_noop_for_unknown_dispute_id() {
        let pool = test_pool().await;
        create_admin_disputes_table(&pool).await;
        let id = Uuid::new_v4();

        reconcile_one_admin_dispute_if_terminal(&pool, &relay_dispute(id, "seller-refunded")).await;

        assert!(AdminDispute::get_by_dispute_id(&pool, &id.to_string())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn reconcile_ignores_non_terminal_relay_status() {
        let pool = test_pool().await;
        create_admin_disputes_table(&pool).await;
        let id = Uuid::new_v4();
        insert_admin_dispute(&pool, &id.to_string(), "in-progress").await;

        reconcile_one_admin_dispute_if_terminal(&pool, &relay_dispute(id, "in-progress")).await;

        let row = AdminDispute::get_by_dispute_id(&pool, &id.to_string())
            .await
            .unwrap()
            .expect("row exists");
        assert_eq!(row.status.as_deref(), Some("in-progress"));
    }

    #[test]
    fn in_memory_sync_copies_seller_refunded_onto_in_progress_row() {
        let id = Uuid::new_v4();
        let mut admin = vec![admin_row(&id.to_string(), "in-progress")];
        let relay = vec![relay_dispute(id, "seller-refunded")];

        let closed = apply_terminal_relay_statuses_to_admin_disputes(&mut admin, &relay);

        assert_eq!(closed, vec![id.to_string()]);
        assert_eq!(admin[0].status.as_deref(), Some("seller-refunded"));
    }

    #[test]
    fn in_memory_sync_skips_already_finalized_and_unknown_ids() {
        let settled_id = Uuid::new_v4();
        let unknown_id = Uuid::new_v4();
        let mut admin = vec![admin_row(&settled_id.to_string(), "settled")];
        let relay = vec![
            relay_dispute(settled_id, "seller-refunded"),
            relay_dispute(unknown_id, "seller-refunded"),
        ];

        let closed = apply_terminal_relay_statuses_to_admin_disputes(&mut admin, &relay);

        assert!(closed.is_empty());
        assert_eq!(admin[0].status.as_deref(), Some("settled"));
    }
}
