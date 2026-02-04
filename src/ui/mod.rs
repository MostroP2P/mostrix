use std::collections::HashMap;
use std::fmt::{self, Display};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Line;

pub const PRIMARY_COLOR: Color = Color::Rgb(177, 204, 51); // #b1cc33
pub const BACKGROUND_COLOR: Color = Color::Rgb(29, 33, 44); // #1D212C

pub mod admin_key_confirm;
pub mod dispute_finalization_confirm;
pub mod dispute_finalization_popup;
pub mod disputes_in_progress_tab;
pub mod disputes_tab;
pub mod exit_confirm;
pub mod helpers;
pub mod key_handler;
pub mod key_input_popup;
pub mod message_notification;
pub mod order_confirm;
pub mod order_form;
pub mod order_result;
pub mod order_take;
pub mod orders_tab;
pub mod settings_tab;
pub mod status;
pub mod tab_content;
pub mod tabs;
pub mod waiting;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserTab {
    Orders,
    MyTrades,
    Messages,
    Settings,
    CreateNewOrder,
    Exit,
}

impl Display for UserTab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                UserTab::Orders => "Orders",
                UserTab::MyTrades => "My Trades",
                UserTab::Messages => "Messages",
                UserTab::Settings => "Settings",
                UserTab::CreateNewOrder => "Create New Order",
                UserTab::Exit => "Exit",
            }
        )
    }
}

impl UserTab {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => UserTab::Orders,
            1 => UserTab::MyTrades,
            2 => UserTab::Messages,
            3 => UserTab::CreateNewOrder,
            4 => UserTab::Settings,
            5 => UserTab::Exit,
            _ => panic!("Invalid user tab index: {}", index),
        }
    }

    pub fn as_index(self) -> usize {
        match self {
            UserTab::Orders => 0,
            UserTab::MyTrades => 1,
            UserTab::Messages => 2,
            UserTab::CreateNewOrder => 3,
            UserTab::Settings => 4,
            UserTab::Exit => 5,
        }
    }

    pub fn count() -> usize {
        6
    }

    pub fn first() -> Self {
        UserTab::Orders
    }

    pub fn last() -> Self {
        UserTab::Exit
    }

    pub fn prev(self) -> Self {
        match self {
            UserTab::Orders => UserTab::Orders,
            UserTab::MyTrades => UserTab::Orders,
            UserTab::Messages => UserTab::MyTrades,
            UserTab::CreateNewOrder => UserTab::Messages,
            UserTab::Settings => UserTab::CreateNewOrder,
            UserTab::Exit => UserTab::Settings,
        }
    }

    pub fn next(self) -> Self {
        match self {
            UserTab::Orders => UserTab::MyTrades,
            UserTab::MyTrades => UserTab::Messages,
            UserTab::Messages => UserTab::CreateNewOrder,
            UserTab::CreateNewOrder => UserTab::Settings,
            UserTab::Settings => UserTab::Exit,
            UserTab::Exit => UserTab::Exit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminTab {
    DisputesPending,
    DisputesInProgress,
    Settings,
    Exit,
}

impl Display for AdminTab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                AdminTab::DisputesPending => "Disputes Pending",
                AdminTab::DisputesInProgress => "Disputes Management",
                AdminTab::Settings => "Settings",
                AdminTab::Exit => "Exit",
            }
        )
    }
}

impl AdminTab {
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => AdminTab::DisputesPending,
            1 => AdminTab::DisputesInProgress,
            2 => AdminTab::Settings,
            3 => AdminTab::Exit,
            _ => panic!("Invalid admin tab index: {}", index),
        }
    }

    pub fn as_index(self) -> usize {
        match self {
            AdminTab::DisputesPending => 0,
            AdminTab::DisputesInProgress => 1,
            AdminTab::Settings => 2,
            AdminTab::Exit => 3,
        }
    }

    pub fn count() -> usize {
        4
    }

    pub fn first() -> Self {
        AdminTab::DisputesPending
    }

    pub fn last() -> Self {
        AdminTab::Exit
    }

    pub fn prev(self) -> Self {
        match self {
            AdminTab::DisputesPending => AdminTab::DisputesPending,
            AdminTab::DisputesInProgress => AdminTab::DisputesPending,
            AdminTab::Settings => AdminTab::DisputesInProgress,
            AdminTab::Exit => AdminTab::Settings,
        }
    }

    pub fn next(self) -> Self {
        match self {
            AdminTab::DisputesPending => AdminTab::DisputesInProgress,
            AdminTab::DisputesInProgress => AdminTab::Settings,
            AdminTab::Settings => AdminTab::Exit,
            AdminTab::Exit => AdminTab::Exit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    User(UserTab),
    Admin(AdminTab),
}

impl Display for Tab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tab::User(tab) => write!(f, "{}", tab),
            Tab::Admin(tab) => write!(f, "{}", tab),
        }
    }
}

