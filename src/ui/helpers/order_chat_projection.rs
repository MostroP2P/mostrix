use std::cmp::Ordering;
use std::collections::HashMap;
use std::str::FromStr;

use mostro_core::prelude::{Action, Kind, Payload, Peer, SmallOrder, Status, UserInfo};
use uuid::Uuid;

use crate::models::Order;
use crate::ui::{AppState, OrderMessage};

/// One row in the My Trades sidebar, derived from order DMs. Live status, amounts, and `Payload::Peer`
/// ratings. Static id/kind/created/trade/initiator come from [`crate::ui::AppState::order_chat_static`].
///
/// Economics are taken from [`OrderMessage::order_snapshot`], `sat_amount`, and any embedded
/// [`SmallOrder`] (`Payload::Order`, `PaymentRequest`, `BondPayoutRequest`) — not only a live
/// `Payload::Order`, because Messages keeps one row per trade whose current action often omits it.
#[derive(Clone)]
pub struct OrderChatListItem {
    pub order_id: String,
    pub status: Option<Status>,
    pub amount: Option<i64>,
    pub fiat: Option<(i64, String)>,
    pub trade_index: Option<i64>,
    pub payment_method: Option<String>,
    pub premium: Option<i64>,
    /// From latest embedded `SmallOrder` seen for this trade (used to attribute `Payload::Peer` reputation).
    pub buyer_trade_pubkey: Option<String>,
    pub seller_trade_pubkey: Option<String>,
    /// Reputation for the buyer/seller when the daemon sent `Payload::Peer` (pubkey match, or
    /// empty pubkey attributed to the counterparty). Also copied from the Messages row so a later
    /// status DM does not drop it.
    pub buyer_reputation: Option<UserInfo>,
    pub seller_reputation: Option<UserInfo>,
    /// Solver pubkey announced by `AdminTookDispute`.
    pub solver_pubkey: Option<String>,
    /// Dispute UUID announced by Mostro for this order.
    pub dispute_id: Option<String>,
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
        solver_pubkey: order.solver_pubkey.clone(),
        dispute_id: order.dispute_id.clone(),
    })
}

fn merge_order_fields(entry: &mut OrderChatListItem, order: &SmallOrder, msg: &OrderMessage) {
    if order.buyer_trade_pubkey.is_some() {
        entry.buyer_trade_pubkey = order.buyer_trade_pubkey.clone();
    }
    if order.seller_trade_pubkey.is_some() {
        entry.seller_trade_pubkey = order.seller_trade_pubkey.clone();
    }
    if entry.amount.is_none() && order.amount > 0 {
        entry.amount = Some(order.amount);
    }
    if entry.fiat.is_none() && (order.fiat_amount > 0 || !order.fiat_code.trim().is_empty()) {
        entry.fiat = Some((order.fiat_amount, order.fiat_code.clone()));
    }
    if entry
        .payment_method
        .as_ref()
        .is_none_or(|s| s.trim().is_empty())
        && !order.payment_method.trim().is_empty()
    {
        entry.payment_method = Some(order.payment_method.clone());
    }
    if entry.premium.is_none() {
        entry.premium = Some(order.premium);
    }
    entry.trade_index = entry.trade_index.or(Some(msg.trade_index));
}

/// Whether the **counterparty** is the buyer, from our maker/taker role and book side.
fn counterpart_is_buyer(is_mine: Option<bool>, kind: Option<Kind>) -> Option<bool> {
    match (is_mine, kind) {
        (Some(true), Some(Kind::Buy)) => Some(false),
        (Some(true), Some(Kind::Sell)) => Some(true),
        (Some(false), Some(Kind::Buy)) => Some(true),
        (Some(false), Some(Kind::Sell)) => Some(false),
        _ => None,
    }
}

