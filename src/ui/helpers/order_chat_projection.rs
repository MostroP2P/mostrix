use std::collections::HashMap;

use mostro_core::prelude::{Payload, Status};

use crate::ui::OrderMessage;

#[derive(Clone)]
pub struct OrderChatListItem {
    pub order_id: String,
    pub status: Option<Status>,
    pub kind: Option<String>,
    pub amount: Option<i64>,
    pub fiat: Option<(i64, String)>,
    pub created_at: Option<i64>,
    pub trade_index: Option<i64>,
    pub payment_method: Option<String>,
    pub premium: Option<i64>,
    pub initiator_pubkey: Option<String>,
    pub is_mine: Option<bool>,
}

fn status_from_message(msg: &OrderMessage) -> Option<Status> {
    msg.order_status
}

fn is_order_chat_actionable(status: Option<Status>) -> bool {
    matches!(
        status,
        Some(Status::SettledHoldInvoice)
            | Some(Status::Active)
            | Some(Status::FiatSent)
            | Some(Status::Success)
    )
}

/// Shared projection for the "My Trades" sidebar and Enter/action handlers.
///
/// Important: ordering must stay stable and match the sidebar ordering, otherwise
/// `selected_order_chat_idx` can desync from the action target.
pub fn build_active_order_chat_list(messages: &[OrderMessage]) -> Vec<OrderChatListItem> {
    let mut by_order: HashMap<String, OrderChatListItem> = HashMap::new();
    for msg in messages {
        let Some(order_id) = msg.order_id else {
            continue;
        };
        let key = order_id.to_string();
        by_order
            .entry(key.clone())
            .and_modify(|entry| {
                entry.status = status_from_message(msg).or(entry.status);
                if entry.amount.is_none() {
                    if let Some(Payload::Order(order)) =
                        &msg.message.get_inner_message_kind().payload
                    {
                        entry.amount = Some(order.amount);
                        entry.fiat = Some((order.fiat_amount, order.fiat_code.clone()));
                        entry.kind = order.kind.map(|k| k.to_string());
                        entry.created_at = order.created_at;
                        entry.trade_index = Some(msg.trade_index);
                        entry.payment_method = Some(order.payment_method.clone());
                        entry.premium = Some(order.premium);
                        entry.initiator_pubkey = Some(msg.sender.to_string());
                        entry.is_mine = msg.is_mine;
                    }
                }
            })
            .or_insert_with(|| {
                let mut amount = None;
                let mut fiat = None;
                let mut kind = None;
                let mut created_at = None;
                let mut payment_method = None;
                let mut premium = None;
                if let Some(Payload::Order(order)) = &msg.message.get_inner_message_kind().payload {
                    amount = Some(order.amount);
                    fiat = Some((order.fiat_amount, order.fiat_code.clone()));
                    kind = order.kind.map(|k| k.to_string());
                    created_at = order.created_at;
                    payment_method = Some(order.payment_method.clone());
                    premium = Some(order.premium);
                }
                OrderChatListItem {
                    order_id: key,
                    status: status_from_message(msg),
                    kind,
                    amount,
                    fiat,
                    created_at,
                    trade_index: Some(msg.trade_index),
                    payment_method,
                    premium,
                    initiator_pubkey: Some(msg.sender.to_string()),
                    is_mine: msg.is_mine,
                }
            });
    }

    let mut rows: Vec<OrderChatListItem> = by_order
        .into_values()
        .filter(|row| is_order_chat_actionable(row.status))
        .collect();
    rows.sort_by(|a, b| a.order_id.cmp(&b.order_id));
    rows
}
