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
use crate::util::order_utils::add_invoice_validate::{
    expected_buyer_invoice_sats, validate_take_sell_add_invoice_reply,
};
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

    // Fixed-price take-sell needs Mostro fee before we reserve an index or send
    // TakeSell — `process_take_order_reply` requires it for PayBondInvoice net persist
    // even when the buyer already supplied a payout invoice.
    ensure_fee_for_fixed_take_sell(&action, order, mostro_instance.and_then(|i| i.fee))?;

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
    let recv_event = wait_for_dm(
        &trade_keys,
        FETCH_EVENTS_TIMEOUT,
        Some(request_id),
        sent_message,
    )
    .await?;

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
                    action.clone(),
                    invoice.clone().filter(|s| !s.is_empty()),
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

/// Fail closed before reservation/send when a fixed-price take-sell cannot compute
/// the buyer-invoice net for PayBondInvoice persist. Invoice-provided takes still
/// hit `PayBondInvoice` when bonds are enabled, so fee is required there too.
fn ensure_fee_for_fixed_take_sell(
    action: &Action,
    order: &SmallOrder,
    fee_rate: Option<f64>,
) -> Result<()> {
    if matches!(action, Action::TakeSell) && order.amount > 0 && fee_rate.is_none() {
        return Err(anyhow::anyhow!(
            "Cannot take fixed-price sell without Mostro fee from instance info"
        ));
    }
    Ok(())
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
    take_action: Action,
    take_buyer_invoice: Option<String>,
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
            // PayBondInvoice SmallOrder.amount is the bond floor (often 1000), not
            // trade sats. For TakeSell (buyer), persist fee-adjusted buyer-invoice net
            // when fee is known so the DM listener can ExactOrMatchLocal. TakeBuy
            // (seller) next step is PayInvoice hold — persist book amount, never net.
            // Bond stays in popup sat_amount only.
            let trade_amount_to_persist = match (&take_action, &action) {
                (Action::TakeSell, Action::PayBondInvoice) => {
                    if requested.amount > 0 {
                        // Always persist fee-adjusted net for fixed take-sell bonds.
                        // Never store gross book as a stand-in "trusted net" when fee is
                        // missing (invoice-provided or not) — that would let a later
                        // AddInvoice pass ExactOrMatchLocal at book size.
                        let Some(rate) = fee_rate else {
                            return Err(anyhow::anyhow!(
                                "Cannot process fixed-price take-sell bond without Mostro fee from instance info"
                            ));
                        };
                        Some(expected_buyer_invoice_sats(requested.amount, rate))
                    } else {
                        Some(0)
                    }
                }
                (_, Action::PayBondInvoice) => {
                    // TakeBuy (or other): keep book/range amount, not buyer-invoice net.
                    Some(if requested.amount > 0 {
                        requested.amount
                    } else {
                        0
                    })
                }
                _ => None,
            };
            let mut order = order;
            if let Some(take_inv) = take_buyer_invoice.filter(|s| !s.is_empty()) {
                if let Some(ref mut small) = order {
                    if small.buyer_invoice.as_ref().is_none_or(|s| s.is_empty()) {
                        small.buyer_invoice = Some(take_inv);
                    }
                }
            }
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
                trade_amount_to_persist,
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
        buyer_reputation: None,
        seller_reputation: None,
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
    use crate::models::Order;
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

    async fn memory_orders_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE orders (
                id TEXT PRIMARY KEY, kind TEXT, status TEXT, amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL, min_amount INTEGER, max_amount INTEGER,
                fiat_amount INTEGER NOT NULL, payment_method TEXT NOT NULL,
                premium INTEGER NOT NULL, trade_keys TEXT, counterparty_pubkey TEXT,
                order_chat_shared_key_hex TEXT, dispute_id TEXT, solver_pubkey TEXT,
                dispute_chat_shared_key_hex TEXT, is_mine INTEGER NOT NULL,
                buyer_invoice TEXT, request_id INTEGER, trade_index INTEGER,
                created_at INTEGER, expires_at INTEGER, last_seen_dm_ts INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("orders table");
        pool
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
    fn ensure_fee_for_fixed_take_sell_runs_before_protocol_send() {
        let fixed = sample_small_order(uuid::Uuid::new_v4());
        let err = ensure_fee_for_fixed_take_sell(&Action::TakeSell, &fixed, None)
            .expect_err("fixed take-sell without fee must abort before reserve/send");
        assert!(err.to_string().contains("fee"));

        ensure_fee_for_fixed_take_sell(&Action::TakeSell, &fixed, Some(0.01))
            .expect("fee present allows take");

        let mut range = fixed.clone();
        range.amount = 0;
        ensure_fee_for_fixed_take_sell(&Action::TakeSell, &range, None)
            .expect("range/market take-sell does not require fee preflight");

        ensure_fee_for_fixed_take_sell(&Action::TakeBuy, &fixed, None)
            .expect("take-buy is not the fixed take-sell bond path");
    }

    #[tokio::test]
    async fn process_take_reply_pay_bond_fixed_persists_net_keeps_bond_in_popup() {
        // Regression: removing trade_amount_to_persist must fail this test.
        let pool = memory_orders_pool().await;
        let order_id = uuid::Uuid::new_v4();
        let book_amount = 21_000_i64;
        let bond_sats = 1_000_i64;
        let fee_rate = 0.01_f64;
        let expected_net = expected_buyer_invoice_sats(book_amount, fee_rate);

        let requested = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::Pending),
            amount: book_amount,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let bond_small = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::WaitingTakerBond),
            amount: bond_sats,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let response = Message::new_order(
            Some(order_id),
            Some(7),
            Some(3),
            Action::PayBondInvoice,
            Some(Payload::PaymentRequest(
                Some(bond_small),
                "lnbc1bond".to_string(),
                None,
            )),
        );
        let inner = response.get_inner_message_kind();
        let trade_keys = Keys::generate();
        let sender = Keys::generate().public_key();

        let result = process_take_order_reply(
            inner,
            &response,
            1,
            sender,
            &requested,
            None,
            Some(fee_rate),
            Action::TakeSell,
            None,
            7,
            3,
            &pool,
            &trade_keys,
            None,
        )
        .await
        .expect("PayBondInvoice take reply");

        match result {
            OperationResult::PaymentRequestRequired {
                order,
                sat_amount,
                action,
                invoice,
                ..
            } => {
                assert_eq!(action, Action::PayBondInvoice);
                assert_eq!(sat_amount, Some(bond_sats), "popup must show bond sats");
                assert_eq!(
                    order.amount, expected_net,
                    "result order must carry trusted buyer-invoice net, not bond"
                );
                assert_eq!(invoice, "lnbc1bond");
            }
            other => panic!("expected PaymentRequestRequired, got {other:?}"),
        }

        let stored = Order::get_by_id(&pool, &order_id.to_string())
            .await
            .expect("order persisted");
        assert_eq!(
            stored.amount, expected_net,
            "SQLite amount must be fee-adjusted net, not bond floor"
        );
        assert_ne!(stored.amount, bond_sats);
        assert_ne!(stored.amount, book_amount);
    }

    #[tokio::test]
    async fn process_take_reply_pay_bond_fixed_without_fee_aborts() {
        let pool = memory_orders_pool().await;
        let order_id = uuid::Uuid::new_v4();
        let book_amount = 21_000_i64;
        let bond_sats = 1_000_i64;

        let requested = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::Pending),
            amount: book_amount,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let bond_small = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::WaitingTakerBond),
            amount: bond_sats,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let response = Message::new_order(
            Some(order_id),
            Some(7),
            Some(3),
            Action::PayBondInvoice,
            Some(Payload::PaymentRequest(
                Some(bond_small),
                "lnbc1bond".to_string(),
                None,
            )),
        );
        let inner = response.get_inner_message_kind();
        let trade_keys = Keys::generate();

        let err = process_take_order_reply(
            inner,
            &response,
            1,
            Keys::generate().public_key(),
            &requested,
            None,
            None, // fee unavailable — must not persist gross book
            Action::TakeSell,
            None,
            7,
            3,
            &pool,
            &trade_keys,
            None,
        )
        .await
        .expect_err("fixed PayBondInvoice without fee must abort");
        assert!(
            err.to_string().contains("fee"),
            "error should mention missing fee, got: {err}"
        );
        assert!(
            Order::get_by_id(&pool, &order_id.to_string())
                .await
                .is_err(),
            "must not persist an order when fee is missing"
        );
    }

    #[tokio::test]
    async fn process_take_reply_pay_bond_range_persists_zero_keeps_bond_in_popup() {
        let pool = memory_orders_pool().await;
        let order_id = uuid::Uuid::new_v4();
        let bond_sats = 1_000_i64;
        let take_fiat = 75_i64;

        let requested = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::Pending),
            amount: 0,
            min_amount: Some(50),
            max_amount: Some(200),
            fiat_code: "USD".to_string(),
            fiat_amount: 0,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let bond_small = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::WaitingTakerBond),
            amount: bond_sats,
            fiat_code: "USD".to_string(),
            fiat_amount: take_fiat,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let response = Message::new_order(
            Some(order_id),
            Some(8),
            Some(4),
            Action::PayBondInvoice,
            Some(Payload::PaymentRequest(
                Some(bond_small),
                "lnbc1rangebond".to_string(),
                None,
            )),
        );
        let inner = response.get_inner_message_kind();
        let trade_keys = Keys::generate();

        let result = process_take_order_reply(
            inner,
            &response,
            1,
            Keys::generate().public_key(),
            &requested,
            Some(take_fiat),
            Some(0.01),
            Action::TakeSell,
            None,
            8,
            4,
            &pool,
            &trade_keys,
            None,
        )
        .await
        .expect("range PayBondInvoice");

        match result {
            OperationResult::PaymentRequestRequired {
                order,
                sat_amount,
                action,
                ..
            } => {
                assert_eq!(action, Action::PayBondInvoice);
                assert_eq!(sat_amount, Some(bond_sats));
                assert_eq!(
                    order.amount, 0,
                    "range/market book amount stays 0 until AddInvoice"
                );
                assert_eq!(order.fiat_amount, take_fiat);
            }
            other => panic!("expected PaymentRequestRequired, got {other:?}"),
        }

        let stored = Order::get_by_id(&pool, &order_id.to_string())
            .await
            .expect("order persisted");
        assert_eq!(stored.amount, 0);
        assert_eq!(stored.fiat_amount, take_fiat);
    }

    #[tokio::test]
    async fn process_take_reply_pay_invoice_preserves_payload_amount() {
        // Non-bond path must not apply trade_amount_to_persist override.
        let pool = memory_orders_pool().await;
        let order_id = uuid::Uuid::new_v4();
        let hold_sats = 21_105_i64;

        let requested = sample_small_order(order_id);
        let hold_small = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::WaitingPayment),
            amount: hold_sats,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let response = Message::new_order(
            Some(order_id),
            Some(9),
            Some(5),
            Action::PayInvoice,
            Some(Payload::PaymentRequest(
                Some(hold_small),
                "lnbc1hold".to_string(),
                None,
            )),
        );
        let inner = response.get_inner_message_kind();
        let trade_keys = Keys::generate();

        let result = process_take_order_reply(
            inner,
            &response,
            1,
            Keys::generate().public_key(),
            &requested,
            None,
            Some(0.01),
            Action::TakeBuy,
            None,
            9,
            5,
            &pool,
            &trade_keys,
            None,
        )
        .await
        .expect("PayInvoice take reply");

        match result {
            OperationResult::PaymentRequestRequired {
                order,
                sat_amount,
                action,
                ..
            } => {
                assert_eq!(action, Action::PayInvoice);
                assert_eq!(sat_amount, Some(hold_sats));
                assert_eq!(order.amount, hold_sats);
            }
            other => panic!("expected PaymentRequestRequired, got {other:?}"),
        }

        let stored = Order::get_by_id(&pool, &order_id.to_string())
            .await
            .expect("order persisted");
        assert_eq!(stored.amount, hold_sats);
    }

    #[tokio::test]
    async fn process_take_reply_take_buy_pay_bond_persists_book_not_buyer_net() {
        let pool = memory_orders_pool().await;
        let order_id = uuid::Uuid::new_v4();
        let book_amount = 21_000_i64;
        let bond_sats = 1_000_i64;
        let fee_rate = 0.01_f64;
        let buyer_net = expected_buyer_invoice_sats(book_amount, fee_rate);

        let requested = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Buy),
            status: Some(Status::Pending),
            amount: book_amount,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let bond_small = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Buy),
            status: Some(Status::WaitingTakerBond),
            amount: bond_sats,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let response = Message::new_order(
            Some(order_id),
            Some(11),
            Some(6),
            Action::PayBondInvoice,
            Some(Payload::PaymentRequest(
                Some(bond_small),
                "lnbc1buybond".to_string(),
                None,
            )),
        );
        let inner = response.get_inner_message_kind();
        let trade_keys = Keys::generate();

        let result = process_take_order_reply(
            inner,
            &response,
            1,
            Keys::generate().public_key(),
            &requested,
            None,
            Some(fee_rate),
            Action::TakeBuy,
            None,
            11,
            6,
            &pool,
            &trade_keys,
            None,
        )
        .await
        .expect("TakeBuy PayBondInvoice");

        match result {
            OperationResult::PaymentRequestRequired {
                order,
                sat_amount,
                action,
                ..
            } => {
                assert_eq!(action, Action::PayBondInvoice);
                assert_eq!(sat_amount, Some(bond_sats));
                assert_eq!(
                    order.amount, book_amount,
                    "TakeBuy must persist book amount, not buyer-invoice net"
                );
                assert_ne!(order.amount, buyer_net);
            }
            other => panic!("expected PaymentRequestRequired, got {other:?}"),
        }

        let stored = Order::get_by_id(&pool, &order_id.to_string())
            .await
            .expect("order persisted");
        assert_eq!(stored.amount, book_amount);
        assert_ne!(stored.amount, buyer_net);
        assert_ne!(stored.amount, bond_sats);
    }

    #[tokio::test]
    async fn process_take_reply_invoice_provided_without_fee_still_requires_fee_on_bond() {
        let pool = memory_orders_pool().await;
        let order_id = uuid::Uuid::new_v4();
        let book_amount = 21_000_i64;
        let bond_sats = 1_000_i64;

        let requested = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::Pending),
            amount: book_amount,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let bond_small = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::WaitingTakerBond),
            amount: bond_sats,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let response = Message::new_order(
            Some(order_id),
            Some(12),
            Some(7),
            Action::PayBondInvoice,
            Some(Payload::PaymentRequest(
                Some(bond_small),
                "lnbc1bond".to_string(),
                None,
            )),
        );
        let inner = response.get_inner_message_kind();
        let trade_keys = Keys::generate();

        let err = process_take_order_reply(
            inner,
            &response,
            1,
            Keys::generate().public_key(),
            &requested,
            None,
            None,
            Action::TakeSell,
            Some("lnbc1buyeratake".into()),
            12,
            7,
            &pool,
            &trade_keys,
            None,
        )
        .await
        .expect_err("invoice-provided must not persist gross book without fee");
        assert!(err.to_string().contains("fee"));
    }

    #[tokio::test]
    async fn process_take_reply_invoice_provided_persists_buyer_invoice_and_net() {
        let pool = memory_orders_pool().await;
        let order_id = uuid::Uuid::new_v4();
        let book_amount = 21_000_i64;
        let bond_sats = 1_000_i64;
        let fee_rate = 0.01_f64;
        let expected_net = expected_buyer_invoice_sats(book_amount, fee_rate);
        let take_inv = "lnbc1buyeratake".to_string();

        let requested = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::Pending),
            amount: book_amount,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let bond_small = SmallOrder {
            id: Some(order_id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::WaitingTakerBond),
            amount: bond_sats,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            ..Default::default()
        };
        let response = Message::new_order(
            Some(order_id),
            Some(13),
            Some(8),
            Action::PayBondInvoice,
            Some(Payload::PaymentRequest(
                Some(bond_small),
                "lnbc1bond".to_string(),
                None,
            )),
        );
        let inner = response.get_inner_message_kind();
        let trade_keys = Keys::generate();

        process_take_order_reply(
            inner,
            &response,
            1,
            Keys::generate().public_key(),
            &requested,
            None,
            Some(fee_rate),
            Action::TakeSell,
            Some(take_inv.clone()),
            13,
            8,
            &pool,
            &trade_keys,
            None,
        )
        .await
        .expect("invoice-provided bond with fee");

        let stored = Order::get_by_id(&pool, &order_id.to_string())
            .await
            .expect("persisted");
        assert_eq!(stored.amount, expected_net);
        assert_eq!(stored.buyer_invoice.as_deref(), Some(take_inv.as_str()));
    }
}
