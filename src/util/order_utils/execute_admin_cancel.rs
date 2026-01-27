// Execute admin cancel dispute functionality (refunds seller)
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use uuid::Uuid;

use crate::util::dm_utils::send_dm;
use crate::SETTINGS;

/// Cancel a dispute and refund the seller (AdminCancel action).
/// This refunds the full escrow amount to the seller.
///
/// **Important**: This is a low-level function that sends the message to Mostro.
/// Callers should use `execute_finalize_dispute()` instead, which includes
/// finalization state checks and database updates. Direct calls to this function
/// should only be made after verifying `AdminDispute::can_cancel()` returns true.
///
/// Requires admin privileges (admin_privkey must be configured)
///
/// # Arguments
///
/// * `order_id` - The UUID of the order associated with this dispute (Mostro expects this ID)
/// * `client` - The Nostr client for sending messages
/// * `mostro_pubkey` - The public key of the Mostro daemon
///
/// # Returns
///
/// Returns `Ok(())` if the cancel message was successfully sent, or an error
/// if the operation failed.
///
/// # Errors
///
/// This function will return an error if:
/// - Settings are not initialized
/// - Admin private key is not configured
/// - Failed to serialize the message
/// - Failed to send the DM
pub async fn execute_admin_cancel(
    order_id: &Uuid,
    client: &Client,
    mostro_pubkey: PublicKey,
) -> Result<()> {
    // Get admin keys from settings
    let settings = SETTINGS
        .get()
        .ok_or(anyhow::anyhow!("Settings not initialized"))?;

    if settings.admin_privkey.is_empty() {
        return Err(anyhow::anyhow!("Admin private key not configured"));
    }

    let admin_keys = Keys::parse(&settings.admin_privkey)?;

    // Create AdminCancel message
    // No payload needed - just the order ID (Mostro expects the order UUID here)
    let cancel_message =
        Message::new_dispute(Some(*order_id), None, None, Action::AdminCancel, None)
            .as_json()
            .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    // Send the DM using admin keys (signed gift wrap)
    send_dm(
        client,
        Some(&admin_keys),
        &admin_keys,
        &mostro_pubkey,
        cancel_message,
        None,
        false,
    )
    .await?;

    log::info!(
        "✅ Admin cancel (refund seller) message sent for order {}",
        order_id
    );
    Ok(())
}
