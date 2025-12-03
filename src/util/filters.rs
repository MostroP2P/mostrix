// Filter creation utilities for Nostr queries
use anyhow::Result;
use mostro_core::prelude::NOSTR_REPLACEABLE_EVENT_KIND;
use nostr_sdk::prelude::*;

use crate::util::types::ListKind;

/// Create a filter for events from the last 7 days
pub fn create_seven_days_filter(
    letter: Alphabet,
    value: String,
    pubkey: PublicKey,
) -> Result<Filter> {
    let since_time = chrono::Utc::now()
        .checked_sub_signed(chrono::Duration::days(7))
        .ok_or(anyhow::anyhow!("Failed to get since days ago"))?
        .timestamp() as u64;
    let timestamp = Timestamp::from(since_time);
    Ok(Filter::new()
        .author(pubkey)
        .limit(50)
        .since(timestamp)
        .custom_tag(SingleLetterTag::lowercase(letter), value)
        .kind(nostr_sdk::Kind::Custom(NOSTR_REPLACEABLE_EVENT_KIND)))
}

/// Create a filter based on list kind
pub fn create_filter(
    list_kind: ListKind,
    pubkey: PublicKey,
    _since: Option<&i64>,
) -> Result<Filter> {
    match list_kind {
        ListKind::Orders => create_seven_days_filter(Alphabet::Z, "order".to_string(), pubkey),
        _ => Err(anyhow::anyhow!("Unsupported ListKind for mostrix")),
    }
}
