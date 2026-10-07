//! Own-reputation sync with Mostro (`Action::UserInfo`).
//!
//! Protocol: <https://mostro.network/protocol/user_info.html>
use std::time::Duration;

use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::models::User;
use crate::ui::OperationResult;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::types::get_cant_do_description;

/// Delay before a second `user-info` fetch after `PurchaseCompleted` so a
/// counterpart rating that arrives shortly after success can update the bar.
pub const OWN_REPUTATION_REFRESH_AFTER_SUCCESS_DELAY: Duration = Duration::from_secs(45);

/// Ask Mostro for this identity's own reputation (`Action::UserInfo`).
///
/// Account-scoped: the identity travels only inside the encrypted identity
/// proof (the daemon resolves the account from `event.identity`). The outer
/// kind-14 is authored by a fresh ephemeral key — never the identity key —
/// and Mostro replies to that ephemeral key with [`Payload::UserInfo`].
///
/// An unknown identity gets zeros and no `since`, not an error. A request
/// without identity proof is answered with
/// [`CantDoReason::ReputationIdentityRequired`]; that maps to the same zeroed
/// [`UserInfo`] (no reputation) so callers can soft-fail the status bar.
pub async fn fetch_user_info_from_mostro(
    client: &Client,
    identity_keys: &Keys,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<UserInfo> {
    let request_id = Uuid::new_v4().as_u128() as u64;
    let kind = MessageKind::new(None, Some(request_id), None, Action::UserInfo, None);
    let message = Message::Restore(kind);
    let message_json = message
        .as_json()
        .map_err(|e| anyhow::anyhow!("Failed to serialize user-info request: {e}"))?;

    log::info!("Requesting own user info from {mostro_pubkey}");

    // Ephemeral author: the daemon resolves the account from the identity
    // proof and uses this key only as the reply address. Not a wallet trade
    // key — no trade index is burned.
    let ephemeral_trade_keys = Keys::generate();

    let sent_message = send_dm(
        client,
        Some(identity_keys),
        &ephemeral_trade_keys,
        &mostro_pubkey,
        message_json,
        None,
        mostro_instance,
    );

    let recv_event = wait_for_dm(
        &ephemeral_trade_keys,
        FETCH_EVENTS_TIMEOUT,
        Some(request_id),
        sent_message,
    )
    .await?;
    let messages = parse_dm_events(recv_event, &ephemeral_trade_keys, None).await;

    let Some((response_message, _, sender)) = messages.first() else {
        return Err(anyhow::anyhow!("No response received for Action::UserInfo"));
    };
    if sender != &mostro_pubkey {
        return Err(anyhow::anyhow!(
            "User-info response signed by {sender}, expected the configured Mostro instance"
        ));
    }

    validate_correlated_response(response_message, request_id)?;
    parse_user_info_response(response_message)
}

/// Reject replayed or unrelated waiter responses before applying reputation.
fn validate_correlated_response(message: &Message, expected_request_id: u64) -> Result<()> {
    match message.get_inner_message_kind().request_id {
        Some(id) if id == expected_request_id => Ok(()),
        Some(id) => Err(anyhow::anyhow!(
            "User-info response request_id mismatch: expected {expected_request_id}, got {id}"
        )),
        None => Err(anyhow::anyhow!(
            "User-info response omitted request_id (expected {expected_request_id})"
        )),
    }
}

fn parse_user_info_response(message: &Message) -> Result<UserInfo> {
    let inner = message.get_inner_message_kind();

    if let Some(Payload::CantDo(reason)) = &inner.payload {
        return match reason {
            Some(CantDoReason::ReputationIdentityRequired) => {
                log::info!(
                    "User-info: Mostro requires identity proof (full privacy / no reputation)"
                );
                Ok(UserInfo::default())
            }
            Some(other) => Err(anyhow::anyhow!(get_cant_do_description(other))),
            None => Err(anyhow::anyhow!(
                "Mostro couldn't process the user-info request"
            )),
        };
    }

    if inner.action != Action::UserInfo {
        return Err(anyhow::anyhow!(
            "Unexpected action in user-info response: {:?}",
            inner.action
        ));
    }

    match &inner.payload {
        Some(Payload::UserInfo(info)) => Ok(info.clone()),
        Some(other) => Err(anyhow::anyhow!(
            "User-info response carried unexpected payload: {other:?}"
        )),
        None => Err(anyhow::anyhow!(
            "User-info response from Mostro omitted user_info payload"
        )),
    }
}

/// Fetch own reputation and push [`OperationResult::OwnReputationUpdated`].
///
/// Soft-fails on network/parse errors (`log::warn` only — no popup).
pub fn spawn_fetch_user_info(
    pool: SqlitePool,
    client: Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<MostroInstanceInfo>,
    order_result_tx: UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        let identity_keys = match User::get_identity_keys(&pool).await {
            Ok(keys) => keys,
            Err(e) => {
                log::warn!("Own reputation fetch skipped: identity keys unavailable: {e}");
                return;
            }
        };
        match fetch_user_info_from_mostro(
            &client,
            &identity_keys,
            mostro_pubkey,
            mostro_instance.as_ref(),
        )
        .await
        {
            Ok(info) => {
                let _ = order_result_tx.send(OperationResult::OwnReputationUpdated { info });
            }
            Err(e) => {
                log::warn!("Own reputation fetch failed: {e}");
            }
        }
    });
}

