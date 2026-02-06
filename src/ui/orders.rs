use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use ratatui::style::{Color, Style};

#[derive(Clone, Debug, Default)]
pub struct OrderSuccess {
    pub order_id: Option<uuid::Uuid>,
    pub kind: Option<mostro_core::order::Kind>,
    pub amount: i64,
    pub fiat_code: String,
    pub fiat_amount: i64,
    pub min_amount: Option<i64>,
    pub max_amount: Option<i64>,
    pub payment_method: String,
    pub premium: i64,
    pub status: Option<Status>,
    pub trade_index: Option<i64>, // Trade index used for this order
}

#[derive(Clone, Debug)]
pub enum OrderResult {
    Success(OrderSuccess),
    /// Payment request required - shows invoice popup for buy orders
    PaymentRequestRequired {
        order: mostro_core::prelude::SmallOrder,
        invoice: String,
        sat_amount: Option<i64>,
        trade_index: i64,
    },
    /// Generic informational popup (e.g. AddInvoice confirmation)
    Info(String),
    Error(String),
}

#[derive(Clone, Debug, Default)]
pub struct FormState {
    pub kind: String,            // buy | sell
    pub fiat_code: String,       // e.g. USD, EUR, ARS
    pub fiat_amount: String,     // numeric (single amount or min for range)
    pub fiat_amount_max: String, // max amount for range (optional)
    pub amount: String,          // amount in sats (0 for market)
    pub payment_method: String,  // comma separated
    pub premium: String,         // premium percentage
    pub invoice: String,         // optional invoice
    pub expiration_days: String, // expiration days (0 for no expiration)
    pub focused: usize,          // field index
    pub use_range: bool,         // whether to use fiat range
}

#[derive(Clone, Debug)]
pub struct TakeOrderState {
    pub order: SmallOrder,
    pub amount_input: String, // For range orders: the amount user wants to take
    pub is_range_order: bool, // Whether this is a range order (has min/max)
    pub validation_error: Option<String>, // Error message if amount is invalid
    pub selected_button: bool, // true for YES, false for NO
}

/// Represents a message related to an order
#[derive(Clone, Debug)]
pub struct OrderMessage {
    pub message: Message,
    pub timestamp: i64,
    pub sender: PublicKey,
    pub order_id: Option<uuid::Uuid>,
    pub trade_index: i64,
    pub sat_amount: Option<i64>,
    pub buyer_invoice: Option<String>,
    pub read: bool, // Whether the message has been read
    /// Whether we've already shown the automatic popup for this message
    pub auto_popup_shown: bool,
}

/// Notification for a new message
#[derive(Clone, Debug)]
pub struct MessageNotification {
    pub order_id: Option<uuid::Uuid>,
    pub message_preview: String,
    pub timestamp: i64,
    pub action: Action,
    pub sat_amount: Option<i64>,
    pub invoice: Option<String>,
}

/// State for handling invoice input in AddInvoice notifications
#[derive(Clone, Debug)]
pub struct InvoiceInputState {
    pub invoice_input: String,
    pub focused: bool,
    pub just_pasted: bool, // Flag to ignore Enter immediately after paste
    pub copied_to_clipboard: bool, // Flag to show "Copied!" message
}

/// State for handling key input (pubkey or privkey) in admin settings
#[derive(Clone, Debug)]
pub struct KeyInputState {
    pub key_input: String,
    pub focused: bool,
    pub just_pasted: bool, // Flag to ignore Enter immediately after paste
}

/// State for viewing a simple message popup
#[derive(Clone, Debug)]
pub struct MessageViewState {
    pub message_content: String, // The message content to display
    pub order_id: Option<uuid::Uuid>,
    pub action: Action,
    pub selected_button: bool, // true for YES, false for NO
}

/// Build a `MessageNotification` from an `OrderMessage` for use in popups.
pub fn order_message_to_notification(msg: &OrderMessage) -> MessageNotification {
    let inner_message_kind = msg.message.get_inner_message_kind();
    let action = inner_message_kind.action.clone();

    let action_str = match action {
        Action::NewOrder => "New Order created",
        Action::AddInvoice => "Invoice Request",
        Action::PayInvoice => "Payment Request",
        Action::FiatSent => "Fiat Sent",
        Action::FiatSentOk => "Fiat payment completed",
        Action::WaitingBuyerInvoice => "Waiting for Buyer to Add Invoice",
        Action::WaitingSellerToPay => "Waiting for Seller to Pay",
        Action::HoldInvoicePaymentAccepted => {
            "Hold Invoice Payment Accepted - Press Yes to confirm fiat payment"
        }
        Action::Rate => "Rate Counterparty",
        Action::RateReceived => "Rate Counterparty received",
        Action::Release | Action::Released => "Release",
        Action::Dispute | Action::DisputeInitiatedByYou => "Dispute",
        _ => "Message",
    };

    MessageNotification {
        order_id: msg.order_id,
        message_preview: action_str.to_string(),
        timestamp: msg.timestamp,
        action,
        sat_amount: msg.sat_amount,
        invoice: msg.buyer_invoice.clone(),
    }
}

/// Apply color coding to order kind cells (adapted for ratatui)
pub fn apply_kind_color(kind: &mostro_core::order::Kind) -> Style {
    match kind {
        mostro_core::order::Kind::Buy => Style::default().fg(Color::Green),
        mostro_core::order::Kind::Sell => Style::default().fg(Color::Red),
    }
}
