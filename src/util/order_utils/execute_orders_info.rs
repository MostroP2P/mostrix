// Ask Mostro for authoritative details of the user's own orders.
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::{Order, SnapshotApply, User};
use crate::util::dm_utils::{parse_dm_events, send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT};
use crate::util::mostro_info::MostroInstanceInfo;

use super::helper::handle_mostro_response;

/// Outcome of an orders-info refresh, for the result popup.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OrdersInfoSummary {
    /// Orders Mostro returned and that were merged into the local database.
    pub refreshed: usize,
    /// Ids of the `refreshed` orders, so the UI resyncs only those rows.
    pub refreshed_ids: Vec<Uuid>,
    /// Orders a newer trade DM advanced mid-request; the snapshot was dropped.
    pub superseded: usize,
    /// Orders that could not be refreshed (missing locally, not returned, or
    /// not persisted).
    pub failed: usize,
}

impl OrdersInfoSummary {
    pub fn to_user_message(&self) -> String {
        let mut msg = format!("Refreshed {} order(s) from Mostro.", self.refreshed);
        if self.superseded > 0 {
            msg.push_str(&format!(
                " {} already had a newer update and were kept as is.",
                self.superseded
            ));
        }
        if self.failed > 0 {
            msg.push_str(&format!(
                " {} could not be refreshed — see log.",
                self.failed
            ));
        }
        msg
    }
}

/// `Action::Orders` with `Payload::Ids` — same batch detail fetch restore uses
/// after `restore-session` returns order ids.
///
/// Account-scoped: Mostro resolves the ids against the identity inside the
/// encrypted proof (`get_user_orders_by_id`) and answers `CantDo(NotFound)`
/// for anything that is not yours. The outer kind-14 is authored by a fresh
/// ephemeral key — never the identity key — and the daemon replies to that
/// key, so wait and decrypt run on it too. The waiter is correlated by
/// `request_id`.
pub(crate) async fn fetch_order_details_from_mostro(
    client: &Client,
    identity_keys: &Keys,
    mostro_pubkey: PublicKey,
    order_ids: &[Uuid],
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<HashMap<Uuid, SmallOrder>> {
    if order_ids.is_empty() {
        return Ok(HashMap::new());
    }

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
        .map_err(|e| anyhow::anyhow!("Failed to serialize orders request: {e}"))?;

    log::info!(
        "Action::Orders: requesting details for {} order(s) from {mostro_pubkey}",
        order_ids.len()
    );

    let ephemeral_trade_keys = Keys::generate();

    let sent_message = send_dm(
        client,
        Some(identity_keys),
        &ephemeral_trade_keys,
        &mostro_pubkey,
        message_json,
        None,
        mostro_instance,
    );

    let recv_event = wait_for_dm(
        &ephemeral_trade_keys,
        FETCH_EVENTS_TIMEOUT,
        Some(request_id),
        sent_message,
    )
    .await?;
    let messages = parse_dm_events(recv_event, &ephemeral_trade_keys, None).await;

    let Some((response_message, _, sender)) = messages.first() else {
        return Err(anyhow::anyhow!("No response received for Action::Orders"));
    };
    if sender != &mostro_pubkey {
        return Err(anyhow::anyhow!(
            "Orders response signed by {sender}, expected the configured Mostro instance"
        ));
    }

    let inner = handle_mostro_response(response_message, request_id)?;
    if inner.action != Action::Orders {
        return Err(anyhow::anyhow!(
            "Unexpected action in orders response: {:?}",
            inner.action
        ));
    }

    let Some(Payload::Orders(orders)) = &inner.payload else {
        return Err(anyhow::anyhow!("Orders response missing Payload::Orders"));
    };

    let mut map = HashMap::with_capacity(orders.len());
    for order in orders {
        if let Some(id) = order.id {
            map.insert(id, order.clone());
        }
    }
    Ok(map)
}

/// Request full details for `order_ids` (`Action::Orders`) and merge them locally.
///
/// Mostro's database is the authority here: unlike the public kind-38383 events,
/// which only carry the terms of pending orders, this answer includes the buyer
/// and seller trade pubkeys. Merging goes through
/// [`Order::upsert_from_small_order_dm`], which keeps the row's trade keys,
/// dispute and chat columns intact and can derive the peer chat secret once
/// those pubkeys are known. Local `buyer_invoice` is kept when Mostro omits it
/// (the daemon strips invoices from this payload).
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

    // Only refresh rows we already hold: the trade keys live there and must not
    // be invented, and an id we never traded has nothing to merge onto. Read
    // them *before* sending so each row doubles as the freshness baseline.
    let mut summary = OrdersInfoSummary::default();
    let mut baselines: Vec<(Uuid, Order, Keys)> = Vec::with_capacity(order_ids.len());
    for &order_id in order_ids {
        let id_str = order_id.to_string();
        let row = match Order::get_by_id(pool, &id_str).await {
            Ok(row) => row,
            Err(e) => {
                log::warn!("OrdersInfo: no local row for {id_str}: {e}");
                summary.failed += 1;
                continue;
            }
        };
        let Some(trade_keys) = row
            .trade_keys
            .as_deref()
            .and_then(|hex| Keys::parse(hex).ok())
        else {
            log::warn!("OrdersInfo: local row {id_str} has no usable trade keys");
            summary.failed += 1;
            continue;
        };
        baselines.push((order_id, row, trade_keys));
    }
    if baselines.is_empty() {
        return Err(anyhow::anyhow!(
            "Could not refresh the selected order — see log."
        ));
    }

    let identity_keys = User::get_identity_keys(pool).await?;
    let requested: Vec<Uuid> = baselines.iter().map(|(id, _, _)| *id).collect();
    let details = fetch_order_details_from_mostro(
        client,
        &identity_keys,
        mostro_pubkey,
        &requested,
        mostro_instance,
    )
    .await?;

    for (order_id, baseline, trade_keys) in &baselines {
        let Some(mut small_order) = details.get(order_id).cloned() else {
            log::warn!("OrdersInfo: Mostro did not return requested order {order_id}");
            summary.failed += 1;
            continue;
        };
        if small_order.buyer_invoice.is_none() {
            small_order.buyer_invoice = baseline.buyer_invoice.clone();
        }

        match Order::apply_mostro_snapshot_if_unchanged(
            pool,
            *order_id,
            small_order,
            trade_keys,
            baseline,
        )
        .await
        {
            Ok(SnapshotApply::Applied) => {
                summary.refreshed += 1;
                summary.refreshed_ids.push(*order_id);
            }
            Ok(SnapshotApply::Superseded) => {
                log::info!(
                    "OrdersInfo: order {order_id} changed while refreshing; keeping the newer local state"
                );
                summary.superseded += 1;
            }
            Err(e) => {
                log::error!("OrdersInfo: failed to merge order {order_id}: {e}");
                summary.failed += 1;
            }
        }
    }

    if summary.refreshed == 0 && summary.superseded == 0 {
        return Err(anyhow::anyhow!(
            "Could not refresh the selected order — see log."
        ));
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
                ..Default::default()
            }
            .to_user_message(),
            "Refreshed 2 order(s) from Mostro."
        );
        let bumpy = OrdersInfoSummary {
            refreshed: 1,
            superseded: 1,
            failed: 2,
            ..Default::default()
        }
        .to_user_message();
        assert!(bumpy.contains("Refreshed 1 order(s)"));
        assert!(bumpy.contains("1 already had a newer update"));
        assert!(bumpy.contains("2 could not be refreshed"));
    }
}
