// Execute admin finalize dispute functionality (settle or cancel)
use anyhow::Result;
use mostro_core::prelude::DisputeStatus;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::AdminDispute;
use crate::util::chat_listener::untrack_dispute_chat_parties;
use crate::util::mostro_info::MostroInstanceInfo;

use super::{execute_admin_cancel, execute_admin_settle, BondSlashChoice};
use crate::util::order_utils::helper::AdminFinalizeAck;

/// Local status after Mostro acks a settle/cancel.
///
/// `AlreadyCooperativelyCanceled` keeps the user-closed status; do not map it
/// to `seller-refunded`. Persistence still goes through
/// [`AdminDispute::set_status_by_dispute_id`] so a late ack cannot overwrite
/// a terminal row the DM listener already wrote.
pub(crate) fn status_for_finalize_ack(ack: AdminFinalizeAck, is_settle: bool) -> DisputeStatus {
    match ack {
        AdminFinalizeAck::AlreadyCooperativelyCanceled => DisputeStatus::CooperativelyCanceled,
        AdminFinalizeAck::Confirmed if is_settle => DisputeStatus::Settled,
        AdminFinalizeAck::Confirmed => DisputeStatus::SellerRefunded,
    }
}

/// Finalize a dispute by either settling (paying buyer) or canceling (refunding seller).
///
/// This function handles both AdminSettle and AdminCancel actions, sends the message
/// to Mostro, and updates the dispute status in the database.
///
/// **Post-Finalization Protection**: This function checks if the dispute is already
/// finalized before attempting any action. If the dispute status is Settled,
/// SellerRefunded, or Released, the action is blocked and an error is returned.
///
/// Requires admin privileges (admin_privkey must be configured)
///
/// # Arguments
///
/// * `dispute_id` - The UUID of the dispute to finalize
/// * `bond` - Anti-abuse bond slash choice for the wire payload
/// * `client` - The Nostr client for sending messages
/// * `mostro_pubkey` - The public key of the Mostro daemon
/// * `pool` - The database connection pool for updating dispute status
/// * `is_settle` - If true, executes AdminSettle (pay buyer), otherwise AdminCancel (refund seller)
///
/// # Returns
///
/// Returns `Ok(AdminFinalizeAck)` if the message was sent and the database was
/// updated, or an error if the operation failed.
///
/// # Errors
///
/// This function will return an error if:
/// - Dispute is already finalized (Settled, SellerRefunded, Released, or
///   CooperativelyCanceled)
/// - Dispute not found in database
/// - Settings are not initialized
/// - Admin private key is not configured
/// - Failed to serialize the message
/// - Failed to send the DM
/// - Failed to update dispute status in database
#[allow(clippy::too_many_arguments)]
pub async fn execute_finalize_dispute(
    dispute_id: &Uuid,
    bond: BondSlashChoice,
    admin_keys: &Keys,
    client: &Client,
    mostro_pubkey: PublicKey,
    pool: &SqlitePool,
    is_settle: bool,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<AdminFinalizeAck> {
    let dispute_id_str = dispute_id.to_string();
    let dispute: AdminDispute = sqlx::query_as::<_, AdminDispute>(
        r#"SELECT * FROM admin_disputes WHERE dispute_id = ? LIMIT 1"#,
    )
    .bind(&dispute_id_str)
    .fetch_one(pool)
    .await?;

    if dispute.is_finalized() {
        let action_name = if is_settle {
            "AdminSettle"
        } else {
            "AdminCancel"
        };
        return Err(anyhow::anyhow!(
            "Cannot execute {}: dispute {} is already finalized (status: {})",
            action_name,
            dispute_id,
            dispute.status.as_deref().unwrap_or("unknown")
        ));
    }

    if is_settle && !dispute.can_settle() {
        return Err(anyhow::anyhow!(
            "Cannot settle dispute {}: action not allowed in current state",
            dispute_id
        ));
    }
    if !is_settle && !dispute.can_cancel() {
        return Err(anyhow::anyhow!(
            "Cannot cancel dispute {}: action not allowed in current state",
            dispute_id
        ));
    }

    let order_id = Uuid::parse_str(&dispute.id)?;

    let ack = if is_settle {
        execute_admin_settle(
            &order_id,
            bond,
            admin_keys,
            client,
            mostro_pubkey,
            mostro_instance,
        )
        .await?
    } else {
        execute_admin_cancel(
            &order_id,
            bond,
            admin_keys,
            client,
            mostro_pubkey,
            mostro_instance,
        )
        .await?
    };

    let cooperatively_canceled = matches!(ack, AdminFinalizeAck::AlreadyCooperativelyCanceled);
    let next_status = status_for_finalize_ack(ack, is_settle);
    let _ = AdminDispute::set_status_by_dispute_id(pool, &dispute_id_str, next_status).await?;

    // Dispute left InProgress: drop buyer/seller shared-key chat subscriptions.
    untrack_dispute_chat_parties(&dispute_id_str);

    let action_name = if cooperatively_canceled {
        "closed (cooperative cancellation accepted)"
    } else if is_settle {
        "settled (buyer paid)"
    } else {
        "canceled (seller refunded)"
    };

    log::info!(
        "✅ Dispute {} {} ({})!",
        dispute_id,
        action_name,
        bond.log_context()
    );
    Ok(ack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::order_utils::helper::AdminFinalizeAck;

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
        .expect("admin_disputes table");
    }

    async fn insert_row(pool: &SqlitePool, dispute_id: &str, status: &str) {
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
        .expect("insert row");
    }

    #[test]
    fn already_cooperatively_canceled_keeps_user_closed_status() {
        assert_eq!(
            status_for_finalize_ack(AdminFinalizeAck::AlreadyCooperativelyCanceled, true),
            DisputeStatus::CooperativelyCanceled
        );
        assert_eq!(
            status_for_finalize_ack(AdminFinalizeAck::AlreadyCooperativelyCanceled, false),
            DisputeStatus::CooperativelyCanceled
        );
        assert_eq!(
            status_for_finalize_ack(AdminFinalizeAck::Confirmed, true),
            DisputeStatus::Settled
        );
        assert_eq!(
            status_for_finalize_ack(AdminFinalizeAck::Confirmed, false),
            DisputeStatus::SellerRefunded
        );
    }

    #[tokio::test]
    async fn late_settle_ack_does_not_overwrite_cooperatively_canceled() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        create_admin_disputes_table(&pool).await;
        let dispute_id = Uuid::new_v4().to_string();
        insert_row(&pool, &dispute_id, "cooperatively-canceled").await;

        let next = status_for_finalize_ack(AdminFinalizeAck::Confirmed, true);
        let updated = AdminDispute::set_status_by_dispute_id(&pool, &dispute_id, next)
            .await
            .expect("update");
        assert!(!updated);

        let row = AdminDispute::get_by_dispute_id(&pool, &dispute_id)
            .await
            .expect("query")
            .expect("row");
        assert_eq!(row.status.as_deref(), Some("cooperatively-canceled"));
    }

    #[tokio::test]
    async fn cooperative_cancel_ack_writes_cooperatively_canceled() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        create_admin_disputes_table(&pool).await;
        let dispute_id = Uuid::new_v4().to_string();
        insert_row(&pool, &dispute_id, "in-progress").await;

        let next = status_for_finalize_ack(AdminFinalizeAck::AlreadyCooperativelyCanceled, false);
        let updated = AdminDispute::set_status_by_dispute_id(&pool, &dispute_id, next)
            .await
            .expect("update");
        assert!(updated);

        let row = AdminDispute::get_by_dispute_id(&pool, &dispute_id)
            .await
            .expect("query")
            .expect("row");
        assert_eq!(row.status.as_deref(), Some("cooperatively-canceled"));
        assert!(row.closed_by_users());
    }
}
