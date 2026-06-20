use crate::settings::load_settings_from_disk;
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use anyhow::{anyhow, Result};
use mostro_core::prelude::{Action, Transport};
use nostr_sdk::prelude::*;
use std::str::FromStr;

/// Nostr kind for Mostro instance status events.
pub const MOSTRO_INSTANCE_INFO_KIND: u16 = 38385;

/// Age in seconds after which instance info is considered stale (7 days).
const INSTANCE_INFO_STALE_SECS: u64 = 604_800;

/// Human-readable age string for a timestamp (e.g. "2 hours ago", "5 days ago").
pub fn format_instance_info_age(ts: &Timestamp) -> String {
    let age_secs = Timestamp::now().as_secs().saturating_sub(ts.as_secs());
    if age_secs < 60 {
        "just now".to_string()
    } else if age_secs < 3600 {
        let m = age_secs / 60;
        format!("{} minute{} ago", m, if m == 1 { "" } else { "s" })
    } else if age_secs < 86400 {
        let h = age_secs / 3600;
        format!("{} hour{} ago", h, if h == 1 { "" } else { "s" })
    } else {
        let d = age_secs / 86400;
        format!("{} day{} ago", d, if d == 1 { "" } else { "s" })
    }
}

/// Structured representation of a Mostro instance info event (kind 38385).
///
/// All fields are optional because different instances may omit some tags.
#[derive(Clone, Debug, Default)]
pub struct MostroInstanceInfo {
    /// When the instance info event was created (set by fetch, not from tags).
    pub last_updated: Option<Timestamp>,
    pub mostro_version: Option<String>,
    pub mostro_commit_hash: Option<String>,
    pub max_order_amount: Option<i64>,
    pub min_order_amount: Option<i64>,
    pub expiration_hours: Option<u64>,
    pub expiration_seconds: Option<u64>,
    pub fiat_currencies_accepted: Vec<String>,
    pub max_orders_per_response: Option<u32>,
    pub fee: Option<f64>,
    pub pow: Option<u32>,
    /// First-contact PoW toll on v2 (kind 38385 tag `pow_first_contact`). When absent, daemon
    /// defaults to [`Self::pow`]; Mostrix mirrors that in [`effective_pow_first_contact_from_instance`].
    pub pow_first_contact: Option<u32>,
    /// Wire transport version from kind-38385 tag `protocol_version` (`"1"` / `"2"`).
    pub protocol_version: Option<u8>,
    /// Anti-abuse bond feature flag from kind-38385 tag `bond_enabled` (`"true"` / `"false"`).
    pub bond_enabled: Option<bool>,
    pub hold_invoice_expiration_window: Option<u64>,
    pub hold_invoice_cltv_delta: Option<u32>,
    pub invoice_expiration_window: Option<u64>,
    pub lnd_version: Option<String>,
    pub lnd_node_pubkey: Option<String>,
    pub lnd_commit_hash: Option<String>,
    pub lnd_node_alias: Option<String>,
    pub lnd_chains: Vec<String>,
    pub lnd_networks: Vec<String>,
    pub lnd_uris: Vec<String>,
}

impl MostroInstanceInfo {
    /// Returns true if this info has a last_updated timestamp older than 7 days.
    pub fn is_stale(&self) -> bool {
        self.last_updated
            .map(|t| {
                let age_seconds = Timestamp::now().as_secs().saturating_sub(t.as_secs());
                age_seconds > INSTANCE_INFO_STALE_SECS
            })
            .unwrap_or(true)
    }
    fn parse_i64(value: &str) -> Option<i64> {
        value.parse::<i64>().ok()
    }

    fn parse_u64(value: &str) -> Option<u64> {
        value.parse::<u64>().ok()
    }

    fn parse_u32(value: &str) -> Option<u32> {
        value.parse::<u32>().ok()
    }

    fn parse_u8(value: &str) -> Option<u8> {
        value.parse::<u8>().ok()
    }

    fn parse_f64(value: &str) -> Option<f64> {
        value.parse::<f64>().ok()
    }

    fn split_csv(value: &str) -> Vec<String> {
        value
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }
}

fn clamp_pow_bits(bits: Option<u32>) -> u8 {
    bits.map(|n| (n.min(u8::MAX as u32)) as u8).unwrap_or(0)
}

