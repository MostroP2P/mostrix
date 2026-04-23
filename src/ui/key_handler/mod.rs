mod admin_handlers;
mod async_tasks;
mod chat_helpers;
mod confirmation;
mod enter_handlers;
mod esc_handlers;
mod form_input;
mod input_helpers;
mod message_handlers;
mod navigation;
mod settings;
mod user_handlers;
mod validation;

use crate::ui::key_handler::chat_helpers::{
    build_order_action_view_state, build_rating_state_for_mytrades,
    resolve_selected_mytrades_order_status,
};
use crate::ui::{
    helpers::{get_visible_attachment_messages, is_dispute_finalized},
    AdminMode, AdminTab, AppState, ChatAttachment, ChatSender, DisputeFilter,
    MostroInfoFetchResult, OperationResult, Tab, TakeOrderState, UiMode, UserMode, UserTab,
    ViewingMessageButtonSelection,
};
use crate::util::MostroInstanceInfo;
use crate::util::OrderDmSubscriptionCmd;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use zeroize::Zeroizing;

fn debug_log_keys(hypothesis_id: &str, location: &str, message: &str, data: serde_json::Value) {
    let payload = serde_json::json!({
        "sessionId": "715880",
        "runId": "pre-fix",
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug-715880.log")
    {
        let _ = writeln!(file, "{payload}");
    }
}

/// Context passed to Enter and confirmation handlers to avoid too many arguments.
pub struct EnterKeyContext<'a> {
    pub orders: &'a Arc<Mutex<Vec<SmallOrder>>>,
    pub disputes: &'a Arc<Mutex<Vec<Dispute>>>,
    pub pool: &'a SqlitePool,
    pub client: &'a Client,
    /// Settings snapshot; prefer locking `current_mostro_pubkey` for the live instance key.
    pub mostro_pubkey: PublicKey,
    pub current_mostro_pubkey: &'a Arc<Mutex<PublicKey>>,
    pub order_result_tx: &'a UnboundedSender<OperationResult>,
    pub key_rotation_tx: &'a UnboundedSender<Result<Zeroizing<String>, String>>,
    pub seed_words_tx: &'a UnboundedSender<Result<Zeroizing<String>, String>>,
    pub mostro_info_tx: &'a UnboundedSender<MostroInfoFetchResult>,
    /// Cached kind 38385 instance info (PoW bits for outbound events).
    pub mostro_info: Option<MostroInstanceInfo>,
    pub admin_chat_keys: Option<&'a Keys>,
    pub dm_subscription_tx: &'a UnboundedSender<OrderDmSubscriptionCmd>,
}

fn is_terminal_order_status(status: Option<Status>) -> bool {
    matches!(
        status,
        Some(
            Status::Success
                | Status::Canceled
                | Status::CanceledByAdmin
                | Status::SettledByAdmin
                | Status::CompletedByAdmin
                | Status::Expired
                | Status::CooperativelyCanceled
        )
    )
}

// Re-export public functions
pub use async_tasks::{
    apply_pending_fetch_scheduler_reload, apply_pending_key_reload, apply_pending_runtime_reloads,
    create_app_channels, reload_runtime_session_after_reconnect, spawn_refresh_mostro_info_task,
    AppChannels, RuntimeReconnectContext,
};
pub use confirmation::{handle_cancel_key, handle_confirm_key};
pub use enter_handlers::handle_enter_key;
pub use esc_handlers::handle_esc_key;
pub use form_input::{handle_backspace, handle_char_input};
pub use input_helpers::{handle_invoice_input, handle_key_input};
pub use navigation::{handle_navigation, handle_tab_navigation};
pub use settings::handle_mode_switch;
pub use validation::{validate_currency, validate_mostro_pubkey, validate_npub, validate_relay};

/// Check if we're in admin chat input mode and handle character input
/// Returns Some(true) if handled, None if should continue to normal processing
/// key_event is needed to check for modifiers (e.g., Shift+F should not be treated as input)
fn handle_admin_chat_input(
    app: &mut AppState,
    code: KeyCode,
    key_event: &crossterm::event::KeyEvent,
) -> Option<bool> {
    if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
        if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
            // Only allow input if chat input is enabled
            if app.admin_chat_input_enabled {
                // Don't treat Shift+F as input (it's used for finalization)
                if (code == KeyCode::Char('f') || code == KeyCode::Char('F'))
                    && key_event
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::SHIFT)
                {
                    return None; // Let Shift+F handler process it
                }
                // Don't treat Shift+I as input (it's used for toggling input)
                if (code == KeyCode::Char('i') || code == KeyCode::Char('I'))
                    && key_event
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::SHIFT)
                {
                    return None; // Let Shift+I handler process it
                }
                match code {
                    KeyCode::Char(c) => {
                        app.admin_chat_input.push(c);
                        return Some(true);
                    }
                    KeyCode::Backspace => {
                        app.admin_chat_input.pop();
                        return Some(true);
                    }
                    _ => {} // For other keys, continue to normal handling
                }
            }
        }
    }
    None
}

