// Helper functions for order utilities
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use crate::util::dm_utils::FETCH_EVENTS_TIMEOUT;
use crate::util::filters::create_filter;
use crate::util::types::{get_cant_do_description, Event, ListKind};

/// Parse order from nostr tags
pub fn order_from_tags(tags: Tags) -> Result<SmallOrder> {
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

/// Parse dispute from nostr tags
pub fn dispute_from_tags(tags: Tags) -> Result<Dispute> {
    let mut dispute = Dispute::default();
    for tag in tags {
        let t = tag.to_vec();

        // Check if tag has at least 2 elements
        if t.len() < 2 {
            continue;
        }

        let key = t.first().map(|s| s.as_str()).unwrap_or("");
        let value = t.get(1).map(|s| s.as_str()).unwrap_or("");

        match key {
            "d" => {
                let id = value
                    .parse::<Uuid>()
                    .map_err(|_| anyhow::anyhow!("Invalid dispute id"))?;
                dispute.id = id;
            }
            "s" => {
                let status = DisputeStatus::from_str(value)
                    .map_err(|_| anyhow::anyhow!("Invalid dispute status"))?;
                dispute.status = status.to_string();
            }
            _ => {}
        }
    }

    Ok(dispute)
}

/// Parse disputes from events
pub fn parse_disputes_events(events: Events) -> Vec<Dispute> {
    let mut disputes_list = Vec::<Dispute>::new();

    // Scan events to extract all disputes
    for event in events.iter() {
        let mut dispute = match dispute_from_tags(event.tags.clone()) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Failed to parse dispute from tags: {:?}", e);
                continue;
            }
        };
        // Get created at field from Nostr event
        dispute.created_at = event.created_at.as_u64() as i64;
        disputes_list.push(dispute);
    }

    let buffer_dispute_list = disputes_list.clone();
    // Order all elements (disputes) received to filter - discard disaligned messages
    // if a dispute has an older message with the state we received is discarded for the latest one
    disputes_list.retain(|keep| {
        !buffer_dispute_list
            .iter()
            .any(|x| x.id == keep.id && x.created_at > keep.created_at)
    });

    // Sort by id to remove duplicates
    disputes_list.sort_by(|a, b| b.id.cmp(&a.id));
    disputes_list.dedup_by(|a, b| a.id == b.id);

    // Finally sort list by creation time (newest first)
    disputes_list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    disputes_list
}

/// Parse orders from events
pub fn parse_orders_events(
    events: Events,
    currencies: Option<Vec<String>>,
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
        .filter(|o| {
            // If currencies filter is provided and not empty, filter by any currency in the list
            // If currencies is None or empty, show all orders (no filter)
            currencies
                .as_ref()
                .map(|currencies| currencies.is_empty() || currencies.contains(&o.fiat_code))
                .unwrap_or(true)
        })
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
    currencies: Option<Vec<String>>,
    kind: Option<mostro_core::order::Kind>,
    client: &Client,
    mostro_pubkey: PublicKey,
    _since: Option<&i64>,
) -> Result<Vec<Event>> {
    match list_kind {
        ListKind::Orders => {
            let filters = create_filter(list_kind, mostro_pubkey, None)?;
            let fetched_events = client.fetch_events(filters, FETCH_EVENTS_TIMEOUT).await?;
            let orders = parse_orders_events(fetched_events, currencies, status, kind);
            Ok(orders.into_iter().map(Event::SmallOrder).collect())
        }
        ListKind::Disputes => {
            let filters = create_filter(list_kind, mostro_pubkey, None)?;
            let fetched_events = client.fetch_events(filters, FETCH_EVENTS_TIMEOUT).await?;
            let disputes = parse_disputes_events(fetched_events);
            Ok(disputes.into_iter().map(Event::Dispute).collect())
        }
        _ => Err(anyhow::anyhow!("Unsupported ListKind for mostrix")),
    }
}

/// Fetch orders from the Mostro network
/// Returns a vector of SmallOrder items filtered by the specified status and currencies
pub async fn get_orders(
    client: &Client,
    mostro_pubkey: PublicKey,
    status: Option<Status>,
    currencies: Option<Vec<String>>,
) -> Result<Vec<SmallOrder>> {
    let fetched_events = fetch_events_list(
        ListKind::Orders,
        status,
        currencies,
        None,
        client,
        mostro_pubkey,
        None,
    )
    .await?;

    let orders: Vec<SmallOrder> = fetched_events
        .into_iter()
        .filter_map(|event| {
            if let Event::SmallOrder(order) = event {
                Some(order)
            } else {
                None
            }
        })
        .collect();

    Ok(orders)
}

/// Fetch disputes from the Mostro network
/// Returns a vector of Dispute items
pub async fn get_disputes(client: &Client, mostro_pubkey: PublicKey) -> Result<Vec<Dispute>> {
    let fetched_events = fetch_events_list(
        ListKind::Disputes,
        None,
        None,
        None,
        client,
        mostro_pubkey,
        None,
    )
    .await?;

    let disputes: Vec<Dispute> = fetched_events
        .into_iter()
        .filter_map(|event| {
            if let Event::Dispute(dispute) = event {
                Some(dispute)
            } else {
                None
            }
        })
        .collect();

    Ok(disputes)
}

/// Helper function to create OrderResult::Success from an order
pub(super) fn create_order_result_success(
    order: &SmallOrder,
    trade_index: i64,
) -> crate::ui::OrderResult {
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
pub(super) fn create_order_result_from_form(
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
pub(super) fn handle_mostro_response(
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
