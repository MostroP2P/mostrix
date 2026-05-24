// Execute add invoice / add bond payout invoice functionality
use anyhow::Result;
use lightning_invoice::Bolt11Invoice as Invoice;
use lnurl::lightning_address::LightningAddress;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::str::FromStr;
use uuid::Uuid;

use crate::models::Order;
use crate::ui::orders::{order_message_to_notification, OperationResult, OrderMessage};
use crate::util::db_utils::{save_order, update_order_status};
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::order_utils::helper::{
    build_order_chat_static_header, handle_mostro_response, inferred_status_from_trade_action,
};

/// Matches the timeout branch in [`wait_for_dm`].
fn is_wait_for_dm_timeout(err: &anyhow::Error) -> bool {
    err.to_string() == "Timeout waiting for DM or gift wrap event"
}

/// Verify if an invoice is valid
fn is_valid_invoice(payment_request: &str) -> Result<Invoice, anyhow::Error> {
    let invoice =
        Invoice::from_str(payment_request).map_err(|_| anyhow::anyhow!("Invalid invoice"))?;
    if invoice.is_expired() {
        return Err(anyhow::anyhow!("Invoice expired"));
    }

    Ok(invoice)
}

async fn payment_request_payload_for_invoice(invoice: &str) -> Result<Option<Payload>> {
    let ln_addr = LightningAddress::from_str(invoice.trim());
    if ln_addr.is_ok() {
        crate::util::ln_address::ln_address_pay_request_reachable(invoice.trim())
            .await
            .map_err(|e| anyhow::anyhow!("Lightning address not verified: {}", e))?;
        Ok(Some(Payload::PaymentRequest(
            None,
            invoice.trim().to_string(),
            None,
        )))
    } else {
        match is_valid_invoice(invoice) {
            Ok(i) => Ok(Some(Payload::PaymentRequest(None, i.to_string(), None))),
            Err(e) => Err(anyhow::anyhow!("Invalid invoice: {}", e)),
        }
    }
}

async fn persist_order_payload_from_dm(
    pool: &sqlx::sqlite::SqlitePool,
    order_id: Uuid,
    action: &Action,
    payload: &Option<Payload>,
    request_id: u64,
    trade_keys: &Keys,
) {
    let msg_request_id = i64::try_from(request_id).ok();
    let small_order = match (action, payload.as_ref()) {
        (Action::AddInvoice, Some(Payload::Order(o))) => Some(o.clone()),
        (Action::PayInvoice, Some(Payload::PaymentRequest(Some(o), _, _))) => Some(o.clone()),
        (Action::PayBondInvoice, Some(Payload::PaymentRequest(Some(o), _, _))) => Some(o.clone()),
        (Action::HoldInvoicePaymentAccepted, Some(Payload::Order(o))) => Some(o.clone()),
        (Action::WaitingBuyerInvoice, Some(Payload::Order(o))) => Some(o.clone()),
        _ => None,
    };
    if let Some(order) = small_order {
        let _ =
            Order::upsert_from_small_order_dm(pool, order_id, order, trade_keys, msg_request_id)
                .await;
    }
}

fn order_kind_from_db(order: &Order) -> Option<mostro_core::order::Kind> {
    order
        .kind
        .as_ref()
        .and_then(|k| mostro_core::order::Kind::from_str(k).ok())
}

fn status_from_db(order: &Order) -> Option<Status> {
    order.status.as_ref().and_then(|s| Status::from_str(s).ok())
}

fn build_order_message_from_reply(
    response_message: &Message,
    timestamp: i64,
    sender: PublicKey,
    order_id: Uuid,
    trade_index: i64,
    db_order: &Order,
) -> OrderMessage {
    let inner = response_message.get_inner_message_kind();
    let (sat_amount, buyer_invoice) = match (&inner.action, &inner.payload) {
        (
            Action::PayInvoice | Action::PayBondInvoice,
            Some(Payload::PaymentRequest(opt, inv, amt)),
        ) => (
            amt.or_else(|| opt.as_ref().map(|o| o.amount)),
            Some(inv.clone()),
        ),
        (Action::AddInvoice, Some(Payload::Order(o))) => (Some(o.amount), None),
        _ => (None, None),
    };
    let status = inner
        .payload
        .as_ref()
        .and_then(|p| match p {
            Payload::Order(o) => o.status,
            _ => None,
        })
        .or_else(|| inferred_status_from_trade_action(&inner.action))
        .or(status_from_db(db_order));

    OrderMessage {
        message: response_message.clone(),
        timestamp,
        sender,
        order_id: Some(order_id),
        trade_index,
        sat_amount,
        buyer_invoice,
        order_kind: order_kind_from_db(db_order),
        is_mine: Some(db_order.is_mine),
        order_status: status,
        read: false,
        auto_popup_shown: false,
    }
}