impl Tab {
    pub fn from_index(index: usize, role: UserRole) -> Self {
        match role {
            UserRole::User => Tab::User(UserTab::from_index(index)),
            UserRole::Admin => Tab::Admin(AdminTab::from_index(index)),
        }
    }

    pub fn as_index(self) -> usize {
        match self {
            Tab::User(tab) => tab.as_index(),
            Tab::Admin(tab) => tab.as_index(),
        }
    }

    pub fn to_line<'a>(self) -> Line<'a> {
        Line::from(self.to_string())
    }

    pub fn count(role: UserRole) -> usize {
        match role {
            UserRole::User => UserTab::count(),
            UserRole::Admin => AdminTab::count(),
        }
    }

    pub fn first(role: UserRole) -> Self {
        match role {
            UserRole::User => Tab::User(UserTab::first()),
            UserRole::Admin => Tab::Admin(AdminTab::first()),
        }
    }

    pub fn last(role: UserRole) -> Self {
        match role {
            UserRole::User => Tab::User(UserTab::last()),
            UserRole::Admin => Tab::Admin(AdminTab::last()),
        }
    }

    pub fn prev(self, role: UserRole) -> Self {
        match (self, role) {
            (Tab::User(tab), UserRole::User) => Tab::User(tab.prev()),
            (Tab::Admin(tab), UserRole::Admin) => Tab::Admin(tab.prev()),
            _ => self, // Invalid combination, return self
        }
    }

    pub fn next(self, role: UserRole) -> Self {
        match (self, role) {
            (Tab::User(tab), UserRole::User) => Tab::User(tab.next()),
            (Tab::Admin(tab), UserRole::Admin) => Tab::Admin(tab.next()),
            _ => self, // Invalid combination, return self
        }
    }

    pub fn get_titles(role: UserRole) -> Vec<String> {
        match role {
            UserRole::User => (0..UserTab::count())
                .map(|i| UserTab::from_index(i).to_string())
                .collect(),
            UserRole::Admin => (0..AdminTab::count())
                .map(|i| AdminTab::from_index(i).to_string())
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserRole {
    User,
    Admin,
}

impl Display for UserRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                UserRole::User => "user",
                UserRole::Admin => "admin",
            }
        )
    }
}

impl FromStr for UserRole {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "user" => Ok(UserRole::User),
            "admin" => Ok(UserRole::Admin),
            _ => Err(anyhow::anyhow!("Invalid user role: {s}")),
        }
    }
}

#[derive(Clone, Debug)]
pub enum UserMode {
    Normal,
    CreatingOrder(FormState),
    ConfirmingOrder(FormState),       // Confirmation popup
    TakingOrder(TakeOrderState),      // Taking an order from the list
    WaitingForMostro(FormState),      // Waiting for Mostro response (order creation)
    WaitingTakeOrder(TakeOrderState), // Waiting for Mostro response (taking order)
    WaitingAddInvoice,                // Waiting for Mostro response (adding invoice)
}

