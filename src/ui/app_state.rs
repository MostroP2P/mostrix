use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mostro_core::prelude::Action;
use zeroize::{Zeroize, Zeroizing};

use crate::models::AdminDispute;
use crate::ui::admin_state::AdminMode;
use crate::ui::chat::{
    AdminChatLastSeen, ChatParty, DisputeChatMessage, DisputeFilter, OrderChatLastSeen,
    UserOrderChatMessage,
};
use crate::ui::navigation::{Tab, UserRole};
use crate::ui::orders::{
    InvoiceInputState, KeyInputState, MessageNotification, MessageViewState, OperationResult,
    OrderMessage, RatingOrderState,
};
use crate::ui::user_state::UserMode;
use crate::util::MostroInstanceInfo;
use nostr_sdk::Keys;

#[derive(Debug)]
pub enum UiMode {
    // Shared modes (available to both user and admin)
    Normal,
    ViewingMessage(MessageViewState), // Simple message popup with yes/no options
    /// Rate the trade counterparty (1–5); Mostro resolves peer from order id.
    RatingOrder(RatingOrderState),
    NewMessageNotification(MessageNotification, Action, InvoiceInputState), // Popup for new message with invoice input state
    OperationResult(OperationResult), // Show operation result (success or error)
    HelpPopup(Tab, Box<UiMode>), // Context-aware shortcuts (Ctrl+H); 2nd = mode to restore on close
    /// Full descriptions for every Settings menu item (Shift+H on Settings); 2nd = mode to restore on close
    SettingsInstructionsPopup(UserRole, Box<UiMode>),
    /// Save attachment popup: list index of selected attachment (Ctrl+S in dispute chat).
    SaveAttachmentPopup(usize),
    /// Observer save attachment popup: list index of selected attachment (Ctrl+S in observer tab).
    ObserverSaveAttachmentPopup(usize),
    AddMostroPubkey(KeyInputState),
    ConfirmMostroPubkey(String, bool), // (key_string, selected_button: true=Yes, false=No)
    AddRelay(KeyInputState),
    ConfirmRelay(String, bool), // (relay_string, selected_button: true=Yes, false=No)
    AddCurrency(KeyInputState),
    ConfirmCurrency(String, bool), // (currency_string, selected_button: true=Yes, false=No)
    ConfirmClearCurrencies(bool),  // (selected_button: true=Yes, false=No)
    ConfirmDeleteHistoryOrder(uuid::Uuid, bool), // (order_id, selected_button)
    ConfirmBulkDeleteHistory(bool), // (selected_button)
    ConfirmExit(bool),             // (selected_button: true=Yes, false=No)

    // Generate new keys flow (Settings tab)
    ConfirmGenerateNewKeys(bool), // (selected_button: true=Yes, false=No)
    BackupNewKeys(Zeroizing<String>), // mnemonic words (zeroized on drop)

    // User-specific modes
    UserMode(UserMode),

    // Admin-specific modes
    AdminMode(AdminMode),
}

