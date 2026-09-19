//! Bundled payment-method suggestions for the New Order method picker.
//!
//! Snapshot of Mostro Mobile `assets/data/payment_methods.json` (currency →
//! display strings), with Mostrix extras (e.g. Satispay on EUR). Protocol
//! `payment_method` remains a free-form comma-separated string; this catalog
//! is UI-only.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Bundled JSON: ISO-4217 code → method names, plus `"default"`.
const PAYMENT_METHODS_JSON: &str = include_str!("payment_methods.json");

fn catalog() -> &'static HashMap<String, Vec<String>> {
    static CATALOG: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(PAYMENT_METHODS_JSON)
            .expect("bundled payment_methods.json must be valid JSON")
    })
}

/// Standard methods for `fiat_code`, or the bundled default list.
pub fn methods_for(fiat_code: &str) -> Vec<String> {
    let data = catalog();
    let key = fiat_code.trim().to_ascii_uppercase();
    if !key.is_empty() {
        if let Some(list) = data.get(&key) {
            return list.clone();
        }
    }
    data.get("default")
        .cloned()
        .unwrap_or_else(|| vec!["Bank Transfer".to_string(), "Cash in person".to_string()])
}

/// Catalog methods plus any already-selected custom names (so they can be
/// unchecked). Selected extras keep their original spelling and are appended.
pub fn listed_methods(fiat_code: &str, selected: &[String]) -> Vec<String> {
    let mut methods = methods_for(fiat_code);
    for name in selected {
        if !methods.iter().any(|m| m.eq_ignore_ascii_case(name)) {
            methods.push(name.clone());
        }
    }
    methods
}

/// Case-insensitive substring filter. Empty query returns `methods` unchanged.
pub fn filter_methods(methods: &[String], query: &str) -> Vec<String> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return methods.to_vec();
    }
    methods
        .iter()
        .filter(|m| m.to_ascii_lowercase().contains(&q))
        .cloned()
        .collect()
}

/// Split a comma-separated `payment_method` string into trimmed names.
pub fn parse_selected(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Join selected methods for display / submit (`", "` between names).
pub fn join_selected(methods: &[String]) -> String {
    methods.join(", ")
}

/// Toggle `method` in `selected` (case-insensitive match). Removing keeps the
/// existing spelling; adding stores `method` as given.
pub fn toggle_method(selected: &mut Vec<String>, method: &str) {
    if let Some(i) = selected.iter().position(|m| m.eq_ignore_ascii_case(method)) {
        selected.remove(i);
    } else {
        selected.push(method.to_string());
    }
}

/// Strip `, " \ [ ] { }`, collapse whitespace, and return `None` if empty.
pub fn sanitize_custom(raw: &str) -> Option<String> {
    let mut out = String::new();
    for ch in raw.chars() {
        if matches!(ch, ',' | '"' | '\\' | '[' | ']' | '{' | '}') {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

/// Sanitized filter text that is not already an exact listed name.
pub fn custom_candidate(filter: &str, listed: &[String]) -> Option<String> {
    let sanitized = sanitize_custom(filter)?;
    if listed.iter().any(|m| m.eq_ignore_ascii_case(&sanitized)) {
        None
    } else {
        Some(sanitized)
    }
}

/// One row in the open picker (catalog/custom-selected, or the add-custom row).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerItem {
    /// A catalog method (or an already-selected custom name) with a checkbox.
    Listed(String),
    /// Virtual row: Enter appends this sanitized name to `payment_method`.
    Custom(String),
}

/// Filtered picker rows for `fiat_code` + current selection + filter text.
pub fn picker_rows(fiat_code: &str, payment_method: &str, filter: &str) -> Vec<PickerItem> {
    let selected = parse_selected(payment_method);
    let listed = listed_methods(fiat_code, &selected);
    let filtered = filter_methods(&listed, filter);
    let mut rows: Vec<PickerItem> = filtered.into_iter().map(PickerItem::Listed).collect();
    if let Some(custom) = custom_candidate(filter, &listed) {
        rows.push(PickerItem::Custom(custom));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_for_known_currency() {
        let eur = methods_for("eur");
        assert!(eur.iter().any(|m| m == "Bizum"));
        assert!(eur.iter().any(|m| m == "SEPA instant"));
        assert!(eur.iter().any(|m| m == "Satispay"));
        assert!(!eur.iter().any(|m| m == "PIX"));
    }

    #[test]
    fn methods_for_unknown_uses_default() {
        let got = methods_for("XYZ");
        assert_eq!(got, vec!["Bank Transfer", "Cash in person"]);
        assert_eq!(methods_for(""), methods_for("XYZ"));
    }

    #[test]
    fn filter_matches_substring_case_insensitive() {
        let usd = methods_for("USD");
        let hits = filter_methods(&usd, "cash");
        assert!(hits.iter().any(|m| m == "Cash App"));
        assert!(hits.iter().any(|m| m == "Cash"));
        assert_eq!(filter_methods(&usd, "").len(), usd.len());
    }

    #[test]
    fn parse_and_join_round_trip() {
        assert!(parse_selected("  ").is_empty());
        let parsed = parse_selected("PIX, TED,  Cash");
        assert_eq!(parsed, vec!["PIX", "TED", "Cash"]);
        assert_eq!(join_selected(&parsed), "PIX, TED, Cash");
    }

    #[test]
    fn toggle_adds_and_removes_case_insensitive() {
        let mut selected = vec!["PIX".to_string()];
        toggle_method(&mut selected, "ted");
        assert_eq!(selected, vec!["PIX", "ted"]);
        toggle_method(&mut selected, "pix");
        assert_eq!(selected, vec!["ted"]);
    }

    #[test]
    fn sanitize_strips_list_breaking_punctuation() {
        assert_eq!(
            sanitize_custom(r#"Bank, "wire" [ACH] {foo}"#).as_deref(),
            Some("Bank wire ACH foo")
        );
        assert_eq!(sanitize_custom("  ,,,  "), None);
        assert_eq!(sanitize_custom("  My   Bank  ").as_deref(), Some("My Bank"));
    }

    #[test]
    fn custom_candidate_skips_exact_listed_names() {
        let listed = methods_for("USD");
        assert!(custom_candidate("zelle", &listed).is_none());
        assert_eq!(
            custom_candidate("My local bank", &listed).as_deref(),
            Some("My local bank")
        );
    }

    #[test]
    fn listed_methods_appends_selected_custom() {
        let listed = listed_methods("USD", &["Zelle".to_string(), "My Bank".to_string()]);
        assert!(listed.iter().any(|m| m == "Zelle"));
        assert_eq!(listed.last().map(String::as_str), Some("My Bank"));
    }

    #[test]
    fn picker_rows_append_custom_when_filter_is_new() {
        let rows = picker_rows("USD", "", "My Bank");
        assert!(matches!(
            rows.last(),
            Some(PickerItem::Custom(s)) if s == "My Bank"
        ));
        assert!(picker_rows("USD", "", "Zelle")
            .iter()
            .all(|r| !matches!(r, PickerItem::Custom(_))));
    }
}