fn handle_user_order_chat_input(
    app: &mut AppState,
    code: KeyCode,
    key_event: &crossterm::event::KeyEvent,
) -> Option<bool> {
    if let Tab::User(UserTab::MyTrades) = app.active_tab {
        if matches!(app.mode, UiMode::UserMode(UserMode::Normal)) && app.order_chat_input_enabled {
            let has_shift = key_event
                .modifiers
                .contains(crossterm::event::KeyModifiers::SHIFT);
            if has_shift {
                // Let Shift+I/C/F/R/V/H be handled by shortcut logic
                if matches!(
                    code,
                    KeyCode::Char('i')
                        | KeyCode::Char('I')
                        | KeyCode::Char('c')
                        | KeyCode::Char('C')
                        | KeyCode::Char('f')
                        | KeyCode::Char('F')
                        | KeyCode::Char('r')
                        | KeyCode::Char('R')
                        | KeyCode::Char('v')
                        | KeyCode::Char('V')
                        | KeyCode::Char('h')
                        | KeyCode::Char('H')
                ) {
                    return None;
                }
            }
            match code {
                KeyCode::Char(c) => {
                    app.order_chat_input.push(c);
                    return Some(true);
                }
                KeyCode::Backspace => {
                    app.order_chat_input.pop();
                    return Some(true);
                }
                _ => {}
            }
        }
    }
    None
}

/// Handle clipboard copy for invoice
fn handle_clipboard_copy(invoice: String) -> bool {
    #[cfg(target_os = "linux")]
    {
        // On Linux, prefer arboard (system clipboard) but run it off the UI thread.
        // Some clipboard backends can emit warnings to stderr; silence stderr during the call
        // to avoid corrupting the TUI.
        std::thread::spawn(move || {
            let copy_result = {
                #[cfg(unix)]
                {
                    use std::os::unix::io::AsRawFd;
                    let saved_stderr = unsafe { libc::dup(libc::STDERR_FILENO) };
                    let devnull = std::fs::File::open("/dev/null");
                    if saved_stderr >= 0 {
                        if let Ok(devnull) = devnull {
                            unsafe {
                                let _ = libc::dup2(devnull.as_raw_fd(), libc::STDERR_FILENO);
                            }
                        }
                    }

                    let r = match arboard::Clipboard::new() {
                        Ok(mut clipboard) => clipboard.set_text(invoice),
                        Err(e) => Err(e),
                    };

                    if saved_stderr >= 0 {
                        unsafe {
                            let _ = libc::dup2(saved_stderr, libc::STDERR_FILENO);
                            let _ = libc::close(saved_stderr);
                        }
                    }
                    r
                }
                #[cfg(not(unix))]
                {
                    match arboard::Clipboard::new() {
                        Ok(mut clipboard) => clipboard.set_text(invoice),
                        Err(e) => Err(e),
                    }
                }
            };

            match copy_result {
                Ok(_) => log::info!("Invoice copied to clipboard"),
                Err(e) => log::warn!("Failed to copy invoice to clipboard: {}", e),
            }
        });
        true
    }

    // Non-Linux: clipboard ops can still block; run off UI thread.
    #[cfg(not(target_os = "linux"))]
    {
        std::thread::spawn(move || {
            let copy_result = match arboard::Clipboard::new() {
                Ok(mut clipboard) => clipboard.set_text(invoice),
                Err(e) => Err(e),
            };

            match copy_result {
                Ok(_) => {
                    log::info!("Invoice copied to clipboard");
                }
                Err(e) => {
                    log::warn!("Failed to copy invoice to clipboard: {}", e);
                }
            }
        });
        true
    }
}

/// Cycle through 3 buttons (Pay Buyer, Refund Seller, Exit) for dispute finalization
fn cycle_finalization_button(selected_button: &mut usize, direction: KeyCode, is_finalized: bool) {
    if is_finalized {
        // If finalized, only allow Exit button (button 2)
        *selected_button = 2;
        return;
    }

    // Normal navigation when not finalized
    if direction == KeyCode::Left {
        *selected_button = if *selected_button == 0 {
            2
        } else {
            *selected_button - 1
        };
    } else {
        *selected_button = if *selected_button == 2 {
            0
        } else {
            *selected_button + 1
        };
    }
}

