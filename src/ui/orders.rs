use std::str::FromStr;

use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use ratatui::style::{Color, Style};

use crate::ui::constants::{
    BUY_ORDER_FLOW_STEPS_MAKER, BUY_ORDER_FLOW_STEPS_TAKER, GENERIC_ORDER_FLOW_STEPS_TAKER,
    SELL_ORDER_FLOW_STEPS_MAKER, SELL_ORDER_FLOW_STEPS_TAKER,
    VIEW_MESSAGE_BUYER_TOOK_ORDER_PREVIEW, VIEW_MESSAGE_HOLD_INVOICE_PREVIEW,
};

pub use crate::ui::constants::StepLabel;

/// Stable My Trades header fields for one trade (maker publish or taker take). Not updated by later DMs.
#[derive(Clone, Debug)]
pub struct OrderChatStaticHeader {
    pub order_id: uuid::Uuid,
    pub kind: Option<mostro_core::order::Kind>,
    pub created_at: Option<i64>,
    pub trade_index: i64,
    /// Local party's trade Nostr pubkey string (for display; matches DM path convention).
    pub initiator_trade_pubkey: String,
    /// `true` = we are maker, `false` = taker.
    pub is_mine: bool,
}

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
    /// Filled on successful create/take for My Trades static header.
    pub static_header: Option<OrderChatStaticHeader>,
}

/// `action` for a post-success placeholder [`OrderMessage`] so My Trades has a row before DMs land.
/// Never returns synthetic book-side `take-buy` / `take-sell` (those break Messages-tab Enter).
fn placeholder_action_for_order_success(os: &OrderSuccess) -> Option<Action> {
    let header = os.static_header.as_ref()?;
    if header.is_mine {
        return Some(Action::NewOrder);
    }
    match os.status {
        Some(Status::WaitingTakerBond) => Some(Action::PayBondInvoice),
        Some(Status::WaitingPayment) => Some(Action::WaitingSellerToPay),
        Some(Status::WaitingBuyerInvoice) | Some(Status::SettledHoldInvoice) => {
            Some(Action::WaitingBuyerInvoice)
        }
        Some(Status::FiatSent) => Some(Action::FiatSent),
        _ => Some(Action::BuyerTookOrder),
    }
}

fn small_order_from_order_success(os: &OrderSuccess) -> SmallOrder {
    SmallOrder {
        id: os.order_id,
        kind: os.kind,
        status: os.status,
        amount: os.amount,
        fiat_code: os.fiat_code.clone(),
        min_amount: os.min_amount,
        max_amount: os.max_amount,
        fiat_amount: os.fiat_amount,
        payment_method: os.payment_method.clone(),
        premium: os.premium,
        buyer_invoice: None,
        created_at: os.static_header.as_ref().and_then(|h| h.created_at),
        expires_at: None,
        ..Default::default()
    }
}

/// One synthetic [`OrderMessage`] when `Success` arrives before any DM row exists (My Trades sidebar).
pub(crate) fn try_placeholder_order_message_from_success(
    os: &OrderSuccess,
) -> Option<OrderMessage> {
    let header = os.static_header.as_ref()?;
    let order_id = os.order_id?;
    let trade_index = os.trade_index.unwrap_or(header.trade_index);
    let action = placeholder_action_for_order_success(os)?;
    let sender = PublicKey::from_str(header.initiator_trade_pubkey.as_str()).ok()?;
    let small = small_order_from_order_success(os);
    let message = Message::new_order(
        Some(order_id),
        None,
        Some(trade_index),
        action,
        Some(Payload::Order(small)),
    );
    Some(OrderMessage {
        message,
        timestamp: chrono::Utc::now().timestamp(),
        sender,
        order_id: Some(order_id),
        trade_index,
        sat_amount: None,
        buyer_invoice: None,
        order_kind: os.kind,
        is_mine: Some(header.is_mine),
        order_status: os.status,
        read: true,
        auto_popup_shown: true,
    })
}

/// Per-order buyer invoice preference when we act as taker on a SELL listing.
/// Stored in-memory only (not persisted to DB); later flows can use it to
/// decide how to pre-fill or submit buyer invoices for that specific order.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BuyerInvoicePreference {
    /// Prefer using the saved Settings `ln_address` (Lightning address) as
    /// the buyer invoice source when appropriate.
    UseSavedLnAddress,
    /// Always prompt for a manual BOLT11 or Lightning address for this order.
    ManualInvoice,
}

