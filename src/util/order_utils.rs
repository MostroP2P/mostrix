// Order-related utilities for Nostr
use anyhow::Result;
use lightning_invoice::Bolt11Invoice as Invoice;
use lnurl::lightning_address::LightningAddress;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use crate::models::{Order, User};
use crate::settings::Settings;
use crate::util::db_utils::save_order;
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::filters::create_filter;
use crate::util::types::{get_cant_do_description, Event, ListKind};

/// Parse order from nostr tags
fn order_from_tags(tags: Tags) -> Result<SmallOrder> {
    let mut order = SmallOrder::default();

    for tag in tags {
        let t = tag.to_vec(); // Vec<String>
        if t.is_empty() {
            continue;
        }

        let key = t[0].as_str();
        let values = &t[1..];

        let v = values.first().map(|s| s.as_str()).unwrap_or_default();

        match key {
            "d" => {
                order.id = Uuid::parse_str(v).ok();
            }
            "k" => {
                order.kind = mostro_core::order::Kind::from_str(v).ok();
            }
            "f" => {
                order.fiat_code = v.to_string();
            }
            "s" => {
                order.status = Status::from_str(v).ok().or(Some(Status::Pending));
            }
            "amt" => {
                order.amount = v.parse::<i64>().unwrap_or(0);
            }
            "fa" => {
                if v.contains('.') {
                    continue;
                }
                if let Some(max_str) = values.get(1) {
                    order.min_amount = v.parse::<i64>().ok();
                    order.max_amount = max_str.parse::<i64>().ok();
                } else {
                    order.fiat_amount = v.parse::<i64>().unwrap_or(0);
                }
            }
            "pm" => {
                order.payment_method = values.join(",");
            }
            "premium" => {
                order.premium = v.parse::<i64>().unwrap_or(0);
            }
            _ => {}
        }
    }

    Ok(order)
}

/// Parse orders from events
pub fn parse_orders_events(
    events: Events,
    currency: Option<String>,
    status: Option<Status>,
    kind: Option<mostro_core::order::Kind>,
) -> Vec<SmallOrder> {
    let mut latest_by_id: HashMap<Uuid, SmallOrder> = HashMap::new();

    for event in events.iter() {
        // Get order from tags
        let mut order = match order_from_tags(event.tags.clone()) {
            Ok(o) => o,
            Err(e) => {
                log::error!("{e:?}");
                continue;
            }
        };
        // Get order id
        let order_id = match order.id {
            Some(id) => id,
            None => {
                log::info!("Order ID is none");
                continue;
            }
        };
        // Check if order kind is none
        if order.kind.is_none() {
            log::info!("Order kind is none");
            continue;
        }
        // Set created at
        order.created_at = Some(event.created_at.as_u64() as i64);
        // Update latest order by id
        latest_by_id
            .entry(order_id)
            .and_modify(|existing| {
                let new_ts = order.created_at.unwrap_or(0);
                let old_ts = existing.created_at.unwrap_or(0);
                if new_ts > old_ts {
                    *existing = order.clone();
                }
            })
            .or_insert(order);
    }

    let mut requested: Vec<SmallOrder> = latest_by_id
        .into_values()
        .filter(|o| status.map(|s| o.status == Some(s)).unwrap_or(true))
        .filter(|o| currency.as_ref().map(|c| o.fiat_code == *c).unwrap_or(true))
        .filter(|o| {
            kind.as_ref()
                .map(|k| o.kind.as_ref() == Some(k))
                .unwrap_or(true)
        })
        .collect();

    requested.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    requested
}

/// Fetch events list using the same logic as mostro-cli (adapted for mostrix)
pub async fn fetch_events_list(
    list_kind: ListKind,
    status: Option<Status>,
    currency: Option<String>,
    kind: Option<mostro_core::order::Kind>,
    client: &Client,
    mostro_pubkey: PublicKey,
    _since: Option<&i64>,
) -> Result<Vec<Event>> {
    match list_kind {
        ListKind::Orders => {
            let filters = create_filter(list_kind, mostro_pubkey, None)?;
            let fetched_events = client.fetch_events(filters, FETCH_EVENTS_TIMEOUT).await?;
            let orders = parse_orders_events(fetched_events, currency, status, kind);
            Ok(orders.into_iter().map(Event::SmallOrder).collect())
        }
        _ => Err(anyhow::anyhow!("Unsupported ListKind for mostrix")),
    }
}

/// Helper function to create OrderResult::Success from an order
fn create_order_result_success(order: &SmallOrder, trade_index: i64) -> crate::ui::OrderResult {
    crate::ui::OrderResult::Success {
        order_id: order.id,
        kind: order.kind,
        amount: order.amount,
        fiat_code: order.fiat_code.clone(),
        fiat_amount: order.fiat_amount,
        min_amount: order.min_amount,
        max_amount: order.max_amount,
        payment_method: order.payment_method.clone(),
        premium: order.premium,
        status: order.status,
        trade_index: Some(trade_index),
    }
}