impl Clone for UiMode {
    fn clone(&self) -> Self {
        match self {
            UiMode::Normal => UiMode::Normal,
            UiMode::ViewingMessage(view_state) => UiMode::ViewingMessage(view_state.clone()),
            UiMode::RatingOrder(state) => UiMode::RatingOrder(state.clone()),
            UiMode::NewMessageNotification(notification, action, invoice_state) => {
                UiMode::NewMessageNotification(
                    notification.clone(),
                    action.clone(),
                    invoice_state.clone(),
                )
            }
            UiMode::OperationResult(result) => UiMode::OperationResult(result.clone()),
            UiMode::HelpPopup(tab, previous_mode) => {
                UiMode::HelpPopup(*tab, Box::new((**previous_mode).clone()))
            }
            UiMode::SettingsInstructionsPopup(role, previous_mode) => {
                UiMode::SettingsInstructionsPopup(*role, Box::new((**previous_mode).clone()))
            }
            UiMode::SaveAttachmentPopup(idx) => UiMode::SaveAttachmentPopup(*idx),
            UiMode::ObserverSaveAttachmentPopup(idx) => UiMode::ObserverSaveAttachmentPopup(*idx),
            UiMode::AddMostroPubkey(state) => UiMode::AddMostroPubkey(state.clone()),
            UiMode::ConfirmMostroPubkey(key, selected) => {
                UiMode::ConfirmMostroPubkey(key.clone(), *selected)
            }
            UiMode::AddRelay(state) => UiMode::AddRelay(state.clone()),
            UiMode::ConfirmRelay(relay, selected) => UiMode::ConfirmRelay(relay.clone(), *selected),
            UiMode::AddCurrency(state) => UiMode::AddCurrency(state.clone()),
            UiMode::ConfirmCurrency(currency, selected) => {
                UiMode::ConfirmCurrency(currency.clone(), *selected)
            }
            UiMode::ConfirmClearCurrencies(selected) => UiMode::ConfirmClearCurrencies(*selected),
            UiMode::ConfirmDeleteHistoryOrder(order_id, selected) => {
                UiMode::ConfirmDeleteHistoryOrder(*order_id, *selected)
            }
            UiMode::ConfirmBulkDeleteHistory(selected) => {
                UiMode::ConfirmBulkDeleteHistory(*selected)
            }
            UiMode::ConfirmExit(selected) => UiMode::ConfirmExit(*selected),
            UiMode::ConfirmGenerateNewKeys(selected) => UiMode::ConfirmGenerateNewKeys(*selected),
            // Clamp cloning of secret mnemonic to avoid duplicating sensitive seed words.
            UiMode::BackupNewKeys(_) => UiMode::BackupNewKeys(Zeroizing::new(String::new())),
            UiMode::UserMode(mode) => UiMode::UserMode(mode.clone()),
            UiMode::AdminMode(mode) => UiMode::AdminMode(mode.clone()),
        }
    }
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
    /// Selected message index for chat navigation (Up/Down) and footer hint; Save Attachment popup uses its own selection.
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
    /// Per-order startup floor for invoice popups: notifications at or below this rumor timestamp
    /// are treated as historical and must not auto-open AddInvoice/PayInvoice modal.
    pub startup_popup_floor_ts: HashMap<uuid::Uuid, i64>,
    pub selected_message_idx: usize, // Selected message in Messages tab
    pub selected_order_chat_idx: usize, // Selected order in Order Chat sidebar
    pub order_chat_input: String,
    pub order_chat_input_enabled: bool,
    pub order_chats: HashMap<String, Vec<UserOrderChatMessage>>, // Chat messages per order id
    pub order_chat_scrollview_state: tui_scrollview::ScrollViewState,
    pub order_chat_selected_message_idx: Option<usize>,
    pub order_chat_line_starts: Vec<usize>,
    pub order_chat_scroll_tracker: Option<(String, usize)>,
    pub order_chat_last_seen: HashMap<String, OrderChatLastSeen>,
    pub pending_notifications: Arc<Mutex<usize>>, // Count of pending notifications (non-critical)
    pub admin_disputes_in_progress: Vec<AdminDispute>, // Taken disputes
    pub dispute_filter: DisputeFilter, // Filter for viewing InProgress or Finalized disputes
    /// Transient toast when a new attachment is received (message text, expiry time). Cleared when expired or on key press.
    pub attachment_toast: Option<(String, Instant)>,
    /// Observer mode: shared key as 64-char hex string (32 bytes).
    pub observer_shared_key_input: String,
    /// Observer mode: chat messages fetched from relays for the pasted shared key.
    pub observer_messages: Vec<DisputeChatMessage>,
    /// Observer mode: scroll state for chat messages.
    pub observer_scrollview_state: tui_scrollview::ScrollViewState,
    /// Observer mode: last seen message count for auto-scroll.
    pub observer_scroll_tracker: Option<usize>,
    /// Observer mode: true while an async fetch is in flight.
    pub observer_loading: bool,
    /// Observer mode: last error message (if any).
    pub observer_error: Option<String>,
    /// Parsed `admin_privkey` from settings (dispute chat, classification). Updated on save / reload.
    pub admin_keys: Option<Keys>,
    /// After switching to admin mode (Settings → Switch Mode) or saving admin key: reload disputes from DB in main.
    pub pending_admin_disputes_reload: bool,
    /// Cached copy of currencies filter from settings (used for UI-side filtering).
    pub currencies_filter: Vec<String>,
    /// Cached Mostro instance info (kind 38385 event), if available.
    pub mostro_info: Option<MostroInstanceInfo>,
    /// Non-blocking overlay shown when relays are unreachable.
    pub offline_overlay_message: Option<String>,
    /// True only when BackupNewKeys was opened after runtime key rotation.
    /// In that case, app must restart to reload in-memory keys safely.
    pub backup_requires_restart: bool,
    /// Set when the user dismisses BackupNewKeys after runtime rotation.
    /// Main loop performs an in-process runtime reload and clears session state.
    pub pending_key_reload: bool,
    /// Set when Mostro pubkey or currency filters change: respawn order/dispute subscriptions and
    /// DM listener without rotating identity keys or clearing the Messages tab.
    pub pending_fetch_scheduler_reload: bool,
    /// When `take_order` completes while an AddInvoice/PayInvoice popup is open, we stash the
    /// [`OperationResult`] here so the invoice UI is not replaced by the success screen (race).
    /// Applied when the user dismisses the popup (Esc), or cleared when they submit the invoice.
    pub pending_post_take_operation_result: Option<OperationResult>,
    /// When set, closing an OperationResult popup (ESC/ENTER) will exit the app.
    /// Used for fatal errors that require a clean restart.
    pub fatal_exit_on_close: bool,
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
            startup_popup_floor_ts: HashMap::new(),
            selected_message_idx: 0,
            selected_order_chat_idx: 0,
            order_chat_input: String::new(),
            order_chat_input_enabled: true,
            order_chats: HashMap::new(),
            order_chat_scrollview_state: tui_scrollview::ScrollViewState::default(),
            order_chat_selected_message_idx: None,
            order_chat_line_starts: Vec::new(),
            order_chat_scroll_tracker: None,
            order_chat_last_seen: HashMap::new(),
            pending_notifications: Arc::new(Mutex::new(0)),
            admin_disputes_in_progress: Vec::new(),
            dispute_filter: DisputeFilter::InProgress, // Default to InProgress view
            attachment_toast: None,
            observer_shared_key_input: String::new(),
            observer_messages: Vec::new(),
            observer_scrollview_state: tui_scrollview::ScrollViewState::default(),
            observer_scroll_tracker: None,
            observer_loading: false,
            observer_error: None,
            admin_keys: None,
            pending_admin_disputes_reload: false,
            currencies_filter: Vec::new(),
            mostro_info: None,
            offline_overlay_message: None,
            backup_requires_restart: false,
            pending_key_reload: false,
            pending_fetch_scheduler_reload: false,
            pending_post_take_operation_result: None,
            fatal_exit_on_close: false,
        }
    }

    /// Securely wipe all observer inputs and fetched content.
    /// Uses `zeroize` to overwrite strings before clearing them, then
    /// resets error state to safe defaults.
    pub fn clear_observer_secrets(&mut self) {
        self.observer_shared_key_input.zeroize();
        self.observer_shared_key_input.clear();

        for msg in &mut self.observer_messages {
            msg.content.zeroize();
        }
        self.observer_messages.clear();
        self.observer_loading = false;

        if let Some(err) = &mut self.observer_error {
            err.zeroize();
        }
        self.observer_error = None;
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
        self.offline_overlay_message = None;
        // Clear observer state when switching roles so sensitive data does not linger
        self.clear_observer_secrets();
        // Note: we intentionally preserve admin_dispute_chats, admin_chat_last_seen,
        // admin_disputes_in_progress, admin_chat_scrollview_state, admin_chat_selected_message_idx,
        // admin_chat_line_starts, admin_chat_scroll_tracker, and dispute_filter across role switches
        // so that admin context is not lost when temporarily viewing user mode.
    }
}
