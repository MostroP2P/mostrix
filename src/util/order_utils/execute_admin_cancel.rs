// Execute admin cancel dispute functionality (refunds seller)
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use uuid::Uuid;

use super::BondSlashChoice;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::order_utils::helper::handle_mostro_response;

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
/// * `bond` - Anti-abuse bond slash choice (`to_optional_payload()` on the wire)
/// * `client` - The Nostr client for sending messages
/// * `mostro_pubkey` - The public key of the Mostro daemon
///
/// # Returns
///
/// Returns `Ok(())` if Mostro confirms with `AdminCanceled`, or an error
/// if the operation failed (including `CantDo` from the daemon).
///
/// # Errors
///
/// This function will return an error if:
/// - Settings are not initialized
/// - Admin private key is not configured
/// - Failed to serialize the message
/// - Failed to send or receive the DM
/// - Mostro replies with `CantDo`
/// - Unexpected response action or sender
pub async fn execute_admin_cancel(
    order_id: &Uuid,
    bond: BondSlashChoice,
    admin_keys: &Keys,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<()> {
    let request_id = Uuid::new_v4().as_u128() as u64;
    let payload = bond.to_optional_payload();
    let cancel_message = Message::new_dispute(
        Some(*order_id),
        Some(request_id),
        None,
        Action::AdminCancel,
        payload,
    )
    .as_json()
    .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    let sent_message = send_dm(
        client,
        Some(admin_keys),
        admin_keys,
        &mostro_pubkey,
        cancel_message,
        None,
        mostro_instance,
    );

    let recv_event = wait_for_dm(admin_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;
    let messages = parse_dm_events(recv_event, admin_keys, None).await;
    let Some((response_message, _, sender_pubkey)) = messages.first() else {
        return Err(anyhow::anyhow!("No response received from Mostro"));
    };
    if *sender_pubkey != mostro_pubkey {
        return Err(anyhow::anyhow!("Received response from wrong sender"));
    }

    let inner_message = handle_mostro_response(response_message, request_id)?;
    if inner_message.action != Action::AdminCanceled {
        return Err(anyhow::anyhow!(
            "Unexpected action in response: {:?}",
            inner_message.action
        ));
    }

    log::info!(
        "✅ Admin cancel (refund seller) confirmed for order {} ({})",
        order_id,
        bond.log_context()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::uuid;

    #[test]
    fn admin_cancel_none_omits_payload() {
        let order_id = uuid!("308e1272-d5f4-47e6-bd97-3504baea9c23");
        let msg = Message::new_dispute(
            Some(order_id),
            None,
            None,
            Action::AdminCancel,
            BondSlashChoice::None.to_optional_payload(),
        );
        assert!(msg.verify());
        let json = msg.as_json().expect("serialize");
        assert!(json.contains("\"payload\":null"));
    }
}
