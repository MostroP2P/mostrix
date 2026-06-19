// Execute admin add solver functionality
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use uuid::Uuid;

use crate::shared::permissions::SolverPermission;
use crate::ui::key_handler::hex_pubkey_to_npub;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::order_utils::helper::handle_mostro_response;

/// Normalize a solver pubkey to bech32 (npub) format.
/// If the input is already bech32, returns it as-is.
/// If the input is a 64-char hex string, converts it to npub.
/// Otherwise returns the original string (Mostro will handle validation).
fn normalize_solver_pubkey(pubkey: &str) -> String {
    let trimmed = pubkey.trim();
    // Already bech32
    if trimmed.starts_with("npub1") {
        return trimmed.to_string();
    }
    // Try hex -> bech32
    if let Some(npub) = hex_pubkey_to_npub(trimmed) {
        return npub;
    }
    // Fall back to original (Mostro will validate)
    trimmed.to_string()
}

fn build_solver_payload_text(pubkey: &str, permission: SolverPermission) -> String {
    let normalized_pubkey = normalize_solver_pubkey(pubkey);
    match permission {
        SolverPermission::Read => format!("{normalized_pubkey}:read"),
        SolverPermission::ReadWrite => normalized_pubkey,
    }
}

/// Add a new solver to the Mostro network
/// Requires admin privileges (admin_privkey must be configured)
pub async fn execute_admin_add_solver(
    solver_pubkey: &str,
    permission: SolverPermission,
    admin_keys: &Keys,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<()> {
    let request_id = Uuid::new_v4().as_u128() as u64;

    let payload_text = build_solver_payload_text(solver_pubkey, permission);

    // Create AddSolver message
    let add_solver_message = Message::new_dispute(
        Some(Uuid::new_v4()),
        Some(request_id),
        None,
        Action::AdminAddSolver,
        Some(Payload::TextMessage(payload_text)),
    )
    .as_json()
    .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    // Send the DM using admin keys (signed gift wrap)
    // Note: Following the example pattern, we don't wait for a response
    let sent_message = send_dm(
        client,
        Some(admin_keys),
        admin_keys,
        &mostro_pubkey,
        add_solver_message,
        None,
        mostro_instance,
    );

    // Wait for Mostro answer and parse it.
    let recv_event = wait_for_dm(admin_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;
    let messages = parse_dm_events(recv_event, admin_keys, None).await;
    let Some((response_message, _, sender_pubkey)) = messages.first() else {
        return Err(anyhow::anyhow!("No response received from Mostro"));
    };
    if *sender_pubkey != mostro_pubkey {
        return Err(anyhow::anyhow!("Received response from wrong sender"));
    }

    let inner_message = handle_mostro_response(response_message, request_id)?;
    if inner_message.action != Action::AdminAddSolver {
        return Err(anyhow::anyhow!(
            "Unexpected action in response: {:?}",
            inner_message.action
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_solver_payload_uses_read_suffix_for_read_permission() {
        let npub = "npub1qqq884wtp2jn96lqhqlnarl4kk3rmvrc9z2nmrvqujx3m4l2ea5qd5d0fq";
        let payload = build_solver_payload_text(npub, SolverPermission::Read);
        assert_eq!(payload, format!("{npub}:read"));
    }

    #[test]
    fn add_solver_payload_keeps_default_format_for_read_write_permission() {
        let npub = "npub1qqq884wtp2jn96lqhqlnarl4kk3rmvrc9z2nmrvqujx3m4l2ea5qd5d0fq";
        let payload = build_solver_payload_text(npub, SolverPermission::ReadWrite);
        assert_eq!(payload, npub);
    }
}