#[derive(Clone, Debug)]
pub enum OperationResult {
    Success(OrderSuccess),
    /// Payment request required - shows invoice popup for buy orders.
    /// `action` discriminates between the trade hold invoice (`Action::PayInvoice`)
    /// and the anti-abuse bond invoice (`Action::PayBondInvoice`); both arrive
    /// with the same `Payload::PaymentRequest` shape and only differ by action.
    PaymentRequestRequired {
        order: mostro_core::prelude::SmallOrder,
        invoice: String,
        sat_amount: Option<i64>,
        trade_index: i64,
        static_header: OrderChatStaticHeader,
        action: mostro_core::prelude::Action,
    },
    /// Generic informational popup (e.g. AddInvoice confirmation)
    Info(String),
    /// AddInvoice DM succeeded; optionally persist [`BuyerInvoicePreference::UseSavedLnAddress`]
    /// for this order after send (main loop normalizes to [`Self::Info`] for display).
    InvoiceSubmitted {
        message: String,
        remember_buyer_saved_ln_address_for_order: Option<uuid::Uuid>,
    },
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
    /// Local-only history cleanup result; remove these order rows from in-memory My Trades cache.
    OrderHistoryDeleted {
        deleted_order_ids: Vec<uuid::Uuid>,
        message: String,
    },
    /// Rebuild [`crate::ui::AppState::my_trades_maker_book`] from SQLite (no UI popup).
    MyTradesMakerBookChanged,
    /// Open invoice / waiting popup from a synchronous execute reply (e.g. bond payout DM).
    OpenInvoicePopup {
        notification: MessageNotification,
        order_message: Box<OrderMessage>,
    },
    /// User order chat attachment sent successfully (append local row + show info).
    OrderChatAttachmentSent {
        order_id: String,
        chat_message: crate::ui::UserOrderChatMessage,
        info_message: String,
    },
    /// Blossom upload succeeded but order-chat DM failed; prepared payload kept for retry.
    OrderChatAttachmentSendFailed {
        prepared: crate::ui::helpers::PreparedOrderChatAttachment,
        error: String,
    },
}

/// Result of async Lightning address LNURL verification and save (settings flow; not order/dispute).
#[derive(Clone, Debug)]
pub enum LnAddressVerifyResult {
    Verified { message: String },
    Err(String),
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
    /// `true` = maker (created the order), `false` = taker.
    ///
    /// `None` = role not hydrated yet (e.g. first take-order DM before `save_order(false)`).
    /// Populated from SQLite in the trade-DM listener after upsert (see `util::dm_utils`).
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
    /// Long explanatory text for waiting-phase popups.
    pub body: Option<String>,
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
        // The anti-abuse bond is outstanding precisely while the order sits in
        // `WaitingTakerBond`. `None` covers fresh DMs that arrive before the local
        // row hydrates. A daemon with bonds disabled never emits this action, so
        // this arm stays dead in that configuration.
        Action::PayBondInvoice => matches!(
            order_status,
            Some(mostro_core::order::Status::WaitingTakerBond) | None
        ),
        Action::AddInvoice => matches!(
            order_status,
            Some(
                mostro_core::order::Status::WaitingBuyerInvoice
                    | mostro_core::order::Status::SettledHoldInvoice
            ) | None
        ),
        Action::AddBondInvoice => !matches!(
            order_status,
            Some(
                mostro_core::order::Status::Canceled | mostro_core::order::Status::CanceledByAdmin
            )
        ),
        _ => false,
    }
}

/// Whether the local user must act on an invoice/payment popup (`AddInvoice`, `PayInvoice`, `PayBondInvoice`).
///
/// Buy/sell listing kind swaps which side is maker vs taker for each action. When [`OrderMessage::is_mine`]
/// is still `None`, we assume **taker** (same as the Messages timeline): safe for the common pre-`save_order`
/// take-order race; once the DM listener hydrates role from SQLite, `Some(true/false)` drives the decision.
#[must_use]
pub fn local_user_must_act_on_invoice_popup(msg: &OrderMessage, popup_action: &Action) -> bool {
    let is_maker = msg.is_mine.unwrap_or(false);
    match (msg.order_kind, popup_action) {
        (Some(mostro_core::order::Kind::Buy), Action::AddInvoice) => is_maker,
        (Some(mostro_core::order::Kind::Buy), Action::PayInvoice | Action::PayBondInvoice) => {
            !is_maker
        }
        (Some(mostro_core::order::Kind::Sell), Action::AddInvoice) => !is_maker,
        (Some(mostro_core::order::Kind::Sell), Action::PayInvoice | Action::PayBondInvoice) => {
            is_maker
        }
        (_, Action::AddBondInvoice) => true,
        (None, Action::PayInvoice | Action::PayBondInvoice) => msg
            .buyer_invoice
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        (None, Action::AddInvoice) => false,
        _ => false,
    }
}

