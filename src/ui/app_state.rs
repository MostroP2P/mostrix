use std::collections::{HashMap, HashSet};

use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

use mostro_core::prelude::{Action, Transport};
use ratatui::widgets::TableState;
use zeroize::{Zeroize, Zeroizing};

use crate::models::AdminDispute;
use crate::ui::admin_state::AdminMode;
use crate::ui::chat::{
    AdminChatLastSeen, ChatParty, DisputeChatMessage, DisputeFilter, OrderChatLastSeen,
    UserChatChannel, UserOrderChatMessage,
};
use crate::ui::helpers::OrderChatListItem;
use crate::ui::navigation::{AdminTab, Tab, UserRole};
use crate::ui::orders::{
    BuyerInvoicePreference, FormState, InvoiceInputState, KeyInputState, MessageNotification,
    MessageViewState, OperationResult, OrderChatStaticHeader, OrderMessage, RatingOrderState,
};
use crate::ui::user_state::UserMode;
use crate::util::{transport_from_instance, MostroInstanceInfo};
use nostr_sdk::prelude::Keys;

#[derive(Debug)]
pub enum UiMode {
    // Shared modes (available to both user and admin)
    Normal,
    ViewingMessage(MessageViewState), // Simple message popup with yes/no options
    /// Rate the trade counterparty (1–5); Mostro resolves peer from order id.
    RatingOrder(RatingOrderState),
    NewMessageNotification(MessageNotification, Action, InvoiceInputState), // Popup for new message with invoice input state
    OperationResult(Box<OperationResult>), // Show operation result (success or error)
    HelpPopup(Tab, Box<UiMode>), // Context-aware shortcuts (Ctrl+H); 2nd = mode to restore on close
    /// Full descriptions for every Settings menu item (Shift+H on Settings); 2nd = mode to restore on close
    SettingsInstructionsPopup(UserRole, Box<UiMode>),
    /// Save attachment popup: list index of selected attachment (Ctrl+S in dispute chat).
    SaveAttachmentPopup(usize),
    /// Observer save attachment popup: list index of selected attachment (Ctrl+S in observer tab).
    ObserverSaveAttachmentPopup(usize),
    /// User order chat save attachment popup: pinned order id + list index (Ctrl+S on My Trades tab).
    UserSaveAttachmentPopup(String, usize),
    /// User order chat send attachment file picker: pinned order id (Ctrl+O on My Trades tab).
    UserSendAttachmentPicker(String),
    /// Settings: enter Mostro pubkey (`npub` or hex).
    AddMostroPubkey(KeyInputState),
    /// Settings: confirm Mostro pubkey (hex string, Yes/No).
    ConfirmMostroPubkey(String, bool),
    AddRelay(KeyInputState),
    ConfirmRelay(String, bool), // (relay_string, selected_button: true=Yes, false=No)
    /// User-mode Settings: buyer Lightning address (`user@domain.com`).
    AddLnAddress(KeyInputState),
    ConfirmLnAddress(String, bool), // (address, selected_button)
    /// User-mode Settings: clear saved buyer Lightning address.
    ConfirmClearLnAddress(bool),
    /// Before AddInvoice: ask whether to use the saved buyer Lightning address from settings.
    ConfirmSavedLnAddressForInvoice(MessageNotification, bool), // selected_button: true = Yes
    AddCurrency(KeyInputState),
    ConfirmCurrency(String, bool), // (currency_string, selected_button: true=Yes, false=No)
    ConfirmClearCurrencies(bool),  // (selected_button: true=Yes, false=No)
    ConfirmDeleteHistoryOrder(uuid::Uuid, bool), // (order_id, selected_button)
    ConfirmBulkDeleteHistory(bool), // (selected_button)
    /// User Settings: ask Mostro to restore this identity's orders and disputes.
    ConfirmRestoreSession(bool), // (selected_button: true=Yes, false=No)
    ConfirmExit(bool),             // (selected_button: true=Yes, false=No)

