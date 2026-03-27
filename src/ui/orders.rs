use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use ratatui::style::{Color, Style};

const BUY_ORDER_FLOW_STEPS_MAKER: [&str; 6] = [
    "Wait for Seller",
    "Paste Invoice",
    "Chat with Seller",
    "Send Fiat",
    "Wait for Sats",
    "Rate Counterparty",
];

const BUY_ORDER_FLOW_STEPS_TAKER: [&str; 6] = [
    "Pay Hold Invoice",
    "Wait for Buyer Invoice",
    "Chat with Buyer",
    "Wait for Fiat",
    "Release Sats",
    "Rate Counterparty",
];

const BUY_ORDER_FLOW_STEPS_MAKER_SELL: [&str; 6] = [
    "Wait for Buyer Invoice",
    "Pay Hold Invoice",
    "Chat with Buyer",
    "Wait for Fiat",
    "Release Sats",
    "Rate Counterparty",
];

const BUY_ORDER_FLOW_STEPS_TAKER_SELL: [&str; 6] = [
    "Add Invoice",
    "Wait for Seller",
    "Chat with Buyer",
    "Send Fiat",
    "Wait for Sats",
    "Rate Counterparty",
];

const GENERIC_ORDER_FLOW_STEPS_TAKER: [&str; 6] =
    ["Payment / Wait", "Invoice", "Chat", "Fiat", "Sats", "Rate"];

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
pub enum OperationResult {
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
    /// Observer chat loaded successfully from relays.
    ObserverChatLoaded(Vec<crate::ui::chat::DisputeChatMessage>),
    /// Observer chat fetch failed.
    ObserverChatError(String),
}

/// Result of an async Mostro instance info fetch (sent from key handlers to main loop).
#[derive(Clone, Debug)]
pub enum MostroInfoFetchResult {
    Ok {
        info: Box<Option<crate::util::MostroInstanceInfo>>,
        message: String,
    },
    Err(String),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum FormField {
    #[default]
    OrderType,
    Currency,
    AmountSats,
    FiatAmount,
    FiatAmountMax,
    PaymentMethod,
    Premium,
    Invoice,
    ExpirationDays,
}

impl FormField {
    pub fn next(self, use_range: bool) -> Self {
        use FormField::*;
        match self {
            OrderType => Currency,
            Currency => AmountSats,
            AmountSats => FiatAmount,
            FiatAmount => {
                if use_range {
                    FiatAmountMax
                } else {
                    PaymentMethod
                }
            }
            FiatAmountMax => PaymentMethod,
            PaymentMethod => Premium,
            Premium => Invoice,
            Invoice => ExpirationDays,
            ExpirationDays => OrderType,
        }
    }

