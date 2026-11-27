// Execute admin order info: read-only snapshot of one order (DB + Lightning).
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use uuid::Uuid;

use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::order_utils::helper::handle_mostro_response;
use crate::util::types::MostroCantDoError;

/// Mostro answered `CantDo` to `AdminOrderInfo`.
#[derive(Debug)]
pub struct OrderInfoRejected {
    pub order_id: Uuid,
    pub reason: CantDoReason,
}

impl std::fmt::Display for OrderInfoRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let order_id = self.order_id;
        match &self.reason {
            // mostrod checks the caller before looking the order up, so this
            // means the key, not the order.
            CantDoReason::InvalidPubkey => write!(
                f,
                "Mostro refused the order info request for {order_id}: this key is neither \
                 the admin nor a registered solver of this node."
            ),
            CantDoReason::NotFound => {
                write!(f, "Mostro has no order {order_id}.")
            }
            other => write!(
                f,
                "Mostro rejected the order info request for {order_id}: {}",
                crate::util::types::get_cant_do_description(other)
            ),
        }
    }
}

impl std::error::Error for OrderInfoRejected {}

/// Serialized `AdminOrderInfo` request. mostro-core only accepts this action
/// in the `order` wrapper (unlike the other admin actions, which use `dispute`).
fn build_admin_order_info_request(order_id: Uuid, request_id: u64) -> Result<String> {
    let message = Message::new_order(
        Some(order_id),
        Some(request_id),
        None,
        Action::AdminOrderInfo,
        None,
    );
    if !message.verify() {
        return Err(anyhow::anyhow!("Invalid AdminOrderInfo request"));
    }
    message
        .as_json()
        .map_err(|_| anyhow::anyhow!("Failed to serialize message"))
}

/// Validate Mostro's reply to the request `request_id` about `order_id` and
/// extract the snapshot.
fn order_info_from_response(
    response: &Message,
    order_id: Uuid,
    request_id: u64,
) -> Result<OrderInfo> {
    let inner = match handle_mostro_response(response, request_id) {
        Ok(inner) => inner,
        Err(e) => {
            return Err(match e.downcast_ref::<MostroCantDoError>() {
                Some(cant) => anyhow::Error::new(OrderInfoRejected {
                    order_id,
                    reason: cant.reason.clone(),
                }),
                None => e,
            });
        }
    };

    if inner.action != Action::AdminOrderInfo {
        return Err(anyhow::anyhow!(
            "Received response with mismatched action. Expected: {:?}, Got: {:?}",
            Action::AdminOrderInfo,
            inner.action
        ));
    }
    // Rejects a reply outside the `order` wrapper and one whose snapshot is
    // about another order than its `id`.
    if !response.verify() {
        return Err(anyhow::anyhow!(
            "Received an invalid AdminOrderInfo response"
        ));
    }
    let Some(Payload::OrderInfo(info)) = &inner.payload else {
        return Err(anyhow::anyhow!(
            "Received AdminOrderInfo response without an order snapshot"
        ));
    };
    if info.id != order_id {
        return Err(anyhow::anyhow!(
            "Order ID mismatch: expected {order_id}, got {}",
            info.id
        ));
    }
    Ok(info.clone())
}