#[derive(Clone, Debug)]
pub enum AdminMode {
    Normal,
    AddSolver(KeyInputState),
    ConfirmAddSolver(String, bool), // (solver_pubkey, selected_button: true=Yes, false=No)
    SetupAdminKey(KeyInputState),
    ConfirmAdminKey(String, bool), // (key_string, selected_button: true=Yes, false=No)
    ConfirmTakeDispute(uuid::Uuid, bool), // (dispute_id, selected_button: true=Yes, false=No)
    WaitingTakeDispute(uuid::Uuid), // (dispute_id)
    ManagingDispute,               // Mode for "Disputes in Progress" tab
    ReviewingDisputeForFinalization(uuid::Uuid, usize), // (dispute_id, selected_button: 0=Pay Buyer, 1=Refund Seller, 2=Exit)
    ConfirmFinalizeDispute(uuid::Uuid, bool, bool), // (dispute_id, is_settle: true=Pay Buyer, false=Refund Seller, selected_button: true=Yes, false=No)
    WaitingDisputeFinalization(uuid::Uuid),         // (dispute_id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatParty {
    Buyer,
    Seller,
}

/// Filter for viewing disputes in the Disputes in Progress tab
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisputeFilter {
    InProgress, // Show only InProgress disputes
    Finalized,  // Show only finalized disputes (Settled, SellerRefunded, Released)
}

impl Display for ChatParty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatParty::Buyer => write!(f, "Buyer"),
            ChatParty::Seller => write!(f, "Seller"),
        }
    }
}

/// Represents the sender of a chat message in dispute resolution
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatSender {
    Admin,
    Buyer,
    Seller,
}

/// A chat message in the dispute resolution interface
#[derive(Clone, Debug)]
pub struct DisputeChatMessage {
    pub sender: ChatSender,
    pub content: String,
    pub timestamp: i64,                  // Unix timestamp
    pub target_party: Option<ChatParty>, // For Admin messages: which party this was sent to
}

/// Per-(dispute, party) last-seen timestamp for admin chat.
/// Used to filter incoming buyer/seller messages so we only process new ones.
#[derive(Clone, Debug)]
pub struct AdminChatLastSeen {
    /// Last seen timestamp (inner/canonical unix seconds) for messages from this party.
    pub last_seen_timestamp: Option<u64>,
}

/// Result of polling for admin chat messages for a single dispute/party.
#[derive(Clone, Debug)]
pub struct AdminChatUpdate {
    pub dispute_id: String,
    pub party: ChatParty,
    /// (content, timestamp, sender_pubkey)
    pub messages: Vec<(String, u64, PublicKey)>,
}

#[derive(Clone, Debug)]
pub enum UiMode {
    // Shared modes (available to both user and admin)
    Normal,
    ViewingMessage(MessageViewState), // Simple message popup with yes/no options
    NewMessageNotification(MessageNotification, Action, InvoiceInputState), // Popup for new message with invoice input state
    OrderResult(OrderResult), // Show order result (success or error)
    AddMostroPubkey(KeyInputState),
    ConfirmMostroPubkey(String, bool), // (key_string, selected_button: true=Yes, false=No)
    AddRelay(KeyInputState),
    ConfirmRelay(String, bool), // (relay_string, selected_button: true=Yes, false=No)
    AddCurrency(KeyInputState),
    ConfirmCurrency(String, bool), // (currency_string, selected_button: true=Yes, false=No)
    ConfirmClearCurrencies(bool),  // (selected_button: true=Yes, false=No)
    ConfirmExit(bool),             // (selected_button: true=Yes, false=No)

    // User-specific modes
    UserMode(UserMode),

    // Admin-specific modes
    AdminMode(AdminMode),
}

