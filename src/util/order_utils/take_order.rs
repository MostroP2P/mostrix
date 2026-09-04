// Take order functionality
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;

use crate::models::User;
use crate::ui::orders::{order_message_to_notification, OperationResult, OrderMessage};
use crate::util::db_utils::save_order;
use crate::util::dm_utils::{
    parse_dm_events, send_dm, send_track_order_cmd, wait_for_dm, FETCH_EVENTS_TIMEOUT,
};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::order_utils::helper::{handle_mostro_response, payment_request_operation_result};
use crate::util::OrderDmSubscriptionCmd;
use tokio::sync::mpsc::UnboundedSender;

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

/// Take an order from the order book.
///
/// On take-sell without a buyer invoice, Mostro replies with `AddInvoice` +
/// `Payload::Order`. That reply is cross-checked against `order` (and optional
/// range `amount` / instance fee) before the AddInvoice popup is framed or the
/// row is persisted (MOSTRO-078): id, kind, status, fiat, and sats
/// (`book_amount − split_fee` when fee is known; market quotes must be positive).
///
/// # Errors
///
/// Returns an error if the reply fails request-id / CantDo checks, the AddInvoice
/// SmallOrder does not match the taken book order, or fixed-price verification
/// lacks a Mostro fee from `mostro_instance`.
#[allow(clippy::too_many_arguments)]
pub async fn take_order(
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    order: &SmallOrder,
    amount: Option<i64>,
    invoice: Option<String>,
    dm_subscription_tx: Option<&UnboundedSender<OrderDmSubscriptionCmd>>,
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<OperationResult, anyhow::Error> {
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

    // Reserve the next trade index atomically; propagate DB errors (e.g. SQLITE_BUSY).
    let (next_idx, trade_keys) = User::reserve_next_trade_index(pool, 1).await?;

    // Subscribe as early as possible for take-order flow so the first
    // Mostro response/event is not missed by the background DM listener.
    if dm_subscription_tx.is_some() {
        // Optimistic TrackOrder via the **current** global DM router sender (not a
        // possibly-stale main-loop clone). Intentionally redundant with the post-
        // `save_order` send below.
        log::info!(
            "[take_order] Early subscribe command for order_id={}, trade_index={}",
            order_id,
            next_idx
        );
        send_track_order_cmd(order_id, next_idx);
    }

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
        mostro_instance,
    );

    // Wait for Mostro response (subscribes first, then sends message to avoid missing messages)
    let recv_event = wait_for_dm(&trade_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;

    // Parse DM events
    let messages = parse_dm_events(recv_event, &trade_keys, None).await;

    if let Some((response_message, timestamp, sender)) = messages.first() {
        let inner_message = handle_mostro_response(response_message, request_id)?;

        match inner_message.request_id {
            Some(id) if request_id == id => {
                process_take_order_reply(
                    inner_message,
                    response_message,
                    *timestamp,
                    *sender,
                    order,
                    amount,
                    mostro_instance.and_then(|i| i.fee),
                    request_id,
                    next_idx,
                    pool,
                    &trade_keys,
                    dm_subscription_tx,
                )
                .await
            }
            Some(_) => Err(anyhow::anyhow!("Mismatched request_id")),
            None => Err(anyhow::anyhow!("Response with null request_id")),
        }
    } else {
        log::error!("No response received from Mostro");
        Err(anyhow::anyhow!("No response received from Mostro"))
    }
}

/// Dispatch a take-order Mostro reply by **action** (not payload alone).
///
/// Take-sell without a buyer invoice is `AddInvoice` + `Payload::Order`; treating that as
/// create-order `Success` showed "Order Created Successfully".
///
/// For `AddInvoice`, validates the daemon SmallOrder against `requested` /
/// `take_fiat_amount` / `fee_rate` before persist and popup framing (MOSTRO-078).
#[allow(clippy::too_many_arguments)]
async fn process_take_order_reply(
    inner_message: &mostro_core::message::MessageKind,
    response_message: &Message,
    timestamp: i64,
    sender: PublicKey,
    requested: &SmallOrder,
    take_fiat_amount: Option<i64>,
    fee_rate: Option<f64>,
    request_id: u64,
    next_idx: i64,
    pool: &sqlx::sqlite::SqlitePool,
    trade_keys: &Keys,
    dm_subscription_tx: Option<&UnboundedSender<OrderDmSubscriptionCmd>>,
) -> Result<OperationResult> {
    let fallback_order_id = requested
        .id
        .ok_or_else(|| anyhow::anyhow!("Order ID is missing"))?;
    match map_take_reply(&inner_message.action, &inner_message.payload)? {
        MappedTakeReply::AddInvoice(returned_order) => {
            // MOSTRO-078: do not frame / persist AddInvoice from an untrusted SmallOrder.
            let trusted_sats = validate_take_sell_add_invoice_reply(
                requested,
                &returned_order,
                take_fiat_amount,
                fee_rate,
            )?;
            let mut to_persist = returned_order;
            to_persist.amount = trusted_sats;
            let normalized = persist_taken_order(
                to_persist,
                fallback_order_id,
                request_id,
                next_idx,
                pool,
                trade_keys,
                dm_subscription_tx,
            )
            .await;
            Ok(take_add_invoice_operation_result(
                response_message,
                &normalized,
                timestamp,
                sender,
                next_idx,
                trusted_sats,
            ))
        }
        MappedTakeReply::PaymentRequest {
            action,
            order,
            invoice,
            amount,
        } => {
            payment_request_operation_result(
                action,
                order,
                invoice,
                amount,
                Some(fallback_order_id),
                request_id,
                next_idx,
                pool,
                trade_keys,
                false,
                dm_subscription_tx,
                "take_order",
            )
            .await
        }
    }
}

#[derive(Debug)]
enum MappedTakeReply {
    AddInvoice(SmallOrder),
    PaymentRequest {
        action: Action,
        order: Option<SmallOrder>,
        invoice: String,
        amount: Option<i64>,
    },
}

fn map_take_reply(action: &Action, payload: &Option<Payload>) -> Result<MappedTakeReply> {
    match (action, payload) {
        (Action::AddInvoice, Some(Payload::Order(order))) => {
            Ok(MappedTakeReply::AddInvoice(order.clone()))
        }
        (Action::AddInvoice, _) => Err(anyhow::anyhow!(
            "Mostro replied with AddInvoice but no Order payload was provided"
        )),
        (
            Action::PayInvoice | Action::PayBondInvoice,
            Some(Payload::PaymentRequest(opt_order, invoice, opt_amount)),
        ) => Ok(MappedTakeReply::PaymentRequest {
            action: action.clone(),
            order: opt_order.clone(),
            invoice: invoice.clone(),
            amount: *opt_amount,
        }),
        (Action::PayInvoice | Action::PayBondInvoice, _) => Err(anyhow::anyhow!(
            "Mostro replied with {:?} but no PaymentRequest payload was provided",
            action
        )),
        (other, _) => {
            log::warn!("Received unexpected take-order action: {other:?}");
            Err(anyhow::anyhow!("Unexpected take-order action: {other:?}"))
        }
    }
}

fn normalize_taken_order(mut order: SmallOrder, fallback_order_id: uuid::Uuid) -> SmallOrder {
    if order.id.is_none() {
        log::warn!(
            "[take_order] Mostro response Order payload missing id; falling back to requested order_id={}",
            fallback_order_id
        );
        order.id = Some(fallback_order_id);
    }
    order
}

/// Mostro split fee charged to each party (`fee_rate * amount / 2`, rounded).
///
/// Mirrors `mostro::util::get_fee` so the buyer-invoice amount can be checked
/// against the book order the user took (MOSTRO-078).
fn mostro_split_fee(amount: i64, fee_rate: f64) -> i64 {
    ((fee_rate * amount as f64) / 2.0).round() as i64
}

/// Buyer payout invoice sats for a fixed-price take: `amount - split_fee`.
fn expected_buyer_invoice_sats(book_amount: i64, fee_rate: f64) -> i64 {
    book_amount.saturating_sub(mostro_split_fee(book_amount, fee_rate))
}

/// Cross-check a take-sell `AddInvoice` SmallOrder against the book order the user took.
///
/// Returns the trusted sats amount to show on the AddInvoice popup and to persist.
/// Fixed-price books (`requested.amount > 0`) must equal
/// [`expected_buyer_invoice_sats`]; market-price books (`amount == 0`) only require a
/// positive daemon quote after id / kind / status / fiat checks.
///
/// All identity fields on the reply (`id`, `kind`, `status`, `fiat_code`, `fiat_amount`)
/// are required — omitted or empty values are rejected (MOSTRO-078 fail-closed).
///
/// # Errors
///
/// Missing/mismatched id, kind, status, fiat, or sats; missing fee for fixed-price;
/// non-positive market quote (MOSTRO-078).
fn validate_take_sell_add_invoice_reply(
    requested: &SmallOrder,
    returned: &SmallOrder,
    take_fiat_amount: Option<i64>,
    fee_rate: Option<f64>,
) -> Result<i64> {
    let req_id = requested
        .id
        .ok_or_else(|| anyhow::anyhow!("Taken order is missing id"))?;
    let ret_id = returned
        .id
        .ok_or_else(|| anyhow::anyhow!("AddInvoice reply missing order id"))?;
    if req_id != ret_id {
        return Err(anyhow::anyhow!(
            "AddInvoice order id mismatch: took {}, daemon sent {}",
            req_id,
            ret_id
        ));
    }

    let kind = returned
        .kind
        .ok_or_else(|| anyhow::anyhow!("AddInvoice reply missing order kind"))?;
    if requested.kind.is_some_and(|k| k != kind) {
        return Err(anyhow::anyhow!(
            "AddInvoice order kind mismatch: expected {:?}, got {:?}",
            requested.kind,
            kind
        ));
    }
    if kind != mostro_core::order::Kind::Sell {
        return Err(anyhow::anyhow!(
            "AddInvoice after take-sell must be a sell order, got {:?}",
            kind
        ));
    }

    let status = returned
        .status
        .ok_or_else(|| anyhow::anyhow!("AddInvoice reply missing order status"))?;
    if status != Status::WaitingBuyerInvoice {
        return Err(anyhow::anyhow!(
            "AddInvoice status mismatch: expected WaitingBuyerInvoice, got {:?}",
            status
        ));
    }

    if returned.fiat_code.is_empty() {
        return Err(anyhow::anyhow!("AddInvoice reply missing fiat code"));
    }
    if returned.fiat_code != requested.fiat_code {
        return Err(anyhow::anyhow!(
            "AddInvoice fiat code mismatch: expected {}, got {}",
            requested.fiat_code,
            returned.fiat_code
        ));
    }

    let expected_fiat = take_fiat_amount.unwrap_or(requested.fiat_amount);
    if expected_fiat <= 0 {
        return Err(anyhow::anyhow!(
            "AddInvoice expected fiat amount must be positive, got {}",
            expected_fiat
        ));
    }
    if returned.fiat_amount != expected_fiat {
        return Err(anyhow::anyhow!(
            "AddInvoice fiat amount mismatch: expected {}, got {}",
            expected_fiat,
            returned.fiat_amount
        ));
    }

    // Fixed-price book orders: buyer invoice is amount − Mostro split fee.
    if requested.amount > 0 {
        let Some(rate) = fee_rate else {
            return Err(anyhow::anyhow!(
                "Cannot verify AddInvoice sats without Mostro fee from instance info"
            ));
        };
        let expected = expected_buyer_invoice_sats(requested.amount, rate);
        if returned.amount != expected {
            return Err(anyhow::anyhow!(
                "AddInvoice sats mismatch: expected {} (book {} minus fee), got {}",
                expected,
                requested.amount,
                returned.amount
            ));
        }
        return Ok(expected);
    }

    // Market-price book (amount == 0): sats are quoted by Mostro; require a positive
    // amount and rely on id/fiat/status checks above.
    if returned.amount <= 0 {
        return Err(anyhow::anyhow!(
            "AddInvoice market-price reply missing positive sats amount"
        ));
    }
    Ok(returned.amount)
}

async fn persist_taken_order(
    returned_order: SmallOrder,
    fallback_order_id: uuid::Uuid,
    request_id: u64,
    next_idx: i64,
    pool: &sqlx::sqlite::SqlitePool,
    trade_keys: &Keys,
    dm_subscription_tx: Option<&UnboundedSender<OrderDmSubscriptionCmd>>,
) -> SmallOrder {
    let normalized = normalize_taken_order(returned_order, fallback_order_id);
    let effective_order_id = normalized.id.unwrap_or(fallback_order_id);
    log::info!(
        "[take_order] Action::AddInvoice mapped to effective_order_id={}, trade_index={}",
        effective_order_id,
        next_idx
    );

    if let Err(e) = save_order(
        normalized.clone(),
        trade_keys,
        request_id,
        next_idx,
        pool,
        false,
    )
    .await
    {
        log::error!("Failed to save order to database: {}", e);
    }
    if dm_subscription_tx.is_some() {
        log::info!(
            "[take_order] Sending DM subscription command for order_id={}, trade_index={}",
            effective_order_id,
            next_idx
        );
        send_track_order_cmd(effective_order_id, next_idx);
    }
    normalized
}

/// Open the Add Invoice UI for a take-sell reply (`AddInvoice` + `Payload::Order`).
///
/// `auto_popup_shown` is set so a later copy of the same DM from the trade-key listener
/// does not open a second popup.
///
/// `trusted_sats` must come from [`validate_take_sell_add_invoice_reply`] — never frame
/// the popup from an unchecked daemon SmallOrder amount (MOSTRO-078).
fn take_add_invoice_operation_result(
    response_message: &Message,
    order: &SmallOrder,
    timestamp: i64,
    sender: PublicKey,
    trade_index: i64,
    trusted_sats: i64,
) -> OperationResult {
    let order_id = order.id;
    let order_status = order
        .status
        .or(Some(mostro_core::order::Status::WaitingBuyerInvoice));
    let mut snapshot = order.clone();
    snapshot.amount = trusted_sats;
    let order_message = OrderMessage {
        message: response_message.clone(),
        timestamp,
        sender,
        order_id,
        trade_index,
        sat_amount: Some(trusted_sats),
        buyer_invoice: None,
        order_kind: order.kind,
        is_mine: Some(false),
        order_status,
        order_snapshot: Some(snapshot),
        read: true,
        auto_popup_shown: true,
    };
    let notification = order_message_to_notification(&order_message);
    OperationResult::OpenInvoicePopup {
        notification,
        order_message: Box::new(order_message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mostro_core::prelude::{Action, Payload, Status};

    fn sample_small_order(id: uuid::Uuid) -> SmallOrder {
        SmallOrder {
            id: Some(id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::WaitingBuyerInvoice),
            amount: 21_000,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn map_take_reply_add_invoice_order_is_not_success() {
        let order = sample_small_order(uuid::Uuid::new_v4());
        let mapped = map_take_reply(&Action::AddInvoice, &Some(Payload::Order(order.clone())))
            .expect("AddInvoice+Order must map");
        match mapped {
            MappedTakeReply::AddInvoice(o) => assert_eq!(o.id, order.id),
            MappedTakeReply::PaymentRequest { .. } => panic!("must not treat AddInvoice as pay"),
        }
    }

    #[test]
    fn map_take_reply_rejects_new_order_as_take_success() {
        let order = sample_small_order(uuid::Uuid::new_v4());
        let err = map_take_reply(&Action::NewOrder, &Some(Payload::Order(order)))
            .expect_err("NewOrder must not be a take success");
        assert!(err.to_string().contains("Unexpected take-order action"));
    }

    #[test]
    fn map_take_reply_pay_invoice_requires_payment_request() {
        let err = map_take_reply(
            &Action::PayInvoice,
            &Some(Payload::Order(sample_small_order(uuid::Uuid::new_v4()))),
        )
        .expect_err("PayInvoice with Order payload is invalid");
        assert!(err.to_string().contains("PaymentRequest"));
    }

    #[test]
    fn take_add_invoice_opens_invoice_popup_not_created_success() {
        let order_id = uuid::Uuid::new_v4();
        let order = sample_small_order(order_id);
        let message = Message::new_order(
            Some(order_id),
            Some(1),
            Some(2),
            Action::AddInvoice,
            Some(Payload::Order(order.clone())),
        );
        let sender = Keys::generate().public_key();
        let trusted_sats = 20_790;
        let result =
            take_add_invoice_operation_result(&message, &order, 1, sender, 2, trusted_sats);
        match result {
            OperationResult::OpenInvoicePopup {
                notification,
                order_message,
            } => {
                assert_eq!(notification.action, Action::AddInvoice);
                assert_eq!(notification.order_id, Some(order_id));
                assert_eq!(notification.sat_amount, Some(trusted_sats));
                assert_eq!(order_message.sat_amount, Some(trusted_sats));
                assert_eq!(
                    order_message.message.get_inner_message_kind().action,
                    Action::AddInvoice
                );
                assert_eq!(order_message.is_mine, Some(false));
                assert!(order_message.auto_popup_shown);
            }
            other => panic!("expected OpenInvoicePopup, got {other:?}"),
        }
    }

    #[test]
    fn split_fee_matches_mostro_half_rate_rounding() {
        // fee_rate 0.01 → 0.5% per party on 21_000 = 105
        assert_eq!(mostro_split_fee(21_000, 0.01), 105);
        assert_eq!(expected_buyer_invoice_sats(21_000, 0.01), 20_895);
    }

    #[test]
    fn validate_add_invoice_accepts_fixed_amount_minus_fee() {
        let id = uuid::Uuid::new_v4();
        let requested = sample_small_order(id);
        let mut returned = sample_small_order(id);
        returned.amount = expected_buyer_invoice_sats(21_000, 0.01);
        let sats =
            validate_take_sell_add_invoice_reply(&requested, &returned, None, Some(0.01)).unwrap();
        assert_eq!(sats, 20_895);
    }

    #[test]
    fn validate_add_invoice_rejects_fabricated_sats() {
        let id = uuid::Uuid::new_v4();
        let requested = sample_small_order(id);
        let mut returned = sample_small_order(id);
        returned.amount = 1; // shave
        let err = validate_take_sell_add_invoice_reply(&requested, &returned, None, Some(0.01))
            .expect_err("fabricated sats must fail");
        assert!(err.to_string().contains("sats mismatch"));
    }

    #[test]
    fn validate_add_invoice_rejects_id_mismatch() {
        let requested = sample_small_order(uuid::Uuid::new_v4());
        let returned = sample_small_order(uuid::Uuid::new_v4());
        let err = validate_take_sell_add_invoice_reply(&requested, &returned, None, Some(0.01))
            .expect_err("id mismatch must fail");
        assert!(err.to_string().contains("order id mismatch"));
    }

    #[test]
    fn validate_add_invoice_rejects_wrong_status() {
        let id = uuid::Uuid::new_v4();
        let requested = sample_small_order(id);
        let mut returned = sample_small_order(id);
        returned.amount = expected_buyer_invoice_sats(21_000, 0.01);
        returned.status = Some(Status::Success);
        let err = validate_take_sell_add_invoice_reply(&requested, &returned, None, Some(0.01))
            .expect_err("wrong status must fail");
        assert!(err.to_string().contains("status mismatch"));
    }

    #[test]
    fn validate_add_invoice_requires_fee_for_fixed_amount() {
        let id = uuid::Uuid::new_v4();
        let requested = sample_small_order(id);
        let mut returned = sample_small_order(id);
        returned.amount = 20_895;
        let err = validate_take_sell_add_invoice_reply(&requested, &returned, None, None)
            .expect_err("fixed amount without fee must fail");
        assert!(err.to_string().contains("fee"));
    }

    #[test]
    fn validate_add_invoice_market_price_requires_positive_sats() {
        let id = uuid::Uuid::new_v4();
        let mut requested = sample_small_order(id);
        requested.amount = 0;
        let mut returned = sample_small_order(id);
        returned.amount = 50_000;
        let sats = validate_take_sell_add_invoice_reply(&requested, &returned, None, None).unwrap();
        assert_eq!(sats, 50_000);

        returned.amount = 0;
        let err = validate_take_sell_add_invoice_reply(&requested, &returned, None, None)
            .expect_err("zero market sats must fail");
        assert!(err.to_string().contains("positive sats"));
    }

    #[test]
    fn validate_add_invoice_checks_range_fiat_amount() {
        let id = uuid::Uuid::new_v4();
        let mut requested = sample_small_order(id);
        requested.amount = 0;
        requested.min_amount = Some(50);
        requested.max_amount = Some(200);
        requested.fiat_amount = 0;
        let mut returned = sample_small_order(id);
        returned.amount = 40_000;
        returned.fiat_amount = 75;
        let sats =
            validate_take_sell_add_invoice_reply(&requested, &returned, Some(75), None).unwrap();
        assert_eq!(sats, 40_000);

        returned.fiat_amount = 99;
        let err = validate_take_sell_add_invoice_reply(&requested, &returned, Some(75), None)
            .expect_err("fiat mismatch must fail");
        assert!(err.to_string().contains("fiat amount mismatch"));
    }

    #[test]
    fn validate_add_invoice_rejects_omitted_identity_fields() {
        let id = uuid::Uuid::new_v4();
        let requested = sample_small_order(id);
        let net = expected_buyer_invoice_sats(21_000, 0.01);

        let mut missing_id = sample_small_order(id);
        missing_id.id = None;
        missing_id.amount = net;
        assert!(
            validate_take_sell_add_invoice_reply(&requested, &missing_id, None, Some(0.01))
                .unwrap_err()
                .to_string()
                .contains("missing order id")
        );

        let mut missing_kind = sample_small_order(id);
        missing_kind.kind = None;
        missing_kind.amount = net;
        assert!(
            validate_take_sell_add_invoice_reply(&requested, &missing_kind, None, Some(0.01))
                .unwrap_err()
                .to_string()
                .contains("missing order kind")
        );

        let mut missing_status = sample_small_order(id);
        missing_status.status = None;
        missing_status.amount = net;
        assert!(validate_take_sell_add_invoice_reply(
            &requested,
            &missing_status,
            None,
            Some(0.01)
        )
        .unwrap_err()
        .to_string()
        .contains("missing order status"));

        let mut empty_fiat_code = sample_small_order(id);
        empty_fiat_code.fiat_code.clear();
        empty_fiat_code.amount = net;
        assert!(validate_take_sell_add_invoice_reply(
            &requested,
            &empty_fiat_code,
            None,
            Some(0.01)
        )
        .unwrap_err()
        .to_string()
        .contains("missing fiat code"));

        let mut zero_fiat = sample_small_order(id);
        zero_fiat.fiat_amount = 0;
        zero_fiat.amount = net;
        assert!(
            validate_take_sell_add_invoice_reply(&requested, &zero_fiat, None, Some(0.01))
                .unwrap_err()
                .to_string()
                .contains("fiat amount mismatch")
        );
    }
}
