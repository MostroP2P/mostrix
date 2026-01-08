use nostr_sdk::prelude::*;

/// Validate if a string is a valid npub (Nostr public key)
/// Returns Ok(()) if valid, Err with error message if invalid
pub fn validate_npub(npub_str: &str) -> Result<(), String> {
    if npub_str.trim().is_empty() {
        return Err("Public key cannot be empty".to_string());
    }

    PublicKey::from_bech32(npub_str.trim()).map_err(|_| "Invalid key format".to_string())?;

    Ok(())
}