/// Apply `Payload::Peer` reputation onto buyer/seller slots.
///
/// Mostro's `notify_taker_reputation` sends an empty `peer.pubkey`; in that case attribute
/// the snapshot to the counterparty from `is_mine` + order kind.
pub(crate) fn assign_peer_reputation(
    buyer_trade_pubkey: Option<&str>,
    seller_trade_pubkey: Option<&str>,
    is_mine: Option<bool>,
    kind: Option<Kind>,
    peer: &Peer,
    buyer_reputation: &mut Option<UserInfo>,
    seller_reputation: &mut Option<UserInfo>,
) {
    let Some(reputation) = peer.reputation.clone() else {
        return;
    };
    if peer.pubkey.is_empty() {
        match counterpart_is_buyer(is_mine, kind) {
            Some(true) => *buyer_reputation = Some(reputation),
            Some(false) => *seller_reputation = Some(reputation),
            None => {}
        }
        return;
    }
    if buyer_trade_pubkey == Some(peer.pubkey.as_str()) {
        *buyer_reputation = Some(reputation.clone());
    }
    if seller_trade_pubkey == Some(peer.pubkey.as_str()) {
        *seller_reputation = Some(reputation);
    }
}

fn merge_peer_fields(entry: &mut OrderChatListItem, peer: &Peer, msg: &OrderMessage) {
    assign_peer_reputation(
        entry.buyer_trade_pubkey.as_deref(),
        entry.seller_trade_pubkey.as_deref(),
        msg.is_mine,
        msg.order_kind,
        peer,
        &mut entry.buyer_reputation,
        &mut entry.seller_reputation,
    );
}