    // Generate new keys flow (Settings tab)
    ConfirmGenerateNewKeys(bool), // (selected_button: true=Yes, false=No)
    BackupNewKeys(Zeroizing<String>), // mnemonic words (zeroized on drop)

    // User-specific modes
    UserMode(UserMode),

    // Admin-specific modes
    AdminMode(AdminMode),
}

impl UiMode {
    pub fn operation_result(result: OperationResult) -> Self {
        Self::OperationResult(Box::new(result))
    }

    /// Default interactive mode after startup or role switch.
    pub fn default_for_role(user_role: UserRole) -> Self {
        match user_role {
            UserRole::User => UiMode::UserMode(UserMode::Normal),
            UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
        }
    }

    /// Whether My Trades chat input, Ctrl+S, and scroll shortcuts should be active.
    #[must_use]
    pub fn user_my_trades_interactive(&self) -> bool {
        matches!(self, UiMode::Normal | UiMode::UserMode(UserMode::Normal))
    }
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
            UiMode::OperationResult(result) => {
                UiMode::OperationResult(Box::new((**result).clone()))
            }
            UiMode::HelpPopup(tab, previous_mode) => {
                UiMode::HelpPopup(*tab, Box::new((**previous_mode).clone()))
            }
            UiMode::SettingsInstructionsPopup(role, previous_mode) => {
                UiMode::SettingsInstructionsPopup(*role, Box::new((**previous_mode).clone()))
            }
            UiMode::SaveAttachmentPopup(idx) => UiMode::SaveAttachmentPopup(*idx),
            UiMode::ObserverSaveAttachmentPopup(idx) => UiMode::ObserverSaveAttachmentPopup(*idx),
            UiMode::UserSaveAttachmentPopup(order_id, idx) => {
                UiMode::UserSaveAttachmentPopup(order_id.clone(), *idx)
            }
            UiMode::UserSendAttachmentPicker(order_id) => {
                UiMode::UserSendAttachmentPicker(order_id.clone())
            }
            UiMode::AddMostroPubkey(state) => UiMode::AddMostroPubkey(state.clone()),
            UiMode::ConfirmMostroPubkey(key, selected) => {
                UiMode::ConfirmMostroPubkey(key.clone(), *selected)
            }
            UiMode::AddRelay(state) => UiMode::AddRelay(state.clone()),
            UiMode::ConfirmRelay(relay, selected) => UiMode::ConfirmRelay(relay.clone(), *selected),
            UiMode::AddLnAddress(state) => UiMode::AddLnAddress(state.clone()),
            UiMode::ConfirmLnAddress(addr, selected) => {
                UiMode::ConfirmLnAddress(addr.clone(), *selected)
            }
            UiMode::ConfirmClearLnAddress(selected) => UiMode::ConfirmClearLnAddress(*selected),
            UiMode::ConfirmSavedLnAddressForInvoice(notification, selected) => {
                UiMode::ConfirmSavedLnAddressForInvoice(notification.clone(), *selected)
            }
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
            UiMode::ConfirmRestoreSession(selected) => UiMode::ConfirmRestoreSession(*selected),
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
    /// Orders tab selection by **order UUID**, resolved through the currency-filtered
    /// book projection (`helpers/order_selection.rs`). Not a raw index into the
    /// unfiltered in-memory order vec — highlight, ↑↓, and Enter share this id.
    pub selected_order_id: Option<uuid::Uuid>,
    /// Persistent scroll state for the Orders tab table
    pub orders_table_state: TableState,
    /// Disputes Pending selection by **dispute UUID**, resolved against the
    /// initiated-status projection (`helpers/dispute_selection.rs`).
    pub selected_pending_dispute_id: Option<uuid::Uuid>,
    /// Persistent scroll state for the Disputes Pending table
    pub disputes_table_state: TableState,
    /// Disputes In Progress / Finalized selection is by **dispute id**, not a raw
    /// index into `admin_disputes_in_progress` (see `helpers/dispute_selection.rs`).
    pub selected_dispute_id: Option<String>, // Selected dispute (by dispute id) in Disputes in Progress tab
    pub active_chat_party: ChatParty, // Which party the admin is currently chatting with
    pub admin_chat_input: String,     // Current message being typed by admin
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
    /// Orders the user removed from local history this session; trade DMs for these ids are ignored
    /// so relays cannot re-upsert deleted rows back into the UI.
    pub dropped_user_history_order_ids: Arc<Mutex<HashSet<uuid::Uuid>>>,
    /// Per-order startup floor for invoice popups: notifications at or below this rumor timestamp
    /// are treated as historical and must not auto-open AddInvoice/PayInvoice/PayBondInvoice modal.
    pub startup_popup_floor_ts: HashMap<uuid::Uuid, i64>,
    /// Per-order buyer invoice preference when we are taker on a SELL listing.
    /// In-memory only; used by Messages/AddInvoice flows to decide how to
    /// source the buyer invoice for a specific trade.
    pub buyer_invoice_preference: HashMap<uuid::Uuid, BuyerInvoicePreference>,
    pub selected_message_idx: usize, // Selected message in Messages tab
    pub selected_order_chat_idx: usize, // Selected order in Order Chat sidebar
    pub order_chat_input: String,
    pub order_chat_input_enabled: bool,
    /// Per-order static header (id, kind, created_at, trade index, initiator) from take/create and DB.
    pub order_chat_static: HashMap<Uuid, OrderChatStaticHeader>,
    /// Maker `pending` listings on the book without a trade-DM row in Messages (refreshed on events).
    pub my_trades_maker_book: Vec<OrderChatListItem>,
    pub order_chats: HashMap<String, Vec<UserOrderChatMessage>>, // Chat messages per order id
    /// User-to-solver dispute messages per order id.
    pub user_dispute_chats: HashMap<String, Vec<UserOrderChatMessage>>,
    /// Active My Trades conversation for the selected order.
    pub active_user_chat_channel: UserChatChannel,
    pub order_chat_scrollview_state: tui_scrollview::ScrollViewState,
    pub order_chat_selected_message_idx: Option<usize>,
    pub order_chat_line_starts: Vec<usize>,
    pub order_chat_scroll_tracker: Option<(String, UserChatChannel, usize)>,
    pub order_chat_last_seen: HashMap<String, OrderChatLastSeen>,
    pub user_dispute_chat_last_seen: HashMap<String, OrderChatLastSeen>,
    pub pending_notifications: Arc<Mutex<usize>>, // Count of pending notifications (non-critical)
    pub admin_disputes_in_progress: Vec<AdminDispute>, // Taken disputes
    pub dispute_filter: DisputeFilter, // Filter for viewing InProgress or Finalized disputes
    /// Transient toast when a new attachment is received (message text, expiry time). Cleared when expired or on key press.
    pub attachment_toast: Option<(String, Instant)>,
    /// Upload succeeded but chat DM failed; retry via `SendOrderAttachmentJob::RetryPrepared`.
    pub pending_order_attachment_sends:
        HashMap<String, crate::ui::helpers::PreparedOrderChatAttachment>,
    /// Active `ratatui-explorer` instance while `UserSendAttachmentPicker` is open.
    pub user_send_attachment_explorer: Option<ratatui_explorer::FileExplorer>,
    /// Order id with an outbound attachment send in progress (blocks duplicate Ctrl+O).
    pub sending_attachment_order_id: Option<String>,
    /// Observer mode: disclosed `K_conv` as 64-char hex (read-only grant).
    pub observer_shared_key_input: String,
    /// Observer mode: chat messages fetched from relays for the pasted `K_conv`.
    pub observer_messages: Vec<DisputeChatMessage>,
    /// Observer mode: scroll state for chat messages.
    pub observer_scrollview_state: tui_scrollview::ScrollViewState,
    /// Observer mode: last seen message count for auto-scroll.
    pub observer_scroll_tracker: Option<usize>,
    /// Observer mode: true while an async fetch is in flight.
    pub observer_loading: bool,
    /// Observer mode: last error message (if any).
    pub observer_error: Option<String>,
    /// Bumped on each Observer fetch and on [`Self::clear_observer_secrets`].
    /// Async `ObserverChatLoaded` / `ObserverChatError` results with an older
    /// generation are ignored so a slow relay reply cannot overwrite a newer
    /// transcript or restored-empty state.
    pub observer_fetch_generation: u64,
    /// Parsed `admin_privkey` from settings (dispute chat, classification). Updated on save / reload.
    pub admin_keys: Option<Keys>,
    /// After switching to admin mode (Settings → Switch Mode) or saving admin key: reload disputes from DB in main.
    pub pending_admin_disputes_reload: bool,
    /// Cached copy of currencies filter from settings (used for UI-side filtering).
    pub currencies_filter: Vec<String>,
    /// Cached Mostro instance info (kind 38385 event), if available.
    pub mostro_info: Option<MostroInstanceInfo>,
    /// Wire transport resolved from [`Self::mostro_info`] (`protocol_version` tag).
    pub transport: Transport,
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
    /// When `take_order` completes while an AddInvoice/PayInvoice/PayBondInvoice popup is open, we
    /// stash the [`OperationResult`] here so the invoice UI is not replaced by the success screen
    /// (race). Applied when the user dismisses the popup (Esc), or cleared when they submit the
    /// invoice.
    pub pending_post_take_operation_result: Option<OperationResult>,
    /// When set, closing an OperationResult popup (ESC/ENTER) will exit the app.
    /// Used for fatal errors that require a clean restart.
    pub fatal_exit_on_close: bool,
    /// Preserved New Order form draft so leaving/returning to the tab keeps input.
    /// Cleared on explicit cancel (Esc) or successful submit.
    pub order_form_draft: Option<FormState>,
}

