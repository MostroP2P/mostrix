use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use ratatui::style::{Color, Style};

use crate::ui::constants::{
    BUY_ORDER_FLOW_STEPS_MAKER, BUY_ORDER_FLOW_STEPS_TAKER, GENERIC_ORDER_FLOW_STEPS_TAKER,
    SELL_ORDER_FLOW_STEPS_MAKER, SELL_ORDER_FLOW_STEPS_TAKER,
};

pub use crate::ui::constants::StepLabel;

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
    /// Trade ended (e.g. cooperative cancel confirmed); remove order from Messages and show `message`.
    TradeClosed {
        order_id: uuid::Uuid,
        message: String,
    },
}

/// Result of an async Mostro instance info fetch (sent from key handlers to main loop).
#[derive(Clone, Debug)]
pub enum MostroInfoFetchResult {
    Ok {
        info: Box<Option<crate::util::MostroInstanceInfo>>,
        message: String,
    },
    /// Startup / background refresh: update `mostro_info` only; do not change mode or show toasts.
    Applied {
        info: Box<Option<crate::util::MostroInstanceInfo>>,
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

/// Drop `new-order` rows and fix selection. Those DMs are not shown in the Messages tab
/// (order creation ack is handled via `send_new_order` / waiting UI).
pub fn strip_new_order_messages_and_clamp_selected(
    messages: &mut Vec<OrderMessage>,
    selected_message_idx: &mut usize,
) {
    messages.retain(|m| !matches!(m.message.get_inner_message_kind().action, Action::NewOrder));
    if *selected_message_idx >= messages.len() {
        *selected_message_idx = messages.len().saturating_sub(1);
    }
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

/// Whether an invoice modal is appropriate for the current trade phase.
/// `PayInvoice` is only for `waiting-payment`; after the seller pays, status moves to
/// `waiting-buyer-invoice` and a stale replayed `pay-invoice` DM must not reopen the payment popup.
#[must_use]
pub fn invoice_popup_allowed_for_order_status(
    action: &Action,
    order_status: Option<mostro_core::order::Status>,
) -> bool {
    match action {
        // Strict: only `waiting-payment` still requires paying the hold invoice.
        Action::PayInvoice => matches!(
            order_status,
            Some(mostro_core::order::Status::WaitingPayment)
        ),
        Action::AddInvoice => matches!(
            order_status,
            Some(
                mostro_core::order::Status::WaitingBuyerInvoice
                    | mostro_core::order::Status::SettledHoldInvoice
            ) | None
        ),
        _ => false,
    }
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

/// Rate counterparty after Mostro prompts with `action: rate` (daemon resolves peer by order id).
#[derive(Clone, Debug)]
pub struct RatingOrderState {
    pub order_id: uuid::Uuid,
    /// 1..=5 (`mostro_core::MIN_RATING`..=`MAX_RATING`).
    pub selected_rating: u8,
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
            "Hold invoice payment accepted — confirm fiat was sent?"
        }
        Action::Cancel => "Cancel",
        Action::CooperativeCancelInitiatedByPeer => "Peer requested cooperative cancel",
        Action::Canceled => "Order canceled",
        Action::AdminCanceled => "Order canceled by admin",
        Action::Dispute | Action::DisputeInitiatedByYou => "Dispute",
        Action::Rate => "Rate Counterparty",
        Action::RateReceived | Action::PurchaseCompleted => "Rate Counterparty completed",
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
        Action::RateReceived => "Rating Received",
        _ => "Message",
    }
}

/// Status-aware compact label for Messages sidebar/detail.
/// Keeps terminal statuses from showing stale action text after reboot replay.
pub fn message_action_compact_label_for_message(msg: &OrderMessage) -> &'static str {
    match msg.order_status {
        Some(Status::Success | Status::SettledByAdmin | Status::CompletedByAdmin) => {
            "Trade Completed"
        }
        Some(Status::Canceled) => "Canceled",
        Some(Status::CanceledByAdmin) => "Admin Canceled",
        Some(Status::CooperativelyCanceled) => "Cooperatively Canceled",
        Some(Status::Expired) => "Expired",
        Some(_) | None => {
            message_action_compact_label(&msg.message.get_inner_message_kind().action)
        }
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
        if let Some(kind) = order.kind {
            return match kind {
                mostro_core::order::Kind::Buy => "BUY",
                mostro_core::order::Kind::Sell => "SELL",
            };
        }
    }
    match inner.action {
        Action::TakeSell => "SELL",
        Action::TakeBuy => "BUY",
        _ => "N/A",
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
#[allow(clippy::enum_variant_names)] // Step* names match UI column semantics
pub enum StepLabelsBuy {
    StepSellerPayment = 1,
    StepBuyerInvoice = 2,
    StepChatActiveOrder = 3,
    StepSendFiat = 4,
    StepReleaseSats = 5,
    StepRate = 6,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
#[allow(clippy::enum_variant_names)] // Step* names match UI column semantics
pub enum StepLabelsSell {
    StepBuyerInvoice = 1,
    StepSellerPayment = 2,
    StepChatActiveOrder = 3,
    StepSendFiat = 4,
    StepReleaseSats = 5,
    StepRate = 6,
}
/// Highlighted column (`1`..=`6`) for the Messages tab stepper; buy vs sell use different label order.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
#[allow(clippy::enum_variant_names)] // Step* names match UI "Step N" labels
pub enum FlowStep {
    BuyFlowStep(StepLabelsBuy),
    SellFlowStep(StepLabelsSell),
}

impl FlowStep {
    /// Step index for UI (`1`..=`6`), aligned with stepper labels.
    #[must_use]
    pub const fn step_number(self) -> usize {
        match self {
            FlowStep::BuyFlowStep(step) => step as usize,
            FlowStep::SellFlowStep(step) => step as usize,
        }
    }
}

/// Resolves the highlighted timeline step for the Messages tab (buy and sell listings).
#[must_use]
pub fn message_trade_timeline_step(msg: &OrderMessage) -> FlowStep {
    let action = msg.message.get_inner_message_kind().action.clone();
    match msg.order_kind {
        Some(mostro_core::order::Kind::Buy) => buy_listing_flow_step(msg),
        Some(mostro_core::order::Kind::Sell) => sell_listing_flow_step(msg),
        None => message_buy_flow_step_fallback(&action),
    }
}

/// Buy-listing timeline step: prefers [`OrderMessage::order_status`] + maker/taker for `Kind::Buy`.
pub fn buy_listing_flow_step(msg: &OrderMessage) -> FlowStep {
    let action = msg.message.get_inner_message_kind().action.clone();
    if msg.order_kind != Some(mostro_core::order::Kind::Buy) {
        return message_buy_flow_step_fallback(&action);
    }
    // Takers often see `is_mine: None` until the local row hydrates; default to taker so the
    // highlighted column matches [`listing_timeline_labels`] (taker wording per kind).
    let is_maker = msg.is_mine.unwrap_or(false);
    // Post-`success` phases are action-specific (`rate` vs release); handle before status.
    if matches!(
        &action,
        Action::Rate | Action::RateReceived | Action::PurchaseCompleted
    ) {
        return FlowStep::BuyFlowStep(StepLabelsBuy::StepRate);
    }
    if let Some(status) = msg.order_status {
        if let Some(step) = listing_step_from_status(mostro_core::order::Kind::Buy, status) {
            return step;
        }
    }
    buy_listing_flow_step_from_action(&action, is_maker)
}

/// Sell-listing timeline step: same pipeline as buy; action fallback matches [`SELL_ORDER_FLOW_STEPS_*`].
pub fn sell_listing_flow_step(msg: &OrderMessage) -> FlowStep {
    let action = msg.message.get_inner_message_kind().action.clone();
    if msg.order_kind != Some(mostro_core::order::Kind::Sell) {
        return message_buy_flow_step_fallback(&action);
    }
    let is_maker = msg.is_mine.unwrap_or(false);
    if matches!(&action, Action::Rate | Action::RateReceived) {
        return FlowStep::SellFlowStep(StepLabelsSell::StepRate);
    }
    if let Some(status) = msg.order_status {
        if let Some(step) = listing_step_from_status(mostro_core::order::Kind::Sell, status) {
            return step;
        }
    }
    sell_listing_flow_step_from_action(&action, is_maker)
}

/// Shared Mostro `Status` machine; column indices differ for buy vs sell (see [`StepLabelsBuy`] vs [`StepLabelsSell`]).
fn listing_step_from_status(kind: mostro_core::order::Kind, status: Status) -> Option<FlowStep> {
    match kind {
        mostro_core::order::Kind::Buy => match status {
            Status::Pending | Status::WaitingPayment => {
                Some(FlowStep::BuyFlowStep(StepLabelsBuy::StepSellerPayment))
            }
            Status::WaitingBuyerInvoice | Status::SettledHoldInvoice => {
                Some(FlowStep::BuyFlowStep(StepLabelsBuy::StepBuyerInvoice))
            }
            Status::InProgress | Status::Active => {
                Some(FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder))
            }
            Status::FiatSent => Some(FlowStep::BuyFlowStep(StepLabelsBuy::StepSendFiat)),
            // On completed trades, keep the timeline in the final column even if the latest
            // replayed action is an older pre-success DM (relay ordering / reboot hydration).
            Status::Success => Some(FlowStep::BuyFlowStep(StepLabelsBuy::StepRate)),
            Status::Canceled
            | Status::CanceledByAdmin
            | Status::CooperativelyCanceled
            | Status::Expired
            | Status::Dispute
            | Status::SettledByAdmin
            | Status::CompletedByAdmin => None,
        },
        mostro_core::order::Kind::Sell => match status {
            Status::Pending | Status::WaitingPayment => {
                Some(FlowStep::SellFlowStep(StepLabelsSell::StepSellerPayment))
            }
            Status::WaitingBuyerInvoice | Status::SettledHoldInvoice => {
                Some(FlowStep::SellFlowStep(StepLabelsSell::StepBuyerInvoice))
            }
            Status::InProgress | Status::Active => {
                Some(FlowStep::SellFlowStep(StepLabelsSell::StepChatActiveOrder))
            }
            Status::FiatSent => Some(FlowStep::SellFlowStep(StepLabelsSell::StepSendFiat)),
            // On completed trades, keep the timeline in the final column even if the latest
            // replayed action is an older pre-success DM (relay ordering / reboot hydration).
            Status::Success => Some(FlowStep::SellFlowStep(StepLabelsSell::StepRate)),
            Status::Canceled
            | Status::CanceledByAdmin
            | Status::CooperativelyCanceled
            | Status::Expired
            | Status::Dispute
            | Status::SettledByAdmin
            | Status::CompletedByAdmin => None,
        },
    }
}

/// Sell listing: maker = seller (created the sell order), taker = buyer.
fn sell_listing_flow_step_from_action(action: &Action, is_maker: bool) -> FlowStep {
    if is_maker {
        match action {
            Action::WaitingSellerToPay | Action::PayInvoice => {
                FlowStep::SellFlowStep(StepLabelsSell::StepSellerPayment)
            }
            Action::AddInvoice | Action::WaitingBuyerInvoice => {
                FlowStep::SellFlowStep(StepLabelsSell::StepBuyerInvoice)
            }
            Action::HoldInvoicePaymentAccepted => {
                FlowStep::SellFlowStep(StepLabelsSell::StepChatActiveOrder)
            }
            Action::FiatSent => FlowStep::SellFlowStep(StepLabelsSell::StepSendFiat),
            Action::FiatSentOk | Action::Release | Action::Released => {
                FlowStep::SellFlowStep(StepLabelsSell::StepReleaseSats)
            }
            Action::Rate | Action::RateReceived => FlowStep::SellFlowStep(StepLabelsSell::StepRate),
            Action::TakeBuy | Action::TakeSell => {
                FlowStep::SellFlowStep(StepLabelsSell::StepChatActiveOrder)
            }
            _ => FlowStep::SellFlowStep(StepLabelsSell::StepChatActiveOrder),
        }
    } else {
        match action {
            Action::AddInvoice | Action::WaitingBuyerInvoice => {
                FlowStep::SellFlowStep(StepLabelsSell::StepBuyerInvoice)
            }
            Action::PayInvoice | Action::WaitingSellerToPay => {
                FlowStep::SellFlowStep(StepLabelsSell::StepSellerPayment)
            }
            Action::HoldInvoicePaymentAccepted => {
                FlowStep::SellFlowStep(StepLabelsSell::StepChatActiveOrder)
            }
            Action::FiatSent => FlowStep::SellFlowStep(StepLabelsSell::StepSendFiat),
            Action::FiatSentOk | Action::Release | Action::Released => {
                FlowStep::SellFlowStep(StepLabelsSell::StepReleaseSats)
            }
            Action::Rate | Action::RateReceived => FlowStep::SellFlowStep(StepLabelsSell::StepRate),
            Action::TakeBuy | Action::TakeSell => {
                FlowStep::SellFlowStep(StepLabelsSell::StepChatActiveOrder)
            }
            _ => FlowStep::SellFlowStep(StepLabelsSell::StepChatActiveOrder),
        }
    }
}

fn buy_listing_flow_step_from_action(action: &Action, is_maker: bool) -> FlowStep {
    if is_maker {
        match action {
            Action::WaitingSellerToPay => FlowStep::BuyFlowStep(StepLabelsBuy::StepSellerPayment),
            Action::AddInvoice | Action::WaitingBuyerInvoice => {
                FlowStep::BuyFlowStep(StepLabelsBuy::StepBuyerInvoice)
            }
            Action::PayInvoice => FlowStep::BuyFlowStep(StepLabelsBuy::StepSellerPayment),
            Action::HoldInvoicePaymentAccepted => {
                FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder)
            }
            Action::FiatSent => FlowStep::BuyFlowStep(StepLabelsBuy::StepSendFiat),
            Action::FiatSentOk | Action::Release | Action::Released => {
                FlowStep::BuyFlowStep(StepLabelsBuy::StepReleaseSats)
            }
            Action::Rate | Action::RateReceived => FlowStep::BuyFlowStep(StepLabelsBuy::StepRate),
            Action::TakeBuy | Action::TakeSell => {
                FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder)
            }
            _ => FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder),
        }
    } else {
        match action {
            Action::PayInvoice | Action::WaitingSellerToPay => {
                FlowStep::BuyFlowStep(StepLabelsBuy::StepSellerPayment)
            }
            Action::HoldInvoicePaymentAccepted => {
                FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder)
            }
            Action::WaitingBuyerInvoice | Action::AddInvoice => {
                FlowStep::BuyFlowStep(StepLabelsBuy::StepBuyerInvoice)
            }
            Action::FiatSent => FlowStep::BuyFlowStep(StepLabelsBuy::StepSendFiat),
            Action::FiatSentOk | Action::Release | Action::Released => {
                FlowStep::BuyFlowStep(StepLabelsBuy::StepReleaseSats)
            }
            Action::Rate | Action::RateReceived => FlowStep::BuyFlowStep(StepLabelsBuy::StepRate),
            Action::TakeBuy | Action::TakeSell => {
                FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder)
            }
            _ => FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder),
        }
    }
}