fn merge_message_into_entry(entry: &mut OrderChatListItem, msg: &OrderMessage) {
    entry.trade_index = entry.trade_index.or(Some(msg.trade_index));
    entry.status = status_from_message(msg).or(entry.status);
    if entry.amount.is_none() {
        if let Some(sats) = msg.sat_amount.filter(|s| *s > 0) {
            entry.amount = Some(sats);
        }
    }
    if let Some(snap) = &msg.order_snapshot {
        merge_order_fields(entry, snap, msg);
    }
    if entry.buyer_reputation.is_none() {
        entry.buyer_reputation.clone_from(&msg.buyer_reputation);
    }
    if entry.seller_reputation.is_none() {
        entry.seller_reputation.clone_from(&msg.seller_reputation);
    }
    let Some(payload) = &msg.message.get_inner_message_kind().payload else {
        return;
    };
    match payload {
        Payload::Order(order) => merge_order_fields(entry, order, msg),
        Payload::PaymentRequest(Some(order), _, _) => merge_order_fields(entry, order, msg),
        Payload::BondPayoutRequest(req) => merge_order_fields(entry, &req.order, msg),
        Payload::Peer(peer) => {
            if msg.message.get_inner_message_kind().action == Action::AdminTookDispute {
                entry.solver_pubkey = Some(peer.pubkey.clone());
            } else {
                merge_peer_fields(entry, peer, msg);
            }
        }
        Payload::Dispute(dispute_id, _) => {
            entry.dispute_id = Some(dispute_id.to_string());
        }
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
                    solver_pubkey: None,
                    dispute_id: None,
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

fn fatal_on_poisoned_messages_lock(e: impl std::fmt::Display) {
    crate::util::request_fatal_restart(format!(
        "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
    ));
}

/// My Trades row count from the shared projection (navigation clamping).
#[must_use]
pub fn active_order_chat_list_len(app: &AppState) -> usize {
    match app.messages.lock() {
        Ok(guard) => build_active_order_chat_list(&guard, &app.my_trades_maker_book).len(),
        Err(e) => {
            fatal_on_poisoned_messages_lock(e);
            0
        }
    }
}

/// My Trades sidebar/action projection from current [`AppState`] (clones `messages` once).
#[must_use]
pub fn active_order_chat_list_snapshot(app: &AppState) -> Vec<OrderChatListItem> {
    match app.messages.lock() {
        Ok(guard) => {
            let messages = guard.clone();
            let mut rows = build_active_order_chat_list(&messages, &app.my_trades_maker_book);
            for row in &mut rows {
                let Some(header) = Uuid::parse_str(&row.order_id)
                    .ok()
                    .and_then(|id| app.order_chat_static.get(&id))
                else {
                    continue;
                };
                row.solver_pubkey = row
                    .solver_pubkey
                    .clone()
                    .or_else(|| header.solver_pubkey.clone());
                row.dispute_id = row.dispute_id.clone().or_else(|| header.dispute_id.clone());
            }
            rows
        }
        Err(e) => {
            fatal_on_poisoned_messages_lock(e);
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{active_order_chat_list_snapshot, build_active_order_chat_list};
    use crate::ui::{AppState, OrderChatStaticHeader, OrderMessage, UserRole};
    use mostro_core::prelude::{
        Action, Kind, Message, Payload, Peer, SmallOrder, Status, UserInfo,
    };
    use nostr_sdk::prelude::Keys;
    use uuid::Uuid;

    fn sample_order_message(
        order_id: Uuid,
        action: Action,
        payload: Option<Payload>,
    ) -> OrderMessage {
        OrderMessage {
            message: Message::new_order(Some(order_id), None, Some(1), action, payload),
            timestamp: 1,
            sender: Keys::generate().public_key(),
            order_id: Some(order_id),
            trade_index: 1,
            sat_amount: None,
            buyer_invoice: None,
            order_kind: Some(Kind::Sell),
            is_mine: Some(false),
            order_status: Some(Status::WaitingBuyerInvoice),
            order_snapshot: None,
            buyer_reputation: None,
            seller_reputation: None,
            read: true,
            auto_popup_shown: true,
        }
    }

    #[test]
    fn dispute_payload_populates_dispute_id() {
        let order_id = Uuid::new_v4();
        let dispute_id = Uuid::new_v4();
        let message = OrderMessage {
            message: Message::new_dispute(
                Some(order_id),
                None,
                Some(1),
                Action::DisputeInitiatedByYou,
                Some(Payload::Dispute(dispute_id, None)),
            ),
            timestamp: 1,
            sender: Keys::generate().public_key(),
            order_id: Some(order_id),
            trade_index: 1,
            sat_amount: None,
            buyer_invoice: None,
            order_kind: None,
            is_mine: Some(false),
            order_status: Some(Status::Dispute),
            order_snapshot: None,
            buyer_reputation: None,
            seller_reputation: None,
            read: true,
            auto_popup_shown: true,
        };

        let rows = build_active_order_chat_list(&[message], &[]);

        assert_eq!(
            rows[0].dispute_id.as_deref(),
            Some(dispute_id.to_string().as_str())
        );
    }

    #[test]
    fn static_dispute_metadata_survives_replaced_order_message() {
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        app.order_chat_static.insert(
            order_id,
            OrderChatStaticHeader {
                order_id,
                kind: Some(Kind::Buy),
                created_at: None,
                trade_index: 1,
                initiator_trade_pubkey: "initiator".to_string(),
                is_mine: false,
                solver_pubkey: Some("solver-pubkey".to_string()),
                dispute_id: Some("dispute-id".to_string()),
            },
        );
        app.messages
            .lock()
            .expect("messages lock")
            .push(OrderMessage {
                message: Message::new_order(Some(order_id), None, Some(1), Action::FiatSent, None),
                timestamp: 2,
                sender: Keys::generate().public_key(),
                order_id: Some(order_id),
                trade_index: 1,
                sat_amount: None,
                buyer_invoice: None,
                order_kind: Some(Kind::Buy),
                is_mine: Some(false),
                order_status: Some(Status::Dispute),
                order_snapshot: None,
                buyer_reputation: None,
                seller_reputation: None,
                read: true,
                auto_popup_shown: true,
            });

        let rows = active_order_chat_list_snapshot(&app);

        assert_eq!(rows[0].solver_pubkey.as_deref(), Some("solver-pubkey"));
        assert_eq!(rows[0].dispute_id.as_deref(), Some("dispute-id"));
    }

    #[test]
    fn snapshot_fills_amount_payment_and_premium_when_payload_has_no_order() {
        let order_id = Uuid::new_v4();
        let mut msg = sample_order_message(order_id, Action::WaitingBuyerInvoice, None);
        msg.order_snapshot = Some(SmallOrder {
            id: Some(order_id),
            amount: 21_000,
            fiat_code: "USD".to_string(),
            fiat_amount: 50,
            payment_method: "SEPA".to_string(),
            premium: 2,
            ..Default::default()
        });
        msg.sat_amount = Some(21_000);

        let rows = build_active_order_chat_list(&[msg], &[]);

        assert_eq!(rows[0].amount, Some(21_000));
        assert_eq!(rows[0].fiat, Some((50, "USD".to_string())));
        assert_eq!(rows[0].payment_method.as_deref(), Some("SEPA"));
        assert_eq!(rows[0].premium, Some(2));
    }

    #[test]
    fn payment_request_payload_fills_economic_fields() {
        let order_id = Uuid::new_v4();
        let order = SmallOrder {
            id: Some(order_id),
            amount: 10_000,
            fiat_code: "EUR".to_string(),
            fiat_amount: 100,
            payment_method: "Bizum".to_string(),
            premium: 0,
            ..Default::default()
        };
        let msg = sample_order_message(
            order_id,
            Action::PayInvoice,
            Some(Payload::PaymentRequest(Some(order), "lnbc1".into(), None)),
        );

        let rows = build_active_order_chat_list(&[msg], &[]);

        assert_eq!(rows[0].amount, Some(10_000));
        assert_eq!(rows[0].payment_method.as_deref(), Some("Bizum"));
        assert_eq!(rows[0].premium, Some(0));
    }

    #[test]
    fn empty_peer_pubkey_reputation_is_attributed_to_counterparty() {
        let order_id = Uuid::new_v4();
        let reputation = UserInfo {
            rating: 4.5,
            reviews: 12,
            operating_days: 30,
        };
        let mut msg = sample_order_message(
            order_id,
            Action::AddInvoice,
            Some(Payload::Peer(Peer {
                pubkey: String::new(),
                reputation: Some(reputation.clone()),
            })),
        );
        // Taker of a sell listing is the buyer → counterparty is the seller.
        msg.order_kind = Some(Kind::Sell);
        msg.is_mine = Some(false);

        let rows = build_active_order_chat_list(&[msg], &[]);

        assert!(rows[0].buyer_reputation.is_none());
        assert_eq!(
            rows[0].seller_reputation.as_ref().map(|r| r.reviews),
            Some(12)
        );
    }

    #[test]
    fn empty_peer_pubkey_on_maker_sell_is_taker_buyer_rating() {
        let order_id = Uuid::new_v4();
        let reputation = UserInfo {
            rating: 3.9,
            reviews: 5,
            operating_days: 9,
        };
        let mut msg = sample_order_message(
            order_id,
            Action::PayInvoice,
            Some(Payload::Peer(Peer {
                pubkey: String::new(),
                reputation: Some(reputation),
            })),
        );
        msg.order_kind = Some(Kind::Sell);
        msg.is_mine = Some(true);

        let rows = build_active_order_chat_list(&[msg], &[]);

        assert_eq!(
            rows[0].buyer_reputation.as_ref().map(|r| r.reviews),
            Some(5)
        );
        assert!(rows[0].seller_reputation.is_none());
    }

    #[test]
    fn stored_reputation_survives_payload_without_peer() {
        let order_id = Uuid::new_v4();
        let mut msg = sample_order_message(order_id, Action::FiatSent, None);
        msg.seller_reputation = Some(UserInfo {
            rating: 3.0,
            reviews: 4,
            operating_days: 10,
        });

        let rows = build_active_order_chat_list(&[msg], &[]);

        assert_eq!(
            rows[0].seller_reputation.as_ref().map(|r| r.reviews),
            Some(4)
        );
    }
}
