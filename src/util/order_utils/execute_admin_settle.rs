// Execute admin settle dispute functionality (pays buyer)
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use uuid::Uuid;

use crate::util::dm_utils::send_dm;
use crate::SETTINGS;

/// Settle a dispute in favor of the buyer (AdminSettle action).
/// This pays the full escrow amount to the buyer.
///
/// **Important**: This is a low-level function that sends the message to Mostro.
/// Callers should use `execute_finalize_dispute()` instead, which includes
/// finalization state checks and database updates. Direct calls to this function
/// should only be made after verifying `AdminDispute::can_settle()` returns true.
///
/// Requires admin privileges (admin_privkey must be configured)
///
/// # Arguments
///
/// * `dispute_id` - The UUID of the dispute to settle
/// * `client` - The Nostr client for sending messages
/// * `mostro_pubkey` - The public key of the Mostro daemon
///
/// # Returns
///
/// Returns `Ok(())` if the settle message was successfully sent, or an error
/// if the operation failed.
///
/// # Errors
///
/// This function will return an error if:
/// - Settings are not initialized
/// - Admin private key is not configured
/// - Failed to serialize the message
/// - Failed to send the DM
pub async fn execute_admin_settle(
    dispute_id: &Uuid,
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

    // Create AdminSettle message
    // No payload needed - just the dispute ID
    let settle_message =
        Message::new_dispute(Some(*dispute_id), None, None, Action::AdminSettle, None)
            .as_json()
            .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    // Send the DM using admin keys (signed gift wrap)
    send_dm(
        client,
        Some(&admin_keys),
        &admin_keys,
        &mostro_pubkey,
        settle_message,
        None,
        false,
    )
    .await?;

    log::info!(
        "✅ Admin settle (pay buyer) message sent for dispute {}",
        dispute_id
    );
    Ok(())
}
