// Send new order functionality
use anyhow::Result;
use mostro_core::prelude::{Kind as OrderKind, *};
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::str::FromStr;

use crate::models::User;
use crate::ui::FormState;
use crate::util::db_utils::save_order;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::order_utils::helper::{create_order_result_success, handle_mostro_response};
use crate::util::OrderDmSubscriptionCmd;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;

/// Send a new order to Mostro
pub async fn send_new_order(
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    form: FormState,
    dm_subscription_tx: Option<&UnboundedSender<OrderDmSubscriptionCmd>>,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<crate::ui::OperationResult, anyhow::Error> {
    // Parse form data
    let kind_str = if form.kind.trim().is_empty() {
        "buy".to_string()
    } else {
        form.kind.trim().to_lowercase()
    };
    let fiat_code = if form.fiat_code.trim().is_empty() {
        "USD".to_string()
    } else {
        form.fiat_code.trim().to_uppercase()
    };

    let amount: i64 = form.amount.trim().parse().unwrap_or(0);

    // Check if fiat currency is available on Yadio if amount is 0
    if amount == 0 {
        let api_req_string = "https://api.yadio.io/currencies".to_string();
        let fiat_list_check = reqwest::get(api_req_string)
            .await?
            .json::<HashMap<String, String>>()
            .await?
            .contains_key(&fiat_code);
        if !fiat_list_check {
            return Err(anyhow::anyhow!("{} is not present in the fiat market, please specify an amount with -a flag to fix the rate", fiat_code));
        }
    }

    let kind_checked =
        OrderKind::from_str(&kind_str).map_err(|_| anyhow::anyhow!("Invalid order kind"))?;

    let expiration_days: i64 = form.expiration_days.trim().parse().unwrap_or(0);
    let expires_at = match expiration_days {
        0 => return Err(anyhow::anyhow!("Minimum expiration time is 1 day")),
        _ => {
            let now = chrono::Utc::now();
            let expires_at = now + chrono::Duration::days(expiration_days);
            Some(expires_at.timestamp())
        }
    };

    // Handle fiat amount (single or range)
    let (fiat_amount, min_amount, max_amount) =
        if form.use_range && !form.fiat_amount_max.trim().is_empty() {
            let min: i64 = form.fiat_amount.trim().parse().unwrap_or(0);
            let max: i64 = form.fiat_amount_max.trim().parse().unwrap_or(0);
            (0, Some(min), Some(max))
        } else {
            let fiat: i64 = form.fiat_amount.trim().parse().unwrap_or(0);
            (fiat, None, None)
        };

    let payment_method = form.payment_method.trim().to_string();
    let premium: i64 = form.premium.trim().parse().unwrap_or(0);
    let invoice = if form.invoice.trim().is_empty() {
        None
    } else {
        Some(form.invoice.trim().to_string())
    };

    // Get user and trade keys
    let user = User::get(pool).await?;
    let next_idx = user.last_trade_index.unwrap_or(1) + 1;
    let trade_keys = user.derive_trade_keys(next_idx)?;
    let _ = User::update_last_trade_index(pool, next_idx).await;

    // Create SmallOrder
    let small_order = SmallOrder::new(
        None,
        Some(kind_checked),
        Some(Status::Pending),
        amount,
        fiat_code.clone(),
        min_amount,
        max_amount,
        fiat_amount,
        payment_method.clone(),
        premium,
        None,
        None,
        invoice.clone(),
        Some(0),
        expires_at,
    );

    // Create message
    let request_id = uuid::Uuid::new_v4().as_u128() as u64;
    let order_content = Payload::Order(small_order);
    let message = Message::new_order(
        None,
        Some(request_id),
        Some(next_idx),
        Action::NewOrder,
        Some(order_content),
    );

    // Serialize message
    let message_json = message
        .as_json()
        .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    log::info!(
        "Sending new order via DM with trade index {} and request_id {}",
        next_idx,
        request_id
    );

    let identity_keys = User::get_identity_keys(pool).await?;
    let new_order_message = send_dm(
        client,
        Some(&identity_keys),
        &trade_keys,
        &mostro_pubkey,
        message_json,
        None,
        false,
        mostro_instance,
    );

    // Wait for Mostro response (subscribes first, then sends message to avoid missing messages)
    let recv_event = wait_for_dm(&trade_keys, FETCH_EVENTS_TIMEOUT, new_order_message).await?;

    // Parse DM events
    let messages = parse_dm_events(recv_event, &trade_keys, None).await;

    if let Some((response_message, _, _)) = messages.first() {
        let inner_message = handle_mostro_response(response_message, request_id)?;

        match inner_message.request_id {
            Some(id) => {
                if request_id == id {
                    // Request ID matches, process the response
                    match inner_message.action {
                        Action::NewOrder => {
                            if let Some(Payload::Order(order)) = &inner_message.payload {
                                log::info!(
                                    "✅ Order created successfully! Order ID: {:?}",
                                    order.id
                                );

                                // Save order to database
                                if let Err(e) = save_order(
                                    order.clone(),
                                    &trade_keys,
                                    request_id,
                                    next_idx,
                                    pool,
                                    true,
                                )
                                .await
                                {
                                    log::error!("Failed to save order to database: {}", e);
                                }
                                if let Some(tx) = dm_subscription_tx {
                                    if let Some(order_id) = order.id {
                                        let _ = tx.send(OrderDmSubscriptionCmd::TrackOrder {
                                            order_id,
                                            trade_index: next_idx,
                                        });
                                    }
                                }

                                Ok(create_order_result_success(order, next_idx))
                            } else {
                                log::error!(
                                    "Mostro replied with Action::NewOrder but payload is missing/invalid. request_id={:?} trade_index={} payload={:?}",
                                    inner_message.request_id,
                                    next_idx,
                                    inner_message.payload
                                );
                                Err(anyhow::anyhow!(
                                    "Mostro replied with NewOrder but no order payload was provided"
                                ))
                            }
                        }
                        _ => {
                            log::warn!("Received unexpected action: {:?}", inner_message.action);
                            Err(anyhow::anyhow!(
                                "Unexpected action: {:?}",
                                inner_message.action
                            ))
                        }
                    }
                } else {
                    Err(anyhow::anyhow!("Mismatched request_id"))
                }
            }
            None if inner_message.action == Action::RateReceived
                || inner_message.action == Action::NewOrder =>
            {
                // Some actions don't require request_id matching
                if let Some(Payload::Order(order)) = &inner_message.payload {
                    // Save order to database
                    if let Err(e) =
                        save_order(order.clone(), &trade_keys, request_id, next_idx, pool, true)
                            .await
                    {
                        log::error!("Failed to save order to database: {}", e);
                    }
                    if let Some(tx) = dm_subscription_tx {
                        if let Some(order_id) = order.id {
                            let _ = tx.send(OrderDmSubscriptionCmd::TrackOrder {
                                order_id,
                                trade_index: next_idx,
                            });
                        }
                    }

                    Ok(create_order_result_success(order, next_idx))
                } else {
                    log::error!(
                        "Mostro replied with Action::{:?} but payload is missing/invalid. request_id={:?} trade_index={} payload={:?}",
                        inner_message.action,
                        inner_message.request_id,
                        next_idx,
                        inner_message.payload
                    );
                    Err(anyhow::anyhow!(
                        "Mostro replied but no order payload was provided"
                    ))
                }
            }
            None => Err(anyhow::anyhow!("Response with null request_id")),
        }
    } else {
        log::error!("No response received from Mostro");
        Err(anyhow::anyhow!("No response received from Mostro"))
    }
}
