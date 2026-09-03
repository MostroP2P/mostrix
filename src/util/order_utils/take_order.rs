// Take order functionality
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;

use crate::models::User;
use crate::ui::orders::{order_message_to_notification, OperationResult, OrderMessage};
use crate::util::db_utils::save_order;
use crate::util::dm_utils::{
    parse_dm_events, send_dm, send_track_order_cmd, wait_for_dm, FETCH_EVENTS_TIMEOUT,
};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::order_utils::helper::{handle_mostro_response, payment_request_operation_result};
use crate::util::OrderDmSubscriptionCmd;
use tokio::sync::mpsc::UnboundedSender;

/// Create payload based on action type and parameters
fn create_take_order_payload(
    action: Action,
    invoice: &Option<String>,
    amount: Option<i64>,
) -> Result<Option<Payload>> {
    match action {
        Action::TakeBuy => Ok(amount.map(Payload::Amount)),
        Action::TakeSell => Ok(Some(match invoice {
            Some(inv) => {
                // For TakeSell with invoice, create PaymentRequest
                // If amount is provided (for range orders), include it
                match amount {
                    Some(amt) => Payload::PaymentRequest(None, inv.clone(), Some(amt)),
                    None => Payload::PaymentRequest(None, inv.clone(), None),
                }
            }
            None => amount.map(Payload::Amount).unwrap_or(Payload::Amount(0)),
        })),
        _ => Err(anyhow::anyhow!("Invalid action for take order")),
    }
}

