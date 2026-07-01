//! Bundled fiat currency metadata for the New Order currency picker.
//!
//! The picker prefers the currencies advertised by the connected Mostro instance
//! (kind 38385 `fiat_currencies_accepted`). When the instance advertises none
//! (meaning "all currencies accepted"), we fall back to this curated ISO-4217
//! list so the user still gets a searchable dropdown with human-readable names.
//!
//! Note: we deliberately do not render emoji flags. Regional-indicator flag
//! emoji are not supported by most terminal fonts and fall back to bare letter
//! pairs (e.g. `🇺🇸` → "US"), which just clutters the code. The ISO code plus
//! the currency name is clearer and renders everywhere.

/// Static metadata for a single fiat currency.
#[derive(Clone, Copy, Debug)]
pub struct CurrencyMeta {
    pub code: &'static str,
    pub name: &'static str,
}

/// An owned currency entry shown in the picker (code + human name).
#[derive(Clone, Debug)]
pub struct CurrencyOption {
    pub code: String,
    pub name: String,
}

/// Curated set of commonly traded fiat currencies (ISO 4217).
pub const CURRENCIES: &[CurrencyMeta] = &[
    CurrencyMeta {
        code: "USD",
        name: "US Dollar",
    },
    CurrencyMeta {
        code: "EUR",
        name: "Euro",
    },
    CurrencyMeta {
        code: "GBP",
        name: "Pound Sterling",
    },
    CurrencyMeta {
        code: "JPY",
        name: "Japanese Yen",
    },
    CurrencyMeta {
        code: "CHF",
        name: "Swiss Franc",
    },
    CurrencyMeta {
        code: "CAD",
        name: "Canadian Dollar",
    },
    CurrencyMeta {
        code: "AUD",
        name: "Australian Dollar",
    },
    CurrencyMeta {
        code: "NZD",
        name: "New Zealand Dollar",
    },
    CurrencyMeta {
        code: "CNY",
        name: "Chinese Yuan",
    },
    CurrencyMeta {
        code: "HKD",
        name: "Hong Kong Dollar",
    },
    CurrencyMeta {
        code: "SGD",
        name: "Singapore Dollar",
    },
    CurrencyMeta {
        code: "INR",
        name: "Indian Rupee",
    },
    CurrencyMeta {
        code: "RUB",
        name: "Russian Ruble",
    },
    CurrencyMeta {
        code: "BRL",
        name: "Brazilian Real",
    },
    CurrencyMeta {
        code: "ARS",
        name: "Argentine Peso",
    },
    CurrencyMeta {
        code: "MXN",
        name: "Mexican Peso",
    },
    CurrencyMeta {
        code: "CLP",
        name: "Chilean Peso",
    },
    CurrencyMeta {
        code: "COP",
        name: "Colombian Peso",
    },
    CurrencyMeta {
        code: "PEN",
        name: "Peruvian Sol",
    },
    CurrencyMeta {
        code: "UYU",
        name: "Uruguayan Peso",
    },
    CurrencyMeta {
        code: "VES",
        name: "Venezuelan Bolívar",
    },
    CurrencyMeta {
        code: "BOB",
        name: "Bolivian Boliviano",
    },
    CurrencyMeta {
        code: "PYG",
        name: "Paraguayan Guaraní",
    },
    CurrencyMeta {
        code: "CRC",
        name: "Costa Rican Colón",
    },
    CurrencyMeta {
        code: "GTQ",
        name: "Guatemalan Quetzal",
    },
    CurrencyMeta {
        code: "DOP",
        name: "Dominican Peso",
    },
    CurrencyMeta {
        code: "CUP",
        name: "Cuban Peso",
    },
    CurrencyMeta {
        code: "ZAR",
        name: "South African Rand",
    },
    CurrencyMeta {
        code: "NGN",
        name: "Nigerian Naira",
    },
    CurrencyMeta {
        code: "KES",
        name: "Kenyan Shilling",
    },
    CurrencyMeta {
        code: "GHS",
        name: "Ghanaian Cedi",
    },
    CurrencyMeta {
        code: "EGP",
        name: "Egyptian Pound",
    },
    CurrencyMeta {
        code: "MAD",
        name: "Moroccan Dirham",
    },
    CurrencyMeta {
        code: "TRY",
        name: "Turkish Lira",
    },
    CurrencyMeta {
        code: "AED",
        name: "UAE Dirham",
    },
    CurrencyMeta {
        code: "SAR",
        name: "Saudi Riyal",
    },
    CurrencyMeta {
        code: "ILS",
        name: "Israeli New Shekel",
    },
    CurrencyMeta {
        code: "PLN",
        name: "Polish Złoty",
    },
    CurrencyMeta {
        code: "CZK",
        name: "Czech Koruna",
    },
    CurrencyMeta {
        code: "HUF",
        name: "Hungarian Forint",
    },
    CurrencyMeta {
        code: "RON",
        name: "Romanian Leu",
    },
    CurrencyMeta {
        code: "SEK",
        name: "Swedish Krona",
    },
    CurrencyMeta {
        code: "NOK",
        name: "Norwegian Krone",
    },
    CurrencyMeta {
        code: "DKK",
        name: "Danish Krone",
    },
    CurrencyMeta {
        code: "UAH",
        name: "Ukrainian Hryvnia",
    },
    CurrencyMeta {
        code: "THB",
        name: "Thai Baht",
    },
    CurrencyMeta {
        code: "IDR",
        name: "Indonesian Rupiah",
    },
    CurrencyMeta {
        code: "MYR",
        name: "Malaysian Ringgit",
    },
    CurrencyMeta {
        code: "PHP",
        name: "Philippine Peso",
    },
    CurrencyMeta {
        code: "VND",
        name: "Vietnamese Đồng",
    },
    CurrencyMeta {
        code: "KRW",
        name: "South Korean Won",
    },
];

