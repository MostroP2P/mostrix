// Integration tests for validation functions
use mostrix::ui::key_handler::validate_npub;
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