/// Helper function to create OrderResult::Success from form data (fallback)
#[allow(clippy::too_many_arguments)]
fn create_order_result_from_form(
    kind: mostro_core::order::Kind,
    amount: i64,
    fiat_code: String,
    fiat_amount: i64,
    min_amount: Option<i64>,
    max_amount: Option<i64>,
    payment_method: String,
    premium: i64,
    trade_index: i64,
) -> crate::ui::OrderResult {
    crate::ui::OrderResult::Success {
        order_id: None,
        kind: Some(kind),
        amount,
        fiat_code,
        fiat_amount,
        min_amount,
        max_amount,
        payment_method,
        premium,
        status: Some(mostro_core::prelude::Status::Pending),
        trade_index: Some(trade_index),
    }
}

/// Helper function to handle Mostro response and check for errors
fn handle_mostro_response(
    response_message: &Message,
    expected_request_id: u64,
) -> Result<&mostro_core::message::MessageKind> {
    let inner_message = response_message.get_inner_message_kind();

    // Check for CantDo payload first (error response)
    if let Some(Payload::CantDo(reason)) = &inner_message.payload {
        let error_msg = match reason {
            Some(r) => get_cant_do_description(r),
            None => "Unknown error - Mostro couldn't process your request".to_string(),
        };
        log::error!("Received CantDo error: {}", error_msg);
        return Err(anyhow::anyhow!(error_msg));
    }

    // Validate request_id if present
    if let Some(id) = inner_message.request_id {
        if id != expected_request_id {
            log::warn!(
                "Received response with mismatched request_id. Expected: {}, Got: {}",
                expected_request_id,
                id
            );
            return Err(anyhow::anyhow!("Mismatched request_id"));
        }
    } else if inner_message.action != Action::RateReceived
        && inner_message.action != Action::NewOrder
        && inner_message.action != Action::AddInvoice
        && inner_message.action != Action::PayInvoice
    {
        log::warn!(
            "Received response with null request_id. Expected: {}",
            expected_request_id
        );
        return Err(anyhow::anyhow!("Response with null request_id"));
    }

    Ok(inner_message)
}

/// Send a new order to Mostro
pub async fn send_new_order(
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    _settings: &Settings,
    mostro_pubkey: PublicKey,
    form: &crate::ui::FormState,
) -> Result<crate::ui::OrderResult, anyhow::Error> {
    use std::collections::HashMap;

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

    let kind_checked = mostro_core::order::Kind::from_str(&kind_str)
        .map_err(|_| anyhow::anyhow!("Invalid order kind"))?;

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
    );

    // Wait for Mostro response (subscribes first, then sends message to avoid missing messages)
    let recv_event =
        wait_for_dm(client, &trade_keys, FETCH_EVENTS_TIMEOUT, new_order_message).await?;

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
                                )
                                .await
                                {
                                    log::error!("Failed to save order to database: {}", e);
                                }

                                Ok(create_order_result_success(order, next_idx))
                            } else {
                                Ok(create_order_result_from_form(
                                    kind_checked,
                                    amount,
                                    fiat_code,
                                    fiat_amount,
                                    min_amount,
                                    max_amount,
                                    payment_method,
                                    premium,
                                    next_idx,
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
                        save_order(order.clone(), &trade_keys, request_id, next_idx, pool).await
                    {
                        log::error!("Failed to save order to database: {}", e);
                    }

                    Ok(create_order_result_success(order, next_idx))
                } else {
                    Ok(create_order_result_from_form(
                        kind_checked,
                        amount,
                        fiat_code,
                        fiat_amount,
                        min_amount,
                        max_amount,
                        payment_method,
                        premium,
                        next_idx,
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
) -> Result<crate::ui::OrderResult, anyhow::Error> {
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
                            return Ok(create_order_result_success(returned_order, next_idx));
                        }
                        Some(Payload::PaymentRequest(opt_order, invoice_string, opt_amount)) => {
                            // For buy orders, we receive PaymentRequest with invoice for seller to pay
                            // Use the order from payload if available, otherwise use the original order
                            let order_to_save = opt_order.as_ref().unwrap_or(order);

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
                            return Ok(crate::ui::OrderResult::PaymentRequestRequired {
                                order: order_to_save.clone(),
                                invoice: invoice_string.clone(),
                                sat_amount: *opt_amount,
                                trade_index: next_idx,
                            });
                        }
                        _ => {
                            log::warn!(
                                "Received response without order details or payment request"
                            );
                            return Err(anyhow::anyhow!(
                                "Response without order details or payment request"
                            ));
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

/// Verify if an invoice is valid
pub fn is_valid_invoice(payment_request: &str) -> Result<Invoice, anyhow::Error> {
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
    let identity_keys = User::get_identity_keys(pool).await?;

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
