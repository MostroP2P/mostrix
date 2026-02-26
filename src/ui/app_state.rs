use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mostro_core::prelude::Action;

use crate::models::AdminDispute;
use crate::ui::admin_state::AdminMode;
use crate::ui::chat::{AdminChatLastSeen, ChatParty, DisputeChatMessage, DisputeFilter};
use crate::ui::navigation::{Tab, UserRole};
use crate::ui::orders::{
    InvoiceInputState, KeyInputState, MessageNotification, MessageViewState, OperationResult,
    OrderMessage,
};
use crate::ui::tabs::observer_tab::ObserverFocus;
use crate::ui::user_state::UserMode;

#[derive(Clone, Debug)]
pub enum UiMode {
    // Shared modes (available to both user and admin)
    Normal,
    ViewingMessage(MessageViewState), // Simple message popup with yes/no options
    NewMessageNotification(MessageNotification, Action, InvoiceInputState), // Popup for new message with invoice input state
    OperationResult(OperationResult), // Show operation result (success or error)
    HelpPopup(Tab, Box<UiMode>), // Context-aware shortcuts (Ctrl+H); 2nd = mode to restore on close
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
    pub admin_chat_scrollview_state: tui_scrollview::ScrollViewState,
    /// Selected message index (for Up/Down and Ctrl+S attachment save)
    pub admin_chat_selected_message_idx: Option<usize>,
    /// Line start index per visible message; updated each frame when rendering chat (for scroll sync)
    pub admin_chat_line_starts: Vec<usize>,
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
    pub admin_disputes_in_progress: Vec<AdminDispute>, // Taken disputes
    pub dispute_filter: DisputeFilter, // Filter for viewing InProgress or Finalized disputes
    /// Transient toast when a new attachment is received (message text, expiry time). Cleared when expired or on key press.
    pub attachment_toast: Option<(String, Instant)>,
    /// Observer mode: path to encrypted chat file (relative to ~/.mostrix/downloads or absolute).
    pub observer_file_path_input: String,
    /// Observer mode: shared key as 64-char hex string (32 bytes).
    pub observer_shared_key_input: String,
    /// Observer mode: which input field is currently focused.
    pub observer_focus: ObserverFocus,
    /// Observer mode: decrypted chat lines for preview.
    pub observer_chat_lines: Vec<String>,
    /// Observer mode: last error message (if any).
    pub observer_error: Option<String>,
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
            admin_chat_scrollview_state: tui_scrollview::ScrollViewState::default(),
            admin_chat_selected_message_idx: None,
            admin_chat_line_starts: Vec::new(),
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
            attachment_toast: None,
            observer_file_path_input: String::new(),
            observer_shared_key_input: String::new(),
            observer_focus: crate::ui::tabs::observer_tab::ObserverFocus::FilePath,
            observer_chat_lines: Vec::new(),
            observer_error: None,
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
        // Note: we intentionally preserve admin_dispute_chats, admin_chat_last_seen,
        // admin_disputes_in_progress, admin_chat_scrollview_state, admin_chat_selected_message_idx,
        // admin_chat_line_starts, admin_chat_scroll_tracker, and dispute_filter across role switches
        // so that admin context is not lost when temporarily viewing user mode.
    }
}