    pub fn prev(self, use_range: bool) -> Self {
        use FormField::*;
        match self {
            OrderType => ExpirationDays,
            Currency => OrderType,
            AmountSats => Currency,
            FiatAmount => AmountSats,
            FiatAmountMax => FiatAmount,
            PaymentMethod => {
                if use_range {
                    FiatAmountMax
                } else {
                    FiatAmount
                }
            }
            Premium => PaymentMethod,
            Invoice => Premium,
            ExpirationDays => Invoice,
        }
    }
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
    pub focused: FormField,      // which field is focused
    pub use_range: bool,         // whether to use fiat range
}

impl FormState {
    /// Create a default order form used when entering the Create New Order tab.
    pub fn new_default_form() -> Self {
        Self {
            kind: "buy".to_string(),
            fiat_code: "USD".to_string(),
            amount: "0".to_string(),
            premium: "0".to_string(),
            expiration_days: "1".to_string(),
            focused: FormField::Currency,
            ..Default::default()
        }
    }
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
    /// Book side (`buy` / `sell`) for this trade, carried across DMs that omit `Payload::Order`.
    pub order_kind: Option<mostro_core::order::Kind>,
    /// `true` = maker (created the order), `false` = taker, from local DB when known.
    pub is_mine: Option<bool>,
    /// Last known Mostro order status for this trade (payload or DB).
    pub order_status: Option<mostro_core::order::Status>,
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
    /// Vertical scroll offset for long invoice display (PayInvoice popup).
    pub scroll_y: u16,
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
        Action::TakeSell => "Take Sell",
        Action::TakeBuy => "Take Buy",
        Action::FiatSent => "Fiat Sent",
        Action::FiatSentOk => "Fiat payment completed",
        Action::WaitingBuyerInvoice => "Waiting for Buyer to Add Invoice",
        Action::WaitingSellerToPay => "Waiting for Seller to Pay",
        Action::HoldInvoicePaymentAccepted => {
            "Hold Invoice Payment Accepted - Press Yes to confirm fiat payment"
        }
        Action::Cancel => "Cancel",
        Action::Canceled => "Order canceled",
        Action::AdminCanceled => "Order canceled by admin",
        Action::Dispute | Action::DisputeInitiatedByYou => "Dispute",
        Action::Rate => "Rate Counterparty",
        Action::RateReceived => "Rate Counterparty received",
        Action::Release | Action::Released => "Release",
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

/// Short, UI-friendly action label for the messages sidebar.
pub fn message_action_compact_label(action: &Action) -> &'static str {
    match action {
        Action::AddInvoice => "Invoice Request",
        Action::PayInvoice => "Payment Request",
        Action::WaitingBuyerInvoice => "Waiting Buyer Invoice",
        Action::WaitingSellerToPay => "Waiting Seller Payment",
        Action::HoldInvoicePaymentAccepted => "Hold Invoice Accepted",
        Action::FiatSent => "Fiat Sent",
        Action::FiatSentOk => "Fiat Confirmed",
        Action::Release | Action::Released => "Release",
        Action::Dispute | Action::DisputeInitiatedByYou => "Dispute",
        Action::Canceled => "Canceled",
        Action::AdminCanceled => "Admin Canceled",
        Action::Rate => "Rate Counterparty",
        _ => "Message",
    }
}

/// Book order kind for sidebar: prefer persisted [`OrderMessage::order_kind`], then payload,
/// then take-action hints when the listing side is implied (`take-sell` → sell listing, etc.).
pub fn message_order_kind_label(msg: &OrderMessage) -> &'static str {
    if let Some(k) = msg.order_kind {
        return match k {
            mostro_core::order::Kind::Buy => "BUY",
            mostro_core::order::Kind::Sell => "SELL",
        };
    }
    let inner = msg.message.get_inner_message_kind();
    if let Some(Payload::Order(order)) = &inner.payload {
        return match order.kind {
            Some(mostro_core::order::Kind::Buy) => "BUY",
            Some(mostro_core::order::Kind::Sell) => "SELL",
            None => "N/A",
        };
    }
    match inner.action {
        Action::TakeSell => "SELL",
        Action::TakeBuy => "BUY",
        _ => "N/A",
    }
}

/// One-based step in the buy-order trade timeline (matches the Messages tab stepper).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum BuyFlowStep {
    WaitForSeller = 1,
    PasteInvoice = 2,
    ChatWithSeller = 3,
    SendFiat = 4,
    ReceiveSats = 5,
    RateCounterparty = 6,
}

impl BuyFlowStep {
    /// Step index for UI (`1`..=`6`), aligned with stepper labels.
    #[must_use]
    pub const fn step_number(self) -> usize {
        self as u8 as usize
    }
}

/// Buy-listing timeline step: prefers [`OrderMessage::order_status`] + maker/taker for `Kind::Buy`.
pub fn buy_listing_flow_step(msg: &OrderMessage) -> BuyFlowStep {
    let action = msg.message.get_inner_message_kind().action.clone();
    if msg.order_kind != Some(mostro_core::order::Kind::Buy) {
        return message_buy_flow_step_fallback(&action);
    }
    let Some(is_maker) = msg.is_mine else {
        return message_buy_flow_step_fallback(&action);
    };
    if let Some(status) = msg.order_status {
        if let Some(step) = buy_listing_step_from_status(status) {
            return step;
        }
    }
    buy_listing_flow_step_from_action(&action, is_maker)
}

fn buy_listing_step_from_status(status: Status) -> Option<BuyFlowStep> {
    match status {
        Status::Pending | Status::WaitingPayment => Some(BuyFlowStep::WaitForSeller),
        Status::WaitingBuyerInvoice | Status::SettledHoldInvoice => Some(BuyFlowStep::PasteInvoice),
        Status::InProgress | Status::Active => Some(BuyFlowStep::ChatWithSeller),
        Status::FiatSent => Some(BuyFlowStep::SendFiat),
        Status::Success => Some(BuyFlowStep::ReceiveSats),
        Status::Canceled
        | Status::CanceledByAdmin
        | Status::CooperativelyCanceled
        | Status::Expired
        | Status::Dispute
        | Status::SettledByAdmin
        | Status::CompletedByAdmin => None,
    }
}

