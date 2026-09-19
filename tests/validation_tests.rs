// Integration tests for validation functions
use mostrix::ui::key_handler::{
    normalize_blossom_server_url, normalize_relay_url, validate_blossom_server,
    validate_mostro_pubkey, validate_npub, validate_relay,
};
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
fn test_validate_mostro_pubkey_accepts_npub_and_rejects_invalid() {
    let keys = Keys::generate();
    let npub = keys.public_key().to_bech32().unwrap();

    assert!(validate_mostro_pubkey(&npub).is_ok());
    assert!(validate_mostro_pubkey("not_hex").is_err());
    assert!(validate_mostro_pubkey("1234").is_err());
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

#[test]
fn test_normalize_relay_url_prepends_wss() {
    // Bare host gets the default wss:// scheme.
    assert_eq!(
        normalize_relay_url("relay.damus.io"),
        "wss://relay.damus.io"
    );
    // Whitespace is trimmed before prefixing.
    assert_eq!(
        normalize_relay_url("  relay.example.com  "),
        "wss://relay.example.com"
    );
}

#[test]
fn test_normalize_relay_url_preserves_explicit_scheme() {
    assert_eq!(
        normalize_relay_url("wss://relay.damus.io"),
        "wss://relay.damus.io"
    );
    assert_eq!(
        normalize_relay_url("ws://relay.example.com"),
        "ws://relay.example.com"
    );
    assert_eq!(normalize_relay_url("  ws://a.b  "), "ws://a.b");
}

#[test]
fn test_normalize_relay_url_empty() {
    assert_eq!(normalize_relay_url(""), "");
    assert_eq!(normalize_relay_url("   "), "");
}

#[test]
fn test_normalize_relay_url_lowercases_scheme() {
    // Scheme is accepted case-insensitively and canonicalized to lowercase.
    assert_eq!(
        normalize_relay_url("WSS://relay.example.com"),
        "wss://relay.example.com"
    );
    assert_eq!(
        normalize_relay_url("  Ws://relay.example.com  "),
        "ws://relay.example.com"
    );
}

#[test]
fn test_validate_relay_accepts_uppercase_scheme() {
    assert!(validate_relay("WSS://relay.damus.io").is_ok());
    assert!(validate_relay(&normalize_relay_url("WSS://relay.damus.io")).is_ok());
}

#[test]
fn test_validate_relay_rejects_missing_host() {
    assert!(validate_relay("wss:///events").is_err());
    assert!(validate_relay("ws://").is_err());
}

#[test]
fn test_normalized_bare_url_passes_validation() {
    // The Add Relay flow normalizes before validating.
    assert!(validate_relay(&normalize_relay_url("relay.damus.io")).is_ok());
}

#[test]
fn test_validate_blossom_server_valid() {
    assert!(validate_blossom_server("https://cdn.hzrd149.com").is_ok());
    assert!(validate_blossom_server("  https://nostr.download  ").is_ok());
}

#[test]
fn test_validate_blossom_server_invalid() {
    assert!(validate_blossom_server("").is_err());
    assert!(validate_blossom_server("http://cdn.hzrd149.com").is_err());
    assert!(validate_blossom_server("wss://relay.example").is_err());
    assert!(validate_blossom_server("https://").is_err());
}

#[test]
fn test_normalize_blossom_server_url_bare_host() {
    assert_eq!(
        normalize_blossom_server_url("cdn.hzrd149.com"),
        "https://cdn.hzrd149.com"
    );
    assert!(validate_blossom_server(&normalize_blossom_server_url("cdn.hzrd149.com")).is_ok());
}