#[derive(Clone, Debug)]
pub enum OrderResult {
    Success {
        order_id: Option<uuid::Uuid>,
        kind: Option<mostro_core::order::Kind>,
        amount: i64,
        fiat_code: String,
        fiat_amount: i64,
        min_amount: Option<i64>,
        max_amount: Option<i64>,
        payment_method: String,
        premium: i64,
        status: Option<Status>,
        trade_index: Option<i64>, // Trade index used for this order
    },
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
    pub timestamp: u64,
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
    pub timestamp: u64,
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

pub struct AppState {
    pub user_role: UserRole,
    pub active_tab: Tab,
    pub selected_order_idx: usize,
    pub selected_dispute_idx: usize, // Selected dispute in Disputes Pending tab
    pub selected_in_progress_idx: usize, // Selected dispute in Disputes in Progress tab
    pub active_chat_party: ChatParty, // Which party the admin is currently chatting with
    pub admin_chat_input: String,    // Current message being typed by admin
    pub admin_chat_input_enabled: bool, // Whether chat input is enabled (toggle with Shift+I)
    pub admin_dispute_chats: HashMap<String, Vec<DisputeChatMessage>>, // Chat messages per dispute ID
    pub admin_chat_list_state: ratatui::widgets::ListState, // ListState for chat scrolling
    /// Tracks (dispute_id, party, visible_count) for auto-scroll when new messages arrive
    pub admin_chat_scroll_tracker: Option<(String, ChatParty, usize)>,
    /// Cached last-seen timestamps per (dispute_id, party) for admin chat.
    pub admin_chat_last_seen: HashMap<(String, ChatParty), AdminChatLastSeen>,
    pub selected_settings_option: usize, // Selected option in Settings tab (admin mode)
    pub mode: UiMode,
    pub messages: Arc<Mutex<Vec<OrderMessage>>>, // Messages related to orders
    pub active_order_trade_indices: Arc<Mutex<HashMap<uuid::Uuid, i64>>>, // Map order_id -> trade_index
    pub selected_message_idx: usize, // Selected message in Messages tab
    pub pending_notifications: Arc<Mutex<usize>>, // Count of pending notifications (non-critical)
    pub admin_disputes_in_progress: Vec<crate::models::AdminDispute>, // Taken disputes
    pub dispute_filter: DisputeFilter, // Filter for viewing InProgress or Finalized disputes
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

impl AppState {
    pub fn new(user_role: UserRole) -> Self {
        let initial_tab = Tab::first(user_role);
        Self {
            user_role,
            active_tab: initial_tab,
            selected_order_idx: 0,
            selected_dispute_idx: 0,
            selected_in_progress_idx: 0,
            active_chat_party: ChatParty::Buyer,
            admin_chat_input: String::new(),
            admin_chat_input_enabled: true, // Chat input enabled by default
            admin_dispute_chats: HashMap::new(),
            admin_chat_list_state: ratatui::widgets::ListState::default(),
            admin_chat_scroll_tracker: None,
            admin_chat_last_seen: HashMap::new(),
            selected_settings_option: 0,
            mode: UiMode::Normal,
            messages: Arc::new(Mutex::new(Vec::new())),
            active_order_trade_indices: Arc::new(Mutex::new(HashMap::new())),
            selected_message_idx: 0,
            pending_notifications: Arc::new(Mutex::new(0)),
            admin_disputes_in_progress: Vec::new(),
            dispute_filter: DisputeFilter::InProgress, // Default to InProgress view
        }
    }