fn buy_listing_flow_step_from_action(action: &Action, is_maker: bool) -> BuyFlowStep {
    if is_maker {
        match action {
            Action::WaitingSellerToPay => BuyFlowStep::WaitForSeller,
            Action::AddInvoice | Action::WaitingBuyerInvoice => BuyFlowStep::PasteInvoice,
            Action::PayInvoice => BuyFlowStep::PasteInvoice,
            Action::HoldInvoicePaymentAccepted => BuyFlowStep::ChatWithSeller,
            Action::FiatSent => BuyFlowStep::SendFiat,
            Action::FiatSentOk | Action::Release | Action::Released => BuyFlowStep::ReceiveSats,
            Action::Rate => BuyFlowStep::RateCounterparty,
            Action::TakeBuy | Action::TakeSell => BuyFlowStep::ChatWithSeller,
            _ => BuyFlowStep::ChatWithSeller,
        }
    } else {
        match action {
            Action::PayInvoice | Action::WaitingSellerToPay => BuyFlowStep::WaitForSeller,
            Action::HoldInvoicePaymentAccepted => BuyFlowStep::PasteInvoice,
            Action::WaitingBuyerInvoice | Action::AddInvoice => BuyFlowStep::PasteInvoice,
            Action::FiatSent => BuyFlowStep::SendFiat,
            Action::FiatSentOk | Action::Release | Action::Released => BuyFlowStep::ReceiveSats,
            Action::Rate => BuyFlowStep::RateCounterparty,
            Action::TakeBuy | Action::TakeSell => BuyFlowStep::ChatWithSeller,
            _ => BuyFlowStep::ChatWithSeller,
        }
    }
}

/// Action-only fallback for non-buy listings or unknown role/status.
pub fn message_buy_flow_step_fallback(action: &Action) -> BuyFlowStep {
    match action {
        Action::AddInvoice | Action::WaitingBuyerInvoice => BuyFlowStep::WaitForSeller,
        Action::PayInvoice | Action::WaitingSellerToPay | Action::HoldInvoicePaymentAccepted => {
            BuyFlowStep::PasteInvoice
        }
        Action::TakeBuy | Action::TakeSell => BuyFlowStep::ChatWithSeller,
        Action::FiatSent => BuyFlowStep::SendFiat,
        Action::FiatSentOk | Action::Release | Action::Released => BuyFlowStep::ReceiveSats,
        Action::Rate => BuyFlowStep::RateCounterparty,
        _ => BuyFlowStep::ChatWithSeller,
    }
}

/// Labels for the six timeline steps; buy listings use role-specific wording for early steps.
pub fn buy_listing_timeline_labels(msg: &OrderMessage) -> [&'static str; 6] {
    match msg.order_kind {
        Some(mostro_core::order::Kind::Buy) => {
            match msg.is_mine {
                Some(true) => BUY_ORDER_FLOW_STEPS_MAKER,
                Some(false) => BUY_ORDER_FLOW_STEPS_TAKER,
                None => GENERIC_ORDER_FLOW_STEPS_TAKER,
            }
        },
        Some(mostro_core::order::Kind::Sell) => {
             match msg.is_mine {
                Some(true) => BUY_ORDER_FLOW_STEPS_TAKER,
                Some(false) => BUY_ORDER_FLOW_STEPS_MAKER,
                None => GENERIC_ORDER_FLOW_STEPS_TAKER,
            }
        }
        None => GENERIC_ORDER_FLOW_STEPS_TAKER,

    // } else {
    //     [
    //         "Trade setup",
    //         "Invoice / Pay",
    //         "Chat",
    //         "Fiat",
    //         "Sats",
    //         "Rate",
    //     ]
    }
}

/// Warning text for non-happy path trade actions.
pub fn message_timeline_warning(action: &Action) -> Option<&'static str> {
    match action {
        Action::Canceled => Some("Trade canceled"),
        Action::AdminCanceled => Some("Trade canceled by admin"),
        Action::Dispute | Action::DisputeInitiatedByYou => Some("Trade in dispute state"),
        _ => None,
    }
}

/// Apply color coding to order kind cells (adapted for ratatui)
pub fn apply_kind_color(kind: &mostro_core::order::Kind) -> Style {
    match kind {
        mostro_core::order::Kind::Buy => Style::default().fg(Color::Green),
        mostro_core::order::Kind::Sell => Style::default().fg(Color::Red),
    }
}