/// Explanatory text when the local user is waiting on the counterparty (no invoice action required).
#[must_use]
pub fn waiting_phase_description(msg: &OrderMessage) -> &'static str {
    let is_maker = msg.is_mine.unwrap_or(false);
    let action = msg.message.get_inner_message_kind().action.clone();
    match (msg.order_kind, is_maker, action) {
        (
            Some(mostro_core::order::Kind::Buy),
            true,
            Action::WaitingSellerToPay | Action::PayInvoice,
        ) => "Your order was taken. Waiting for the seller to pay the hold invoice. You will be prompted to add your Lightning invoice when it is your turn.",
        (
            Some(mostro_core::order::Kind::Sell),
            false,
            Action::WaitingSellerToPay | Action::PayInvoice,
        ) => "Waiting for the seller to pay the hold invoice.",
        (
            Some(mostro_core::order::Kind::Buy),
            false,
            Action::WaitingBuyerInvoice | Action::AddInvoice,
        ) => "Waiting for the buyer to add their Lightning invoice.",
        (
            Some(mostro_core::order::Kind::Sell),
            true,
            Action::WaitingBuyerInvoice | Action::AddInvoice,
        ) => "Waiting for the buyer to add their Lightning invoice.",
        (_, true, Action::PayBondInvoice) => {
            "Waiting for the taker to pay the anti-abuse bond."
        }
        _ => "Waiting for the counterparty. No action is required from you right now.",
    }
}

/// Short phase title for waiting-phase popups.
#[must_use]
pub fn waiting_phase_short_label(msg: &OrderMessage) -> &'static str {
    let is_maker = msg.is_mine.unwrap_or(false);
    let action = msg.message.get_inner_message_kind().action.clone();
    match (msg.order_kind, is_maker, action) {
        (_, true, Action::PayBondInvoice) => "Waiting for Taker Bond",
        (
            Some(mostro_core::order::Kind::Buy),
            true,
            Action::WaitingSellerToPay | Action::PayInvoice,
        )
        | (
            Some(mostro_core::order::Kind::Sell),
            false,
            Action::WaitingSellerToPay | Action::PayInvoice,
        ) => "Waiting for Seller to Pay",
        (
            Some(mostro_core::order::Kind::Buy),
            false,
            Action::WaitingBuyerInvoice | Action::AddInvoice,
        )
        | (
            Some(mostro_core::order::Kind::Sell),
            true,
            Action::WaitingBuyerInvoice | Action::AddInvoice,
        ) => "Waiting for Buyer to Add Invoice",
        _ => "Trade status",
    }
}

/// UI action for a waiting-phase popup (preserves phase semantics for rendering).
#[must_use]
pub fn waiting_popup_action_for_message(msg: &OrderMessage) -> Action {
    match msg.message.get_inner_message_kind().action {
        Action::WaitingBuyerInvoice | Action::AddInvoice => Action::WaitingBuyerInvoice,
        _ => Action::WaitingSellerToPay,
    }
}

/// Build a waiting-phase notification from an order message row.
#[must_use]
pub fn order_message_to_waiting_notification(msg: &OrderMessage) -> MessageNotification {
    let mut notification = order_message_to_notification(msg);
    notification.message_preview = waiting_phase_short_label(msg).to_string();
    notification.body = Some(waiting_phase_description(msg).to_string());
    notification.action = waiting_popup_action_for_message(msg);
    notification
}

/// State for handling invoice input in AddInvoice notifications
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InvoiceNotificationActionSelection {
    Primary,
    Cancel,
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
    /// Selected action in AddInvoice/PayInvoice popup.
    pub action_selection: InvoiceNotificationActionSelection,
}

/// State for handling key input (pubkey or privkey) in admin settings
#[derive(Clone, Debug)]
pub struct KeyInputState {
    pub key_input: String,
    pub focused: bool,
    pub just_pasted: bool, // Flag to ignore Enter immediately after paste
}

/// YES/NO selection for `ViewingMessage` popups (FiatSentOk, My Trades actions, etc.).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ThreeState {
    Yes,
    No,
    Cancel,
}

