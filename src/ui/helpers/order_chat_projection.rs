use std::cmp::Ordering;
use std::collections::HashMap;

use mostro_core::prelude::{Payload, Peer, SmallOrder, Status, UserInfo};

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
    /// From latest `Payload::Order` seen for this trade (used to attribute `Payload::Peer` reputation).
    pub buyer_trade_pubkey: Option<String>,
    pub seller_trade_pubkey: Option<String>,
    /// Reputation for the buyer/seller trade pubkey when the daemon sent `Payload::Peer` with matching pubkey.
    pub buyer_reputation: Option<UserInfo>,
    pub seller_reputation: Option<UserInfo>,
}

fn merge_order_fields(entry: &mut OrderChatListItem, order: &SmallOrder, msg: &OrderMessage) {
    if order.buyer_trade_pubkey.is_some() {
        entry.buyer_trade_pubkey = order.buyer_trade_pubkey.clone();
    }
    if order.seller_trade_pubkey.is_some() {
        entry.seller_trade_pubkey = order.seller_trade_pubkey.clone();
    }
    if entry.amount.is_none() {
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

fn merge_peer_fields(entry: &mut OrderChatListItem, peer: &Peer) {
    let Some(reputation) = peer.reputation.clone() else {
        return;
    };
    if entry.buyer_trade_pubkey.as_ref() == Some(&peer.pubkey) {
        entry.buyer_reputation = Some(reputation.clone());
    }
    if entry.seller_trade_pubkey.as_ref() == Some(&peer.pubkey) {
        entry.seller_reputation = Some(reputation);
    }
}

fn merge_message_into_entry(entry: &mut OrderChatListItem, msg: &OrderMessage) {
    entry.status = status_from_message(msg).or(entry.status);
    let Some(payload) = &msg.message.get_inner_message_kind().payload else {
        return;
    };
    match payload {
        Payload::Order(order) => merge_order_fields(entry, order, msg),
        Payload::Peer(peer) => merge_peer_fields(entry, peer),
        _ => {}
    }
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
            .and_modify(|entry| merge_message_into_entry(entry, msg))
            .or_insert_with(|| {
                let mut entry = OrderChatListItem {
                    order_id: key,
                    status: status_from_message(msg),
                    kind: None,
                    amount: None,
                    fiat: None,
                    created_at: None,
                    trade_index: Some(msg.trade_index),
                    payment_method: None,
                    premium: None,
                    initiator_pubkey: Some(msg.sender.to_string()),
                    is_mine: msg.is_mine,
                    buyer_trade_pubkey: None,
                    seller_trade_pubkey: None,
                    buyer_reputation: None,
                    seller_reputation: None,
                };
                merge_message_into_entry(&mut entry, msg);
                entry
            });
    }

    let mut rows: Vec<OrderChatListItem> = by_order
        .into_values()
        .filter(|row| is_order_chat_actionable(row.status))
        .collect();
    // Newest trades first: higher NIP-06 trade index ⇒ more recently allocated key.
    rows.sort_by(|a, b| match (a.trade_index, b.trade_index) {
        (Some(ia), Some(ib)) => match ib.cmp(&ia) {
            Ordering::Equal => a.order_id.cmp(&b.order_id),
            o => o,
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.order_id.cmp(&b.order_id),
    });
    rows
}
