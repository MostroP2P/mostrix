// Filter creation utilities for Nostr queries
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;

use crate::util::types::ListKind;

/// Protocol DM filter for inbound Mostro → client traffic (signed kind 14).
///
/// Normal path: authored by Mostro with the trade key in `#p`.
///
/// When `mostro_pubkey == trade_pubkey` (admin using the Mostro nsec), the
/// daemon may omit `#p` on self-addressed replies, so this path subscribes by
/// author+kind only.
///
/// That broader filter can also match the client's own outbound request. `wait_for_dm`
/// waiters ignore signed self-authored kind-14 events (`is_own_signed_v2_outbound`) so the
/// request is not consumed as the daemon reply.
pub fn filter_protocol_dm_from_mostro(
    _transport: Transport,
    mostro_pubkey: PublicKey,
    trade_pubkey: PublicKey,
) -> Filter {
    if mostro_pubkey == trade_pubkey {
        Filter::new()
            .author(mostro_pubkey)
            .kind(nostr_sdk::prelude::Kind::PrivateDirectMessage)
    } else {
        Filter::new()
            .author(mostro_pubkey)
            .pubkey(trade_pubkey)
            .kind(nostr_sdk::prelude::Kind::PrivateDirectMessage)
    }
}

/// Relay fetch cap for Mostro-published [`nostr_sdk::prelude::Kind::Custom`] order/dispute list snapshots.
pub const MOSTRO_LIST_FETCH_EVENT_LIMIT: usize = 500;

/// Build a fetch filter for Mostro list snapshots: events authored by `pubkey`, a given custom
/// `kind`, and at most [`MOSTRO_LIST_FETCH_EVENT_LIMIT`] results.
///
/// There is **no** `since` time window; relay ordering decides which events fall inside the limit.
pub fn create_mostro_list_fetch_filter(kind: u16, pubkey: PublicKey) -> Result<Filter> {
    Ok(Filter::new()
        .author(pubkey)
        .limit(MOSTRO_LIST_FETCH_EVENT_LIMIT)
        .kind(nostr_sdk::prelude::Kind::Custom(kind)))
}

/// Create a filter based on list kind
pub fn create_filter(
    list_kind: ListKind,
    pubkey: PublicKey,
    _since: Option<&i64>,
) -> Result<Filter> {
    match list_kind {
        ListKind::Orders => create_mostro_list_fetch_filter(NOSTR_ORDER_EVENT_KIND, pubkey),
        ListKind::Disputes => create_mostro_list_fetch_filter(NOSTR_DISPUTE_EVENT_KIND, pubkey),
        _ => Err(anyhow::anyhow!("Unsupported ListKind for mostrix")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mostro_core::prelude::Transport;
    use nostr_sdk::prelude::Keys;

    #[test]
    fn filter_protocol_dm_v2_uses_mostro_author_trade_p_tag_and_kind_14() {
        let trade = Keys::generate().public_key();
        let mostro = Keys::generate().public_key();
        let filter = filter_protocol_dm_from_mostro(Transport::Nip44Direct, mostro, trade);
        let json = filter.as_json();
        assert!(json.contains(&format!(r#""authors":["{}"]"#, mostro)));
        assert!(json.contains(&format!("\"#p\":[\"{}\"]", trade)));
        assert!(json.contains(r#""kinds":[14]"#));
    }

    #[test]
    fn filter_protocol_dm_v2_self_admin_omits_p_tag() {
        let mostro = Keys::generate().public_key();
        let filter = filter_protocol_dm_from_mostro(Transport::Nip44Direct, mostro, mostro);
        let json = filter.as_json();
        assert!(json.contains(&format!(r#""authors":["{}"]"#, mostro)));
        assert!(json.contains(r#""kinds":[14]"#));
        assert!(!json.contains("\"#p\""));
    }
}