/// Look up bundled metadata for a currency code (case-insensitive).
pub fn lookup(code: &str) -> Option<&'static CurrencyMeta> {
    let upper = code.trim().to_ascii_uppercase();
    CURRENCIES.iter().find(|c| c.code == upper)
}

/// Human-readable name for a currency code, or an empty string if unknown.
pub fn name_for(code: &str) -> &'static str {
    lookup(code).map(|m| m.name).unwrap_or("")
}

/// Build the option list shown in the picker.
///
/// When `accepted` is non-empty, only those instance-advertised codes are shown
/// (enriched with a bundled name where known). Otherwise the full curated list
/// is returned.
pub fn resolve_options(accepted: &[String]) -> Vec<CurrencyOption> {
    if accepted.is_empty() {
        return CURRENCIES
            .iter()
            .map(|c| CurrencyOption {
                code: c.code.to_string(),
                name: c.name.to_string(),
            })
            .collect();
    }

    accepted
        .iter()
        .map(|code| {
            let upper = code.trim().to_ascii_uppercase();
            CurrencyOption {
                name: name_for(&upper).to_string(),
                code: upper,
            }
        })
        .collect()
}

/// Filter options by a query, matching on code prefix or name substring
/// (both case-insensitive).
pub fn filter_options(options: &[CurrencyOption], query: &str) -> Vec<CurrencyOption> {
    let q = query.trim().to_ascii_uppercase();
    if q.is_empty() {
        return options.to_vec();
    }
    options
        .iter()
        .filter(|o| o.code.starts_with(&q) || o.name.to_ascii_uppercase().contains(&q))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_falls_back_to_full_list_when_empty() {
        let opts = resolve_options(&[]);
        assert!(opts.len() >= 20);
        assert!(opts.iter().any(|o| o.code == "USD"));
    }

    #[test]
    fn resolve_uses_accepted_and_enriches_known_codes() {
        let opts = resolve_options(&["eur".to_string(), "XYZ".to_string()]);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].code, "EUR");
        assert_eq!(opts[0].name, "Euro");
        assert_eq!(opts[1].code, "XYZ");
        assert!(opts[1].name.is_empty());
    }

    #[test]
    fn filter_matches_code_prefix_and_name_substring() {
        let opts = resolve_options(&[]);
        assert!(filter_options(&opts, "us").iter().any(|o| o.code == "USD"));
        assert!(filter_options(&opts, "peso")
            .iter()
            .all(|o| o.name.to_ascii_uppercase().contains("PESO")));
        assert_eq!(filter_options(&opts, "").len(), opts.len());
    }
}
