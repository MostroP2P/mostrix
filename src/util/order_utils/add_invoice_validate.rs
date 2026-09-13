//! MOSTRO-078 take-sell `AddInvoice` sats validation (sync take path + DM listener).
//!
//! Reply `status` is lifecycle-aware via [`AddInvoicePhase`]: take/bond uses
//! `WaitingBuyerInvoice`; post-retry replacement invoices keep
//! `SettledHoldInvoice` (Mostro `check_failure_retries` does not rewind).

use anyhow::Result;
use mostro_core::prelude::*;

/// Which `AddInvoice` SmallOrder status the daemon is expected to send.
///
/// Initial take/bond replies stay `WaitingBuyerInvoice`. After payout retries
/// fail, Mostro's `check_failure_retries` clones the live order (status
/// `SettledHoldInvoice`) into `Payload::Order` — it does not rewind to waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddInvoicePhase {
    /// First buyer invoice after take / bond.
    Take,
    /// Replacement invoice after failed Lightning payout retries.
    PostRetry,
}

fn returned_status_ok_for_phase(status: Status, phase: AddInvoicePhase) -> bool {
    match phase {
        AddInvoicePhase::Take => status == Status::WaitingBuyerInvoice,
        AddInvoicePhase::PostRetry => {
            matches!(status, Status::SettledHoldInvoice | Status::Success)
        }
    }
}

/// How to verify fixed-price buyer-invoice sats when Mostro fee may be missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeCheck {
    /// Sync `take_order`: fee is required; amount must equal book − split fee.
    /// `requested.amount` is the **book** gross.
    ExactRequired,
    /// DM listener after bond: if fee is known, treat `requested.amount` as book and
    /// require exact net. If fee is missing, require `returned.amount == requested.amount`
    /// (local row must already hold the trusted buyer-invoice net from the take path).
    /// Never accept an arbitrary positive amount below book (bond floor would pass).
    ExactOrMatchLocal,
}

/// Mostro split fee charged to each party (`fee_rate * amount / 2`, rounded).
///
/// Mirrors `mostro::util::get_fee` so the buyer-invoice amount can be checked
/// against the book order the user took (MOSTRO-078).
pub fn mostro_split_fee(amount: i64, fee_rate: f64) -> i64 {
    ((fee_rate * amount as f64) / 2.0).round() as i64
}

/// Buyer payout invoice sats for a fixed-price take: `amount - split_fee`.
pub fn expected_buyer_invoice_sats(book_amount: i64, fee_rate: f64) -> i64 {
    book_amount.saturating_sub(mostro_split_fee(book_amount, fee_rate))
}

/// Cross-check a take-sell `AddInvoice` SmallOrder against the book order the user took.
///
/// This wrapper is the **take/bond** path ([`AddInvoicePhase::Take`]): daemon
/// `status` must be `WaitingBuyerInvoice`. Post-retry replacement invoices use
/// [`validate_take_sell_add_invoice_reply_with_fee_check`] with
/// [`AddInvoicePhase::PostRetry`].
///
/// Returns the trusted sats amount to show on the AddInvoice popup and to persist.
/// Fixed-price books (`requested.amount > 0`) must equal
/// [`expected_buyer_invoice_sats`] when fee is available; market-price books
/// (`amount == 0`) only require a positive daemon quote after id / kind / status / fiat checks.
///
/// All identity fields on the reply (`id`, `kind`, `status`, `fiat_code`, `fiat_amount`)
/// are required — omitted or empty values are rejected (MOSTRO-078 fail-closed).
///
/// # Errors
///
/// Missing/mismatched id, kind, status, fiat, or sats; missing fee for fixed-price
/// under [`FeeCheck::ExactRequired`]; non-positive market quote (MOSTRO-078).
pub fn validate_take_sell_add_invoice_reply(
    requested: &SmallOrder,
    returned: &SmallOrder,
    take_fiat_amount: Option<i64>,
    fee_rate: Option<f64>,
) -> Result<i64> {
    validate_take_sell_add_invoice_reply_with_fee_check(
        requested,
        returned,
        take_fiat_amount,
        fee_rate,
        FeeCheck::ExactRequired,
        AddInvoicePhase::Take,
    )
}