async fn apply_status_from_reply(
    pool: &sqlx::sqlite::SqlitePool,
    order_id: Uuid,
    action: &Action,
    payload: &Option<Payload>,
    baseline: Option<Status>,
) {
    let candidate = if let Some(Payload::Order(o)) = payload {
        o.status
            .or_else(|| inferred_status_from_trade_action(action))
    } else {
        inferred_status_from_trade_action(action)
    };
    if let Some(status) = candidate {
        if baseline != Some(status) {
            let _ = update_order_status(pool, &order_id.to_string(), status).await;
        }
    }
}

struct BondInvoiceReplyCtx<'a> {
    response_message: &'a Message,
    timestamp: i64,
    sender: PublicKey,
    order_id: Uuid,
    db_order: &'a Order,
    order_trade_keys: &'a Keys,
    pool: &'a sqlx::sqlite::SqlitePool,
    request_id: u64,
}

/// Map a successful `AddBondInvoice` DM reply into the next UI step when Mostro sends one.
async fn operation_result_from_bond_invoice_reply(
    ctx: BondInvoiceReplyCtx<'_>,
) -> Result<Option<OperationResult>> {
    handle_mostro_response(ctx.response_message, ctx.request_id)?;
    let inner = ctx.response_message.get_inner_message_kind();
    let trade_index = ctx
        .db_order
        .trade_index
        .ok_or_else(|| anyhow::anyhow!("Missing trade_index for order"))?;

    persist_order_payload_from_dm(
        ctx.pool,
        ctx.order_id,
        &inner.action,
        &inner.payload,
        ctx.request_id,
        ctx.order_trade_keys,
    )
    .await;
    apply_status_from_reply(
        ctx.pool,
        ctx.order_id,
        &inner.action,
        &inner.payload,
        status_from_db(ctx.db_order),
    )
    .await;

    let order_msg = build_order_message_from_reply(
        ctx.response_message,
        ctx.timestamp,
        ctx.sender,
        ctx.order_id,
        trade_index,
        ctx.db_order,
    );

    match (&inner.action, &inner.payload) {
        (
            Action::PayBondInvoice | Action::PayInvoice,
            Some(Payload::PaymentRequest(opt_order, invoice, opt_amount)),
        ) => {
            let mut order_to_save = opt_order
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Order details missing from PaymentRequest"))?;
            if order_to_save.id.is_none() {
                order_to_save.id = Some(ctx.order_id);
            }
            let _ = save_order(
                order_to_save.clone(),
                ctx.order_trade_keys,
                ctx.request_id,
                trade_index,
                ctx.pool,
                ctx.db_order.is_mine,
            )
            .await;
            let popup_action = if inner.action == Action::PayBondInvoice {
                Action::PayBondInvoice
            } else {
                Action::PayInvoice
            };
            let static_header = build_order_chat_static_header(
                &order_to_save,
                trade_index,
                ctx.order_trade_keys,
                ctx.db_order.is_mine,
            )
            .ok_or_else(|| anyhow::anyhow!("failed to build static header"))?;
            let sat_amount = opt_amount.or(Some(order_to_save.amount));
            Ok(Some(OperationResult::PaymentRequestRequired {
                order: order_to_save,
                invoice: invoice.clone(),
                sat_amount,
                trade_index,
                static_header,
                action: popup_action,
            }))
        }
        (Action::WaitingBuyerInvoice, _)
        | (Action::AddInvoice, _)
        | (Action::WaitingSellerToPay, _)
        | (Action::HoldInvoicePaymentAccepted, _)
        | (Action::BuyerInvoiceAccepted, _) => {
            let notification = order_message_to_notification(&order_msg);
            Ok(Some(OperationResult::OpenInvoicePopup {
                notification,
                order_message: Box::new(order_msg),
            }))
        }
        _ => {
            log::info!(
                "Bond payout invoice acknowledged by Mostro (action={:?})",
                inner.action
            );
            Ok(None)
        }
    }
}

