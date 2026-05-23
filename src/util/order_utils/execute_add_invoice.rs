// Execute add invoice / add bond payout invoice functionality
use anyhow::Result;
use lightning_invoice::Bolt11Invoice as Invoice;
use lnurl::lightning_address::LightningAddress;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::str::FromStr;
use uuid::Uuid;

use crate::models::Order;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::order_utils::helper::handle_mostro_response;

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

    let recv_event = match wait_for_dm(&order_trade_keys, FETCH_EVENTS_TIMEOUT, sent_message).await
    {
        Ok(events) => events,
        Err(e) if action == Action::AddBondInvoice && is_wait_for_dm_timeout(&e) => {
            // Mostro may accept the bolt11 without a follow-up DM.
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let messages = parse_dm_events(recv_event, &order_trade_keys, None).await;

    let Some((response_message, _, _)) = messages.first() else {
        if action == Action::AddBondInvoice {
            // Mostro may accept the bolt11 without a follow-up DM.
            return Ok(());
        }
        return Err(anyhow::anyhow!(
            "No response received from Mostro for {:?}",
            action
        ));
    };

    let inner_message = handle_mostro_response(response_message, request_id)?;

    if action == Action::AddBondInvoice {
        return Ok(());
    }

    match inner_message.action {
        Action::WaitingSellerToPay | Action::HoldInvoicePaymentAccepted => Ok(()),
        _ => Err(anyhow::anyhow!(
            "Unexpected action: {:?}",
            inner_message.action
        )),
    }
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
) -> Result<()> {
    execute_payment_request_reply(
        order_id,
        invoice,
        Action::AddBondInvoice,
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
