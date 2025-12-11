// Execute add invoice functionality
use anyhow::Result;
use lightning_invoice::Bolt11Invoice as Invoice;
use lnurl::lightning_address::LightningAddress;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::str::FromStr;
use uuid::Uuid;

use crate::models::Order;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::order_utils::helper::handle_mostro_response;

/// Verify if an invoice is valid
fn is_valid_invoice(payment_request: &str) -> Result<Invoice, anyhow::Error> {
    let invoice =
        Invoice::from_str(payment_request).map_err(|_| anyhow::anyhow!("Invalid invoice"))?;
    if invoice.is_expired() {
        return Err(anyhow::anyhow!("Invoice expired"));
    }

    Ok(invoice)
}

pub async fn execute_add_invoice(
    order_id: &Uuid,
    invoice: &str,
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
) -> Result<()> {
    // Get order from order id
    let order = Order::get_by_id(pool, &order_id.to_string()).await?;
    // Get trade keys of specific order
    let trade_keys = order
        .trade_keys
        .clone()
        .ok_or(anyhow::anyhow!("Missing trade keys"))?;

    let order_trade_keys = Keys::parse(&trade_keys)?;

    // Check invoice string
    let ln_addr = LightningAddress::from_str(invoice);
    let payload = if ln_addr.is_ok() {
        Some(Payload::PaymentRequest(None, invoice.to_string(), None))
    } else {
        match is_valid_invoice(invoice) {
            Ok(i) => Some(Payload::PaymentRequest(None, i.to_string(), None)),
            Err(e) => {
                return Err(anyhow::anyhow!("Invalid invoice: {}", e));
            }
        }
    };

    // Create request id
    let request_id = Uuid::new_v4().as_u128() as u64;
    // Create AddInvoice message
    let add_invoice_message = Message::new_order(
        Some(*order_id),
        Some(request_id),
        None,
        Action::AddInvoice,
        payload,
    );

    //
    let identity_keys = crate::models::User::get_identity_keys(pool).await?;

    // Serialize the message
    let message_json = add_invoice_message
        .as_json()
        .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    // Send the DM
    let sent_message = send_dm(
        client,
        Some(&identity_keys),
        &order_trade_keys,
        &mostro_pubkey,
        message_json,
        None,
        false,
    );

    // Wait for the DM to be sent from mostro
    let recv_event = wait_for_dm(
        client,
        &order_trade_keys,
        FETCH_EVENTS_TIMEOUT,
        sent_message,
    )
    .await?;

    let messages = parse_dm_events(recv_event, &order_trade_keys, None).await;

    // Handle the response
    let Some((response_message, _, _)) = messages.first() else {
        return Err(anyhow::anyhow!(
            "No response received from Mostro for AddInvoice"
        ));
    };
    let inner_message = handle_mostro_response(response_message, request_id)?;
    match inner_message.action {
        Action::WaitingSellerToPay => Ok(()),
        _ => Err(anyhow::anyhow!(
            "Unexpected action: {:?}",
            inner_message.action
        )),
    }
}
