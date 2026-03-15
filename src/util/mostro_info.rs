use crate::settings::load_settings_from_disk;
use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use anyhow::{anyhow, Result};
use nostr_sdk::prelude::*;
use std::str::FromStr;

/// Nostr kind for Mostro instance status events.
pub const MOSTRO_INSTANCE_INFO_KIND: u16 = 38385;

/// Age in seconds after which instance info is considered stale (7 days).
const INSTANCE_INFO_STALE_SECS: u64 = 604_800;

/// Human-readable age string for a timestamp (e.g. "2 hours ago", "5 days ago").
pub fn format_instance_info_age(ts: &Timestamp) -> String {
    let age_secs = Timestamp::now().as_u64().saturating_sub(ts.as_u64());
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
                let age_seconds = Timestamp::now().as_u64().saturating_sub(t.as_u64());
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

    // Fetch tests would require a mock or test double for nostr_sdk::Client
    // (e.g. a trait + impl for production and a mock that returns empty events
    // or simulates timeout). Not implemented here to avoid refactoring the public API.
    // Intended behavior:
    // - fetch_returns_none_when_no_event_exists: client returns []; assert Ok(None)
    // - fetch_handles_network_timeout: client returns/timeouts with error; assert Err
}