/// Like [`spawn_fetch_user_info`], but waits `delay` first (e.g. after success).
pub fn spawn_fetch_user_info_delayed(
    pool: SqlitePool,
    client: Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<MostroInstanceInfo>,
    order_result_tx: UnboundedSender<OperationResult>,
    delay: Duration,
) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        spawn_fetch_user_info(
            pool,
            client,
            mostro_pubkey,
            mostro_instance,
            order_result_tx,
        );
    });
}

/// Whether a live trade-DM action should refresh the status-bar reputation cache.
pub fn should_refresh_own_reputation_after_action(action: &Action) -> bool {
    matches!(action, Action::PurchaseCompleted | Action::RateReceived)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_user_info_response, should_refresh_own_reputation_after_action,
        validate_correlated_response,
    };
    use mostro_core::prelude::*;

    fn user_info_message(request_id: Option<u64>, info: UserInfo) -> Message {
        Message::Restore(MessageKind::new(
            None,
            request_id,
            None,
            Action::UserInfo,
            Some(Payload::UserInfo(info)),
        ))
    }

    #[test]
    fn parse_user_info_response_reads_payload() {
        let info = UserInfo {
            rating: 4.8,
            reviews: 23,
            operating_days: 142,
            since: Some(1_700_784_000),
        };
        let message = user_info_message(Some(123456), info.clone());
        validate_correlated_response(&message, 123456).expect("request_id");
        let parsed = parse_user_info_response(&message).expect("parse");
        assert_eq!(parsed.rating, 4.8);
        assert_eq!(parsed.reviews, 23);
        assert_eq!(parsed.operating_days, 142);
        assert_eq!(parsed.since, Some(1_700_784_000));
    }

    #[test]
    fn parse_user_info_response_accepts_unknown_identity_zeros() {
        let info = UserInfo {
            rating: 0.0,
            reviews: 0,
            operating_days: 0,
            since: None,
        };
        let message = user_info_message(Some(1), info);
        let parsed = parse_user_info_response(&message).expect("parse");
        assert_eq!(parsed.reviews, 0);
        assert!(parsed.since.is_none());
    }

    #[test]
    fn parse_user_info_response_maps_reputation_identity_required_to_zeros() {
        let kind = MessageKind::new(
            None,
            Some(7),
            None,
            Action::CantDo,
            Some(Payload::CantDo(Some(
                CantDoReason::ReputationIdentityRequired,
            ))),
        );
        let message = Message::CantDo(kind);
        validate_correlated_response(&message, 7).expect("request_id");
        let parsed = parse_user_info_response(&message).expect("soft-fail zeros");
        assert_eq!(parsed.rating, 0.0);
        assert_eq!(parsed.reviews, 0);
        assert_eq!(parsed.operating_days, 0);
        assert!(parsed.since.is_none());
    }

    #[test]
    fn parse_user_info_response_rejects_other_cant_do() {
        let kind = MessageKind::new(
            None,
            None,
            None,
            Action::CantDo,
            Some(Payload::CantDo(Some(CantDoReason::InvalidSignature))),
        );
        let message = Message::CantDo(kind);
        assert!(parse_user_info_response(&message).is_err());
    }

    #[test]
    fn parse_user_info_response_rejects_missing_payload() {
        let kind = MessageKind::new(None, Some(1), None, Action::UserInfo, None);
        let message = Message::Restore(kind);
        let err = parse_user_info_response(&message).expect_err("missing payload");
        assert!(
            err.to_string().contains("omitted user_info"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_user_info_response_rejects_wrong_action() {
        let kind = MessageKind::new(None, Some(1), None, Action::LastTradeIndex, None);
        let message = Message::Restore(kind);
        let err = parse_user_info_response(&message).expect_err("wrong action");
        assert!(
            err.to_string().contains("Unexpected action"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_user_info_roundtrips_protocol_json() {
        let json = r#"{"restore":{"version":2,"request_id":123456,"action":"user-info","payload":{"user_info":{"rating":4.8,"reviews":23,"operating_days":142,"since":1700784000}}}}"#;
        let message = Message::from_json(json).expect("protocol fixture");
        validate_correlated_response(&message, 123456).expect("request_id");
        let parsed = parse_user_info_response(&message).expect("parse");
        assert_eq!(parsed.rating, 4.8);
        assert_eq!(parsed.reviews, 23);
        assert_eq!(parsed.since, Some(1_700_784_000));
    }

    #[test]
    fn validate_correlated_response_rejects_mismatched_request_id() {
        let message = user_info_message(Some(9), UserInfo::default());
        let err = validate_correlated_response(&message, 1).expect_err("mismatch");
        assert!(err.to_string().contains("request_id mismatch"));
    }

    #[test]
    fn validate_correlated_response_rejects_null_request_id() {
        let message = user_info_message(None, UserInfo::default());
        let err = validate_correlated_response(&message, 1).expect_err("missing rid");
        assert!(err.to_string().contains("omitted request_id"));
    }

    #[test]
    fn should_refresh_own_reputation_after_success_or_rate_received() {
        assert!(should_refresh_own_reputation_after_action(
            &Action::PurchaseCompleted
        ));
        assert!(should_refresh_own_reputation_after_action(
            &Action::RateReceived
        ));
        assert!(!should_refresh_own_reputation_after_action(&Action::Rate));
        assert!(!should_refresh_own_reputation_after_action(
            &Action::FiatSent
        ));
    }
}