impl ThreeState {
    pub const fn next(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Cancel,
            Self::Cancel => Self::Yes,
        }
    }

    pub const fn prev(self) -> Self {
        match self {
            Self::Yes => Self::Cancel,
            Self::No => Self::Yes,
            Self::Cancel => Self::No,
        }
    }

    pub const fn index(self) -> u8 {
        match self {
            Self::Yes => 0,
            Self::No => 1,
            Self::Cancel => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewingMessageButtonSelection {
    /// `true` = YES highlighted, `false` = NO.
    Two { yes_selected: bool },
    /// Hold-invoice confirmation only: YES / NO / CANCEL.
    Three(ThreeState),
}

impl ViewingMessageButtonSelection {
    pub fn cycle_three_prev(&mut self) {
        if let Self::Three(selected) = self {
            *selected = selected.prev();
        }
    }

    pub fn cycle_three_next(&mut self) {
        if let Self::Three(selected) = self {
            *selected = selected.next();
        }
    }
}

/// State for viewing a simple message popup
#[derive(Clone, Debug)]
pub struct MessageViewState {
    pub message_content: String, // The message content to display
    pub order_id: Option<uuid::Uuid>,
    pub action: Action,
    pub button_selection: ViewingMessageButtonSelection,
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
        Action::AddBondInvoice => "Bond Payout Invoice",
        Action::PayInvoice => "Payment Request",
        Action::PayBondInvoice => "Bond Invoice",
        Action::TakeSell => "Take Sell",
        Action::TakeBuy => "Take Buy",
        Action::FiatSent => "Fiat Sent",
        Action::FiatSentOk => "Fiat payment completed",
        Action::WaitingBuyerInvoice => "Waiting for Buyer to Add Invoice",
        Action::WaitingSellerToPay => "Waiting for Seller to Pay",
        Action::HoldInvoicePaymentAccepted => VIEW_MESSAGE_HOLD_INVOICE_PREVIEW,
        Action::BuyerTookOrder => VIEW_MESSAGE_BUYER_TOOK_ORDER_PREVIEW,
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

    let body = if matches!(action, Action::AddBondInvoice) {
        match inner_message_kind.payload.as_ref() {
            Some(Payload::BondPayoutRequest(req)) => {
                Some(bond_payout_notification_body(req.slashed_at))
            }
            _ => None,
        }
    } else {
        None
    };

    MessageNotification {
        order_id: msg.order_id,
        message_preview: action_str.to_string(),
        timestamp: msg.timestamp,
        action,
        sat_amount: msg.sat_amount,
        invoice: msg.buyer_invoice.clone(),
        body,
    }
}

fn bond_payout_notification_body(slashed_at: i64) -> String {
    let anchor = chrono::DateTime::from_timestamp(slashed_at, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!("Slash recorded: {anchor}. Claim deadline = anchor + instance payout window.")
}

/// Short, UI-friendly action label for the messages sidebar.
pub fn message_action_compact_label(action: &Action) -> &'static str {
    match action {
        Action::AddInvoice => "Invoice Request",
        Action::AddBondInvoice => "Bond Payout Invoice",
        Action::PayInvoice => "Payment Request",
        Action::PayBondInvoice => "Bond Invoice",
        Action::WaitingBuyerInvoice => "Waiting Buyer Invoice",
        Action::WaitingSellerToPay => "Waiting Seller Payment",
        Action::HoldInvoicePaymentAccepted => "Hold Invoice Accepted",
        Action::BuyerTookOrder => "Buyer Took Order",
        Action::FiatSent => "Fiat Sent",
        Action::FiatSentOk => "Fiat Confirmed",
        Action::Release | Action::Released => "Release sats",
        Action::Dispute | Action::DisputeInitiatedByYou => "Dispute",
        Action::Canceled => "Canceled",
        Action::AdminCanceled => "Admin Canceled",
        Action::Rate => "Rate Counterparty",
        Action::RateReceived => "Rating Received",
        Action::CooperativeCancelInitiatedByPeer => "Cooperative Cancel Initiated by Peer",
        Action::CooperativeCancelInitiatedByYou => "Cooperative Cancel Initiated by You",
        Action::NewOrder => "New Order Created",
        _ => "Unknown Message",
    }
}

/// Status-aware compact label for Messages sidebar/detail.
/// Keeps terminal statuses from showing stale action text after reboot replay.
pub fn message_action_compact_label_for_message(msg: &OrderMessage) -> &'static str {
    match msg.order_status {
        Some(Status::Pending) => "Pending order",
        Some(Status::Success) => "Trade Completed",
        Some(Status::SettledByAdmin) => "Settled by admin",
        Some(Status::CompletedByAdmin) => "Completed by admin",
        Some(Status::Canceled) => "Canceled",
        Some(Status::CanceledByAdmin) => "Admin Canceled",
        Some(Status::CooperativelyCanceled) => "Cooperatively Canceled",
        Some(Status::WaitingBuyerInvoice) => "Waiting Buyer Invoice",
        Some(Status::WaitingPayment) => "Waiting Seller Payment",
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
    StepPendingOrder = 0,
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
    StepPendingOrder = 0,
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
    if matches!(&action, Action::FiatSentOk) {
        return FlowStep::BuyFlowStep(StepLabelsBuy::StepReleaseSats);
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
    if matches!(&action, Action::FiatSentOk) {
        return FlowStep::SellFlowStep(StepLabelsSell::StepReleaseSats);
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
            // Initial stat of order - no green steps visulized, orders is still pending.
            Status::Pending | Status::WaitingTakerBond => {
                Some(FlowStep::BuyFlowStep(StepLabelsBuy::StepPendingOrder))
            }
            // `WaitingTakerBond` (Mostro Phase 1.5+): order matched but trade flow has not
            // started; treat like `Pending` for the timeline.
            Status::WaitingPayment => Some(FlowStep::BuyFlowStep(StepLabelsBuy::StepSellerPayment)),
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
            | Status::CompletedByAdmin => {
                Some(FlowStep::BuyFlowStep(StepLabelsBuy::StepPendingOrder))
            }
        },
        mostro_core::order::Kind::Sell => match status {
            // Initial stat of order - no green steps visulized, orders is still pending.
            Status::Pending | Status::WaitingTakerBond => {
                Some(FlowStep::SellFlowStep(StepLabelsSell::StepPendingOrder))
            }
            // `WaitingTakerBond` (Mostro Phase 1.5+): order matched but trade flow has not
            // started; treat like `Pending` for the timeline.
            Status::WaitingPayment => {
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
            | Status::CompletedByAdmin => {
                Some(FlowStep::SellFlowStep(StepLabelsSell::StepPendingOrder))
            }
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
mod order_success_placeholder_tests {
    use super::*;
    use nostr_sdk::Keys;

    fn sample_order_success(
        is_mine: bool,
        status: Option<Status>,
        kind: Option<mostro_core::order::Kind>,
    ) -> OrderSuccess {
        let keys = Keys::generate();
        let order_id = uuid::Uuid::new_v4();
        OrderSuccess {
            order_id: Some(order_id),
            kind,
            amount: 100,
            fiat_code: "EUR".to_string(),
            fiat_amount: 50,
            min_amount: None,
            max_amount: None,
            payment_method: "SEPA".to_string(),
            premium: 0,
            status,
            trade_index: Some(1),
            static_header: Some(OrderChatStaticHeader {
                order_id,
                kind,
                created_at: Some(1),
                trade_index: 1,
                initiator_trade_pubkey: keys.public_key().to_string(),
                is_mine,
            }),
        }
    }

    #[test]
    fn maker_placeholder_uses_new_order_action() {
        let os = sample_order_success(
            true,
            Some(Status::Pending),
            Some(mostro_core::order::Kind::Sell),
        );
        let msg = try_placeholder_order_message_from_success(&os).expect("msg");
        assert_eq!(
            msg.message.get_inner_message_kind().action,
            Action::NewOrder
        );
        assert_eq!(msg.order_status, Some(Status::Pending));
    }

    #[test]
    fn taker_waiting_buyer_invoice_uses_waiting_buyer_invoice_action() {
        let os = sample_order_success(
            false,
            Some(Status::WaitingBuyerInvoice),
            Some(mostro_core::order::Kind::Sell),
        );
        let msg = try_placeholder_order_message_from_success(&os).expect("msg");
        assert_eq!(
            msg.message.get_inner_message_kind().action,
            Action::WaitingBuyerInvoice
        );
    }

    #[test]
    fn skips_without_static_header() {
        let mut os = sample_order_success(
            true,
            Some(Status::Pending),
            Some(mostro_core::order::Kind::Sell),
        );
        os.static_header = None;
        assert!(try_placeholder_order_message_from_success(&os).is_none());
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

    #[test]
    fn buy_taker_pay_bond_invoice_waiting_taker_bond_maps_to_pending_order_step() {
        let m = sample_order_message(
            Action::PayBondInvoice,
            Some(mostro_core::order::Kind::Buy),
            Some(false),
            Some(mostro_core::order::Status::WaitingTakerBond),
        );
        assert_eq!(
            message_trade_timeline_step(&m),
            FlowStep::BuyFlowStep(StepLabelsBuy::StepPendingOrder)
        );
    }

    #[test]
    fn sell_taker_pay_bond_invoice_waiting_taker_bond_maps_to_pending_order_step() {
        let m = sample_order_message(
            Action::PayBondInvoice,
            Some(mostro_core::order::Kind::Sell),
            Some(false),
            Some(mostro_core::order::Status::WaitingTakerBond),
        );
        assert_eq!(
            message_trade_timeline_step(&m),
            FlowStep::SellFlowStep(StepLabelsSell::StepPendingOrder)
        );
    }
}

#[cfg(test)]
mod invoice_popup_role_tests {
    use super::*;

    fn sample_order_message(
        action: Action,
        order_kind: Option<mostro_core::order::Kind>,
        is_mine: Option<bool>,
    ) -> OrderMessage {
        let keys = nostr_sdk::Keys::generate();
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
            order_status: None,
            read: false,
            auto_popup_shown: false,
        }
    }

    #[test]
    fn buy_maker_waiting_seller_pay_does_not_act_on_pay_invoice() {
        let m = sample_order_message(
            Action::WaitingSellerToPay,
            Some(mostro_core::order::Kind::Buy),
            Some(true),
        );
        assert!(!local_user_must_act_on_invoice_popup(
            &m,
            &Action::PayInvoice
        ));
    }

    #[test]
    fn buy_taker_pays_hold_invoice() {
        let m = sample_order_message(
            Action::PayInvoice,
            Some(mostro_core::order::Kind::Buy),
            Some(false),
        );
        assert!(local_user_must_act_on_invoice_popup(
            &m,
            &Action::PayInvoice
        ));
    }

    #[test]
    fn sell_maker_pays_hold_invoice() {
        let m = sample_order_message(
            Action::PayInvoice,
            Some(mostro_core::order::Kind::Sell),
            Some(true),
        );
        assert!(local_user_must_act_on_invoice_popup(
            &m,
            &Action::PayInvoice
        ));
    }

    #[test]
    fn sell_taker_waiting_seller_pay_does_not_act_on_pay_invoice() {
        let m = sample_order_message(
            Action::WaitingSellerToPay,
            Some(mostro_core::order::Kind::Sell),
            Some(false),
        );
        assert!(!local_user_must_act_on_invoice_popup(
            &m,
            &Action::PayInvoice
        ));
    }

    #[test]
    fn buy_maker_adds_invoice() {
        let m = sample_order_message(
            Action::AddInvoice,
            Some(mostro_core::order::Kind::Buy),
            Some(true),
        );
        assert!(local_user_must_act_on_invoice_popup(
            &m,
            &Action::AddInvoice
        ));
    }

    #[test]
    fn sell_taker_adds_invoice() {
        let m = sample_order_message(
            Action::AddInvoice,
            Some(mostro_core::order::Kind::Sell),
            Some(false),
        );
        assert!(local_user_must_act_on_invoice_popup(
            &m,
            &Action::AddInvoice
        ));
    }

    #[test]
    fn buy_maker_does_not_pay_bond() {
        let m = sample_order_message(
            Action::PayBondInvoice,
            Some(mostro_core::order::Kind::Buy),
            Some(true),
        );
        assert!(!local_user_must_act_on_invoice_popup(
            &m,
            &Action::PayBondInvoice
        ));
    }

    /// When `is_mine` is still `None` (pre-`save_order` hydration), timeline defaults to taker;
    /// buy taker must still see PayInvoice as actionable.
    #[test]
    fn buy_taker_unknown_role_still_acts_on_pay_invoice() {
        let m = sample_order_message(
            Action::PayInvoice,
            Some(mostro_core::order::Kind::Buy),
            None,
        );
        assert!(local_user_must_act_on_invoice_popup(
            &m,
            &Action::PayInvoice
        ));
    }

    /// Unknown role must not be treated as maker for buy AddInvoice (would block maker popup).
    #[test]
    fn buy_maker_unknown_role_does_not_act_on_add_invoice_via_taker_default() {
        let m = sample_order_message(
            Action::AddInvoice,
            Some(mostro_core::order::Kind::Buy),
            None,
        );
        assert!(!local_user_must_act_on_invoice_popup(
            &m,
            &Action::AddInvoice
        ));
    }
}
