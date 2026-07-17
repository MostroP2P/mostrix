// Filter creation utilities for Nostr queries
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;

use crate::util::types::ListKind;

/// GiftWrap events (NIP-59) addressed to `pubkey` as recipient (`p` tag target).
/// Chain `.since()`, `.limit()`, etc. for subscriptions or `fetch_events`.
pub fn filter_giftwrap_to_recipient(pubkey: PublicKey) -> Filter {
    Filter::new().pubkey(pubkey).kind(nostr_sdk::Kind::GiftWrap)
}

/// Protocol DM filter for inbound Mostro → client traffic on the active wire transport.
///
/// v1: GiftWrap addressed to the trade key (`p` tag).
/// v2: signed kind-14 from Mostro with trade key in `#p`.
///
/// When `mostro_pubkey == trade_pubkey` on v2 (admin using the Mostro nsec), nostr-sdk's
/// [`EventBuilder`] strips self `#p` tags unless `allow_self_tagging` is set. Mostro replies
/// therefore often have **no** `#p`. In that case subscribe by author+kind only so the
/// waiter can still receive self-addressed replies.
pub fn filter_protocol_dm_from_mostro(
    transport: Transport,
    mostro_pubkey: PublicKey,
    trade_pubkey: PublicKey,
) -> Filter {
    match transport {
        Transport::GiftWrap => filter_giftwrap_to_recipient(trade_pubkey),
        Transport::Nip44Direct if mostro_pubkey == trade_pubkey => Filter::new()
            .author(mostro_pubkey)
            .kind(nostr_sdk::Kind::PrivateDirectMessage),
        Transport::Nip44Direct => Filter::new()
            .author(mostro_pubkey)
            .pubkey(trade_pubkey)
            .kind(nostr_sdk::Kind::PrivateDirectMessage),
    }
}

/// Relay fetch cap for Mostro-published [`Kind::Custom`] order/dispute list snapshots.
pub const MOSTRO_LIST_FETCH_EVENT_LIMIT: usize = 500;

/// Build a fetch filter for Mostro list snapshots: events authored by `pubkey`, a given custom
/// `kind`, and at most [`MOSTRO_LIST_FETCH_EVENT_LIMIT`] results.
///
/// There is **no** `since` time window; relay ordering decides which events fall inside the limit.
pub fn create_mostro_list_fetch_filter(kind: u16, pubkey: PublicKey) -> Result<Filter> {
    Ok(Filter::new()
        .author(pubkey)
        .limit(MOSTRO_LIST_FETCH_EVENT_LIMIT)
        .kind(nostr_sdk::Kind::Custom(kind)))
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
    fn filter_protocol_dm_v1_matches_giftwrap_to_recipient() {
        let trade = Keys::generate().public_key();
        let mostro = Keys::generate().public_key();
        let v1 = filter_protocol_dm_from_mostro(Transport::GiftWrap, mostro, trade);
        let legacy = filter_giftwrap_to_recipient(trade);
        assert_eq!(v1.as_json(), legacy.as_json());
    }

    #[test]
    fn filter_protocol_dm_v1_does_not_filter_by_mostro_author() {
        let trade = Keys::generate().public_key();
        let mostro = Keys::generate().public_key();
        let filter = filter_protocol_dm_from_mostro(Transport::GiftWrap, mostro, trade);
        let json = filter.as_json();
        assert!(json.contains(r#""kinds":[1059]"#));
        assert!(json.contains(&format!("\"#p\":[\"{}\"]", trade)));
        assert!(!json.contains(&format!(r#""authors":["{}"]"#, mostro)));
    }

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
