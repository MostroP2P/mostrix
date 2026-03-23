// Take order functionality
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;

use crate::models::User;
use crate::settings::Settings;
use crate::ui::OperationResult;
use crate::util::db_utils::save_order;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::order_utils::helper::{create_order_result_success, handle_mostro_response};
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
    _settings: &Settings,
    mostro_pubkey: PublicKey,
    order: &SmallOrder,
    amount: Option<i64>,
    invoice: Option<String>,
    dm_subscription_tx: Option<&UnboundedSender<OrderDmSubscriptionCmd>>,
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

    // Subscribe as early as possible for take-order flow so the first
    // Mostro response/event is not missed by the background DM listener.
    if let Some(tx) = dm_subscription_tx {
        log::info!(
            "[take_order] Early subscribe command for order_id={}, trade_index={}",
            order_id,
            next_idx
        );
        let _ = tx.send(OrderDmSubscriptionCmd::TrackOrder {
            order_id,
            trade_index: next_idx,
        });
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
                            let mut normalized_order = returned_order.clone();
                            if normalized_order.id.is_none() {
                                log::warn!(
                                    "[take_order] Mostro response Order payload missing id; falling back to requested order_id={}",
                                    order_id
                                );
                                normalized_order.id = Some(order_id);
                            }
                            let effective_order_id = normalized_order.id.unwrap_or(order_id);
                            log::info!(
                                "[take_order] Action::Order response mapped to effective_order_id={}, trade_index={}",
                                effective_order_id,
                                next_idx
                            );

                            // Save order to database
                            if let Err(e) = save_order(
                                normalized_order.clone(),
                                &trade_keys,
                                request_id,
                                next_idx,
                                pool,
                                false,
                            )
                            .await
                            {
                                log::error!("Failed to save order to database: {}", e);
                            }
                            if let Some(tx) = dm_subscription_tx {
                                log::info!(
                                    "[take_order] Sending DM subscription command for order_id={}, trade_index={}",
                                    effective_order_id,
                                    next_idx
                                );
                                let _ = tx.send(OrderDmSubscriptionCmd::TrackOrder {
                                    order_id: effective_order_id,
                                    trade_index: next_idx,
                                });
                            }
                            Ok(create_order_result_success(&normalized_order, next_idx))
                        }
                        Some(Payload::PaymentRequest(opt_order, invoice_string, opt_amount)) => {
                            // For buy orders, we receive PaymentRequest with invoice for seller to pay
                            // Use the order from payload if available, otherwise use the original order
                            let mut order_to_save = if let Some(order_to_save) = opt_order {
                                order_to_save.clone()
                            } else {
                                return Err(anyhow::anyhow!(
                                    "Order details are missing from payload"
                                ));
                            };
                            if order_to_save.id.is_none() {
                                log::warn!(
                                    "[take_order] Mostro PaymentRequest payload order missing id; falling back to requested order_id={}",
                                    order_id
                                );
                                order_to_save.id = Some(order_id);
                            }
                            let effective_order_id = order_to_save.id.unwrap_or(order_id);
                            log::info!(
                                "[take_order] Action::PaymentRequest response mapped to effective_order_id={}, trade_index={}",
                                effective_order_id,
                                next_idx
                            );

                            // Save order to database
                            if let Err(e) = save_order(
                                order_to_save.clone(),
                                &trade_keys,
                                request_id,
                                next_idx,
                                pool,
                                false,
                            )
                            .await
                            {
                                log::error!("Failed to save order to database: {}", e);
                            }
                            if let Some(tx) = dm_subscription_tx {
                                log::info!(
                                    "[take_order] Sending DM subscription command for order_id={}, trade_index={}",
                                    effective_order_id,
                                    next_idx
                                );
                                let _ = tx.send(OrderDmSubscriptionCmd::TrackOrder {
                                    order_id: effective_order_id,
                                    trade_index: next_idx,
                                });
                            }

                            log::info!(
                                "Received PaymentRequest for buy order {} with invoice",
                                order_id
                            );

                            // Return PaymentRequestRequired to trigger invoice popup
                            Ok(OperationResult::PaymentRequestRequired {
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