async fn execute_payment_request_reply(
    order_id: &Uuid,
    invoice: &str,
    action: Action,
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<()> {
    let order = Order::get_by_id(pool, &order_id.to_string()).await?;
    let trade_keys = order
        .trade_keys
        .clone()
        .ok_or(anyhow::anyhow!("Missing trade keys"))?;
    let order_trade_keys = Keys::parse(&trade_keys)?;
    let payload = payment_request_payload_for_invoice(invoice).await?;

    let request_id = Uuid::new_v4().as_u128() as u64;
    let message = Message::new_order(
        Some(*order_id),
        Some(request_id),
        None,
        action.clone(),
        payload,
    );

    let identity_keys = crate::models::User::get_identity_keys(pool).await?;
    let message_json = message
        .as_json()
        .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    let sent_message = send_dm(
        client,
        Some(&identity_keys),
        &order_trade_keys,
        &mostro_pubkey,
        message_json,
        None,
        false,
        mostro_instance,
    );

    let recv_event = wait_for_dm(&order_trade_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;
    let messages = parse_dm_events(recv_event, &order_trade_keys, None).await;

    let Some((response_message, _, _)) = messages.first() else {
        return Err(anyhow::anyhow!(
            "No response received from Mostro for {:?}",
            action
        ));
    };

    let inner_message = handle_mostro_response(response_message, request_id)?;

    match inner_message.action {
        Action::WaitingSellerToPay | Action::HoldInvoicePaymentAccepted => Ok(()),
        _ => Err(anyhow::anyhow!(
            "Unexpected action: {:?}",
            inner_message.action
        )),
    }
}

async fn execute_bond_payment_request_reply(
    order_id: &Uuid,
    invoice: &str,
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<Option<OperationResult>> {
    let order = Order::get_by_id(pool, &order_id.to_string()).await?;
    let trade_keys = order
        .trade_keys
        .clone()
        .ok_or(anyhow::anyhow!("Missing trade keys"))?;
    let order_trade_keys = Keys::parse(&trade_keys)?;
    let payload = payment_request_payload_for_invoice(invoice).await?;

    let request_id = Uuid::new_v4().as_u128() as u64;
    let message = Message::new_order(
        Some(*order_id),
        Some(request_id),
        None,
        Action::AddBondInvoice,
        payload,
    );

    let identity_keys = crate::models::User::get_identity_keys(pool).await?;
    let message_json = message
        .as_json()
        .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    let sent_message = send_dm(
        client,
        Some(&identity_keys),
        &order_trade_keys,
        &mostro_pubkey,
        message_json,
        None,
        false,
        mostro_instance,
    );

    let recv_event = match wait_for_dm(&order_trade_keys, FETCH_EVENTS_TIMEOUT, sent_message).await
    {
        Ok(events) => events,
        Err(e) if is_wait_for_dm_timeout(&e) => {
            log::info!(
                "No DM within timeout after AddBondInvoice for order {} (Mostro may still process the invoice)",
                order_id
            );
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let messages = parse_dm_events(recv_event, &order_trade_keys, None).await;

    let Some((response_message, timestamp, sender_pubkey)) = messages.first() else {
        log::info!(
            "Empty DM after AddBondInvoice for order {} (invoice accepted without follow-up)",
            order_id
        );
        return Ok(None);
    };

    operation_result_from_bond_invoice_reply(BondInvoiceReplyCtx {
        response_message,
        timestamp: *timestamp,
        sender: *sender_pubkey,
        order_id: *order_id,
        db_order: &order,
        order_trade_keys: &order_trade_keys,
        pool,
        request_id,
    })
    .await
}

pub async fn execute_add_invoice(
    order_id: &Uuid,
    invoice: &str,
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<()> {
    execute_payment_request_reply(
        order_id,
        invoice,
        Action::AddInvoice,
        pool,
        client,
        mostro_pubkey,
        mostro_instance,
    )
    .await
}

pub async fn execute_add_bond_invoice(
    order_id: &Uuid,
    invoice: &str,
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<Option<OperationResult>> {
    execute_bond_payment_request_reply(
        order_id,
        invoice,
        pool,
        client,
        mostro_pubkey,
        mostro_instance,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_for_dm_timeout_is_recognized_for_add_bond_invoice() {
        let timeout = anyhow::anyhow!("Timeout waiting for DM or gift wrap event");
        assert!(is_wait_for_dm_timeout(&timeout));
        let canceled = anyhow::anyhow!("DM waiter canceled before receiving an event");
        assert!(!is_wait_for_dm_timeout(&canceled));
    }
}