/// NIP-13 difficulty bits for outbound events from cached Mostro instance info (kind 38385 tag `pow`).
/// Returns `0` when info is missing or the tag is absent. Values above `u8::MAX` clamp to 255.
pub fn nostr_pow_from_instance(instance: Option<&MostroInstanceInfo>) -> u8 {
    clamp_pow_bits(instance.and_then(|i| i.pow))
}

/// Effective first-contact PoW bits (mirrors Mostro daemon `effective_pow_first_contact`).
///
/// Uses tag `pow_first_contact` when present; otherwise falls back to base `pow`.
pub fn effective_pow_first_contact_from_instance(instance: Option<&MostroInstanceInfo>) -> u8 {
    match instance {
        Some(i) => clamp_pow_bits(i.pow_first_contact.or(i.pow)),
        None => 0,
    }
}

/// Protocol actions that introduce a new trade key to Mostro (v2 spam gate first-contact lane).
pub fn is_v2_first_contact_protocol_action(action: &Action) -> bool {
    matches!(
        action,
        Action::NewOrder | Action::TakeBuy | Action::TakeSell
    )
}

/// NIP-13 bits for a protocol DM toward Mostro.
///
/// v1 GiftWrap: always base instance `pow` (daemon spam gate is v2-only).
/// v2 NIP-44: first-contact actions use `max(pow, pow_first_contact)` so new order/take
/// clears the daemon's stiffer toll when operators set `pow_first_contact` above `pow`.
pub fn nostr_pow_for_protocol_dm(instance: Option<&MostroInstanceInfo>, action: &Action) -> u8 {
    let base = nostr_pow_from_instance(instance);
    if transport_from_instance(instance) == Transport::Nip44Direct
        && is_v2_first_contact_protocol_action(action)
    {
        base.max(effective_pow_first_contact_from_instance(instance))
    } else {
        base
    }
}

/// Whether the connected Mostro instance has anti-abuse bonds enabled (kind 38385 `bond_enabled`).
///
/// Treats missing instance info or an absent/false tag as disabled so slash UI stays hidden
/// until the daemon explicitly advertises `"true"`.
pub fn instance_bonds_enabled(instance: Option<&MostroInstanceInfo>) -> bool {
    instance.and_then(|i| i.bond_enabled) == Some(true)
}

/// Resolve the Mostro protocol wire transport from cached instance info.
///
/// Missing or unknown `protocol_version` defaults to legacy v1 GiftWrap.
pub fn transport_from_instance(info: Option<&MostroInstanceInfo>) -> Transport {
    match info.and_then(|i| i.protocol_version) {
        Some(2) => Transport::Nip44Direct,
        _ => Transport::GiftWrap,
    }
}

