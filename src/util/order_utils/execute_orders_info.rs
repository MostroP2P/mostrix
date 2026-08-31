// Ask Mostro for authoritative details of the user's own orders.
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::{Order, User};
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;

use super::helper::handle_mostro_response;

/// Outcome of an orders-info refresh, for the result popup.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OrdersInfoSummary {
    /// Orders Mostro returned and that were merged into the local database.
    pub refreshed: usize,
    /// Orders Mostro returned that could not be persisted locally.
    pub failed: usize,
}

impl OrdersInfoSummary {
    pub fn to_user_message(&self) -> String {
        let mut msg = format!("Refreshed {} order(s) from Mostro.", self.refreshed);
        if self.failed > 0 {
            msg.push_str(&format!(
                " {} could not be saved locally — see log.",
                self.failed
            ));
        }
        msg
    }
}

/// Request full details for `order_ids` (`Action::Orders`) and merge them locally.
///
/// Account-scoped, like restore: Mostro resolves the ids against the requesting
/// **identity** pubkey (`get_user_orders_by_id`) and answers `CantDo(NotFound)`
/// for anything that is not yours, so the whole exchange runs on the identity
/// keys and carries no trade index.
///
/// Mostro's database is the authority here: unlike the public kind-38383 events,
/// which only carry the terms of pending orders, this answer includes the buyer
/// and seller trade pubkeys. Merging goes through
/// [`Order::upsert_from_small_order_dm`], which keeps the row's trade keys,
/// dispute and chat columns intact and can derive the peer chat secret once
/// those pubkeys are known.
pub async fn execute_orders_info(
    order_ids: &[Uuid],
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<OrdersInfoSummary> {
    if order_ids.is_empty() {
        return Err(anyhow::anyhow!("No order selected"));
    }

    let identity_keys = User::get_identity_keys(pool).await?;
    let request_id = Uuid::new_v4().as_u128() as u64;
    let message = Message::new_order(
        None,
        Some(request_id),
        None,
        Action::Orders,
        Some(Payload::Ids(order_ids.to_vec())),
    );
    let message_json = message
        .as_json()
        .map_err(|e| anyhow::anyhow!("Failed to serialize message: {e}"))?;

    log::info!(
        "OrdersInfo: requesting {} order(s) from {mostro_pubkey}",
        order_ids.len()
    );

    let sent_message = send_dm(
        client,
        Some(&identity_keys),
        &identity_keys,
        &mostro_pubkey,
        message_json,
        None,
        mostro_instance,
    );

    let recv_event = wait_for_dm(&identity_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;
    let messages = parse_dm_events(recv_event, &identity_keys, None).await;

    let Some((response_message, _, sender)) = messages.first() else {
        return Err(anyhow::anyhow!("No response received from Mostro"));
    };
    if sender != &mostro_pubkey {
        return Err(anyhow::anyhow!(
            "Orders response signed by {sender}, expected the configured Mostro instance"
        ));
    }

    let inner = handle_mostro_response(response_message, request_id)?;
    if inner.action != Action::Orders {
        return Err(anyhow::anyhow!(
            "Unexpected action in response: {:?}",
            inner.action
        ));
    }
    let Some(Payload::Orders(orders)) = &inner.payload else {
        return Err(anyhow::anyhow!("No orders payload in response"));
    };

    let mut summary = OrdersInfoSummary::default();
    for small_order in orders {
        let Some(order_id) = small_order.id else {
            log::warn!("OrdersInfo: Mostro returned an order without id, skipping");
            summary.failed += 1;
            continue;
        };
        let id_str = order_id.to_string();

        // Only refresh rows we already hold: the trade keys live there and must
        // not be invented, and an id we never traded has nothing to merge onto.
        let trade_keys = match Order::get_by_id(pool, &id_str).await {
            Ok(row) => row
                .trade_keys
                .as_deref()
                .and_then(|hex| Keys::parse(hex).ok()),
            Err(e) => {
                log::warn!("OrdersInfo: no local row for {id_str}: {e}");
                summary.failed += 1;
                continue;
            }
        };
        let Some(trade_keys) = trade_keys else {
            log::warn!("OrdersInfo: local row {id_str} has no usable trade keys");
            summary.failed += 1;
            continue;
        };

        match Order::upsert_from_small_order_dm(
            pool,
            order_id,
            small_order.clone(),
            &trade_keys,
            None,
        )
        .await
        {
            Ok(_) => summary.refreshed += 1,
            Err(e) => {
                log::error!("OrdersInfo: failed to merge order {id_str}: {e}");
                summary.failed += 1;
            }
        }
    }

    log::info!("OrdersInfo: {}", summary.to_user_message());
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::OrdersInfoSummary;

    #[test]
    fn summary_message_reports_failures_only_when_present() {
        assert_eq!(
            OrdersInfoSummary {
                refreshed: 2,
                failed: 0
            }
            .to_user_message(),
            "Refreshed 2 order(s) from Mostro."
        );
        let bumpy = OrdersInfoSummary {
            refreshed: 1,
            failed: 2,
        }
        .to_user_message();
        assert!(bumpy.contains("Refreshed 1 order(s)"));
        assert!(bumpy.contains("2 could not be saved locally"));
    }
}
