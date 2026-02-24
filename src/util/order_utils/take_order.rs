// Take order functionality
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;

use crate::models::User;
use crate::settings::Settings;
use crate::util::db_utils::save_order;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::order_utils::helper::{create_order_result_success, handle_mostro_response};

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
pub async fn take_order(
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    _settings: &Settings,
    mostro_pubkey: PublicKey,
    order: &SmallOrder,
    amount: Option<i64>,
    invoice: Option<String>,
) -> Result<crate::ui::OperationResult, anyhow::Error> {
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

    // Get user and trade keys
    let user = User::get(pool).await?;
    let next_idx = user.last_trade_index.unwrap_or(1) + 1;
    let trade_keys = user.derive_trade_keys(next_idx)?;
    let _ = User::update_last_trade_index(pool, next_idx).await;

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
        false,
    );

    // Wait for Mostro response (subscribes first, then sends message to avoid missing messages)
    let recv_event = wait_for_dm(client, &trade_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;

    // Parse DM events
    let messages = parse_dm_events(recv_event, &trade_keys, None).await;

    if let Some((response_message, _, _)) = messages.first() {
        let inner_message = handle_mostro_response(response_message, request_id)?;

        match inner_message.request_id {
            Some(id) => {
                if request_id == id {
                    // Request ID matches, process the response
                    match &inner_message.payload {
                        Some(Payload::Order(returned_order)) => {
                            // Save order to database
                            if let Err(e) = save_order(
                                returned_order.clone(),
                                &trade_keys,
                                request_id,
                                next_idx,
                                pool,
                            )
                            .await
                            {
                                log::error!("Failed to save order to database: {}", e);
                            }
                            Ok(create_order_result_success(returned_order, next_idx))
                        }
                        Some(Payload::PaymentRequest(opt_order, invoice_string, opt_amount)) => {
                            // For buy orders, we receive PaymentRequest with invoice for seller to pay
                            // Use the order from payload if available, otherwise use the original order
                            let order_to_save = if let Some(order_to_save) = opt_order {
                                order_to_save
                            } else {
                                return Err(anyhow::anyhow!(
                                    "Order details are missing from payload"
                                ));
                            };

                            // Save order to database
                            if let Err(e) = save_order(
                                order_to_save.clone(),
                                &trade_keys,
                                request_id,
                                next_idx,
                                pool,
                            )
                            .await
                            {
                                log::error!("Failed to save order to database: {}", e);
                            }

                            log::info!(
                                "Received PaymentRequest for buy order {} with invoice",
                                order_id
                            );

                            // Return PaymentRequestRequired to trigger invoice popup
                            Ok(crate::ui::OperationResult::PaymentRequestRequired {
                                order: order_to_save.clone(),
                                invoice: invoice_string.clone(),
                                sat_amount: *opt_amount,
                                trade_index: next_idx,
                            })
                        }
                        _ => {
                            log::warn!(
                                "Received response without order details or payment request"
                            );
                            Err(anyhow::anyhow!(
                                "Response without order details or payment request"
                            ))
                        }
                    }
                } else {
                    Err(anyhow::anyhow!("Mismatched request_id"))
                }
            }
            None => Err(anyhow::anyhow!("Response with null request_id")),
        }
    } else {
        log::error!("No response received from Mostro");
        Err(anyhow::anyhow!("No response received from Mostro"))
    }
}
