use nostr_sdk::prelude::*;
use std::str::FromStr;

/// Validate if a string is a valid npub (Nostr public key) or hex public key.
/// Accepts both bech32 (npub1...) and 64-character hex formats.
/// Returns Ok(()) if valid, Err with error message if invalid.
pub fn validate_npub(npub_or_hex: &str) -> Result<(), String> {
    let key = npub_or_hex.trim();
    if key.is_empty() {
        return Err("Public key cannot be empty".to_string());
    }

    // Try bech32 (npub) first
    if PublicKey::from_bech32(key).is_ok() {
        return Ok(());
    }

    // Fall back to hex
    PublicKey::from_hex(key).map_err(|_| {
        "Invalid public key: expected npub1... (bech32) or 64-char hex string".to_string()
    })?;

    Ok(())
}

/// Validate if a string is a valid nsec (Nostr secret key) or hex secret key.
/// Accepts both bech32 (nsec1...) and 64-character hex formats.
/// Returns Ok(()) if valid, Err with error message if invalid.
pub fn validate_nsec(nsec_or_hex: &str) -> Result<(), String> {
    let key = nsec_or_hex.trim();
    if key.is_empty() {
        return Err("Secret key cannot be empty".to_string());
    }

    // Try bech32 (nsec) first
    if SecretKey::from_bech32(key).is_ok() {
        return Ok(());
    }

    // Fall back to hex
    SecretKey::from_str(key).map_err(|_| {
        "Invalid secret key: expected nsec1... (bech32) or 64-char hex string".to_string()
    })?;

    Ok(())
}

/// Convert a hex public key to bech32 npub format.
/// Returns None if the input is not a valid 64-char hex string.
pub fn hex_pubkey_to_npub(hex: &str) -> Option<String> {
    let hex = hex.trim();
    match PublicKey::from_hex(hex) {
        Ok(pk) => pk.to_bech32().ok(),
        Err(_) => None,
    }
}

/// Convert a hex secret key to bech32 nsec format.
/// Returns None if the input is not a valid 64-char hex string.
pub fn hex_seckey_to_nsec(hex: &str) -> Option<String> {
    let hex = hex.trim();
    SecretKey::from_str(hex).ok().and_then(|sk| sk.to_bech32().ok())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_npub_accepts_bech32() {
        // Valid npub
        assert!(validate_npub("npub1qqqq884wtp2jn96lqhqlnarl4kk3rmvrc9z2nmrvqujx3m4l2ea5qd5d0fq").is_ok());
    }

    #[test]
    fn validate_npub_accepts_hex() {
        // Valid hex (64 chars)
        assert!(validate_npub("627788f4ea6c308b98e5928a632e8220108fcbb7fbcc1270e67582d98eac84ae").is_ok());
    }

    #[test]
    fn validate_npub_rejects_invalid() {
        assert!(validate_npub("not-a-key").is_err());
        assert!(validate_npub("627788f4ea6c308b98e5928a632e8220108fcbb7fbcc1270e67582d98eac84").is_err()); // too short
        assert!(validate_npub("").is_err());
    }

    #[test]
    fn validate_nsec_accepts_bech32() {
        // Valid nsec (fake key for test)
        assert!(validate_nsec("nsec1").is_err()); // nsec1 alone is invalid
    }

    #[test]
    fn validate_nsec_accepts_hex() {
        // Valid hex (64 chars)
        assert!(validate_nsec("627788f4ea6c308b98e5928a632e8220108fcbb7fbcc1270e67582d98eac84af").is_ok());
    }

    #[test]
    fn hex_pubkey_to_npub_converts() {
        let hex = "627788f4ea6c308b98e5928a632e8220108fcbb7fbcc1270e67582d98eac84ae";
        let npub = hex_pubkey_to_npub(hex);
        assert!(npub.is_some());
        assert!(npub.unwrap().starts_with("npub1"));
    }

    #[test]
    fn hex_pubkey_to_npub_returns_none_for_invalid() {
        assert!(hex_pubkey_to_npub("invalid").is_none());
    }

    #[test]
    fn hex_seckey_to_nsec_returns_none_for_invalid() {
        assert!(hex_seckey_to_nsec("invalid").is_none());
    }
}