/// Take an order from the order book
#[allow(clippy::too_many_arguments)]
pub async fn take_order(
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    order: &SmallOrder,
    amount: Option<i64>,
    invoice: Option<String>,
    dm_subscription_tx: Option<&UnboundedSender<OrderDmSubscriptionCmd>>,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<OperationResult, anyhow::Error> {
    // Determine action based on order kind
    let action = match order.kind {
        Some(mostro_core::order::Kind::Buy) => {
            // Taking a Buy order = Selling (need invoice for TakeSell)
            Action::TakeBuy
        }
        Some(mostro_core::order::Kind::Sell) => {
            // Taking a Sell order = Buying (provide amount if range)
            Action::TakeSell
        }
        None => {
            return Err(anyhow::anyhow!("Order kind is not specified"));
        }
    };

    let order_id = order
        .id
        .ok_or_else(|| anyhow::anyhow!("Order ID is missing"))?;

    // Reserve the next trade index atomically; propagate DB errors (e.g. SQLITE_BUSY).
    let (next_idx, trade_keys) = User::reserve_next_trade_index(pool, 1).await?;

    // Subscribe as early as possible for take-order flow so the first
    // Mostro response/event is not missed by the background DM listener.
    if dm_subscription_tx.is_some() {
        // Optimistic TrackOrder via the **current** global DM router sender (not a
        // possibly-stale main-loop clone). Intentionally redundant with the post-
        // `save_order` send below.
        log::info!(
            "[take_order] Early subscribe command for order_id={}, trade_index={}",
            order_id,
            next_idx
        );
        send_track_order_cmd(order_id, next_idx);
    }

    // Create payload based on action type
    let payload = create_take_order_payload(action.clone(), &invoice, amount)?;

    // Create request id
    let request_id = uuid::Uuid::new_v4().as_u128() as u64;

    // Create message
    let take_order_message = Message::new_order(
        Some(order_id),
        Some(request_id),
        Some(next_idx),
        action.clone(),
        payload,
    );

    log::info!(
        "Taking order {} with trade index {} and request_id {}",
        order_id,
        next_idx,
        request_id
    );

    // Serialize message
    let message_json = take_order_message
        .as_json()
        .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    let identity_keys = User::get_identity_keys(pool).await?;

    // Send the DM (this returns a future)
    let sent_message = send_dm(
        client,
        Some(&identity_keys),
        &trade_keys,
        &mostro_pubkey,
        message_json,
        None,
        mostro_instance,
    );

    // Wait for Mostro response (subscribes first, then sends message to avoid missing messages)
    let recv_event = wait_for_dm(&trade_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;

    // Parse DM events
    let messages = parse_dm_events(recv_event, &trade_keys, None).await;

    if let Some((response_message, timestamp, sender)) = messages.first() {
        let inner_message = handle_mostro_response(response_message, request_id)?;

        match inner_message.request_id {
            Some(id) if request_id == id => {
                process_take_order_reply(
                    inner_message,
                    response_message,
                    *timestamp,
                    *sender,
                    order_id,
                    request_id,
                    next_idx,
                    pool,
                    &trade_keys,
                    dm_subscription_tx,
                )
                .await
            }
            Some(_) => Err(anyhow::anyhow!("Mismatched request_id")),
            None => Err(anyhow::anyhow!("Response with null request_id")),
        }
    } else {
        log::error!("No response received from Mostro");
        Err(anyhow::anyhow!("No response received from Mostro"))
    }
}

/// Dispatch a take-order Mostro reply by **action** (not payload alone).
///
/// Take-sell without a buyer invoice is `AddInvoice` + `Payload::Order`; treating that as
/// create-order `Success` showed "Order Created Successfully".
#[allow(clippy::too_many_arguments)]
async fn process_take_order_reply(
    inner_message: &mostro_core::message::MessageKind,
    response_message: &Message,
    timestamp: i64,
    sender: PublicKey,
    fallback_order_id: uuid::Uuid,
    request_id: u64,
    next_idx: i64,
    pool: &sqlx::sqlite::SqlitePool,
    trade_keys: &Keys,
    dm_subscription_tx: Option<&UnboundedSender<OrderDmSubscriptionCmd>>,
) -> Result<OperationResult> {
    match map_take_reply(&inner_message.action, &inner_message.payload)? {
        MappedTakeReply::AddInvoice(returned_order) => {
            let normalized = persist_taken_order(
                returned_order,
                fallback_order_id,
                request_id,
                next_idx,
                pool,
                trade_keys,
                dm_subscription_tx,
            )
            .await;
            Ok(take_add_invoice_operation_result(
                response_message,
                &normalized,
                timestamp,
                sender,
                next_idx,
            ))
        }
        MappedTakeReply::PaymentRequest {
            action,
            order,
            invoice,
            amount,
        } => {
            payment_request_operation_result(
                action,
                order,
                invoice,
                amount,
                Some(fallback_order_id),
                request_id,
                next_idx,
                pool,
                trade_keys,
                false,
                dm_subscription_tx,
                "take_order",
            )
            .await
        }
    }
}

#[derive(Debug)]
enum MappedTakeReply {
    AddInvoice(SmallOrder),
    PaymentRequest {
        action: Action,
        order: Option<SmallOrder>,
        invoice: String,
        amount: Option<i64>,
    },
}

fn map_take_reply(action: &Action, payload: &Option<Payload>) -> Result<MappedTakeReply> {
    match (action, payload) {
        (Action::AddInvoice, Some(Payload::Order(order))) => {
            Ok(MappedTakeReply::AddInvoice(order.clone()))
        }
        (Action::AddInvoice, _) => Err(anyhow::anyhow!(
            "Mostro replied with AddInvoice but no Order payload was provided"
        )),
        (
            Action::PayInvoice | Action::PayBondInvoice,
            Some(Payload::PaymentRequest(opt_order, invoice, opt_amount)),
        ) => Ok(MappedTakeReply::PaymentRequest {
            action: action.clone(),
            order: opt_order.clone(),
            invoice: invoice.clone(),
            amount: *opt_amount,
        }),
        (Action::PayInvoice | Action::PayBondInvoice, _) => Err(anyhow::anyhow!(
            "Mostro replied with {:?} but no PaymentRequest payload was provided",
            action
        )),
        (other, _) => {
            log::warn!("Received unexpected take-order action: {other:?}");
            Err(anyhow::anyhow!("Unexpected take-order action: {other:?}"))
        }
    }
}

fn normalize_taken_order(mut order: SmallOrder, fallback_order_id: uuid::Uuid) -> SmallOrder {
    if order.id.is_none() {
        log::warn!(
            "[take_order] Mostro response Order payload missing id; falling back to requested order_id={}",
            fallback_order_id
        );
        order.id = Some(fallback_order_id);
    }
    order
}

async fn persist_taken_order(
    returned_order: SmallOrder,
    fallback_order_id: uuid::Uuid,
    request_id: u64,
    next_idx: i64,
    pool: &sqlx::sqlite::SqlitePool,
    trade_keys: &Keys,
    dm_subscription_tx: Option<&UnboundedSender<OrderDmSubscriptionCmd>>,
) -> SmallOrder {
    let normalized = normalize_taken_order(returned_order, fallback_order_id);
    let effective_order_id = normalized.id.unwrap_or(fallback_order_id);
    log::info!(
        "[take_order] Action::AddInvoice mapped to effective_order_id={}, trade_index={}",
        effective_order_id,
        next_idx
    );

    if let Err(e) = save_order(
        normalized.clone(),
        trade_keys,
        request_id,
        next_idx,
        pool,
        false,
    )
    .await
    {
        log::error!("Failed to save order to database: {}", e);
    }
    if dm_subscription_tx.is_some() {
        log::info!(
            "[take_order] Sending DM subscription command for order_id={}, trade_index={}",
            effective_order_id,
            next_idx
        );
        send_track_order_cmd(effective_order_id, next_idx);
    }
    normalized
}

/// Open the Add Invoice UI for a take-sell reply (`AddInvoice` + `Payload::Order`).
///
/// `auto_popup_shown` is set so a later copy of the same DM from the trade-key listener
/// does not open a second popup.
fn take_add_invoice_operation_result(
    response_message: &Message,
    order: &SmallOrder,
    timestamp: i64,
    sender: PublicKey,
    trade_index: i64,
) -> OperationResult {
    let order_id = order.id;
    let order_status = order
        .status
        .or(Some(mostro_core::order::Status::WaitingBuyerInvoice));
    let order_message = OrderMessage {
        message: response_message.clone(),
        timestamp,
        sender,
        order_id,
        trade_index,
        sat_amount: Some(order.amount),
        buyer_invoice: None,
        order_kind: order.kind,
        is_mine: Some(false),
        order_status,
        order_snapshot: Some(order.clone()),
        read: true,
        auto_popup_shown: true,
    };
    let notification = order_message_to_notification(&order_message);
    OperationResult::OpenInvoicePopup {
        notification,
        order_message: Box::new(order_message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mostro_core::prelude::{Action, Payload, Status};

    fn sample_small_order(id: uuid::Uuid) -> SmallOrder {
        SmallOrder {
            id: Some(id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::WaitingBuyerInvoice),
            amount: 21_000,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn map_take_reply_add_invoice_order_is_not_success() {
        let order = sample_small_order(uuid::Uuid::new_v4());
        let mapped = map_take_reply(&Action::AddInvoice, &Some(Payload::Order(order.clone())))
            .expect("AddInvoice+Order must map");
        match mapped {
            MappedTakeReply::AddInvoice(o) => assert_eq!(o.id, order.id),
            MappedTakeReply::PaymentRequest { .. } => panic!("must not treat AddInvoice as pay"),
        }
    }

    #[test]
    fn map_take_reply_rejects_new_order_as_take_success() {
        let order = sample_small_order(uuid::Uuid::new_v4());
        let err = map_take_reply(&Action::NewOrder, &Some(Payload::Order(order)))
            .expect_err("NewOrder must not be a take success");
        assert!(err.to_string().contains("Unexpected take-order action"));
    }

    #[test]
    fn map_take_reply_pay_invoice_requires_payment_request() {
        let err = map_take_reply(
            &Action::PayInvoice,
            &Some(Payload::Order(sample_small_order(uuid::Uuid::new_v4()))),
        )
        .expect_err("PayInvoice with Order payload is invalid");
        assert!(err.to_string().contains("PaymentRequest"));
    }

    #[test]
    fn take_add_invoice_opens_invoice_popup_not_created_success() {
        let order_id = uuid::Uuid::new_v4();
        let order = sample_small_order(order_id);
        let message = Message::new_order(
            Some(order_id),
            Some(1),
            Some(2),
            Action::AddInvoice,
            Some(Payload::Order(order.clone())),
        );
        let sender = Keys::generate().public_key();
        let result = take_add_invoice_operation_result(&message, &order, 1, sender, 2);
        match result {
            OperationResult::OpenInvoicePopup {
                notification,
                order_message,
            } => {
                assert_eq!(notification.action, Action::AddInvoice);
                assert_eq!(notification.order_id, Some(order_id));
                assert_eq!(
                    order_message.message.get_inner_message_kind().action,
                    Action::AddInvoice
                );
                assert_eq!(order_message.is_mine, Some(false));
                assert!(order_message.auto_popup_shown);
            }
            other => panic!("expected OpenInvoicePopup, got {other:?}"),
        }
    }
}
