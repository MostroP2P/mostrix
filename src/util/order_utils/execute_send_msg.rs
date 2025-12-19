// Execute send message functionality
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use uuid::Uuid;

use crate::models::{Order, User};
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::order_utils::helper::handle_mostro_response;

async fn create_msg_payload(
    action: &Action,
    order: &Order,
    pool: &sqlx::SqlitePool,
) -> Result<Option<Payload>> {
    match action {
        Action::FiatSent | Action::Release => {
            // Check if this is a range order that needs NextTrade payload
            if let (Some(min_amount), Some(max_amount)) = (order.min_amount, order.max_amount) {
                if max_amount - order.fiat_amount >= min_amount {
                    // This is a range order with remaining amount, create NextTrade payload
                    let user = User::get(pool).await?;
                    let next_trade_index = user.last_trade_index.unwrap_or(0) + 1;
                    let next_trade_keys = user.derive_trade_keys(next_trade_index)?;

                    // Update last trade index
                    User::update_last_trade_index(pool, next_trade_index).await?;

                    Ok(Some(Payload::NextTrade(
                        next_trade_keys.public_key().to_string(),
                        next_trade_index as u32,
                    )))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

pub async fn execute_send_msg(
    order_id: &Uuid,
    action: Action,
    pool: &sqlx::SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
) -> Result<()> {
    // Get order from database
    let order = Order::get_by_id(pool, &order_id.to_string()).await?;

    // Get trade keys of specific order
    let trade_keys = order
        .trade_keys
        .clone()
        .ok_or(anyhow::anyhow!("Missing trade keys"))?;

    let order_trade_keys = Keys::parse(&trade_keys)?;

    // Get identity keys
    let identity_keys = User::get_identity_keys(pool).await?;

    // Determine payload based on action
    // For FiatSent on range orders, we might need NextTrade payload
    let payload: Option<Payload> = create_msg_payload(&action, &order, pool).await?;

    // Create request id
    let request_id = Uuid::new_v4().as_u128() as u64;

    // Create message
    let message = Message::new_order(
        Some(*order_id),
        Some(request_id),
        None,
        action.clone(),
        payload,
    );

    // Serialize the message
    let message_json = message
        .as_json()
        .map_err(|e| anyhow::anyhow!("Failed to serialize message: {e}"))?;

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

    // Wait for the DM response from Mostro
    let recv_event = wait_for_dm(
        client,
        &order_trade_keys,
        FETCH_EVENTS_TIMEOUT,
        sent_message,
    )
    .await?;

    // Parse DM events
    let messages = parse_dm_events(recv_event, &order_trade_keys, None).await;

    // Handle the response
    let Some((response_message, _, _)) = messages.first() else {
        return Err(anyhow::anyhow!("No response received from Mostro"));
    };

    let inner_message = handle_mostro_response(response_message, request_id)?;

    // Validate the response action matches what we expect
    match action {
        Action::FiatSent => match inner_message.action {
            Action::FiatSentOk | Action::WaitingSellerToPay => Ok(()),
            _ => Err(anyhow::anyhow!(
                "Unexpected action in response: {:?}",
                inner_message.action
            )),
        },
        Action::FiatSentOk => match inner_message.action {
            Action::Release | Action::Released => Ok(()),
            _ => Err(anyhow::anyhow!(
                "Unexpected action in response: {:?}",
                inner_message.action
            )),
        },
        Action::Release => match inner_message.action {
            Action::PurchaseCompleted | Action::Rate => Ok(()),
            _ => Err(anyhow::anyhow!(
                "Unexpected action in response: {:?}",
                inner_message.action
            )),
        },
        _ => Err(anyhow::anyhow!("Unsupported action: {:?}", action)),
    }
}
