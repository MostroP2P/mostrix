use nostr_sdk::prelude::*;

/// Validate if a string is a valid npub (Nostr public key)
/// Returns Ok(()) if valid, Err with error message if invalid
pub fn validate_npub(npub_str: &str) -> Result<(), String> {
    let npub = npub_str.trim();
    if npub.is_empty() {
        return Err("Public key cannot be empty".to_string());
    }

    PublicKey::from_bech32(npub).map_err(|_| "Invalid npub key format".to_string())?;

    Ok(())
}

/// Validate if a string is a valid nsec (Nostr secret key)
/// Returns Ok(()) if valid, Err with error message if invalid
pub fn validate_nsec(nsec_str: &str) -> Result<(), String> {
    let nsec = nsec_str.trim();
    if nsec.is_empty() {
        return Err("Secret key cannot be empty".to_string());
    }

    SecretKey::from_bech32(nsec).map_err(|_| "Invalid nsec key format".to_string())?;

    Ok(())
}

/// Validate if a string is a valid hex-encoded Mostro pubkey
/// Example: 627788f4ea6c308b98e5928a632e8220108fcbb7fbcc1270e67582d98eac84ae
pub fn validate_mostro_pubkey(pubkey_str: &str) -> Result<(), String> {
    let key = pubkey_str.trim();
    if key.is_empty() {
        return Err("Mostro pubkey cannot be empty".to_string());
    }

    // Use nostr-sdk parsing to ensure it's a valid public key
    PublicKey::from_hex(key).map_err(|_| {
        "Invalid Mostro pubkey format, expected 64-character hex string".to_string()
    })?;

    Ok(())
}

/// Validate if a relay URL has a valid format (must start with wss://)
pub fn validate_relay(relay_str: &str) -> Result<(), String> {
    let relay = relay_str.trim();
    if relay.is_empty() {
        return Err("Relay URL cannot be empty".to_string());
    }

    if !relay.starts_with("wss://") && !relay.starts_with("ws://") {
        return Err("Relay URL must start with \"wss://\" or \"ws://\"".to_string());
    }

    Ok(())
}

/// Validate if a currency code is valid (non-empty, typically 3 uppercase letters)
pub fn validate_currency(currency_str: &str) -> Result<(), String> {
    let currency = currency_str.trim();
    if currency.is_empty() {
        return Err("Currency code cannot be empty".to_string());
    }

    // Currency codes are typically 3 uppercase letters (e.g., USD, EUR, BTC)
    // But we'll be lenient and just check it's not empty and reasonable length
    if currency.len() > 10 {
        return Err("Currency code is too long (max 10 characters)".to_string());
    }

    Ok(())
}