/// Same as [`validate_take_sell_add_invoice_reply`] with explicit [`FeeCheck`]
/// and [`AddInvoicePhase`] policies.
///
/// `phase` selects which daemon `status` is acceptable:
/// [`AddInvoicePhase::Take`] requires `WaitingBuyerInvoice`;
/// [`AddInvoicePhase::PostRetry`] requires `SettledHoldInvoice` or legacy `Success`.
pub fn validate_take_sell_add_invoice_reply_with_fee_check(
    requested: &SmallOrder,
    returned: &SmallOrder,
    take_fiat_amount: Option<i64>,
    fee_rate: Option<f64>,
    fee_check: FeeCheck,
    phase: AddInvoicePhase,
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
    if !returned_status_ok_for_phase(status, phase) {
        return Err(anyhow::anyhow!(
            "AddInvoice status mismatch for {:?}: got {:?}",
            phase,
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

    // Fixed-price: buyer invoice is amount − Mostro split fee (when `requested` is book),
    // or an exact match to a locally trusted net already persisted at take/bond time.
    if requested.amount > 0 {
        if let Some(rate) = fee_rate {
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
        match fee_check {
            FeeCheck::ExactRequired => {
                return Err(anyhow::anyhow!(
                    "Cannot verify AddInvoice sats without Mostro fee from instance info"
                ));
            }
            FeeCheck::ExactOrMatchLocal => {
                if returned.amount <= 0 || returned.amount != requested.amount {
                    return Err(anyhow::anyhow!(
                        "AddInvoice sats mismatch without fee: got {} (local trusted {})",
                        returned.amount,
                        requested.amount
                    ));
                }
                return Ok(returned.amount);
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_small_order(id: uuid::Uuid) -> SmallOrder {
        SmallOrder {
            id: Some(id),
            kind: Some(mostro_core::order::Kind::Sell),
            status: Some(Status::WaitingBuyerInvoice),
            amount: 21_000,
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            payment_method: "SEPA".to_string(),
            premium: 0,
            ..Default::default()
        }
    }

    #[test]
    fn split_fee_matches_mostro_half_rate_rounding() {
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
        returned.amount = 1;
        let err = validate_take_sell_add_invoice_reply(&requested, &returned, None, Some(0.01))
            .expect_err("fabricated sats must fail");
        assert!(err.to_string().contains("sats mismatch"));
    }

    #[test]
    fn validate_add_invoice_requires_fee_for_fixed_amount_exact() {
        let id = uuid::Uuid::new_v4();
        let requested = sample_small_order(id);
        let mut returned = sample_small_order(id);
        returned.amount = 20_895;
        let err = validate_take_sell_add_invoice_reply(&requested, &returned, None, None)
            .expect_err("fixed amount without fee must fail");
        assert!(err.to_string().contains("fee"));
    }

    #[test]
    fn validate_add_invoice_match_local_without_fee() {
        let id = uuid::Uuid::new_v4();
        let mut requested = sample_small_order(id);
        // Local row already stores trusted buyer-invoice net from take/bond.
        requested.amount = 20_895;
        let mut returned = sample_small_order(id);
        returned.amount = 20_895;
        let sats = validate_take_sell_add_invoice_reply_with_fee_check(
            &requested,
            &returned,
            None,
            None,
            FeeCheck::ExactOrMatchLocal,
            AddInvoicePhase::Take,
        )
        .unwrap();
        assert_eq!(sats, 20_895);

        // Bond floor / forged under-amount must not pass.
        returned.amount = 1_000;
        let err = validate_take_sell_add_invoice_reply_with_fee_check(
            &requested,
            &returned,
            None,
            None,
            FeeCheck::ExactOrMatchLocal,
            AddInvoicePhase::Take,
        )
        .expect_err("bond-sized amount must fail");
        assert!(err.to_string().contains("mismatch without fee"));
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

        let mut wrong_kind = sample_small_order(id);
        wrong_kind.kind = Some(mostro_core::order::Kind::Buy);
        wrong_kind.amount = net;
        let wrong_kind_err =
            validate_take_sell_add_invoice_reply(&requested, &wrong_kind, None, Some(0.01))
                .unwrap_err()
                .to_string();
        assert!(
            wrong_kind_err.contains("kind mismatch") || wrong_kind_err.contains("must be a sell"),
            "got: {wrong_kind_err}"
        );

        // Same identity failures apply under the listener policy.
        assert!(validate_take_sell_add_invoice_reply_with_fee_check(
            &requested,
            &missing_id,
            None,
            None,
            FeeCheck::ExactOrMatchLocal,
            AddInvoicePhase::Take,
        )
        .unwrap_err()
        .to_string()
        .contains("missing order id"));
    }

    #[test]
    fn validate_add_invoice_take_rejects_settled_hold_invoice_status() {
        let id = uuid::Uuid::new_v4();
        let requested = sample_small_order(id);
        let mut returned = sample_small_order(id);
        returned.amount = expected_buyer_invoice_sats(21_000, 0.01);
        returned.status = Some(Status::SettledHoldInvoice);
        let err = validate_take_sell_add_invoice_reply(&requested, &returned, None, Some(0.01))
            .expect_err("initial take must not accept settled-hold-invoice");
        assert!(err.to_string().contains("status mismatch"));
    }

    #[test]
    fn validate_add_invoice_post_retry_accepts_settled_hold_invoice() {
        let id = uuid::Uuid::new_v4();
        let mut requested = sample_small_order(id);
        requested.amount = 20_895;
        requested.status = Some(Status::SettledHoldInvoice);
        let mut returned = requested.clone();
        let sats = validate_take_sell_add_invoice_reply_with_fee_check(
            &requested,
            &returned,
            None,
            None,
            FeeCheck::ExactOrMatchLocal,
            AddInvoicePhase::PostRetry,
        )
        .expect("daemon replacement invoice keeps SettledHoldInvoice");
        assert_eq!(sats, 20_895);

        returned.status = Some(Status::WaitingBuyerInvoice);
        let err = validate_take_sell_add_invoice_reply_with_fee_check(
            &requested,
            &returned,
            None,
            None,
            FeeCheck::ExactOrMatchLocal,
            AddInvoicePhase::PostRetry,
        )
        .expect_err("post-retry must not accept waiting-buyer-invoice");
        assert!(err.to_string().contains("status mismatch"));
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
}