fn parse_bond_enabled(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

/// Build a `MostroInstanceInfo` from the tags of a kind 38385 event.
///
/// Unknown tags are ignored. Missing tags simply leave the corresponding
/// fields as `None` or empty collections.
pub fn mostro_info_from_tags(tags: Tags) -> Result<MostroInstanceInfo> {
    let mut info = MostroInstanceInfo::default();

    for tag in tags {
        let values = tag.to_vec();
        if values.is_empty() {
            continue;
        }

        let key = values[0].as_str();
        let value = values.get(1).map(|s| s.as_str()).unwrap_or_default();

        match key {
            "mostro_version" => {
                info.mostro_version = Some(value.to_string());
            }
            "mostro_commit_hash" => {
                info.mostro_commit_hash = Some(value.to_string());
            }
            "max_order_amount" => {
                info.max_order_amount = MostroInstanceInfo::parse_i64(value);
            }
            "min_order_amount" => {
                info.min_order_amount = MostroInstanceInfo::parse_i64(value);
            }
            "expiration_hours" => {
                info.expiration_hours = MostroInstanceInfo::parse_u64(value);
            }
            "expiration_seconds" => {
                info.expiration_seconds = MostroInstanceInfo::parse_u64(value);
            }
            "fiat_currencies_accepted" => {
                info.fiat_currencies_accepted = MostroInstanceInfo::split_csv(value);
            }
            "max_orders_per_response" => {
                info.max_orders_per_response = MostroInstanceInfo::parse_u32(value);
            }
            "fee" => {
                info.fee = MostroInstanceInfo::parse_f64(value);
            }
            "pow" => {
                info.pow = MostroInstanceInfo::parse_u32(value);
            }
            "pow_first_contact" => {
                info.pow_first_contact = MostroInstanceInfo::parse_u32(value);
            }
            "protocol_version" => {
                info.protocol_version = MostroInstanceInfo::parse_u8(value);
            }
            "bond_enabled" => {
                info.bond_enabled = parse_bond_enabled(value);
            }
            "hold_invoice_expiration_window" => {
                info.hold_invoice_expiration_window = MostroInstanceInfo::parse_u64(value);
            }
            "hold_invoice_cltv_delta" => {
                info.hold_invoice_cltv_delta = MostroInstanceInfo::parse_u32(value);
            }
            "invoice_expiration_window" => {
                info.invoice_expiration_window = MostroInstanceInfo::parse_u64(value);
            }
            "lnd_version" => {
                info.lnd_version = Some(value.to_string());
            }
            "lnd_node_pubkey" => {
                info.lnd_node_pubkey = Some(value.to_string());
            }
            "lnd_commit_hash" => {
                info.lnd_commit_hash = Some(value.to_string());
            }
            "lnd_node_alias" => {
                info.lnd_node_alias = Some(value.to_string());
            }
            "lnd_chains" => {
                info.lnd_chains = MostroInstanceInfo::split_csv(value);
            }
            "lnd_networks" => {
                info.lnd_networks = MostroInstanceInfo::split_csv(value);
            }
            "lnd_uris" => {
                info.lnd_uris = MostroInstanceInfo::split_csv(value);
            }
            _ => {}
        }
    }

    Ok(info)
}

/// Fetch the latest Mostro instance info event for the given Mostro pubkey.
///
/// Filters on:
/// - kind 38385 (Mostro instance status)
/// - author = Mostro pubkey
/// - `d` tag / identifier = Mostro pubkey
pub async fn fetch_mostro_instance_info(
    client: &Client,
    mostro_pubkey: PublicKey,
) -> Result<Option<MostroInstanceInfo>> {
    let filter = Filter::new()
        .author(mostro_pubkey)
        .kind(nostr_sdk::Kind::Custom(MOSTRO_INSTANCE_INFO_KIND))
        .identifier(mostro_pubkey.to_string())
        .limit(1);

    let events = client
        .fetch_events(filter, FETCH_EVENTS_TIMEOUT)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch Mostro instance info from relays: {}", e))?;

    let event = match events.iter().next() {
        Some(ev) => ev,
        None => return Ok(None),
    };

    let mut info = mostro_info_from_tags(event.tags.clone())?;
    info.last_updated = Some(event.created_at);
    Ok(Some(info))
}

/// Convenience helper: load the latest settings from disk, parse the configured
/// Mostro pubkey, and fetch instance info for that pubkey from the relays.
pub async fn fetch_mostro_instance_info_from_settings(
    client: &Client,
) -> Result<Option<MostroInstanceInfo>> {
    let settings =
        load_settings_from_disk().map_err(|e| anyhow!("Failed to load settings: {}", e))?;
    let mostro_pubkey = PublicKey::from_str(&settings.mostro_pubkey)
        .map_err(|e| anyhow!("Invalid Mostro pubkey in settings: {}", e))?;
    fetch_mostro_instance_info(client, mostro_pubkey).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mostro_core::prelude::Transport;
    use nostr_sdk::prelude::{Tag, Tags};

    #[test]
    fn split_csv_trims_and_ignores_empty_values() {
        let result = MostroInstanceInfo::split_csv("USD, EUR, ,ARS ,  ");
        assert_eq!(
            result,
            vec!["USD".to_string(), "EUR".to_string(), "ARS".to_string()]
        );
    }

    #[test]
    fn parse_helpers_handle_invalid_input() {
        assert_eq!(MostroInstanceInfo::parse_i64("123"), Some(123));
        assert_eq!(MostroInstanceInfo::parse_i64("not-a-number"), None);

        assert_eq!(MostroInstanceInfo::parse_u64("42"), Some(42));
        assert_eq!(MostroInstanceInfo::parse_u64("oops"), None);

        assert_eq!(MostroInstanceInfo::parse_u32("7"), Some(7));
        assert_eq!(MostroInstanceInfo::parse_u32("bad"), None);

        assert_eq!(MostroInstanceInfo::parse_f64("0.006"), Some(0.006));
        assert_eq!(MostroInstanceInfo::parse_f64("NaN?"), None);
    }

    #[test]
    fn parse_handles_empty_tags() {
        let tags = Tags::new();
        let result = mostro_info_from_tags(tags).unwrap();
        assert_eq!(result.fiat_currencies_accepted, Vec::<String>::new());
        assert!(result.mostro_version.is_none());
        assert!(result.max_order_amount.is_none());
    }

    #[test]
    fn parse_handles_malformed_tags() {
        let mut tags = Tags::new();
        tags.push(Tag::parse(["max_order_amount", "not-a-number"]).unwrap());
        tags.push(Tag::parse(["min_order_amount", "nope"]).unwrap());
        tags.push(Tag::parse(["fee", "invalid"]).unwrap());
        tags.push(Tag::parse(["expiration_hours"]).unwrap()); // missing value
        tags.push(Tag::parse(["unknown_tag", "ignored"]).unwrap());

        let result = mostro_info_from_tags(tags).unwrap();
        assert!(result.max_order_amount.is_none());
        assert!(result.min_order_amount.is_none());
        assert!(result.fee.is_none());
        assert!(result.expiration_hours.is_none());
        assert_eq!(result.fiat_currencies_accepted, Vec::<String>::new());
    }

    #[test]
    fn parse_empty_fiat_currencies_returns_empty_list() {
        let mut tags = Tags::new();
        tags.push(Tag::parse(["fiat_currencies_accepted", ""]).unwrap());
        let result = mostro_info_from_tags(tags).unwrap();
        assert_eq!(result.fiat_currencies_accepted, Vec::<String>::new());
    }

    #[test]
    fn parse_valid_tags_roundtrip() {
        let mut tags = Tags::new();
        tags.push(Tag::parse(["mostro_version", "0.1.0"]).unwrap());
        tags.push(Tag::parse(["max_order_amount", "1000000"]).unwrap());
        tags.push(Tag::parse(["fiat_currencies_accepted", "USD,EUR"]).unwrap());
        let result = mostro_info_from_tags(tags).unwrap();
        assert_eq!(result.mostro_version.as_deref(), Some("0.1.0"));
        assert_eq!(result.max_order_amount, Some(1_000_000));
        assert_eq!(result.fiat_currencies_accepted, vec!["USD", "EUR"]);
    }

    #[test]
    fn nostr_pow_from_instance_none_or_missing_tag_is_zero() {
        assert_eq!(nostr_pow_from_instance(None), 0);
        assert_eq!(
            nostr_pow_from_instance(Some(&MostroInstanceInfo::default())),
            0
        );
    }

    #[test]
    fn parse_bond_enabled_tag_values() {
        assert_eq!(parse_bond_enabled("true"), Some(true));
        assert_eq!(parse_bond_enabled("TRUE"), Some(true));
        assert_eq!(parse_bond_enabled("false"), Some(false));
        assert_eq!(parse_bond_enabled("invalid"), None);
    }

    #[test]
    fn instance_bonds_enabled_requires_explicit_true() {
        assert!(!instance_bonds_enabled(None));
        assert!(!instance_bonds_enabled(
            Some(&MostroInstanceInfo::default())
        ));
        assert!(!instance_bonds_enabled(Some(&MostroInstanceInfo {
            bond_enabled: Some(false),
            ..Default::default()
        })));
        assert!(instance_bonds_enabled(Some(&MostroInstanceInfo {
            bond_enabled: Some(true),
            ..Default::default()
        })));
    }

    #[test]
    fn parse_bond_enabled_from_tags() {
        let mut tags = Tags::new();
        tags.push(Tag::parse(["bond_enabled", "true"]).unwrap());
        let result = mostro_info_from_tags(tags).unwrap();
        assert_eq!(result.bond_enabled, Some(true));

        let mut tags = Tags::new();
        tags.push(Tag::parse(["bond_enabled", "false"]).unwrap());
        let result = mostro_info_from_tags(tags).unwrap();
        assert_eq!(result.bond_enabled, Some(false));
    }

    #[test]
    fn nostr_pow_from_instance_uses_tag_and_clamps() {
        assert_eq!(
            nostr_pow_from_instance(Some(&MostroInstanceInfo {
                pow: Some(0),
                ..Default::default()
            })),
            0
        );
        assert_eq!(
            nostr_pow_from_instance(Some(&MostroInstanceInfo {
                pow: Some(8),
                ..Default::default()
            })),
            8
        );
        assert_eq!(
            nostr_pow_from_instance(Some(&MostroInstanceInfo {
                pow: Some(u32::MAX),
                ..Default::default()
            })),
            u8::MAX
        );
    }

    #[test]
    fn parse_protocol_version_from_tags() {
        let mut tags = Tags::new();
        tags.push(Tag::parse(["protocol_version", "2"]).unwrap());
        let result = mostro_info_from_tags(tags).unwrap();
        assert_eq!(result.protocol_version, Some(2));

        let mut tags = Tags::new();
        tags.push(Tag::parse(["protocol_version", "1"]).unwrap());
        let result = mostro_info_from_tags(tags).unwrap();
        assert_eq!(result.protocol_version, Some(1));

        let mut tags = Tags::new();
        tags.push(Tag::parse(["protocol_version", "bad"]).unwrap());
        let result = mostro_info_from_tags(tags).unwrap();
        assert!(result.protocol_version.is_none());
    }

    #[test]
    fn transport_from_instance_resolves_protocol_version() {
        assert_eq!(transport_from_instance(None), Transport::GiftWrap);
        assert_eq!(
            transport_from_instance(Some(&MostroInstanceInfo::default())),
            Transport::GiftWrap
        );
        assert_eq!(
            transport_from_instance(Some(&MostroInstanceInfo {
                protocol_version: Some(1),
                ..Default::default()
            })),
            Transport::GiftWrap
        );
        assert_eq!(
            transport_from_instance(Some(&MostroInstanceInfo {
                protocol_version: Some(2),
                ..Default::default()
            })),
            Transport::Nip44Direct
        );
        assert_eq!(
            transport_from_instance(Some(&MostroInstanceInfo {
                protocol_version: Some(99),
                ..Default::default()
            })),
            Transport::GiftWrap
        );
    }

    #[test]
    fn is_v2_first_contact_protocol_action_matches_new_order_and_takes() {
        assert!(is_v2_first_contact_protocol_action(&Action::NewOrder));
        assert!(is_v2_first_contact_protocol_action(&Action::TakeBuy));
        assert!(is_v2_first_contact_protocol_action(&Action::TakeSell));
        assert!(!is_v2_first_contact_protocol_action(&Action::AddInvoice));
        assert!(!is_v2_first_contact_protocol_action(&Action::PayInvoice));
    }

    #[test]
    fn parse_pow_first_contact_from_tags() {
        let mut tags = Tags::new();
        tags.push(Tag::parse(["pow", "8"]).unwrap());
        tags.push(Tag::parse(["pow_first_contact", "16"]).unwrap());
        let result = mostro_info_from_tags(tags).unwrap();
        assert_eq!(result.pow, Some(8));
        assert_eq!(result.pow_first_contact, Some(16));
    }

    #[test]
    fn effective_pow_first_contact_falls_back_to_base_pow() {
        assert_eq!(effective_pow_first_contact_from_instance(None), 0);
        assert_eq!(
            effective_pow_first_contact_from_instance(Some(&MostroInstanceInfo {
                pow: Some(8),
                ..Default::default()
            })),
            8
        );
        assert_eq!(
            effective_pow_first_contact_from_instance(Some(&MostroInstanceInfo {
                pow: Some(8),
                pow_first_contact: Some(16),
                ..Default::default()
            })),
            16
        );
    }

    #[test]
    fn nostr_pow_for_protocol_dm_v2_first_contact_uses_max_toll() {
        let info = MostroInstanceInfo {
            pow: Some(8),
            pow_first_contact: Some(16),
            protocol_version: Some(2),
            ..Default::default()
        };
        assert_eq!(
            nostr_pow_for_protocol_dm(Some(&info), &Action::NewOrder),
            16
        );
        assert_eq!(nostr_pow_for_protocol_dm(Some(&info), &Action::TakeBuy), 16);
        assert_eq!(
            nostr_pow_for_protocol_dm(Some(&info), &Action::AddInvoice),
            8
        );
    }

    #[test]
    fn nostr_pow_for_protocol_dm_v1_ignores_first_contact_toll() {
        let info = MostroInstanceInfo {
            pow: Some(8),
            pow_first_contact: Some(16),
            protocol_version: Some(1),
            ..Default::default()
        };
        assert_eq!(nostr_pow_for_protocol_dm(Some(&info), &Action::NewOrder), 8);
    }

    // Fetch tests would require a mock or test double for nostr_sdk::Client
    // (e.g. a trait + impl for production and a mock that returns empty events
    // or simulates timeout). Not implemented here to avoid refactoring the public API.
    // Intended behavior:
    // - fetch_returns_none_when_no_event_exists: client returns []; assert Ok(None)
    // - fetch_handles_network_timeout: client returns/timeouts with error; assert Err
}
