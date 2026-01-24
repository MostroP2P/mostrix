// Execute admin take dispute functionality
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::AdminDispute;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::SETTINGS;

/// Take a dispute as an admin.
///
/// This function sends an `AdminTakeDispute` message to the Mostro daemon
/// and waits for a confirmation response. The admin must have a valid
/// `admin_privkey` configured in settings.
///
/// # Arguments
///
/// * `dispute_id` - The UUID of the dispute to take
/// * `client` - The Nostr client for sending messages
/// * `mostro_pubkey` - The public key of the Mostro daemon
/// * `pool` - The database connection pool for saving dispute information
///
/// # Returns
///
/// Returns `Ok(())` if the dispute was successfully taken and saved, or an error
/// if the operation failed (e.g., admin key not configured, wrong sender,
/// timeout, or database save failure).
///
/// # Errors
///
/// This function will return an error if:
/// - Settings are not initialized
/// - Admin private key is not configured
/// - Failed to serialize the message
/// - Failed to send or receive the DM
/// - Received response from wrong sender
/// - Received response with mismatched action
/// - No response received from Mostro
/// - SolverDisputeInfo not found in response payload
/// - Failed to save dispute to database
pub async fn execute_take_dispute(
    dispute_id: &Uuid,
    client: &Client,
    mostro_pubkey: PublicKey,
    pool: &SqlitePool,
) -> Result<()> {
    // Get admin keys from settings
    let settings = SETTINGS
        .get()
        .ok_or(anyhow::anyhow!("Settings not initialized"))?;

    if settings.admin_privkey.is_empty() {
        return Err(anyhow::anyhow!("Admin private key not configured"));
    }

    let admin_keys = Keys::parse(&settings.admin_privkey)?;

    // Create take dispute message
    let take_dispute_message = Message::new_dispute(
        Some(*dispute_id),
        None,
        None,
        Action::AdminTakeDispute,
        None,
    )
    .as_json()
    .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    // Send the DM using admin keys (signed gift wrap)
    let sent_message = send_dm(
        client,
        Some(&admin_keys),
        &admin_keys,
        &mostro_pubkey,
        take_dispute_message,
        None,
        false,
    );

    // Wait for incoming DM response
    let recv_event = wait_for_dm(client, &admin_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;

    // Parse the incoming DM
    let messages = parse_dm_events(recv_event, &admin_keys, None).await;
    if let Some((response_message, _, sender_pubkey)) = messages.first() {
        if *sender_pubkey != mostro_pubkey {
            return Err(anyhow::anyhow!("Received response from wrong sender"));
        }
        let inner_message = response_message.get_inner_message_kind();
        if inner_message.action == Action::AdminTookDispute {
            // Extract SolverDisputeInfo from payload
            if let Some(Payload::Dispute(id, Some(dispute_info))) = &inner_message.payload {
                // Verify the dispute ID matches
                if *id != *dispute_id {
                    return Err(anyhow::anyhow!(
                        "Dispute ID mismatch: expected {}, got {}",
                        dispute_id,
                        dispute_info.id
                    ));
                }

                // Clone and override status to InProgress before saving - this admin is now resolving it
                let mut dispute_info_clone = dispute_info.clone();
                dispute_info_clone.status = "InProgress".to_string();

                // Save dispute info to database with InProgress status
                // Pass the dispute_id (from the function parameter) to distinguish it from order_id
                if let Err(e) =
                    AdminDispute::new(pool, dispute_info_clone, dispute_id.to_string()).await
                {
                    log::error!("Failed to save dispute to database: {}", e);
                    return Err(anyhow::anyhow!("Failed to save dispute to database: {}", e));
                }

                // Also explicitly update status to ensure it's set (in case of update path)
                // Use dispute_info.id to ensure we're updating the correct record
                if let Err(e) =
                    AdminDispute::set_status_in_progress(pool, &dispute_info.id.to_string()).await
                {
                    log::error!("Failed to update dispute status to InProgress: {}", e);
                    return Err(anyhow::anyhow!(
                        "Failed to update dispute status to InProgress: {}",
                        e
                    ));
                }
                log::info!(
                    "✅ Dispute {} taken successfully and saved to database with InProgress status!",
                    dispute_info.id
                );
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Received AdminTookDispute response but SolverDisputeInfo not found in payload"
                ))
            }
        } else {
            Err(anyhow::anyhow!(
                "Received response with mismatched action. Expected: {:?}, Got: {:?}",
                Action::AdminTookDispute,
                inner_message.action
            ))
        }
    } else {
        Err(anyhow::anyhow!("No response received from Mostro"))
    }
}