impl AppState {
    pub fn new(user_role: UserRole) -> Self {
        let initial_tab = Tab::first(user_role);
        Self {
            user_role,
            active_tab: initial_tab,
            selected_order_id: None,
            orders_table_state: TableState::default(),
            selected_pending_dispute_id: None,
            disputes_table_state: TableState::default(),
            selected_dispute_id: None,
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
            mode: UiMode::default_for_role(user_role),
            messages: Arc::new(Mutex::new(Vec::new())),
            active_order_trade_indices: Arc::new(Mutex::new(HashMap::new())),
            dropped_user_history_order_ids: Arc::new(Mutex::new(HashSet::new())),
            startup_popup_floor_ts: HashMap::new(),
            buyer_invoice_preference: HashMap::new(),
            selected_message_idx: 0,
            selected_order_chat_idx: 0,
            order_chat_input: String::new(),
            order_chat_input_enabled: true,
            order_chat_static: HashMap::new(),
            my_trades_maker_book: Vec::new(),
            order_chats: HashMap::new(),
            user_dispute_chats: HashMap::new(),
            active_user_chat_channel: UserChatChannel::Peer,
            order_chat_scrollview_state: tui_scrollview::ScrollViewState::default(),
            order_chat_selected_message_idx: None,
            order_chat_line_starts: Vec::new(),
            order_chat_scroll_tracker: None,
            order_chat_last_seen: HashMap::new(),
            user_dispute_chat_last_seen: HashMap::new(),
            pending_notifications: Arc::new(Mutex::new(0)),
            admin_disputes_in_progress: Vec::new(),
            dispute_filter: DisputeFilter::InProgress, // Default to InProgress view
            attachment_toast: None,
            pending_order_attachment_sends: HashMap::new(),
            user_send_attachment_explorer: None,
            sending_attachment_order_id: None,
            observer_shared_key_input: String::new(),
            observer_messages: Vec::new(),
            observer_scrollview_state: tui_scrollview::ScrollViewState::default(),
            observer_scroll_tracker: None,
            observer_loading: false,
            observer_error: None,
            observer_fetch_generation: 0,
            admin_keys: None,
            pending_admin_disputes_reload: false,
            currencies_filter: Vec::new(),
            mostro_info: None,
            transport: Transport::default(),
            offline_overlay_message: None,
            backup_requires_restart: false,
            pending_key_reload: false,
            pending_fetch_scheduler_reload: false,
            pending_post_take_operation_result: None,
            fatal_exit_on_close: false,
            order_form_draft: None,
        }
    }

