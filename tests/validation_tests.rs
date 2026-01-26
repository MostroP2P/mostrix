// Integration tests for validation functions
use mostrix::ui::key_handler::{validate_mostro_pubkey, validate_npub, validate_relay};
use nostr_sdk::prelude::{Keys, ToBech32};

#[test]
fn test_validate_npub_valid() {
    // Generate a valid npub for testing
    let keys = Keys::generate();
    let npub = keys.public_key().to_bech32().unwrap();

    assert!(validate_npub(&npub).is_ok());
}

#[test]
fn test_validate_npub_empty() {
    assert!(validate_npub("").is_err());
    assert!(validate_npub("   ").is_err());
}

#[test]
fn test_validate_npub_invalid_format() {
    assert!(validate_npub("invalid").is_err());
    assert!(validate_npub("npub1invalid").is_err());
    assert!(validate_npub("not_an_npub").is_err());
}

#[test]
fn test_validate_npub_with_whitespace() {
    let keys = Keys::generate();
    let npub = keys.public_key().to_bech32().unwrap();

    // Should trim whitespace and still work
    assert!(validate_npub(&format!("  {}  ", npub)).is_ok());
}

#[test]
fn test_validate_mostro_pubkey_valid() {
    let keys = Keys::generate();
    let hex = keys.public_key().to_string(); // nostr-sdk hex encoding
    assert!(validate_mostro_pubkey(&hex).is_ok());
}

#[test]
fn test_validate_mostro_pubkey_empty() {
    assert!(validate_mostro_pubkey("").is_err());
    assert!(validate_mostro_pubkey("   ").is_err());
}

#[test]
fn test_validate_mostro_pubkey_invalid() {
    // npub should NOT be valid for Mostro pubkey
    let keys = Keys::generate();
    let npub = keys.public_key().to_bech32().unwrap();

    assert!(validate_mostro_pubkey("not_hex").is_err());
    assert!(validate_mostro_pubkey("1234").is_err());
    assert!(validate_mostro_pubkey(&npub).is_err());
}

#[test]
fn test_validate_mostro_pubkey_with_whitespace() {
    let keys = Keys::generate();
    let hex = keys.public_key().to_string();

    assert!(validate_mostro_pubkey(&format!("  {}  ", hex)).is_ok());
}

#[test]
fn test_validate_relay_valid() {
    assert!(validate_relay("wss://relay.damus.io").is_ok());
    assert!(validate_relay("ws://relay.example.com").is_ok());
    assert!(validate_relay("  wss://example.com  ").is_ok());
    assert!(validate_relay("  ws://example.com  ").is_ok());
}

#[test]
fn test_validate_relay_invalid() {
    assert!(validate_relay("").is_err());
    assert!(validate_relay("   ").is_err());
    assert!(validate_relay("https://example.com").is_err());
    assert!(validate_relay("relay.damus.io").is_err());
    assert!(validate_relay("http://example.com").is_err());
}