/// Ask Mostro for the real state of `order_id` (`AdminOrderInfo`).
///
/// Read-only: nothing changes on the daemon or locally. Allowed for the admin
/// key and any registered solver (`read` or `read-write`), on any order of the
/// node in any status. Database and Lightning values in the returned
/// [`OrderInfo`] are side by side and may disagree on purpose.
///
/// # Errors
///
/// - Failed to serialize, send or receive the DM
/// - Response from the wrong sender, or `request_id` / action / order mismatch
/// - [`OrderInfoRejected`] when Mostro replies `CantDo` (`invalid_pubkey`:
///   not admin nor solver; `not_found`: unknown order)
pub async fn execute_admin_order_info(
    order_id: &Uuid,
    admin_keys: &Keys,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<OrderInfo> {
    let request_id = Uuid::new_v4().as_u128() as u64;
    let request = build_admin_order_info_request(*order_id, request_id)?;

    let sent_message = send_dm(
        client,
        Some(admin_keys),
        admin_keys,
        &mostro_pubkey,
        request,
        None,
        mostro_instance,
    );

    let recv_event = wait_for_dm(
        admin_keys,
        FETCH_EVENTS_TIMEOUT,
        Some(request_id),
        sent_message,
    )
    .await?;
    let messages = parse_dm_events(recv_event, admin_keys, None).await;
    let Some((response_message, _, sender_pubkey)) = messages.first() else {
        return Err(anyhow::anyhow!("No response received from Mostro"));
    };
    if *sender_pubkey != mostro_pubkey {
        return Err(anyhow::anyhow!("Received response from wrong sender"));
    }

    let info = order_info_from_response(response_message, *order_id, request_id)?;
    log::info!(
        "Admin order info for {order_id}: status {}, hold invoice {:?}, payout {:?}",
        info.status,
        info.hold_invoice.as_ref().map(|h| h.state),
        info.payout.as_ref().map(|p| p.state)
    );
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::uuid;

    const ORDER: Uuid = uuid!("308e1272-d5f4-47e6-bd97-3504baea9c23");
    const RID: u64 = 0xC0FF_EE00_1234_5678;

    fn info(id: Uuid) -> OrderInfo {
        OrderInfo {
            id,
            kind: "sell".into(),
            status: "dispute".into(),
            hold_invoice: Some(HoldInvoiceInfo {
                state: HoldInvoiceState::Accepted,
                held_at: Some(1),
            }),
            ..Default::default()
        }
    }

    fn reply(id: Uuid, request_id: u64, payload: Option<Payload>) -> Message {
        Message::new_order(
            Some(id),
            Some(request_id),
            None,
            Action::AdminOrderInfo,
            payload,
        )
    }

    #[test]
    fn request_uses_the_order_wrapper_without_payload() {
        let json = build_admin_order_info_request(ORDER, RID).expect("request");
        let msg = Message::from_json(&json).expect("parse");
        assert!(matches!(msg, Message::Order(_)), "got {msg:?}");
        assert!(msg.verify());
        let inner = msg.get_inner_message_kind();
        assert_eq!(inner.action, Action::AdminOrderInfo);
        assert_eq!(inner.id, Some(ORDER));
        assert_eq!(inner.request_id, Some(RID));
        assert!(inner.payload.is_none());
    }

    #[test]
    fn matching_reply_yields_the_snapshot() {
        let msg = reply(ORDER, RID, Some(Payload::OrderInfo(info(ORDER))));
        let got = order_info_from_response(&msg, ORDER, RID).expect("snapshot");
        assert_eq!(got, info(ORDER));
    }

    #[test]
    fn reply_about_another_order_is_rejected() {
        let other = Uuid::new_v4();
        let msg = reply(other, RID, Some(Payload::OrderInfo(info(other))));
        assert!(order_info_from_response(&msg, ORDER, RID).is_err());
    }

    #[test]
    fn reply_with_inconsistent_snapshot_id_is_rejected() {
        let msg = reply(ORDER, RID, Some(Payload::OrderInfo(info(Uuid::new_v4()))));
        assert!(order_info_from_response(&msg, ORDER, RID).is_err());
    }

    #[test]
    fn reply_outside_order_wrapper_is_rejected() {
        let msg = Message::new_dispute(
            Some(ORDER),
            Some(RID),
            None,
            Action::AdminOrderInfo,
            Some(Payload::OrderInfo(info(ORDER))),
        );
        assert!(order_info_from_response(&msg, ORDER, RID).is_err());
    }

    #[test]
    fn reply_without_snapshot_or_with_other_action_is_rejected() {
        assert!(order_info_from_response(&reply(ORDER, RID, None), ORDER, RID).is_err());
        let wrong_action =
            Message::new_order(Some(ORDER), Some(RID), None, Action::AdminSettled, None);
        assert!(order_info_from_response(&wrong_action, ORDER, RID).is_err());
    }

    #[test]
    fn cant_do_maps_to_a_readable_rejection() {
        for (reason, expected) in [
            (
                CantDoReason::InvalidPubkey,
                "neither the admin nor a registered solver",
            ),
            (CantDoReason::NotFound, "Mostro has no order"),
        ] {
            let msg = Message::cant_do(
                Some(ORDER),
                Some(RID),
                Some(Payload::CantDo(Some(reason.clone()))),
            );
            let err = order_info_from_response(&msg, ORDER, RID).expect_err("cant-do");
            let rejected = err
                .downcast_ref::<OrderInfoRejected>()
                .expect("typed rejection");
            assert_eq!(rejected.reason, reason);
            assert!(err.to_string().contains(expected), "got {err}");
        }
    }
}
