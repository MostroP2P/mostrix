//! Trusted Mostro instance catalog (mirrored from Mostro Mobile `communities.dart`).
//!
//! Settings uses this list for the instance picker; users can still enter a
//! custom npub/hex when it does not match a trusted row.

use std::sync::OnceLock;

use serde::Deserialize;

const MOSTRO_INSTANCES_JSON: &str = include_str!("mostro_instances.json");

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct TrustedMostroInstance {
    pub pubkey: String,
    pub region: String,
}

fn catalog() -> &'static [TrustedMostroInstance] {
    static CATALOG: OnceLock<Vec<TrustedMostroInstance>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(MOSTRO_INSTANCES_JSON)
                .expect("bundled mostro_instances.json must be valid JSON")
        })
        .as_slice()
}

/// All trusted Mostro instances (region label + hex pubkey).
#[must_use]
pub fn trusted_instances() -> &'static [TrustedMostroInstance] {
    catalog()
}

/// Case-insensitive filter on region label or pubkey substring.
#[must_use]
pub fn filter_trusted(query: &str) -> Vec<&'static TrustedMostroInstance> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return catalog().iter().collect();
    }
    catalog()
        .iter()
        .filter(|n| {
            n.region.to_ascii_lowercase().contains(&q) || n.pubkey.to_ascii_lowercase().contains(&q)
        })
        .collect()
}

/// Short display for list rows: region + truncated pubkey.
#[must_use]
pub fn row_label(instance: &TrustedMostroInstance) -> String {
    let pk = instance.pubkey.trim();
    let short = if pk.len() > 16 {
        format!("{}…{}", &pk[..8], &pk[pk.len() - 6..])
    } else {
        pk.to_string()
    };
    format!("{}  ({short})", instance.region)
}

/// Whether `hex` matches a trusted pubkey (case-insensitive).
#[must_use]
pub fn is_trusted_pubkey(hex: &str) -> bool {
    let h = hex.trim().to_ascii_lowercase();
    catalog().iter().any(|n| n.pubkey.eq_ignore_ascii_case(&h))
}

/// Region label for a known hex pubkey, if trusted.
#[must_use]
pub fn region_for_pubkey(hex: &str) -> Option<&'static str> {
    let h = hex.trim().to_ascii_lowercase();
    catalog()
        .iter()
        .find(|n| n.pubkey.eq_ignore_ascii_case(&h))
        .map(|n| n.region.as_str())
}

/// Picker UI state for Settings → Change Mostro Pubkey.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MostroInstancePicker {
    pub filter: String,
    pub selected: usize,
}

/// One row in the picker list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MostroInstancePickerRow {
    Trusted(&'static TrustedMostroInstance),
    /// Normalized custom npub/hex typed in the filter box.
    Custom(String),
}

/// Build visible rows: filtered trusted list, plus a custom row when the filter
/// looks like a pubkey that is not already listed.
#[must_use]
pub fn picker_rows(filter: &str) -> Vec<MostroInstancePickerRow> {
    let trusted = filter_trusted(filter);
    let mut rows: Vec<MostroInstancePickerRow> = trusted
        .into_iter()
        .map(MostroInstancePickerRow::Trusted)
        .collect();

    let trimmed = filter.trim();
    if !trimmed.is_empty() {
        // Defer full validation to Enter; here only avoid duplicating an exact trusted hex.
        let looks_custom = !catalog().iter().any(|n| {
            n.pubkey.eq_ignore_ascii_case(trimmed) || n.region.eq_ignore_ascii_case(trimmed)
        });
        if looks_custom {
            rows.push(MostroInstancePickerRow::Custom(trimmed.to_string()));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_includes_default_cuba_and_mostroeuropa() {
        let all = trusted_instances();
        assert!(all.len() >= 9);
        assert!(all.iter().any(|n| n.region.contains("Default")));
        assert!(all.iter().any(|n| n.region.contains("Cuba")));
        assert!(all.iter().any(|n| n.region.contains("MostroEuropa")));
        assert!(is_trusted_pubkey(
            "82fa8cb978b43c79b2156585bac2c011176a21d2aead6d9f7c575c005be88390"
        ));
        assert!(is_trusted_pubkey(
            "da23a31d75572138ab8149911a04224812a34bda679caba7cb1824fdf7c592ec"
        ));
    }

    #[test]
    fn filter_matches_region_and_pubkey_prefix() {
        let cuba = filter_trusted("cuba");
        assert_eq!(cuba.len(), 1);
        assert!(cuba[0].region.contains("Cuba"));

        let by_pk = filter_trusted("00000235");
        assert_eq!(by_pk.len(), 1);
    }

    #[test]
    fn picker_rows_add_custom_when_filter_unmatched() {
        let rows = picker_rows("npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq");
        assert!(
            rows.iter()
                .any(|r| matches!(r, MostroInstancePickerRow::Custom(_))),
            "custom row expected for unmatched filter"
        );

        let empty = picker_rows("");
        assert!(
            !empty.is_empty()
                && empty
                    .iter()
                    .all(|r| matches!(r, MostroInstancePickerRow::Trusted(_))),
            "empty filter lists trusted only"
        );

        // Exact region label suppresses the Custom row.
        let exact = picker_rows("🌐 Default");
        assert!(
            exact
                .iter()
                .all(|r| matches!(r, MostroInstancePickerRow::Trusted(_))),
            "exact region match should not add Custom"
        );
    }
}
