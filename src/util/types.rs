// Common types and enums used across nostr utilities
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;

#[derive(Clone, Debug)]
pub enum ListKind {
    Orders,
    Disputes,
    DirectMessagesUser,
    DirectMessagesAdmin,
    PrivateDirectMessagesUser,
}

#[derive(Clone, Debug)]
pub enum Event {
    SmallOrder(SmallOrder),
    Dispute(Dispute),
    MessageTuple(Box<(Message, u64, PublicKey)>),
}

/// Convert CantDoReason to user-friendly description
pub fn get_cant_do_description(reason: &CantDoReason) -> String {
    match reason {
        CantDoReason::InvalidSignature => "Invalid signature - authentication failed".to_string(),
        CantDoReason::InvalidTradeIndex => "Invalid trade index - please try again".to_string(),
        CantDoReason::InvalidAmount => "Invalid amount - check your order values".to_string(),
        CantDoReason::InvalidInvoice => {
            "Invalid invoice - please provide a valid lightning invoice".to_string()
        }
        CantDoReason::InvalidPaymentRequest => "Invalid payment request".to_string(),
        CantDoReason::InvalidPeer => "Invalid peer information".to_string(),
        CantDoReason::InvalidRating => "Invalid rating value".to_string(),
        CantDoReason::InvalidTextMessage => "Invalid text message".to_string(),
        CantDoReason::InvalidOrderKind => {
            "Invalid order kind - must be 'buy' or 'sell'".to_string()
        }
        CantDoReason::InvalidOrderStatus => "Invalid order status".to_string(),
        CantDoReason::InvalidPubkey => "Invalid public key".to_string(),
        CantDoReason::InvalidParameters => {
            "Invalid parameters - check your order details".to_string()
        }
        CantDoReason::InvalidPayload => {
            "Invalid payload - check bond slash choices or message format".to_string()
        }
        CantDoReason::OrderAlreadyCanceled => "Order is already canceled".to_string(),
        CantDoReason::CantCreateUser => "Cannot create user - please contact support".to_string(),
        CantDoReason::IsNotYourOrder => "This is not your order".to_string(),
        CantDoReason::NotAllowedByStatus => {
            "Action not allowed - order status prevents this operation".to_string()
        }
        CantDoReason::OutOfRangeFiatAmount => "Fiat amount is out of acceptable range".to_string(),
        CantDoReason::OutOfRangeSatsAmount => {
            "Satoshis amount is out of acceptable range".to_string()
        }
        CantDoReason::IsNotYourDispute => "This is not your dispute".to_string(),
        CantDoReason::DisputeTakenByAdmin => {
            "Dispute has been taken over by an administrator".to_string()
        }
        CantDoReason::DisputeCreationError => "Cannot create dispute for this order".to_string(),
        CantDoReason::NotFound => "Resource not found".to_string(),
        CantDoReason::InvalidDisputeStatus => "Invalid dispute status".to_string(),
        CantDoReason::InvalidAction => "Invalid action for current state".to_string(),
        CantDoReason::PendingOrderExists => {
            "You already have a pending order - please complete or cancel it first".to_string()
        }
        CantDoReason::InvalidFiatCurrency => {
            "Invalid fiat currency - currency not supported or specify a fixed rate".to_string()
        }
        CantDoReason::TooManyRequests => {
            "Too many requests - please wait and try again".to_string()
        }
        CantDoReason::NotAuthorized => "Not authorized to perform this action".to_string(),
    }
}
