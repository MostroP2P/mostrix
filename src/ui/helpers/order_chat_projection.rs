use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;

use mostro_core::prelude::{Payload, Peer, SmallOrder, Status, UserInfo};

use crate::models::Order;
use crate::ui::{AppState, OrderMessage};

/// One row in the My Trades sidebar, derived from order DMs. Live status, amounts, and `Payload::Peer`
/// ratings. Static id/kind/created/trade/initiator come from [`crate::ui::AppState::order_chat_static`].
#[derive(Clone)]
pub struct OrderChatListItem {
    pub order_id: String,
    pub status: Option<Status>,
    pub amount: Option<i64>,
    pub fiat: Option<(i64, String)>,
    pub trade_index: Option<i64>,
    pub payment_method: Option<String>,
    pub premium: Option<i64>,
    /// From latest `Payload::Order` seen for this trade (used to attribute `Payload::Peer` reputation).
    pub buyer_trade_pubkey: Option<String>,
    pub seller_trade_pubkey: Option<String>,
    /// Reputation for the buyer/seller trade pubkey when the daemon sent `Payload::Peer` with matching pubkey.
    pub buyer_reputation: Option<UserInfo>,
    pub seller_reputation: Option<UserInfo>,
}

/// Maker listings back on the book (`pending`) with no active trade-DM row in Messages.
#[must_use]
pub fn order_chat_list_item_from_db_order(order: &Order) -> Option<OrderChatListItem> {
    if !order.is_mine {
        return None;
    }
    let status = order
        .status
        .as_deref()
        .and_then(|s| Status::from_str(s).ok());
    if status != Some(Status::Pending) {
        return None;
    }
    let order_id = order.id.as_deref()?.to_string();
    Some(OrderChatListItem {
        order_id,
        status,
        amount: Some(order.amount),
        fiat: Some((order.fiat_amount, order.fiat_code.clone())),
        trade_index: order.trade_index,
        payment_method: Some(order.payment_method.clone()),
        premium: Some(order.premium),
        buyer_trade_pubkey: None,
        seller_trade_pubkey: None,
        buyer_reputation: None,
        seller_reputation: None,
    })
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
        entry.payment_method = Some(order.payment_method.clone());
        entry.premium = Some(order.premium);
    }
    entry.trade_index = entry.trade_index.or(Some(msg.trade_index));
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
    entry.trade_index = entry.trade_index.or(Some(msg.trade_index));
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

fn sort_order_chat_rows(rows: &mut [OrderChatListItem]) {
    rows.sort_by(|a, b| match (a.trade_index, b.trade_index) {
        (Some(ia), Some(ib)) => match ib.cmp(&ia) {
            Ordering::Equal => a.order_id.cmp(&b.order_id),
            o => o,
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.order_id.cmp(&b.order_id),
    });
}

fn build_order_chat_list_from_messages(messages: &[OrderMessage]) -> Vec<OrderChatListItem> {
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
                    amount: None,
                    fiat: None,
                    trade_index: Some(msg.trade_index),
                    payment_method: None,
                    premium: None,
                    buyer_trade_pubkey: None,
                    seller_trade_pubkey: None,
                    buyer_reputation: None,
                    seller_reputation: None,
                };
                merge_message_into_entry(&mut entry, msg);
                entry
            });
    }
    by_order.into_values().collect()
}

/// Append maker-on-book rows that have no trade-DM row in Messages (DM rows win on duplicate id).
fn append_maker_book_rows_without_dm(
    rows: &mut Vec<OrderChatListItem>,
    maker_book: &[OrderChatListItem],
) {
    let message_ids: std::collections::HashSet<String> =
        rows.iter().map(|r| r.order_id.clone()).collect();
    for item in maker_book {
        if !message_ids.contains(&item.order_id) {
            rows.push(item.clone());
        }
    }
}

/// Shared projection for the "My Trades" sidebar and Enter/action handlers.
///
/// Trade DMs in `messages` take precedence; `maker_book` fills maker `pending` rows with no DM row
/// (e.g. after a pre-Active taker cancel republish).
///
/// Important: ordering must stay stable and match the sidebar ordering, otherwise
/// `selected_order_chat_idx` can desync from the action target.
pub fn build_active_order_chat_list(
    messages: &[OrderMessage],
    maker_book: &[OrderChatListItem],
) -> Vec<OrderChatListItem> {
    let mut rows = build_order_chat_list_from_messages(messages);
    append_maker_book_rows_without_dm(&mut rows, maker_book);
    sort_order_chat_rows(&mut rows);
    rows
}

/// My Trades row count from the shared projection (navigation clamping).
#[must_use]
pub fn active_order_chat_list_len(app: &AppState) -> usize {
    match app.messages.lock() {
        Ok(g) => build_active_order_chat_list(&g, &app.my_trades_maker_book).len(),
        Err(_) => 0,
    }
}

/// My Trades sidebar/action projection from current [`AppState`] (clones `messages` once).
#[must_use]
pub fn active_order_chat_list_snapshot(app: &AppState) -> Vec<OrderChatListItem> {
    let messages = match app.messages.lock() {
        Ok(g) => g.clone(),
        Err(_) => return Vec::new(),
    };
    build_active_order_chat_list(&messages, &app.my_trades_maker_book)
}
