use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};

pub const PRIMARY_COLOR: Color = Color::Rgb(177, 204, 51); // #b1cc33
pub const BACKGROUND_COLOR: Color = Color::Rgb(29, 33, 44); // #1D212C

pub mod order_confirm;
pub mod order_form;
pub mod order_result;
pub mod order_take;
pub mod orders_tab;
pub mod status;
pub mod tab_content;
pub mod tabs;
pub mod waiting;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Orders,
    MyTrades,
    Messages,
    Settings,
    CreateNewOrder,
}

impl Tab {
    /// Convert tab to usize index for ratatui Tabs widget
    pub fn as_index(self) -> usize {
        match self {
            Tab::Orders => 0,
            Tab::MyTrades => 1,
            Tab::Messages => 2,
            Tab::Settings => 3,
            Tab::CreateNewOrder => 4,
        }
    }

    /// Get the total number of tabs
    pub fn count() -> usize {
        5
    }

    /// Get the first tab
    pub fn first() -> Self {
        Tab::Orders
    }

    /// Get the last tab
    pub fn last() -> Self {
        Tab::CreateNewOrder
    }

    /// Move to the previous tab, wrapping around
    pub fn prev(self) -> Self {
        match self {
            Tab::Orders => Tab::Orders, // Don't wrap, stay at first
            Tab::MyTrades => Tab::Orders,
            Tab::Messages => Tab::MyTrades,
            Tab::Settings => Tab::Messages,
            Tab::CreateNewOrder => Tab::Settings,
        }
    }

    /// Move to the next tab, wrapping around
    pub fn next(self) -> Self {
        match self {
            Tab::Orders => Tab::MyTrades,
            Tab::MyTrades => Tab::Messages,
            Tab::Messages => Tab::Settings,
            Tab::Settings => Tab::CreateNewOrder,
            Tab::CreateNewOrder => Tab::CreateNewOrder, // Don't wrap, stay at last
        }
    }
}

#[derive(Clone, Debug)]
pub enum UiMode {
    Normal,
    CreatingOrder(FormState),
    ConfirmingOrder(FormState),                  // Confirmation popup
    TakingOrder(TakeOrderState),                 // Taking an order from the list
    WaitingForMostro(FormState),                 // Waiting for Mostro response (order creation)
    WaitingTakeOrder(TakeOrderState),            // Waiting for Mostro response (taking order)
    OrderResult(OrderResult),                    // Show order result (success or error)
    NewMessageNotification(MessageNotification), // Popup for new message
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
        status: Option<mostro_core::prelude::Status>,
        trade_index: Option<i64>, // Trade index used for this order
    },
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
}

/// Notification for a new message
#[derive(Clone, Debug)]
pub struct MessageNotification {
    pub order_id: Option<uuid::Uuid>,
    pub message_preview: String,
    pub timestamp: u64,
}

pub struct AppState {
    pub active_tab: Tab,
    pub selected_order_idx: usize,
    pub mode: UiMode,
    pub messages: Arc<Mutex<Vec<OrderMessage>>>, // Messages related to orders
    pub active_order_trade_indices: Arc<Mutex<HashMap<uuid::Uuid, i64>>>, // Map order_id -> trade_index
    pub selected_message_idx: usize, // Selected message in Messages tab
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            active_tab: Tab::Orders,
            selected_order_idx: 0,
            mode: UiMode::Normal,
            messages: Arc::new(Mutex::new(Vec::new())),
            active_order_trade_indices: Arc::new(Mutex::new(HashMap::new())),
            selected_message_idx: 0,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Apply color coding to status cells based on status type (adapted for ratatui)
pub(crate) fn apply_status_color(status: &str) -> Style {
    let status_lower = status.to_lowercase();
    if status_lower.contains("init")
        || status_lower.contains("pending")
        || status_lower.contains("waiting")
    {
        Style::default().fg(Color::Yellow)
    } else if status_lower.contains("active")
        || status_lower.contains("released")
        || status_lower.contains("settled")
        || status_lower.contains("taken")
        || status_lower.contains("success")
    {
        Style::default().fg(Color::Green)
    } else if status_lower.contains("fiat") {
        Style::default().fg(Color::Cyan)
    } else if status_lower.contains("dispute")
        || status_lower.contains("cancel")
        || status_lower.contains("canceled")
    {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
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
    app: &AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    status_line: Option<&str>,
) {
    // Create layout: one row for tabs and the rest for content.
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ],
    )
    .split(f.area());

    // Render tabs
    tabs::render_tabs(f, chunks[0], app.active_tab);

    // Render content based on active tab
    let content_area = chunks[1];
    match app.active_tab {
        Tab::Orders => {
            orders_tab::render_orders_tab(f, content_area, orders, app.selected_order_idx)
        }
        Tab::MyTrades => tab_content::render_coming_soon(f, content_area, "My Trades"),
        Tab::Messages => {
            let messages = app.messages.lock().unwrap();
            tab_content::render_messages_tab(f, content_area, &messages, app.selected_message_idx)
        }
        Tab::Settings => tab_content::render_coming_soon(f, content_area, "Settings"),
        Tab::CreateNewOrder => {
            if let UiMode::CreatingOrder(form) = &app.mode {
                order_form::render_order_form(f, content_area, form);
            } else {
                order_form::render_form_initializing(f, content_area);
            }
        }
    }

    // Bottom status bar
    if let Some(line) = status_line {
        status::render_status_bar(f, chunks[2], line);
    }

    // Confirmation popup overlay
    if let UiMode::ConfirmingOrder(form) = &app.mode {
        order_confirm::render_order_confirm(f, form);
    }

    // Waiting for Mostro popup overlay
    if let UiMode::WaitingForMostro(_) = &app.mode {
        waiting::render_waiting(f);
    }

    // Waiting for take order popup overlay
    if let UiMode::WaitingTakeOrder(_) = &app.mode {
        waiting::render_waiting(f);
    }

    // Order result popup overlay
    if let UiMode::OrderResult(result) = &app.mode {
        order_result::render_order_result(f, result);
    }

    // Taking order popup overlay
    if let UiMode::TakingOrder(take_state) = &app.mode {
        order_take::render_order_take(f, take_state);
    }

    // New message notification popup overlay
    if let UiMode::NewMessageNotification(notification) = &app.mode {
        tab_content::render_message_notification(f, notification);
    }
}
