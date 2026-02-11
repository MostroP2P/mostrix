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
/// **Post-Finalization Protection**: This function checks if the dispute is already
/// finalized before attempting any action. If the dispute status is Settled,
/// SellerRefunded, or Released, the action is blocked and an error is returned.
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
/// - Dispute is already finalized (Settled, SellerRefunded, or Released)
/// - Dispute not found in database
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
    // First, fetch the dispute and check if it's already finalized
    let dispute_id_str = dispute_id.to_string();
    let dispute: AdminDispute = sqlx::query_as::<_, AdminDispute>(
        r#"SELECT * FROM admin_disputes WHERE dispute_id = ? LIMIT 1"#,
    )
    .bind(&dispute_id_str)
    .fetch_one(pool)
    .await?;

    // Check if dispute is already finalized - block further actions
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

    // Check if the specific action can be performed
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

    // Parse the related order ID (stored in AdminDispute.id) - this is the ID Mostro expects
    let order_id = Uuid::parse_str(&dispute.id)?;

    // Execute the appropriate action (settle or cancel) using the order ID
    let result = if is_settle {
        execute_admin_settle(&order_id, client, mostro_pubkey).await
    } else {
        execute_admin_cancel(&order_id, client, mostro_pubkey).await
    };

    result?; // Propagate error if action failed

    // Update dispute status in database using the order_id (primary key)
    if is_settle {
        AdminDispute::set_status_settled(pool, &dispute.id).await?;
    } else {
        AdminDispute::set_status_seller_refunded(pool, &dispute.id).await?;
    }

    let action_name = if is_settle {
        "settled (buyer paid)"
    } else {
        "canceled (seller refunded)"
    };

    log::info!("✅ Dispute {} {}!", dispute_id, action_name);
    Ok(())
}
