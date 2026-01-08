// Integration tests for helper functions
use mostrix::util::types::get_cant_do_description;
use mostro_core::prelude::CantDoReason;

#[test]
fn test_get_cant_do_description_all_reasons() {
    // Test all CantDoReason variants return non-empty descriptions
    let reasons = vec![
        CantDoReason::InvalidSignature,
        CantDoReason::InvalidTradeIndex,
        CantDoReason::InvalidAmount,
        CantDoReason::InvalidInvoice,
        CantDoReason::InvalidPaymentRequest,
        CantDoReason::InvalidPeer,
        CantDoReason::InvalidRating,
        CantDoReason::InvalidTextMessage,
        CantDoReason::InvalidOrderKind,
        CantDoReason::InvalidOrderStatus,
        CantDoReason::InvalidPubkey,
        CantDoReason::InvalidParameters,
        CantDoReason::OrderAlreadyCanceled,
        CantDoReason::CantCreateUser,
        CantDoReason::IsNotYourOrder,
        CantDoReason::NotAllowedByStatus,
        CantDoReason::OutOfRangeFiatAmount,
        CantDoReason::OutOfRangeSatsAmount,
        CantDoReason::IsNotYourDispute,
        CantDoReason::DisputeTakenByAdmin,
        CantDoReason::DisputeCreationError,
        CantDoReason::NotFound,
        CantDoReason::InvalidDisputeStatus,
        CantDoReason::InvalidAction,
        CantDoReason::PendingOrderExists,
        CantDoReason::InvalidFiatCurrency,
        CantDoReason::TooManyRequests,
    ];

    for reason in reasons {
        let description = get_cant_do_description(&reason);
        assert!(
            !description.is_empty(),
            "Description should not be empty for {:?}",
            reason
        );
    }
}
