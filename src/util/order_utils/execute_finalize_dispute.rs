// Execute admin finalize dispute functionality (settle or cancel)
use anyhow::Result;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::AdminDispute;

use super::{execute_admin_cancel, execute_admin_settle};

/// Finalize a dispute by either settling (paying buyer) or canceling (refunding seller).
///
/// This function handles both AdminSettle and AdminCancel actions, sends the message
/// to Mostro, and updates the dispute status in the database.
///
/// Requires admin privileges (admin_privkey must be configured)
///
/// # Arguments
///
/// * `dispute_id` - The UUID of the dispute to finalize
/// * `client` - The Nostr client for sending messages
/// * `mostro_pubkey` - The public key of the Mostro daemon
/// * `pool` - The database connection pool for updating dispute status
/// * `is_settle` - If true, executes AdminSettle (pay buyer), otherwise AdminCancel (refund seller)
///
/// # Returns
///
/// Returns `Ok(())` if the finalization message was successfully sent and the database
/// was updated, or an error if the operation failed.
///
/// # Errors
///
/// This function will return an error if:
/// - Settings are not initialized
/// - Admin private key is not configured
/// - Failed to serialize the message
/// - Failed to send the DM
/// - Failed to update dispute status in database
pub async fn execute_finalize_dispute(
    dispute_id: &Uuid,
    client: &Client,
    mostro_pubkey: PublicKey,
    pool: &SqlitePool,
    is_settle: bool,
) -> Result<()> {
    // Execute the appropriate action (settle or cancel)
    let result = if is_settle {
        execute_admin_settle(dispute_id, client, mostro_pubkey).await
    } else {
        execute_admin_cancel(dispute_id, client, mostro_pubkey).await
    };

    result?; // Propagate error if action failed

    // Update dispute status in database
    // First, get the order_id (id field) from the dispute_id
    let dispute_id_str = dispute_id.to_string();
    let order_id: String = sqlx::query_scalar::<_, String>(
        r#"SELECT id FROM admin_disputes WHERE dispute_id = ? LIMIT 1"#,
    )
    .bind(&dispute_id_str)
    .fetch_one(pool)
    .await?;

    // Now update using the order_id (primary key)
    if is_settle {
        AdminDispute::set_status_settled(pool, &order_id).await?;
    } else {
        AdminDispute::set_status_seller_refunded(pool, &order_id).await?;
    }

    let action_name = if is_settle {
        "settled (buyer paid)"
    } else {
        "canceled (seller refunded)"
    };

    log::info!("✅ Dispute {} {}!", dispute_id, action_name);
    Ok(())
}