    /// Replace cached instance info and keep [`Self::transport`] in sync.
    pub fn set_mostro_info(&mut self, info: Option<MostroInstanceInfo>) {
        self.transport = transport_from_instance(info.as_ref());
        self.mostro_info = info;
    }

    /// True when the Observer Shared key field should accept typing and paste.
    ///
    /// False while a modal (`HelpPopup`, `OperationResult`, save-attachment, …) owns input.
    pub fn observer_inputs_editable(&self) -> bool {
        matches!(self.active_tab, Tab::Admin(AdminTab::Observer))
            && matches!(
                self.mode,
                UiMode::Normal | UiMode::AdminMode(AdminMode::Normal)
            )
    }

    fn bump_observer_fetch_generation(&mut self) -> u64 {
        self.observer_fetch_generation = self.observer_fetch_generation.saturating_add(1);
        self.observer_fetch_generation
    }

    /// Invalidate in-flight Observer fetches, then mark a new fetch as current.
    pub fn begin_observer_fetch(&mut self) -> u64 {
        let generation = self.bump_observer_fetch_generation();
        for msg in &mut self.observer_messages {
            msg.content.zeroize();
        }
        self.observer_messages.clear();
        self.observer_error = None;
        self.observer_loading = true;
        generation
    }

    /// Securely wipe all observer inputs and fetched content.
    /// Uses `zeroize` to overwrite strings before clearing them, then
    /// resets error state to safe defaults.
    pub fn clear_observer_secrets(&mut self) {
        self.bump_observer_fetch_generation();
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
        self.mode = UiMode::default_for_role(new_role);
        self.selected_pending_dispute_id = None;
        self.disputes_table_state = TableState::default();
        self.selected_settings_option = 0;
        self.selected_order_id = None;
        self.orders_table_state = TableState::default();
        self.selected_dispute_id = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chat::{ChatSender, DisputeChatMessage};

    fn dummy_observer_message(content: &str) -> DisputeChatMessage {
        DisputeChatMessage {
            sender: ChatSender::Buyer,
            content: content.to_string(),
            timestamp: 1,
            target_party: None,
            attachment: None,
        }
    }

    #[test]
    fn observer_inputs_editable_only_in_plain_observer_mode() {
        let mut app = AppState::new(UserRole::Admin);
        app.active_tab = Tab::Admin(AdminTab::Observer);
        app.mode = UiMode::AdminMode(AdminMode::Normal);
        assert!(app.observer_inputs_editable());

        app.mode = UiMode::HelpPopup(
            app.active_tab,
            Box::new(UiMode::AdminMode(AdminMode::Normal)),
        );
        assert!(!app.observer_inputs_editable());

        app.mode = UiMode::operation_result(crate::ui::OperationResult::Error("x".into()));
        assert!(!app.observer_inputs_editable());

        app.mode = UiMode::ObserverSaveAttachmentPopup(0);
        assert!(!app.observer_inputs_editable());
    }

    #[test]
    fn clear_observer_secrets_invalidates_in_flight_fetch_generation() {
        let mut app = AppState::new(UserRole::Admin);
        let gen = app.begin_observer_fetch();
        app.observer_messages.push(dummy_observer_message("from-a"));
        app.clear_observer_secrets();
        assert_ne!(app.observer_fetch_generation, gen);
        assert!(app.observer_messages.is_empty());
        assert!(!app.observer_loading);
    }
}