/// Action-only fallback for non-buy listings or unknown role/status.
pub fn message_buy_flow_step_fallback(action: &Action) -> FlowStep {
    match action {
        Action::AddInvoice | Action::WaitingBuyerInvoice => {
            FlowStep::BuyFlowStep(StepLabelsBuy::StepBuyerInvoice)
        }
        Action::PayInvoice | Action::WaitingSellerToPay => {
            FlowStep::BuyFlowStep(StepLabelsBuy::StepSellerPayment)
        }
        Action::HoldInvoicePaymentAccepted => {
            FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder)
        }
        Action::TakeBuy | Action::TakeSell => {
            FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder)
        }
        Action::FiatSent => FlowStep::BuyFlowStep(StepLabelsBuy::StepSendFiat),
        Action::FiatSentOk | Action::Release | Action::Released => {
            FlowStep::BuyFlowStep(StepLabelsBuy::StepReleaseSats)
        }
        Action::Rate | Action::RateReceived => FlowStep::BuyFlowStep(StepLabelsBuy::StepRate),
        _ => FlowStep::BuyFlowStep(StepLabelsBuy::StepChatActiveOrder),
    }
}

/// Labels for the six timeline steps (wording); column order matches [`StepLabelsBuy`] / [`StepLabelsSell`].
pub fn listing_timeline_labels(msg: &OrderMessage) -> [StepLabel; 6] {
    match msg.order_kind {
        Some(mostro_core::order::Kind::Buy) => match msg.is_mine {
            Some(true) => BUY_ORDER_FLOW_STEPS_MAKER,
            // Unknown role: use buy-listing taker copy (matches default in [`buy_listing_flow_step`]).
            Some(false) | None => BUY_ORDER_FLOW_STEPS_TAKER,
        },
        Some(mostro_core::order::Kind::Sell) => match msg.is_mine {
            Some(true) => SELL_ORDER_FLOW_STEPS_MAKER,
            Some(false) | None => SELL_ORDER_FLOW_STEPS_TAKER,
        },
        None => GENERIC_ORDER_FLOW_STEPS_TAKER,
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

/// Warning text from persisted [`OrderMessage::order_status`] (preferred over last DM [`Action`] when they disagree).
pub fn message_timeline_warning_for_order_status(
    status: Option<mostro_core::order::Status>,
) -> Option<&'static str> {
    let s = status?;
    match s {
        Status::Canceled => Some("Trade canceled"),
        Status::CanceledByAdmin => Some("Trade canceled by admin"),
        Status::CooperativelyCanceled => Some("Trade cooperatively canceled"),
        Status::Expired => Some("Trade expired"),
        Status::Dispute => Some("Trade in dispute state"),
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

#[cfg(test)]
mod timeline_step_tests {
    use super::*;
    use nostr_sdk::Keys;

    fn sample_order_message(
        action: Action,
        order_kind: Option<mostro_core::order::Kind>,
        is_mine: Option<bool>,
        order_status: Option<mostro_core::order::Status>,
    ) -> OrderMessage {
        let keys = Keys::generate();
        OrderMessage {
            message: Message::new_order(None, None, None, action, None),
            timestamp: 0,
            sender: keys.public_key(),
            order_id: None,
            trade_index: 0,
            sat_amount: None,
            buyer_invoice: None,
            order_kind,
            is_mine,
            order_status,
            read: false,
            auto_popup_shown: false,
        }
    }

    #[test]
    fn buy_maker_add_invoice_maps_to_invoice_step() {
        let m = sample_order_message(
            Action::AddInvoice,
            Some(mostro_core::order::Kind::Buy),
            Some(true),
            None,
        );
        assert_eq!(
            message_trade_timeline_step(&m),
            FlowStep::BuyFlowStep(StepLabelsBuy::StepBuyerInvoice)
        );
    }

    #[test]
    fn sell_maker_pay_invoice_maps_to_seller_payment_step() {
        let m = sample_order_message(
            Action::PayInvoice,
            Some(mostro_core::order::Kind::Sell),
            Some(true),
            None,
        );
        assert_eq!(
            message_trade_timeline_step(&m),
            FlowStep::SellFlowStep(StepLabelsSell::StepSellerPayment)
        );
    }

    #[test]
    fn sell_taker_add_invoice_maps_to_first_column() {
        let m = sample_order_message(
            Action::AddInvoice,
            Some(mostro_core::order::Kind::Sell),
            Some(false),
            None,
        );
        assert_eq!(
            message_trade_timeline_step(&m),
            FlowStep::SellFlowStep(StepLabelsSell::StepBuyerInvoice)
        );
    }

    #[test]
    fn sell_taker_unknown_role_uses_taker_columns_like_is_mine_false() {
        let m = sample_order_message(
            Action::AddInvoice,
            Some(mostro_core::order::Kind::Sell),
            None,
            None,
        );
        assert_eq!(
            message_trade_timeline_step(&m),
            FlowStep::SellFlowStep(StepLabelsSell::StepBuyerInvoice)
        );
        let labels = listing_timeline_labels(&m);
        assert_eq!(labels[0].as_single_line(), "Add Invoice");
    }

    #[test]
    fn sell_success_and_rate_maps_to_rate_step() {
        let m = sample_order_message(
            Action::Rate,
            Some(mostro_core::order::Kind::Sell),
            Some(true),
            Some(mostro_core::order::Status::Success),
        );
        assert_eq!(
            message_trade_timeline_step(&m),
            FlowStep::SellFlowStep(StepLabelsSell::StepRate)
        );
    }

    #[test]
    fn sell_waiting_payment_uses_status_before_action() {
        let m = sample_order_message(
            Action::FiatSent,
            Some(mostro_core::order::Kind::Sell),
            Some(false),
            Some(mostro_core::order::Status::WaitingPayment),
        );
        assert_eq!(
            message_trade_timeline_step(&m),
            FlowStep::SellFlowStep(StepLabelsSell::StepSellerPayment)
        );
    }
}
