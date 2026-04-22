// Execute admin add solver functionality
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::str::FromStr;
use uuid::Uuid;

use crate::util::dm_utils::send_dm;
use crate::util::mostro_info::MostroInstanceInfo;
use crate::SETTINGS;

/// Convert a hex public key to bech32 npub format.
/// Returns None if the input is not a valid 64-char hex string.
fn hex_pubkey_to_npub(hex: &str) -> Option<String> {
    let hex = hex.trim();
    match PublicKey::from_hex(hex) {
        Ok(pk) => pk.to_bech32().ok(),
        Err(_) => None,
    }
}

/// Normalize a solver pubkey to bech32 (npub) format.
/// If the input is already bech32, returns it as-is.
/// If the input is a 64-char hex string, converts it to npub.
/// Otherwise returns the original string (Mostro will handle validation).
fn normalize_solver_pubkey(pubkey: &str) -> String {
    let trimmed = pubkey.trim();
    // Already bech32
    if trimmed.starts_with("npub1") || trimmed.starts_with("nsec1") {
        return trimmed.to_string();
    }
    // Try hex -> bech32
    if let Some(npub) = hex_pubkey_to_npub(trimmed) {
        return npub;
    }
    // Fall back to original (Mostro will validate)
    trimmed.to_string()
}

/// Add a new solver to the Mostro network
/// Requires admin privileges (admin_privkey must be configured)
pub async fn execute_admin_add_solver(
    solver_pubkey: &str,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<()> {
    // Get admin keys from settings
    let settings = SETTINGS
        .get()
        .ok_or(anyhow::anyhow!("Settings not initialized"))?;

    if settings.admin_privkey.is_empty() {
        return Err(anyhow::anyhow!("Admin private key not configured"));
    }

    let admin_keys = Keys::parse(&settings.admin_privkey)?;

    // Normalize solver pubkey to bech32 (hex input → npub conversion)
    let normalized_pubkey = normalize_solver_pubkey(solver_pubkey);

    // Create AddSolver message
    let add_solver_message = Message::new_dispute(
        Some(Uuid::new_v4()),
        None,
        None,
        Action::AdminAddSolver,
        Some(Payload::TextMessage(normalized_pubkey)),
    )
    .as_json()
    .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    // Send the DM using admin keys (signed gift wrap)
    // Note: Following the example pattern, we don't wait for a response
    send_dm(
        client,
        Some(&admin_keys),
        &admin_keys,
        &mostro_pubkey,
        add_solver_message,
        None,
        false,
        mostro_instance,
    )
    .await?;

    Ok(())
}
