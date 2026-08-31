// Trade-index sync with Mostro (`Action::LastTradeIndex`).
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use uuid::Uuid;

use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::types::get_cant_do_description;

/// Outcome of `Action::LastTradeIndex` from Mostro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastTradeIndexSync {
    /// Highest trade index Mostro has recorded for this identity.
    pub last_used_index: i64,
    /// True when Mostro reports no prior trade history (`CantDo::NotFound`).
    pub no_history: bool,
}

/// Highest index that must be reflected locally before reserving a new trade key.
///
/// Mobile sets the next key to `lastTradeIndex + 1`; Mostrix stores the last
/// used index in SQLite and [`crate::models::User::reserve_next_trade_index`]
/// derives `last + 1`.
pub fn effective_last_trade_index(restore_max: i64, mostro_last: i64) -> i64 {
    restore_max.max(mostro_last)
}

/// Ask Mostro for this identity's last used trade index (`Action::LastTradeIndex`).
///
/// Uses identity keys (account-scoped, same as restore session). The response
/// carries the index on the inner [`MessageKind::trade_index`] field.
pub async fn fetch_last_trade_index_from_mostro(
    client: &Client,
    identity_keys: &Keys,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<LastTradeIndexSync> {
    let request_id = Uuid::new_v4().as_u128() as u64;
    let kind = MessageKind::new(None, Some(request_id), None, Action::LastTradeIndex, None);
    let message = Message::Restore(kind);
    let message_json = message
        .as_json()
        .map_err(|e| anyhow::anyhow!("Failed to serialize last-trade-index request: {e}"))?;

    log::info!("Requesting last trade index from {mostro_pubkey}");

    let sent_message = send_dm(
        client,
        Some(identity_keys),
        identity_keys,
        &mostro_pubkey,
        message_json,
        None,
        mostro_instance,
    );

    let recv_event = wait_for_dm(identity_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;
    let messages = parse_dm_events(recv_event, identity_keys, None).await;

    let Some((response_message, _, sender)) = messages.first() else {
        return Err(anyhow::anyhow!(
            "No response received for Action::LastTradeIndex"
        ));
    };
    if sender != &mostro_pubkey {
        return Err(anyhow::anyhow!(
            "Last-trade-index response signed by {sender}, expected the configured Mostro instance"
        ));
    }

    validate_correlated_response(response_message, request_id)?;
    parse_last_trade_index_response(response_message)
}

/// Reject replayed or unrelated waiter responses before applying the trade index.
fn validate_correlated_response(message: &Message, expected_request_id: u64) -> Result<()> {
    match message.get_inner_message_kind().request_id {
        Some(id) if id == expected_request_id => Ok(()),
        Some(id) => Err(anyhow::anyhow!(
            "Last-trade-index response request_id mismatch: expected {expected_request_id}, got {id}"
        )),
        None => Err(anyhow::anyhow!(
            "Last-trade-index response omitted request_id (expected {expected_request_id})"
        )),
    }
}

fn parse_last_trade_index_response(message: &Message) -> Result<LastTradeIndexSync> {
    let inner = message.get_inner_message_kind();

    if let Some(Payload::CantDo(reason)) = &inner.payload {
        return match reason {
            Some(CantDoReason::NotFound) => Ok(LastTradeIndexSync {
                last_used_index: 0,
                no_history: true,
            }),
            Some(CantDoReason::InvalidTradeIndex) => Err(anyhow::anyhow!(
                "Mostro rejected the last-trade-index request (invalid trade index)"
            )),
            Some(other) => Err(anyhow::anyhow!(get_cant_do_description(other))),
            None => Err(anyhow::anyhow!(
                "Mostro couldn't process the last-trade-index request"
            )),
        };
    }

    if inner.action != Action::LastTradeIndex {
        return Err(anyhow::anyhow!(
            "Unexpected action in last-trade-index response: {:?}",
            inner.action
        ));
    }

    let (has_trade_index, last_used_index) = inner.has_trade_index();
    if !has_trade_index {
        return Err(anyhow::anyhow!(
            "Last-trade-index response from Mostro omitted trade_index"
        ));
    }
    if last_used_index <= 0 {
        return Err(anyhow::anyhow!(
            "Last-trade-index response from Mostro returned invalid trade index: {last_used_index}"
        ));
    }

    Ok(LastTradeIndexSync {
        last_used_index,
        no_history: false,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        effective_last_trade_index, parse_last_trade_index_response,
        validate_correlated_response, LastTradeIndexSync,
    };
    use mostro_core::prelude::*;

    #[test]
    fn effective_last_trade_index_uses_the_higher_value() {
        assert_eq!(effective_last_trade_index(0, 0), 0);
        assert_eq!(effective_last_trade_index(3, 7), 7);
        assert_eq!(effective_last_trade_index(9, 4), 9);
    }

    #[test]
    fn parse_last_trade_index_response_reads_trade_index_field() {
        let kind = MessageKind::new(None, Some(42), Some(12), Action::LastTradeIndex, None);
        let message = Message::Restore(kind);
        validate_correlated_response(&message, 42).expect("request_id");
        assert_eq!(
            parse_last_trade_index_response(&message).expect("parse"),
            LastTradeIndexSync {
                last_used_index: 12,
                no_history: false,
            }
        );
    }

    #[test]
    fn parse_last_trade_index_response_maps_not_found_to_empty_history() {
        let kind = MessageKind::new(
            None,
            Some(7),
            None,
            Action::CantDo,
            Some(Payload::CantDo(Some(CantDoReason::NotFound))),
        );
        let message = Message::CantDo(kind);
        validate_correlated_response(&message, 7).expect("request_id");
        assert_eq!(
            parse_last_trade_index_response(&message).expect("parse"),
            LastTradeIndexSync {
                last_used_index: 0,
                no_history: true,
            }
        );
    }

    #[test]
    fn parse_last_trade_index_response_rejects_missing_trade_index() {
        let kind = MessageKind::new(None, None, None, Action::LastTradeIndex, None);
        let message = Message::Restore(kind);
        let err = parse_last_trade_index_response(&message)
            .expect_err("missing trade_index must fail closed");
        assert!(
            err.to_string().contains("omitted trade_index"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_last_trade_index_response_rejects_non_positive_trade_index() {
        let kind = MessageKind::new(None, None, Some(0), Action::LastTradeIndex, None);
        let message = Message::Restore(kind);
        let err = parse_last_trade_index_response(&message)
            .expect_err("zero trade_index must fail closed");
        assert!(
            err.to_string().contains("invalid trade index"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_last_trade_index_response_rejects_invalid_trade_index_cant_do() {
        let kind = MessageKind::new(
            None,
            None,
            None,
            Action::CantDo,
            Some(Payload::CantDo(Some(CantDoReason::InvalidTradeIndex))),
        );
        let message = Message::CantDo(kind);
        assert!(parse_last_trade_index_response(&message).is_err());
    }

    #[test]
    fn validate_correlated_response_rejects_mismatched_request_id() {
        let kind = MessageKind::new(None, Some(9), Some(3), Action::LastTradeIndex, None);
        let message = Message::Restore(kind);
        let err = validate_correlated_response(&message, 1).expect_err("mismatch");
        assert!(err.to_string().contains("request_id mismatch"));
    }

    #[test]
    fn validate_correlated_response_rejects_null_request_id() {
        let kind = MessageKind::new(None, None, Some(3), Action::LastTradeIndex, None);
        let message = Message::Restore(kind);
        let err = validate_correlated_response(&message, 1).expect_err("missing rid");
        assert!(err.to_string().contains("omitted request_id"));
    }
}
