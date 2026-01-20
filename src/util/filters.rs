// Filter creation utilities for Nostr queries
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;

use crate::util::types::ListKind;

/// Create a filter for events from the last 7 days
pub fn create_seven_days_filter(kind: u16, pubkey: PublicKey) -> Result<Filter> {
    Ok(Filter::new()
        .author(pubkey)
        .limit(50)
        .kind(nostr_sdk::Kind::Custom(kind)))
}

/// Create a filter based on list kind
pub fn create_filter(
    list_kind: ListKind,
    pubkey: PublicKey,
    _since: Option<&i64>,
) -> Result<Filter> {
    match list_kind {
        ListKind::Orders => create_seven_days_filter(NOSTR_ORDER_EVENT_KIND, pubkey),
        ListKind::Disputes => create_seven_days_filter(NOSTR_DISPUTE_EVENT_KIND, pubkey),
        _ => Err(anyhow::anyhow!("Unsupported ListKind for mostrix")),
    }
}
