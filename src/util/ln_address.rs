//! LNURL-pay metadata checks for Lightning addresses (and raw `lnurl1…` URLs).

use anyhow::Context;
use lnurl::lightning_address::LightningAddress;
use lnurl::lnurl::LnUrl;
use serde_json::Value;
use std::str::FromStr;

const LNURL_HTTP_TIMEOUT_SECS: u64 = 12;

/// Resolve the HTTP URL that returns LNURL-pay metadata JSON (`tag: payRequest`).
fn resolve_lnurlp_metadata_url(address: &str) -> Result<String, anyhow::Error> {
    let trimmed = address.trim();
    if trimmed.to_lowercase().starts_with("lnurl") {
        let lnurl =
            LnUrl::decode(trimmed.to_string()).map_err(|_| anyhow::anyhow!("Invalid LNURL"))?;
        Ok(lnurl.url)
    } else {
        let la = LightningAddress::from_str(trimmed)
            .map_err(|_| anyhow::anyhow!("Invalid Lightning address format"))?;
        Ok(la.lnurlp_url())
    }
}

/// GET the LNURL-pay metadata URL and ensure JSON declares `tag: "payRequest"` (same idea as Mostro `ln_exists`).
pub async fn ln_address_pay_request_reachable(address: &str) -> Result<(), anyhow::Error> {
    let url = resolve_lnurlp_metadata_url(address)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(LNURL_HTTP_TIMEOUT_SECS))
        .user_agent(concat!("mostrix/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .context("build HTTP client")?;

    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("LNURL metadata HTTP {}", response.status());
    }

    let body = response.text().await.context("read LNURL metadata body")?;

    let value: Value = serde_json::from_str(&body).context("LNURL metadata is not valid JSON")?;

    match value.get("tag").and_then(|t| t.as_str()) {
        Some("payRequest") => Ok(()),
        Some(other) => anyhow::bail!("unexpected LNURL tag {:?} (expected payRequest)", other),
        None => anyhow::bail!("LNURL metadata missing tag"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_well_known_lnurlp_from_lightning_address() {
        let url = resolve_lnurlp_metadata_url("user@example.com").unwrap();
        assert_eq!(url, "https://example.com/.well-known/lnurlp/user");
    }
}