fn read_clipboard_text_best_effort() -> Option<String> {
    match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
        Ok(t) => Some(t),
        Err(e) => {
            log::warn!("Failed to read clipboard text: {}", e);
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Main key event handler - dispatches to appropriate handlers
pub fn handle_key_event(
    key_event: KeyEvent,
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<Dispute>>>,
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    current_mostro_pubkey: &Arc<Mutex<PublicKey>>,
    order_result_tx: &UnboundedSender<OperationResult>,
    key_rotation_tx: &UnboundedSender<Result<Zeroizing<String>, String>>,
    seed_words_tx: &UnboundedSender<Result<Zeroizing<String>, String>>,
    mostro_info_tx: &UnboundedSender<MostroInfoFetchResult>,
    validate_range_amount: &dyn Fn(&mut TakeOrderState),
    admin_chat_keys: Option<&nostr_sdk::Keys>,
    save_attachment_tx: Option<&UnboundedSender<(String, ChatAttachment)>>,
    dm_subscription_tx: &UnboundedSender<OrderDmSubscriptionCmd>,
) -> Option<bool> {
    // Returns Some(true) to continue, Some(false) to break, None to continue normally
    let code = key_event.code;

    // Clear transient attachment toast on any key press
    app.attachment_toast = None;

    // Help popup (Ctrl+H): close on Esc, Enter, or Ctrl+H; restore previous mode so input state is preserved
    if let UiMode::HelpPopup(_, ref previous_mode) = &app.mode {
        if (key_event.modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('h'))
            || code == KeyCode::Esc
            || code == KeyCode::Enter
        {
            app.mode = (**previous_mode).clone();
            return Some(true);
        }
        return Some(true); // consume all other keys while help is open
    }

    // Settings instructions (Shift+H): close like help (also Shift+H toggles)
    if let UiMode::SettingsInstructionsPopup(_, ref previous_mode) = &app.mode {
        let shift_h = key_event.modifiers.contains(KeyModifiers::SHIFT)
            && matches!(code, KeyCode::Char('h') | KeyCode::Char('H'));
        if shift_h
            || (key_event.modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('h'))
            || code == KeyCode::Esc
            || code == KeyCode::Enter
        {
            app.mode = (**previous_mode).clone();
            return Some(true);
        }
        return Some(true);
    }

    // PayInvoice popup: allow scrolling the (wrapped) invoice text.
    if let UiMode::NewMessageNotification(_, Action::PayInvoice, ref mut invoice_state) = app.mode {
        match code {
            KeyCode::Up => {
                invoice_state.scroll_y = invoice_state.scroll_y.saturating_sub(1);
                return Some(true);
            }
            KeyCode::Down => {
                invoice_state.scroll_y = invoice_state.scroll_y.saturating_add(1);
                return Some(true);
            }
            KeyCode::PageUp => {
                invoice_state.scroll_y = invoice_state.scroll_y.saturating_sub(10);
                return Some(true);
            }
            KeyCode::PageDown => {
                invoice_state.scroll_y = invoice_state.scroll_y.saturating_add(10);
                return Some(true);
            }
            _ => {}
        }
    }

    // Observer tab paste fallback for terminals without bracketed paste (notably cmd.exe):
    // - Shift+Insert (classic Windows paste)
    // - Ctrl+Shift+V (Windows Terminal-style paste shortcut)
    // - Ctrl+V when the console delivers it as a key event
    if let Tab::Admin(AdminTab::Observer) = app.active_tab {
        let is_ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
        let is_shift = key_event.modifiers.contains(KeyModifiers::SHIFT);
        let is_paste_shortcut = match key_event.code {
            KeyCode::Insert => is_shift,
            KeyCode::Char('v') | KeyCode::Char('V') => is_ctrl,
            _ => false,
        } || (is_ctrl
            && is_shift
            && matches!(key_event.code, KeyCode::Char('v') | KeyCode::Char('V')));

        if is_paste_shortcut {
            if let Some(text) = read_clipboard_text_best_effort() {
                let filtered: String = text.chars().filter(|c| !c.is_control()).collect();
                if !filtered.is_empty() {
                    app.observer_shared_key_input.push_str(&filtered);
                    return Some(true);
                }
            }
        }
    }
    // Rate counterparty: 1..=5 stars (Left/Right or +/-).
    if let UiMode::RatingOrder(ref mut s) = app.mode {
        match code {
            KeyCode::Left => {
                s.selected_rating = s.selected_rating.saturating_sub(1).max(MIN_RATING);
                return Some(true);
            }
            KeyCode::Right => {
                s.selected_rating = (s.selected_rating + 1).min(MAX_RATING);
                return Some(true);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                s.selected_rating = (s.selected_rating + 1).min(MAX_RATING);
                return Some(true);
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                s.selected_rating = s.selected_rating.saturating_sub(1).max(MIN_RATING);
                return Some(true);
            }
            _ => {}
        }
    }

    // Save attachment popup: Up/Down to select, Enter to save, Esc to cancel
    if matches!(app.mode, UiMode::SaveAttachmentPopup(_)) {
        let dispute_id_key = app
            .admin_disputes_in_progress
            .get(app.selected_in_progress_idx)
            .map(|d| d.dispute_id.clone());
        let list_len = dispute_id_key
            .as_ref()
            .map(|id| get_visible_attachment_messages(app, id).len())
            .unwrap_or(0);
        let selected_idx = match &app.mode {
            UiMode::SaveAttachmentPopup(i) => *i,
            _ => 0,
        };
        match code {
            KeyCode::Esc => {
                app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
                return Some(true);
            }
            KeyCode::Up => {
                if selected_idx > 0 {
                    if let UiMode::SaveAttachmentPopup(ref mut idx) = app.mode {
                        *idx = selected_idx - 1;
                    }
                }
                return Some(true);
            }
            KeyCode::Down => {
                if list_len > 0 && selected_idx + 1 < list_len {
                    if let UiMode::SaveAttachmentPopup(ref mut idx) = app.mode {
                        *idx = selected_idx + 1;
                    }
                }
                return Some(true);
            }
            KeyCode::Enter => {
                if let (Some(tx), Some(dispute), Some(id)) = (
                    save_attachment_tx,
                    app.admin_disputes_in_progress
                        .get(app.selected_in_progress_idx),
                    dispute_id_key.as_ref(),
                ) {
                    let list = get_visible_attachment_messages(app, id);
                    if let Some(msg) = list.get(selected_idx) {
                        if let Some(att) = &msg.attachment {
                            let mut attachment = att.clone();
                            if attachment.decryption_key.is_none() {
                                if let (Some(admin_keys), Some(pk_str)) = (
                                    admin_chat_keys,
                                    match msg.sender {
                                        ChatSender::Buyer => dispute.buyer_pubkey.as_deref(),
                                        ChatSender::Seller => dispute.seller_pubkey.as_deref(),
                                        ChatSender::Admin => None,
                                    },
                                ) {
                                    if let Ok(sender_pk) = PublicKey::parse(pk_str) {
                                        if let Ok(shared) = crate::util::blossom::derive_shared_key(
                                            admin_keys, &sender_pk,
                                        ) {
                                            attachment.decryption_key = Some(shared.to_vec());
                                        }
                                    }
                                }
                            }
                            let _ = tx.send((dispute.dispute_id.clone(), attachment));
                        }
                    }
                }
                app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
                return Some(true);
            }
            _ => return Some(true), // consume other keys while popup is open
        }
    }

    // Observer save attachment popup: Up/Down to select, Enter to save, Esc to cancel
    if let UiMode::ObserverSaveAttachmentPopup(selected_idx) = app.mode {
        let list_len = app
            .observer_messages
            .iter()
            .filter(|m| m.attachment.is_some())
            .count();
        match code {
            KeyCode::Esc => {
                app.mode = UiMode::AdminMode(AdminMode::Normal);
                return Some(true);
            }
            KeyCode::Up => {
                if selected_idx > 0 {
                    app.mode = UiMode::ObserverSaveAttachmentPopup(selected_idx - 1);
                }
                return Some(true);
            }
            KeyCode::Down => {
                if list_len > 0 && selected_idx + 1 < list_len {
                    app.mode = UiMode::ObserverSaveAttachmentPopup(selected_idx + 1);
                }
                return Some(true);
            }
            KeyCode::Enter => {
                let attachments: Vec<&crate::ui::ChatAttachment> = app
                    .observer_messages
                    .iter()
                    .filter_map(|m| m.attachment.as_ref())
                    .collect();
                if let Some(att) = attachments.get(selected_idx) {
                    if let Some(tx) = save_attachment_tx {
                        let key_prefix: String =
                            app.observer_shared_key_input.chars().take(8).collect();
                        let id = format!("observer_{}", key_prefix);

                        // For Observer mode (pure P2P chats), attachment JSON often omits a `key`
                        // and expects decryption using the same shared key used for messages.
                        // If no explicit decryption_key was provided, derive it from the pasted
                        // shared key hex so the saved file is decrypted instead of left encrypted.
                        let mut att_clone = (*att).clone();
                        if att_clone.decryption_key.is_none() {
                            if let Some(keys) = crate::util::chat_utils::keys_from_shared_hex(
                                &app.observer_shared_key_input,
                            ) {
                                att_clone.decryption_key =
                                    Some(keys.secret_key().secret_bytes().to_vec());
                            }
                        }

                        let _ = tx.send((id, att_clone));
                    }
                }
                app.mode = UiMode::AdminMode(AdminMode::Normal);
                return Some(true);
            }
            _ => return Some(true),
        }
    }

    // Ctrl+H: open context-aware help popup when in normal/managing-dispute mode (store current mode to restore on close)
    if key_event.modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('h') {
        let can_open = matches!(
            app.mode,
            UiMode::Normal
                | UiMode::UserMode(UserMode::Normal)
                | UiMode::AdminMode(AdminMode::Normal)
                | UiMode::AdminMode(AdminMode::ManagingDispute)
        );
        if can_open {
            let previous = app.mode.clone();
            app.mode = UiMode::HelpPopup(app.active_tab, Box::new(previous));
            return Some(true);
        }
    }

    // Shift+H on Settings tab: explain every menu option (admin vs user text)
    if key_event.modifiers.contains(KeyModifiers::SHIFT)
        && matches!(code, KeyCode::Char('h') | KeyCode::Char('H'))
        && matches!(
            app.active_tab,
            Tab::Admin(AdminTab::Settings) | Tab::User(UserTab::Settings)
        )
    {
        let can_open = matches!(
            app.mode,
            UiMode::Normal
                | UiMode::UserMode(UserMode::Normal)
                | UiMode::AdminMode(AdminMode::Normal)
        );
        if can_open {
            let previous = app.mode.clone();
            app.mode = UiMode::SettingsInstructionsPopup(app.user_role, Box::new(previous));
            return Some(true);
        }
    }

    // Ctrl+S: open save attachment popup (list of attachments) or do nothing if none
    if key_event.modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('s') {
        if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
            if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
                if let Some(dispute) = app
                    .admin_disputes_in_progress
                    .get(app.selected_in_progress_idx)
                {
                    let list = get_visible_attachment_messages(app, &dispute.dispute_id);
                    if !list.is_empty() {
                        app.mode = UiMode::SaveAttachmentPopup(0);
                        return Some(true);
                    }
                }
            }
        }
        // Observer tab: open save attachment popup for observer messages
        if let Tab::Admin(AdminTab::Observer) = app.active_tab {
            let has_attachments = app.observer_messages.iter().any(|m| m.attachment.is_some());
            if has_attachments {
                app.mode = UiMode::ObserverSaveAttachmentPopup(0);
                return Some(true);
            }
        }
    }

    // Handle invoice input first (before other key handling)
    if let UiMode::NewMessageNotification(_, Action::AddInvoice, ref mut invoice_state) = app.mode {
        if invoice_state.focused && handle_invoice_input(code, invoice_state) {
            return Some(true); // Skip further processing
        }
    }

    // Handle key input for shared settings popups and admin popups
    if matches!(
        app.mode,
        UiMode::AddMostroPubkey(_)
            | UiMode::AddRelay(_)
            | UiMode::AddCurrency(_)
            | UiMode::AdminMode(AdminMode::AddSolver(_))
            | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
    ) {
        let key_state = match &mut app.mode {
            UiMode::AddMostroPubkey(ref mut ks) => Some(ks),
            UiMode::AddRelay(ref mut ks) => Some(ks),
            UiMode::AddCurrency(ref mut ks) => Some(ks),
            UiMode::AdminMode(AdminMode::AddSolver(ref mut ks)) => Some(ks),
            UiMode::AdminMode(AdminMode::SetupAdminKey(ref mut ks)) => Some(ks),
            _ => None,
        };

        if let Some(ks) = key_state {
            if ks.focused && handle_key_input(code, ks) {
                return Some(true); // Skip further processing
            }
        }
    }

    // Clear "copied" indicator when any key is pressed (except C which sets it)
    if let UiMode::NewMessageNotification(_, Action::PayInvoice, ref mut invoice_state) = app.mode {
        if code != KeyCode::Char('c') && code != KeyCode::Char('C') {
            invoice_state.copied_to_clipboard = false;
        }
    }

    // Handle Shift+F and Shift+I BEFORE other key processing to ensure they're not intercepted
    // Check these BEFORE handle_admin_chat_input to prevent interception
    if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
        let has_shift = key_event
            .modifiers
            .contains(crossterm::event::KeyModifiers::SHIFT);

        // Handle Shift+F to open dispute finalization popup (check this first)
        if has_shift && (code == KeyCode::Char('f') || code == KeyCode::Char('F')) {
            // Only handle if we're in ManagingDispute mode
            if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
                // Open finalization popup if a dispute is selected
                if let Some(selected_dispute) = app
                    .admin_disputes_in_progress
                    .get(app.selected_in_progress_idx)
                {
                    if let Ok(dispute_id) = uuid::Uuid::parse_str(&selected_dispute.dispute_id) {
                        app.mode = UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization {
                            dispute_id,
                            // Default to first button (Pay Buyer)
                            selected_button_index: 0,
                        });
                        return Some(true);
                    }
                }
            }
        }

        // Handle Shift+C to toggle between InProgress and Finalized filters
        if has_shift && (code == KeyCode::Char('c') || code == KeyCode::Char('C')) {
            // Toggle filter between InProgress and Finalized
            app.dispute_filter = match app.dispute_filter {
                DisputeFilter::InProgress => DisputeFilter::Finalized,
                DisputeFilter::Finalized => DisputeFilter::InProgress,
            };
            // Reset selection index when switching filters
            app.selected_in_progress_idx = 0;
            return Some(true);
        }

        // Handle Shift+I to toggle chat input enabled/disabled
        if has_shift
            && (code == KeyCode::Char('i') || code == KeyCode::Char('I'))
            && matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute))
        {
            app.admin_chat_input_enabled = !app.admin_chat_input_enabled;
            return Some(true);
        }
    }

    if let Tab::User(UserTab::MyTrades) = app.active_tab {
        let has_shift = key_event
            .modifiers
            .contains(crossterm::event::KeyModifiers::SHIFT);
        let has_ctrl = key_event
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL);
        if code == KeyCode::Delete {
            if has_ctrl {
                app.mode = UiMode::ConfirmBulkDeleteHistory(true);
                return Some(true);
            }
            if let Some((order_id, status)) = resolve_selected_mytrades_order_status(app) {
                if is_terminal_order_status(status) {
                    app.mode = UiMode::ConfirmDeleteHistoryOrder(order_id, true);
                } else {
                    app.mode = UiMode::OperationResult(OperationResult::Info(
                        "Delete is only available for terminal orders.".to_string(),
                    ));
                }
                return Some(true);
            }
        }
        if has_shift {
            match code {
                KeyCode::Char('i') | KeyCode::Char('I') => {
                    app.order_chat_input_enabled = !app.order_chat_input_enabled;
                    return Some(true);
                }
                KeyCode::Char('h') | KeyCode::Char('H') => {
                    let can_open = matches!(
                        app.mode,
                        UiMode::Normal | UiMode::UserMode(UserMode::Normal)
                    );
                    if can_open {
                        let previous = app.mode.clone();
                        app.mode = UiMode::HelpPopup(app.active_tab, Box::new(previous));
                        return Some(true);
                    }
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    if let Some((order_id, status)) = resolve_selected_mytrades_order_status(app) {
                        if is_terminal_order_status(status) {
                            app.mode = UiMode::OperationResult(OperationResult::Info(
                                "Cancel is disabled for terminal orders.".to_string(),
                            ));
                            return Some(true);
                        }
                        let msg = crate::ui::constants::HELP_MY_TRADES_CANCEL_MSG;
                        let view_state = build_order_action_view_state(
                            order_id,
                            Action::Cancel,
                            msg.to_string(),
                        );
                        app.mode = UiMode::ViewingMessage(view_state);
                        return Some(true);
                    }
                }
                KeyCode::Char('f') | KeyCode::Char('F') => {
                    if let Some((order_id, status)) = resolve_selected_mytrades_order_status(app) {
                        if is_terminal_order_status(status) {
                            app.mode = UiMode::OperationResult(OperationResult::Info(
                                "FiatSent is disabled for terminal orders.".to_string(),
                            ));
                            return Some(true);
                        }
                        let msg = crate::ui::constants::HELP_MY_TRADES_FIAT_SENT_MSG;
                        let view_state = build_order_action_view_state(
                            order_id,
                            Action::FiatSent,
                            msg.to_string(),
                        );
                        app.mode = UiMode::ViewingMessage(view_state);
                        return Some(true);
                    }
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    if let Some((order_id, status)) = resolve_selected_mytrades_order_status(app) {
                        if is_terminal_order_status(status) {
                            app.mode = UiMode::OperationResult(OperationResult::Info(
                                "Release is disabled for terminal orders.".to_string(),
                            ));
                            return Some(true);
                        }
                        let msg = crate::ui::constants::HELP_MY_TRADES_RELEASE_MSG;
                        let view_state = build_order_action_view_state(
                            order_id,
                            Action::Release,
                            msg.to_string(),
                        );
                        app.mode = UiMode::ViewingMessage(view_state);
                        return Some(true);
                    }
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    if let Some(state) = build_rating_state_for_mytrades(app, 5) {
                        app.mode = UiMode::RatingOrder(state);
                        return Some(true);
                    }
                }
                _ => {}
            }
        }
    }

    // Check if we're in admin chat input mode FIRST - this takes priority over all other key handling
    // (except invoice and key input which are handled earlier)
    // Note: Shift+F and Shift+I are handled before this, so they won't be intercepted
    if let Some(result) = handle_admin_chat_input(app, code, &key_event) {
        return Some(result);
    }
    if let Some(result) = handle_user_order_chat_input(app, code, &key_event) {
        return Some(result);
    }

    // Observer tab: handle all character and backspace input early so y/n/m/c etc. go to the shared key input.
    // Skip when a modal result popup is active so we don't edit inputs behind the overlay.
    if let Tab::Admin(AdminTab::Observer) = app.active_tab {
        if !matches!(app.mode, UiMode::OperationResult(_)) {
            let is_ctrl = key_event
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL);
            if !is_ctrl {
                match code {
                    KeyCode::Char(c) => {
                        app.observer_shared_key_input.push(c);
                        return Some(true);
                    }
                    KeyCode::Backspace => {
                        app.observer_shared_key_input.pop();
                        return Some(true);
                    }
                    _ => {}
                }
            }
        }
    }

    match code {
        KeyCode::Left | KeyCode::Right => {
            // Handle Left/Right for button selection in confirmation popups
            match &mut app.mode {
                UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, ref mut selected_button))
                | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, ref mut selected_button))
                | UiMode::AdminMode(AdminMode::ConfirmTakeDispute(_, ref mut selected_button))
                | UiMode::ConfirmMostroPubkey(_, ref mut selected_button)
                | UiMode::ConfirmRelay(_, ref mut selected_button)
                | UiMode::ConfirmCurrency(_, ref mut selected_button)
                | UiMode::ConfirmClearCurrencies(ref mut selected_button)
                | UiMode::ConfirmDeleteHistoryOrder(_, ref mut selected_button)
                | UiMode::ConfirmBulkDeleteHistory(ref mut selected_button)
                | UiMode::ConfirmGenerateNewKeys(ref mut selected_button)
                | UiMode::ConfirmExit(ref mut selected_button) => {
                    *selected_button = !*selected_button; // Toggle between YES and NO
                    return Some(true);
                }
                UiMode::ViewingMessage(ref mut view_state) => {
                    match &mut view_state.button_selection {
                        ViewingMessageButtonSelection::Two { yes_selected } => {
                            if code == KeyCode::Left {
                                *yes_selected = true;
                            } else if code == KeyCode::Right {
                                *yes_selected = false;
                            }
                            if matches!(view_state.action, Action::CooperativeCancelInitiatedByPeer)
                            {
                                // #region agent log
                                debug_log_keys(
                                    "H7",
                                    "src/ui/key_handler/mod.rs:viewing_message_lr_toggle",
                                    "Toggled peer cooperative-cancel confirmation selection",
                                    serde_json::json!({
                                        "key": format!("{:?}", code),
                                        "action": format!("{:?}", view_state.action),
                                        "yes_selected": *yes_selected,
                                        "order_id": view_state.order_id.map(|id| id.to_string()),
                                    }),
                                );
                                // #endregion
                            }
                        }
                        ViewingMessageButtonSelection::Three { selected } => {
                            if code == KeyCode::Left {
                                *selected = (*selected + 2) % 3;
                            } else {
                                *selected = (*selected + 1) % 3;
                            }
                        }
                    }
                    return Some(true);
                }
                UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization {
                    dispute_id,
                    ref mut selected_button_index,
                }) => {
                    // Check if dispute is finalized to skip disabled buttons
                    let dispute_is_finalized = app
                        .admin_disputes_in_progress
                        .iter()
                        .find(|d| d.dispute_id == dispute_id.to_string())
                        .and_then(is_dispute_finalized)
                        .unwrap_or(false);

                    cycle_finalization_button(selected_button_index, code, dispute_is_finalized);
                    return Some(true);
                }
                _ => {}
            }
            handle_navigation(code, app, orders, disputes);
            Some(true)
        }
        KeyCode::Up | KeyCode::Down => {
            // Handle chat message navigation when input is disabled (Disputes in Progress)
            if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
                if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                    if !app.admin_chat_input_enabled {
                        let dispute_id_key = app
                            .admin_disputes_in_progress
                            .get(app.selected_in_progress_idx)
                            .map(|d| d.dispute_id.clone());
                        if let Some(dispute_id_key) = dispute_id_key {
                            if chat_helpers::navigate_chat_messages(app, &dispute_id_key, code) {
                                return Some(true);
                            }
                        }
                    }
                }
            }

            // Observer tab: use Up/Down to scroll the chat vertically
            if let Tab::Admin(AdminTab::Observer) = app.active_tab {
                match code {
                    KeyCode::Up => {
                        app.observer_scrollview_state.scroll_up();
                        return Some(true);
                    }
                    KeyCode::Down => {
                        app.observer_scrollview_state.scroll_down();
                        return Some(true);
                    }
                    _ => {}
                }
            }

            handle_navigation(code, app, orders, disputes);
            Some(true)
        }
        KeyCode::PageUp | KeyCode::PageDown => {
            // Handle chat scrolling in ManagingDispute mode using ListState
            if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
                if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                    let dispute_id_key = app
                        .admin_disputes_in_progress
                        .get(app.selected_in_progress_idx)
                        .map(|d| d.dispute_id.clone());
                    if let Some(dispute_id_key) = dispute_id_key {
                        if chat_helpers::scroll_chat_messages(app, &dispute_id_key, code) {
                            return Some(true);
                        }
                    }
                }
            }

            // Observer tab: PageUp/PageDown scroll the observer chat
            if let Tab::Admin(AdminTab::Observer) = app.active_tab {
                match code {
                    KeyCode::PageUp => {
                        app.observer_scrollview_state.scroll_page_up();
                        return Some(true);
                    }
                    KeyCode::PageDown => {
                        app.observer_scrollview_state.scroll_page_down();
                        return Some(true);
                    }
                    _ => {}
                }
            }

            Some(true)
        }
        KeyCode::Tab | KeyCode::BackTab => {
            handle_tab_navigation(code, app);
            Some(true)
        }
        KeyCode::Enter => {
            let ctx = EnterKeyContext {
                orders,
                disputes,
                pool,
                client,
                mostro_pubkey,
                current_mostro_pubkey,
                order_result_tx,
                key_rotation_tx,
                seed_words_tx,
                mostro_info_tx,
                mostro_info: app.mostro_info.clone(),
                admin_chat_keys,
                dm_subscription_tx,
            };
            let should_continue = handle_enter_key(app, &ctx);
            Some(should_continue)
        }
        KeyCode::Esc => {
            let should_continue = handle_esc_key(app);
            Some(should_continue)
        }
        KeyCode::End => {
            // Jump to bottom of chat (latest messages)
            if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
                if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                    let dispute_id_key = app
                        .admin_disputes_in_progress
                        .get(app.selected_in_progress_idx)
                        .map(|d| d.dispute_id.clone());
                    if let Some(dispute_id_key) = dispute_id_key {
                        if chat_helpers::jump_to_chat_bottom(app, &dispute_id_key) {
                            return Some(true);
                        }
                    }
                }
            }
            Some(true)
        }
        // 'q' key removed - use Exit tab instead.
        // For confirmations, prefer using Enter on the focused button instead of 'y'/'n'.
        KeyCode::Char('n') | KeyCode::Char('N') => {
            handle_cancel_key(app);
            Some(true)
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            // In Observer tab, Ctrl+C clears inputs and decrypted content
            if let (Tab::Admin(AdminTab::Observer), true) = (
                app.active_tab,
                key_event
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL),
            ) {
                app.clear_observer_secrets();
                return Some(true);
            }

            // Handle copy invoice for PayInvoice notifications
            if let UiMode::NewMessageNotification(
                ref notification,
                Action::PayInvoice,
                ref mut invoice_state,
            ) = app.mode
            {
                if let Some(invoice) = &notification.invoice {
                    invoice_state.copied_to_clipboard = handle_clipboard_copy(invoice.clone());
                }
            }
            Some(true)
        }
        KeyCode::Char(_) | KeyCode::Backspace => {
            // Observer tab input is handled early in handle_key_event
            // Chat input is handled at the top (takes priority)
            // This handles form inputs and other character entry
            handle_char_input(code, app, validate_range_amount);
            if code == KeyCode::Backspace {
                handle_backspace(app, validate_range_amount);
            }
            Some(true)
        }
        _ => None,
    }
}