    pub fn switch_role(&mut self, new_role: UserRole) {
        self.user_role = new_role;
        self.active_tab = Tab::first(new_role);
        self.mode = UiMode::Normal;
        self.selected_dispute_idx = 0;
        self.selected_settings_option = 0;
        self.selected_in_progress_idx = 0;
        self.active_chat_party = ChatParty::Buyer;
        self.admin_chat_input.clear();
    }
}

/// Apply color coding to order kind cells (adapted for ratatui)
pub(crate) fn apply_kind_color(kind: &mostro_core::order::Kind) -> Style {
    match kind {
        mostro_core::order::Kind::Buy => Style::default().fg(Color::Green),
        mostro_core::order::Kind::Sell => Style::default().fg(Color::Red),
    }
}

pub fn ui_draw(
    f: &mut ratatui::Frame,
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<mostro_core::prelude::Dispute>>>,
    status_line: Option<&[String]>,
) {
    // Create layout: one row for tabs, content area, and status bar (3 lines for status)
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3), // Status bar with 3 lines
        ],
    )
    .split(f.area());

    // Render tabs
    tabs::render_tabs(f, chunks[0], app.active_tab, app.user_role);

    // Render content based on active tab and role
    let content_area = chunks[1];
    match (&app.active_tab, app.user_role) {
        (Tab::User(UserTab::Orders), UserRole::User) => {
            orders_tab::render_orders_tab(f, content_area, orders, app.selected_order_idx)
        }
        (Tab::User(UserTab::MyTrades), UserRole::User) => {
            tab_content::render_coming_soon(f, content_area, "My Trades")
        }
        (Tab::User(UserTab::Messages), UserRole::User) => {
            let messages = app.messages.lock().unwrap();
            tab_content::render_messages_tab(f, content_area, &messages, app.selected_message_idx)
        }
        (Tab::User(UserTab::Settings), UserRole::User) => settings_tab::render_settings_tab(
            f,
            content_area,
            app.user_role,
            app.selected_settings_option,
        ),
        (Tab::User(UserTab::CreateNewOrder), UserRole::User) => {
            if let UiMode::UserMode(UserMode::CreatingOrder(form)) = &app.mode {
                order_form::render_order_form(f, content_area, form);
            } else {
                order_form::render_form_initializing(f, content_area);
            }
        }
        (Tab::Admin(AdminTab::DisputesPending), UserRole::Admin) => {
            disputes_tab::render_disputes_tab(f, content_area, disputes, app.selected_dispute_idx)
        }
        (Tab::Admin(AdminTab::DisputesInProgress), UserRole::Admin) => {
            disputes_in_progress_tab::render_disputes_in_progress(f, content_area, app)
        }
        (Tab::Admin(AdminTab::Settings), UserRole::Admin) => settings_tab::render_settings_tab(
            f,
            content_area,
            app.user_role,
            app.selected_settings_option,
        ),
        (Tab::User(UserTab::Exit), UserRole::User)
        | (Tab::Admin(AdminTab::Exit), UserRole::Admin) => {
            tab_content::render_exit_tab(f, content_area)
        }
        _ => {
            // Fallback for invalid combinations
            tab_content::render_coming_soon(f, content_area, "Unknown")
        }
    }

    // Bottom status bar
    if let Some(lines) = status_line {
        let pending_count = *app.pending_notifications.lock().unwrap();
        status::render_status_bar(f, chunks[2], lines, pending_count);
    }

    // Confirmation popup overlay (user mode only)
    if let UiMode::UserMode(UserMode::ConfirmingOrder(form)) = &app.mode {
        order_confirm::render_order_confirm(f, form);
    }

    // Waiting for Mostro popup overlay (user mode only)
    if let UiMode::UserMode(UserMode::WaitingForMostro(_)) = &app.mode {
        waiting::render_waiting(f);
    }

    // Waiting for take order popup overlay (user mode only)
    if let UiMode::UserMode(UserMode::WaitingTakeOrder(_)) = &app.mode {
        waiting::render_waiting(f);
    }

    // Waiting for AddInvoice popup overlay (user mode only)
    if let UiMode::UserMode(UserMode::WaitingAddInvoice) = &app.mode {
        waiting::render_waiting(f);
    }

    // Waiting for take dispute popup overlay (admin mode only)
    if let UiMode::AdminMode(AdminMode::WaitingTakeDispute(_)) = &app.mode {
        waiting::render_waiting(f);
    }

    // Order result popup overlay (shared)
    if let UiMode::OrderResult(result) = &app.mode {
        order_result::render_order_result(f, result);
    }

    // Shared settings popups
    if let UiMode::AddMostroPubkey(key_state) = &app.mode {
        key_input_popup::render_key_input_popup(
            f,
            "🌐 Add Mostro Pubkey",
            "Enter Mostro public key (npub...):",
            "npub...",
            key_state,
            false,
        );
    }
    if let UiMode::ConfirmMostroPubkey(key_string, selected_button) = &app.mode {
        admin_key_confirm::render_admin_key_confirm(
            f,
            "🌐 Confirm Mostro Pubkey",
            key_string,
            *selected_button,
        );
    }
    if let UiMode::AddRelay(key_state) = &app.mode {
        key_input_popup::render_key_input_popup(
            f,
            "📡 Add Relay",
            "Enter relay URL (wss:// or ws://...):",
            "wss://...",
            key_state,
            false,
        );
    }
    if let UiMode::ConfirmRelay(relay_string, selected_button) = &app.mode {
        admin_key_confirm::render_admin_key_confirm(
            f,
            "📡 Confirm Relay",
            relay_string,
            *selected_button,
        );
    }
    if let UiMode::AddCurrency(key_state) = &app.mode {
        key_input_popup::render_key_input_popup(
            f,
            "💱 Add Currency Filter",
            "Enter currency code (e.g., USD, EUR):",
            "USD",
            key_state,
            false,
        );
    }
    if let UiMode::ConfirmCurrency(currency_string, selected_button) = &app.mode {
        admin_key_confirm::render_admin_key_confirm_with_message(
            f,
            "💱 Confirm Currency Filter",
            currency_string,
            *selected_button,
            Some("Do you want to add this currency filter?"),
        );
    }
    if let UiMode::ConfirmClearCurrencies(selected_button) = &app.mode {
        admin_key_confirm::render_admin_key_confirm_with_message(
            f,
            "💱 Clear Currency Filters",
            "",
            *selected_button,
            Some("Are you sure you want to clear all currencies filters?"),
        );
    }

    // Admin key input popup overlay
    if let UiMode::AdminMode(AdminMode::AddSolver(key_state)) = &app.mode {
        key_input_popup::render_key_input_popup(
            f,
            "Add Solver",
            "Enter solver public key (npub...):",
            "npub...",
            key_state,
            false,
        );
    }
    if let UiMode::AdminMode(AdminMode::SetupAdminKey(key_state)) = &app.mode {
        key_input_popup::render_key_input_popup(
            f,
            "🔐 Setup Admin Key",
            "Enter admin private key (nsec...):",
            "nsec...",
            key_state,
            true,
        );
    }

    // Admin confirmation popups
    if let UiMode::AdminMode(AdminMode::ConfirmTakeDispute(dispute_id, selected_button)) = &app.mode
    {
        admin_key_confirm::render_admin_key_confirm_with_message(
            f,
            "👑 Take Dispute",
            &dispute_id.to_string(),
            *selected_button,
            Some(&format!(
                "Do you want to take the dispute with id: {}?",
                dispute_id
            )),
        );
    }
    if let UiMode::AdminMode(AdminMode::ConfirmAddSolver(solver_pubkey, selected_button)) =
        &app.mode
    {
        admin_key_confirm::render_admin_key_confirm_with_message(
            f,
            "Add Solver",
            solver_pubkey,
            *selected_button,
            Some("Are you sure you want to add this pubkey as dispute solver?"),
        );
    }
    if let UiMode::AdminMode(AdminMode::ConfirmAdminKey(key_string, selected_button)) = &app.mode {
        admin_key_confirm::render_admin_key_confirm(
            f,
            "🔐 Confirm Admin Key",
            key_string,
            *selected_button,
        );
    }

    // Exit confirmation popup
    if let UiMode::ConfirmExit(selected_button) = &app.mode {
        exit_confirm::render_exit_confirm(f, *selected_button);
    }

    // Dispute finalization popup
    if let UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization(
        dispute_id,
        selected_button,
    )) = &app.mode
    {
        dispute_finalization_popup::render_finalization_popup(f, app, dispute_id, *selected_button);
    }

    // Dispute finalization confirmation popup
    if let UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute(
        dispute_id,
        is_settle,
        selected_button,
    )) = &app.mode
    {
        dispute_finalization_confirm::render_finalization_confirm(
            f,
            app,
            dispute_id,
            *is_settle,
            *selected_button,
        );
    }

    // Waiting for dispute finalization
    if let UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(_)) = &app.mode {
        waiting::render_waiting(f);
    }

    // Taking order popup overlay (user mode only)
    if let UiMode::UserMode(UserMode::TakingOrder(take_state)) = &app.mode {
        order_take::render_order_take(f, take_state);
    }

    // New message notification popup overlay
    if let UiMode::NewMessageNotification(notification, action, invoice_state) = &app.mode {
        message_notification::render_message_notification(
            f,
            notification,
            action.clone(),
            invoice_state,
        );
    }

    // Viewing message popup overlay
    if let UiMode::ViewingMessage(view_state) = &app.mode {
        tab_content::render_message_view(f, view_state);
    }
}
