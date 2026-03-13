use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use anyhow::Result;
use nostr_sdk::prelude::*;

/// Nostr kind for Mostro instance status events.
pub const MOSTRO_INSTANCE_INFO_KIND: u16 = 38385;

/// Structured representation of a Mostro instance info event (kind 38385).
///
/// All fields are optional because different instances may omit some tags.
#[derive(Clone, Debug, Default)]
pub struct MostroInstanceInfo {
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

    let info = mostro_info_from_tags(event.tags.clone())?;
    Ok(Some(info))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
