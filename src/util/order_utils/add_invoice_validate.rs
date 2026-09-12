//! MOSTRO-078 take-sell `AddInvoice` sats validation (sync take path + DM listener).

use anyhow::Result;
use mostro_core::prelude::*;

/// How to verify fixed-price buyer-invoice sats when Mostro fee may be missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeCheck {
    /// Sync `take_order`: fee is required; amount must equal book − split fee.
    ExactRequired,
    /// DM listener after bond: if fee is known, exact match; otherwise accept
    /// `0 < returned.amount <= book` after identity/fiat checks.
    ExactOrUpperBound,
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
    )
}

/// Same as [`validate_take_sell_add_invoice_reply`] with an explicit [`FeeCheck`] policy.
pub fn validate_take_sell_add_invoice_reply_with_fee_check(
    requested: &SmallOrder,
    returned: &SmallOrder,
    take_fiat_amount: Option<i64>,
    fee_rate: Option<f64>,
    fee_check: FeeCheck,
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
            FeeCheck::ExactOrUpperBound => {
                if returned.amount <= 0 || returned.amount > requested.amount {
                    return Err(anyhow::anyhow!(
                        "AddInvoice sats out of range without fee: got {} (book {})",
                        returned.amount,
                        requested.amount
                    ));
                }
                log::warn!(
                    "AddInvoice sats accepted with upper-bound check only (fee unavailable): {} <= book {}",
                    returned.amount,
                    requested.amount
                );
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
    fn validate_add_invoice_upper_bound_without_fee() {
        let id = uuid::Uuid::new_v4();
        let requested = sample_small_order(id);
        let mut returned = sample_small_order(id);
        returned.amount = 20_895;
        let sats = validate_take_sell_add_invoice_reply_with_fee_check(
            &requested,
            &returned,
            None,
            None,
            FeeCheck::ExactOrUpperBound,
        )
        .unwrap();
        assert_eq!(sats, 20_895);

        returned.amount = 21_001;
        let err = validate_take_sell_add_invoice_reply_with_fee_check(
            &requested,
            &returned,
            None,
            None,
            FeeCheck::ExactOrUpperBound,
        )
        .expect_err("above book must fail");
        assert!(err.to_string().contains("out of range"));
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
