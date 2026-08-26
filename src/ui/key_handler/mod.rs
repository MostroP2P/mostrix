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
    helpers::{
        active_order_chat_list_snapshot, get_order_attachment_messages,
        get_visible_attachment_messages, is_dispute_finalized, selected_filtered_dispute,
    },
    send_attachment_picker::{
        close_user_send_attachment_picker, explorer_selection_is_sendable_file,
        open_user_send_attachment_picker,
    },
    AdminMode, AdminTab, AppState, ChatAttachment, ChatSender, DisputeFilter,
    InvoiceNotificationActionSelection, LnAddressVerifyResult, MostroInfoFetchResult,
    OperationResult, Tab, TakeOrderState, UiMode, UserChatChannel, UserMode, UserTab,
    ViewingMessageButtonSelection,
};
use crate::util::{MostroInstanceInfo, OrderDmSubscriptionCmd, SendOrderAttachmentJob};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use zeroize::Zeroizing;

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
    pub ln_address_result_tx: &'a UnboundedSender<LnAddressVerifyResult>,
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

/// Mostro only accepts a dispute once the trade is under way: sats locked and the fiat leg
/// in flight. `None` (status not yet synced locally) is let through so a lagging local DB
/// never blocks the user — Mostro answers `CantDo` if it disagrees.
fn can_dispute_order_status(status: Option<Status>) -> bool {
    match status {
        None => true,
        Some(s) => matches!(s, Status::Active | Status::FiatSent),
    }
}

/// Resolve what a Shift+C/F/R order-action shortcut on My Trades should do,
/// or `None` to ignore the key.
///
/// Same duplicate-submit guard as [`dispute_shortcut_next_mode`]: after a
/// confirmation, the send task switches the UI into a waiting mode while the
/// selected row still shows an actionable status, so without the
/// `user_my_trades_interactive()` check a second press could open another
/// confirmation and submit the action to Mostro twice.
fn trade_action_shortcut_next_mode(
    mode: &UiMode,
    selected: Option<(uuid::Uuid, Option<Status>)>,
    action: Action,
) -> Option<UiMode> {
    if !mode.user_my_trades_interactive() {
        return None;
    }
    let (order_id, status) = selected?;
    let (label, confirm_msg) = match action {
        Action::Cancel => ("Cancel", crate::ui::constants::HELP_MY_TRADES_CANCEL_MSG),
        Action::FiatSent => (
            "FiatSent",
            crate::ui::constants::HELP_MY_TRADES_FIAT_SENT_MSG,
        ),
        Action::Release => ("Release", crate::ui::constants::HELP_MY_TRADES_RELEASE_MSG),
        _ => return None,
    };
    if is_terminal_order_status(status) {
        return Some(UiMode::operation_result(OperationResult::Info(format!(
            "{label} is disabled for terminal orders."
        ))));
    }
    Some(UiMode::ViewingMessage(build_order_action_view_state(
        order_id,
        action,
        confirm_msg.to_string(),
    )))
}

/// Resolve what Shift+D on My Trades should do, or `None` to ignore the key.
///
/// The `user_my_trades_interactive()` guard is what prevents a duplicate
/// submit: after the first confirmation, `spawn_dispute()` switches the UI
/// into a waiting mode while the local row still reads Active/FiatSent, so
/// without it a second Shift+D would start a second remote dispute flow.
fn dispute_shortcut_next_mode(
    mode: &UiMode,
    selected: Option<(uuid::Uuid, Option<Status>)>,
) -> Option<UiMode> {
    if !mode.user_my_trades_interactive() {
        return None;
    }
    let (order_id, status) = selected?;
    if matches!(status, Some(Status::Dispute)) {
        return Some(UiMode::operation_result(OperationResult::Info(
            "This order already has an open dispute.".to_string(),
        )));
    }
    if !can_dispute_order_status(status) {
        return Some(UiMode::operation_result(OperationResult::Info(
            crate::ui::constants::HELP_MY_TRADES_DISPUTE_UNAVAILABLE.to_string(),
        )));
    }
    let view_state = build_order_action_view_state(
        order_id,
        Action::Dispute,
        crate::ui::constants::HELP_MY_TRADES_DISPUTE_MSG.to_string(),
    );
    Some(UiMode::ViewingMessage(view_state))
}

// Re-export public functions
pub use async_tasks::{
    apply_pending_fetch_scheduler_reload, apply_pending_key_reload, apply_pending_runtime_reloads,
    create_app_channels, reload_runtime_session_after_reconnect, respawn_chat_listener,
    respawn_trade_dm_listener, spawn_refresh_mostro_info_task, AppChannels,
    RuntimeReconnectContext,
};
pub use enter_handlers::handle_enter_key;
pub use esc_handlers::handle_esc_key;
pub use form_input::{handle_backspace, handle_char_input, is_creating_order_text_input};
pub use input_helpers::{handle_invoice_input, handle_key_input};
pub use navigation::{handle_navigation, handle_tab_navigation};
pub use settings::handle_mode_switch;
pub use validation::{
    hex_pubkey_to_npub, hex_seckey_to_nsec, normalize_mostro_pubkey, validate_currency,
    validate_mostro_pubkey, validate_npub, validate_relay,
};

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
                // Don't treat Shift+R as input (recover missing taken disputes)
                if (code == KeyCode::Char('r') || code == KeyCode::Char('R'))
                    && key_event
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::SHIFT)
                {
                    return None; // Let Shift+R handler process it
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
        if app.mode.user_my_trades_interactive() && app.order_chat_input_enabled {
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

/// Reset the Shift+K Shared key disclosure popup's "copied" indicator on any
/// key other than an unmodified `c`/`C`. Ctrl+C (Observer clear-all / generic
/// clear shortcuts elsewhere) must also clear the indicator, so it is treated
/// the same as any other non-copy key. Mirrors the invoice-copy indicator
/// reset above.
fn reset_disclosure_copied_indicator(mode: &mut UiMode, key_event: &KeyEvent) {
    if let UiMode::OperationResult(ref mut result) = mode {
        if let OperationResult::ConversationDisclosure {
            copied_to_clipboard,
            ..
        } = result.as_mut()
        {
            let is_unmodified_copy_key =
                matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('C'))
                    && !key_event.modifiers.contains(KeyModifiers::CONTROL);
            if !is_unmodified_copy_key {
                *copied_to_clipboard = false;
            }
        }
    }
}

/// Copy the disclosed Shared key (`K_conv`) to the clipboard when the Shift+K
/// disclosure popup is open, updating its "copied" indicator.
///
/// Only `conv_hex` (the Shared key) is ever copied — the signing key itself
/// is never disclosed. Returns `true` if the popup was open and handled the key.
fn copy_disclosed_shared_key_if_open(mode: &mut UiMode) -> bool {
    copy_disclosed_shared_key_if_open_with(mode, handle_clipboard_copy)
}

/// Testable core of [`copy_disclosed_shared_key_if_open`]; `copy_fn` is injected
/// so tests can assert on both success and failure without touching the real
/// system clipboard.
fn copy_disclosed_shared_key_if_open_with(
    mode: &mut UiMode,
    copy_fn: impl FnOnce(String) -> bool,
) -> bool {
    if let UiMode::OperationResult(ref mut result) = mode {
        if let OperationResult::ConversationDisclosure {
            conv_hex,
            copied_to_clipboard,
        } = result.as_mut()
        {
            *copied_to_clipboard = copy_fn(conv_hex.clone());
            return true;
        }
    }
    false
}

/// Silence stderr while running a clipboard operation so backend warnings do
/// not corrupt the TUI.
#[cfg(unix)]
fn with_stderr_silenced<T>(operation: impl FnOnce() -> T) -> T {
    use std::os::unix::io::AsRawFd;

    let saved_stderr = unsafe { libc::dup(libc::STDERR_FILENO) };
    if saved_stderr >= 0 {
        if let Ok(devnull) = std::fs::File::open("/dev/null") {
            unsafe {
                let _ = libc::dup2(devnull.as_raw_fd(), libc::STDERR_FILENO);
            }
        }
    }

    let result = operation();

    if saved_stderr >= 0 {
        unsafe {
            let _ = libc::dup2(saved_stderr, libc::STDERR_FILENO);
            let _ = libc::close(saved_stderr);
        }
    }

    result
}

/// Linux/X11/Wayland clipboard ownership is held by the process that wrote the
/// selection; drop the `Clipboard` and the paste target may see empty data. Run
/// the initial write on a background thread, report its result to the caller,
/// then keep that thread alive with `SetExtLinux::wait` until another app
/// replaces the selection (same pattern as the pre-#134 invoice copy path).
#[cfg(target_os = "linux")]
fn linux_clipboard_copy_worker(text: String, result_tx: std::sync::mpsc::Sender<bool>) {
    use arboard::SetExtLinux;

    let copy_result = {
        #[cfg(unix)]
        {
            with_stderr_silenced(|| match arboard::Clipboard::new() {
                Ok(mut clipboard) => {
                    let write_result = clipboard.set().text(text.clone());
                    let _ = result_tx.send(write_result.is_ok());
                    if write_result.is_ok() {
                        let _ = clipboard.set().wait().text(text);
                    }
                    write_result
                }
                Err(e) => {
                    let _ = result_tx.send(false);
                    Err(e)
                }
            })
        }
        #[cfg(not(unix))]
        {
            match arboard::Clipboard::new() {
                Ok(mut clipboard) => {
                    let write_result = clipboard.set().text(text.clone());
                    let _ = result_tx.send(write_result.is_ok());
                    if write_result.is_ok() {
                        let _ = clipboard.set().wait().text(text);
                    }
                    write_result
                }
                Err(e) => {
                    let _ = result_tx.send(false);
                    Err(e)
                }
            }
        }
    };

    match copy_result {
        Ok(_) => log::info!("Copied to clipboard"),
        Err(e) => log::warn!("Failed to copy to clipboard: {}", e),
    }
}

/// Handle clipboard copy for text (invoice, Shared key, etc.)
///
/// Returns whether the write actually succeeded — `copied_to_clipboard` at the
/// call site should only be set to `true` when this returns `true`, not merely
/// because a copy attempt was made. On Linux the write runs on a background
/// thread that keeps serving the selection after the result is reported.
fn handle_clipboard_copy(text: String) -> bool {
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || linux_clipboard_copy_worker(text, tx));
        rx.recv().unwrap_or(false)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let copy_result = {
            #[cfg(unix)]
            {
                with_stderr_silenced(|| {
                    arboard::Clipboard::new().and_then(|mut c| c.set_text(text))
                })
            }
            #[cfg(not(unix))]
            {
                arboard::Clipboard::new().and_then(|mut c| c.set_text(text))
            }
        };

        match copy_result {
            Ok(_) => {
                log::info!("Copied to clipboard");
                true
            }
            Err(e) => {
                log::warn!("Failed to copy to clipboard: {}", e);
                false
            }
        }
    }
}

/// Cycle bond slash options (Up/Down) in the picker popup.
fn cycle_bond_slash_choice(selected_choice_index: &mut usize, direction: KeyCode) {
    let len = crate::util::order_utils::BondSlashChoice::ALL.len();
    if direction == KeyCode::Up {
        *selected_choice_index = selected_choice_index.saturating_sub(1);
    } else {
        *selected_choice_index = (*selected_choice_index + 1).min(len.saturating_sub(1));
    }
}

/// Cycle finalize popup buttons (Pay Buyer, Refund Seller, optional Bond slash).
fn cycle_finalization_button(
    selected_button: &mut usize,
    direction: KeyCode,
    is_finalized: bool,
    bond_ui_enabled: bool,
) {
    let max_index = if bond_ui_enabled { 2 } else { 1 };

    if is_finalized {
        if bond_ui_enabled {
            *selected_button = 2;
        }
        return;
    }

    if direction == KeyCode::Left {
        *selected_button = if *selected_button == 0 {
            max_index
        } else {
            selected_button.saturating_sub(1)
        };
    } else {
        *selected_button = if *selected_button >= max_index {
            0
        } else {
            selected_button.saturating_add(1)
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

/// Mouse right-click paste fallback for AddInvoice notification popup.
///
/// Returns `true` when the event is fully handled and should be consumed by the caller.
pub fn handle_mouse_invoice_paste_fallback(event: &Event, app: &mut AppState) -> bool {
    let Event::Mouse(mouse_event) = event else {
        return false;
    };
    if !matches!(mouse_event.kind, MouseEventKind::Down(MouseButton::Right)) {
        return false;
    }
    log::debug!(
        "Detected right-click mouse event at x={}, y={}",
        mouse_event.column,
        mouse_event.row
    );
    let UiMode::NewMessageNotification(
        _,
        Action::AddInvoice | Action::AddBondInvoice,
        ref mut invoice_state,
    ) = app.mode
    else {
        return false;
    };
    if !invoice_state.focused {
        return false;
    }
    if let Some(text) = read_clipboard_text_best_effort() {
        let filtered_text: String = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect();
        if !filtered_text.is_empty() {
            log::debug!(
                "Right-click paste fallback appended {} chars to AddInvoice input",
                filtered_text.chars().count()
            );
            invoice_state.invoice_input.push_str(&filtered_text);
            invoice_state.just_pasted = true;
        } else {
            log::debug!("Right-click paste fallback found only control characters");
        }
    } else {
        log::debug!("Right-click paste fallback could not read clipboard text");
    }
    true
}

fn is_paste_shortcut(key_event: &KeyEvent) -> bool {
    let is_ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
    let is_shift = key_event.modifiers.contains(KeyModifiers::SHIFT);
    match key_event.code {
        KeyCode::Insert => is_shift,
        KeyCode::Char('v') | KeyCode::Char('V') => is_ctrl,
        _ => false,
    }
}

fn update_invoice_notification_action_selection(
    code: KeyCode,
    invoice_state: &mut crate::ui::InvoiceInputState,
) -> bool {
    match code {
        KeyCode::Left => {
            invoice_state.action_selection = InvoiceNotificationActionSelection::Primary;
            true
        }
        KeyCode::Right => {
            invoice_state.action_selection = InvoiceNotificationActionSelection::Cancel;
            true
        }
        _ => false,
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
    ln_address_result_tx: &UnboundedSender<LnAddressVerifyResult>,
    key_rotation_tx: &UnboundedSender<Result<Zeroizing<String>, String>>,
    seed_words_tx: &UnboundedSender<Result<Zeroizing<String>, String>>,
    mostro_info_tx: &UnboundedSender<MostroInfoFetchResult>,
    validate_range_amount: &dyn Fn(&mut TakeOrderState),
    admin_chat_keys: Option<&Keys>,
    save_attachment_tx: Option<&UnboundedSender<(String, ChatAttachment)>>,
    send_order_attachment_tx: Option<&UnboundedSender<SendOrderAttachmentJob>>,
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

    // PayInvoice / PayBondInvoice popup: allow scrolling the (wrapped) invoice text.
    if let UiMode::NewMessageNotification(
        _,
        Action::PayInvoice | Action::PayBondInvoice,
        ref mut invoice_state,
    ) = app.mode
    {
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

    // AddInvoice popup paste fallback for terminals without bracketed paste support.
    if let UiMode::NewMessageNotification(
        _,
        Action::AddInvoice | Action::AddBondInvoice,
        ref mut invoice_state,
    ) = app.mode
    {
        if is_paste_shortcut(&key_event) {
            if let Some(text) = read_clipboard_text_best_effort() {
                let filtered_text: String = text.chars().filter(|c| !c.is_control()).collect();
                if !filtered_text.is_empty() {
                    invoice_state.invoice_input.push_str(&filtered_text);
                    invoice_state.just_pasted = true;
                    return Some(true);
                }
            }
        }
    }

    // Observer tab paste fallback for terminals without bracketed paste (notably cmd.exe).
    if app.observer_inputs_editable() && is_paste_shortcut(&key_event) {
        if let Some(text) = read_clipboard_text_best_effort() {
            let filtered: String = text.chars().filter(|c| !c.is_control()).collect();
            if !filtered.is_empty() {
                app.observer_shared_key_input.push_str(&filtered);
                return Some(true);
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
        let selected_dispute = selected_filtered_dispute(app);
        let dispute_id_key = selected_dispute.as_ref().map(|d| d.dispute_id.clone());
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
                    selected_dispute.as_ref(),
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

    // User send attachment file picker: explorer navigation + Enter to send, Esc to cancel
    if let UiMode::UserSendAttachmentPicker(ref pinned_order_id) = app.mode {
        match code {
            KeyCode::Esc => {
                close_user_send_attachment_picker(app);
                return Some(true);
            }
            KeyCode::Enter => {
                let sendable = app
                    .user_send_attachment_explorer
                    .as_ref()
                    .map(|ex| explorer_selection_is_sendable_file(ex.current()))
                    .unwrap_or(false);
                if sendable {
                    if let (Some(explorer), Some(tx)) = (
                        app.user_send_attachment_explorer.as_ref(),
                        send_order_attachment_tx,
                    ) {
                        let path = explorer.current().path.clone();
                        let order_id = pinned_order_id.clone();
                        let _ = tx.send(SendOrderAttachmentJob::FromPath {
                            order_id: order_id.clone(),
                            path,
                        });
                        app.sending_attachment_order_id = Some(order_id);
                    }
                    close_user_send_attachment_picker(app);
                }
                return Some(true);
            }
            _ => {
                if let Some(explorer) = app.user_send_attachment_explorer.as_mut() {
                    let event = Event::Key(key_event);
                    if let Err(e) = explorer.handle(&event) {
                        log::warn!("send attachment picker: {}", e);
                    }
                }
                return Some(true);
            }
        }
    }

    // User order chat save attachment popup: Up/Down to select, Enter to save, Esc to cancel
    if let UiMode::UserSaveAttachmentPopup(ref pinned_order_id, selected_idx) = app.mode {
        let list_len = get_order_attachment_messages(app, pinned_order_id).len();
        match code {
            KeyCode::Esc => {
                app.mode = UiMode::UserMode(UserMode::Normal);
                return Some(true);
            }
            KeyCode::Up => {
                if selected_idx > 0 {
                    if let UiMode::UserSaveAttachmentPopup(_, ref mut idx) = app.mode {
                        *idx = selected_idx - 1;
                    }
                }
                return Some(true);
            }
            KeyCode::Down => {
                if list_len > 0 && selected_idx + 1 < list_len {
                    if let UiMode::UserSaveAttachmentPopup(_, ref mut idx) = app.mode {
                        *idx = selected_idx + 1;
                    }
                }
                return Some(true);
            }
            KeyCode::Enter => {
                if let Some(tx) = save_attachment_tx {
                    let list = get_order_attachment_messages(app, pinned_order_id);
                    if let Some(msg) = list.get(selected_idx) {
                        if let Some(att) = &msg.attachment {
                            let mut attachment = att.clone();
                            let order_id = pinned_order_id.clone();
                            let pool = pool.clone();
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                if attachment.decryption_key.is_none() {
                                    if let Ok(order) =
                                        crate::models::Order::get_by_id(&pool, &order_id).await
                                    {
                                        attachment.decryption_key =
                                            crate::util::chat_utils::order_chat_decryption_key_bytes(
                                                &order,
                                            );
                                    }
                                }
                                let _ = tx.send((order_id, attachment));
                            });
                        }
                    }
                }
                app.mode = UiMode::UserMode(UserMode::Normal);
                return Some(true);
            }
            _ => return Some(true),
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

                        // Observer holds K_conv only; use it as the ChaCha key when the
                        // attachment JSON omitted an inline key.
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
                if let Some(dispute) = selected_filtered_dispute(app) {
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
        if let Tab::User(UserTab::MyTrades) = app.active_tab {
            if app.mode.user_my_trades_interactive()
                && app.active_user_chat_channel == UserChatChannel::Peer
            {
                if let Some(row) =
                    active_order_chat_list_snapshot(app).get(app.selected_order_chat_idx)
                {
                    let list = get_order_attachment_messages(app, &row.order_id);
                    if !list.is_empty() {
                        app.mode = UiMode::UserSaveAttachmentPopup(row.order_id.clone(), 0);
                        return Some(true);
                    }
                }
            }
        }
    }

    // Ctrl+O / Ctrl+Shift+O: send attachment picker / retry without re-upload (My Trades)
    if key_event.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(code, KeyCode::Char('o') | KeyCode::Char('O'))
    {
        if let Tab::User(UserTab::MyTrades) = app.active_tab {
            if app.mode.user_my_trades_interactive()
                && app.active_user_chat_channel == UserChatChannel::Peer
            {
                if let Some(row) =
                    active_order_chat_list_snapshot(app).get(app.selected_order_chat_idx)
                {
                    let order_id = row.order_id.clone();
                    if key_event.modifiers.contains(KeyModifiers::SHIFT) {
                        if app.sending_attachment_order_id.is_some() {
                            return Some(true);
                        }
                        if let Some(prepared) =
                            app.pending_order_attachment_sends.get(&order_id).cloned()
                        {
                            if let Some(tx) = send_order_attachment_tx {
                                let _ = tx.send(SendOrderAttachmentJob::RetryPrepared(prepared));
                                app.sending_attachment_order_id = Some(order_id);
                            }
                        }
                        return Some(true);
                    }
                    if app.sending_attachment_order_id.is_some() {
                        return Some(true);
                    }
                    match open_user_send_attachment_picker(app, order_id) {
                        Ok(()) => {}
                        Err(e) => {
                            app.mode = UiMode::operation_result(OperationResult::Error(format!(
                                "Could not open file picker: {e}"
                            )));
                        }
                    }
                    return Some(true);
                }
            }
        }
    }

    // Handle invoice input first (before other key handling)
    if let UiMode::NewMessageNotification(
        _,
        Action::AddInvoice | Action::AddBondInvoice,
        ref mut invoice_state,
    ) = app.mode
    {
        if invoice_state.focused && handle_invoice_input(code, invoice_state) {
            return Some(true); // Skip further processing
        }
    }

    // Handle key input for shared settings popups and admin popups
    if matches!(
        app.mode,
        UiMode::AddMostroPubkey(_)
            | UiMode::AddRelay(_)
            | UiMode::AddLnAddress(_)
            | UiMode::AddCurrency(_)
            | UiMode::AdminMode(AdminMode::AddSolver(_))
            | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
    ) {
        let key_state = match &mut app.mode {
            UiMode::AddMostroPubkey(ref mut ks) => Some(ks),
            UiMode::AddRelay(ref mut ks) => Some(ks),
            UiMode::AddLnAddress(ref mut ks) => Some(ks),
            UiMode::AddCurrency(ref mut ks) => Some(ks),
            UiMode::AdminMode(AdminMode::AddSolver(ref mut state)) => Some(&mut state.key_input),
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
    if let UiMode::NewMessageNotification(
        _,
        Action::PayInvoice | Action::PayBondInvoice,
        ref mut invoice_state,
    ) = app.mode
    {
        if code != KeyCode::Char('c') && code != KeyCode::Char('C') {
            invoice_state.copied_to_clipboard = false;
        }
    }

    // Same "copied" indicator reset for the Shift+K Shared key disclosure popup.
    reset_disclosure_copied_indicator(&mut app.mode, &key_event);

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
                if let Some(selected_dispute) = selected_filtered_dispute(app) {
                    if let Ok(dispute_id) = uuid::Uuid::parse_str(&selected_dispute.dispute_id) {
                        app.mode = UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization {
                            dispute_id,
                            selected_button_index: 0,
                            bond: crate::util::order_utils::BondSlashChoice::None,
                            slash_submenu_open: false,
                            slash_submenu_index: 0,
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
            // Reset selection when switching filters
            app.selected_dispute_id = None;
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

        // Shift+R: recover relay in-progress disputes missing from local admin_disputes
        if has_shift && (code == KeyCode::Char('r') || code == KeyCode::Char('R')) {
            let can_recover = matches!(
                app.mode,
                UiMode::AdminMode(AdminMode::Normal)
                    | UiMode::AdminMode(AdminMode::ManagingDispute)
            );
            if can_recover {
                admin_handlers::begin_recover_taken_disputes(app, disputes);
                return Some(true);
            }
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
                // Default NO: avoid accidental Enter after Ctrl+Delete wiping all terminal history.
                app.mode = UiMode::ConfirmBulkDeleteHistory(false);
                return Some(true);
            }
            if let Some((order_id, status)) = resolve_selected_mytrades_order_status(app) {
                if is_terminal_order_status(status) {
                    app.mode = UiMode::ConfirmDeleteHistoryOrder(order_id, true);
                } else {
                    app.mode = UiMode::operation_result(OperationResult::Info(
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
                    let selected = resolve_selected_mytrades_order_status(app);
                    if let Some(next_mode) =
                        trade_action_shortcut_next_mode(&app.mode, selected, Action::Cancel)
                    {
                        app.mode = next_mode;
                        return Some(true);
                    }
                }
                KeyCode::Char('f') | KeyCode::Char('F') => {
                    let selected = resolve_selected_mytrades_order_status(app);
                    if let Some(next_mode) =
                        trade_action_shortcut_next_mode(&app.mode, selected, Action::FiatSent)
                    {
                        app.mode = next_mode;
                        return Some(true);
                    }
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    let selected = resolve_selected_mytrades_order_status(app);
                    if let Some(next_mode) =
                        trade_action_shortcut_next_mode(&app.mode, selected, Action::Release)
                    {
                        app.mode = next_mode;
                        return Some(true);
                    }
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    if let Some(state) = build_rating_state_for_mytrades(app, 5) {
                        app.mode = UiMode::RatingOrder(state);
                        return Some(true);
                    }
                }
                KeyCode::Char('d') | KeyCode::Char('D') => {
                    let selected = resolve_selected_mytrades_order_status(app);
                    if let Some(next_mode) = dispute_shortcut_next_mode(&app.mode, selected) {
                        app.mode = next_mode;
                        return Some(true);
                    }
                }
                KeyCode::Char('k') | KeyCode::Char('K') => {
                    if !app.mode.user_my_trades_interactive() {
                        return Some(true);
                    }
                    let Some((order_id, _)) = resolve_selected_mytrades_order_status(app) else {
                        app.mode = UiMode::operation_result(OperationResult::Info(
                            "Select an order to reveal the Shared key.".to_string(),
                        ));
                        return Some(true);
                    };
                    let pool = pool.clone();
                    let tx = order_result_tx.clone();
                    tokio::spawn(async move {
                        let result = match crate::models::Order::get_by_id(
                            &pool,
                            &order_id.to_string(),
                        )
                        .await
                        {
                            Ok(order) => {
                                let trade_keys = order
                                    .trade_keys
                                    .as_deref()
                                    .and_then(|h| Keys::parse(h).ok());
                                match crate::util::chat_utils::conversation_disclosure_from_order(
                                    order.order_chat_shared_key_hex.as_deref(),
                                    trade_keys.as_ref(),
                                    order.counterparty_pubkey.as_deref(),
                                ) {
                                    Some((conv, _sign_pk)) => {
                                        OperationResult::ConversationDisclosure {
                                            conv_hex: conv,
                                            copied_to_clipboard: false,
                                        }
                                    }
                                    None => OperationResult::Error(
                                        "No Shared key for this order yet.".to_string(),
                                    ),
                                }
                            }
                            Err(e) => OperationResult::Error(format!(
                                "Could not load order for Shared key: {e}"
                            )),
                        };
                        let _ = tx.send(result);
                    });
                    return Some(true);
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

    // Observer tab: handle all character and backspace input early so y/n/m/c etc. go to the field.
    // Skip when a modal owns input so we don't edit the Shared key behind an overlay.
    if app.observer_inputs_editable() {
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

    // Currency dropdown (Create New Order) captures keys while open and on open triggers.
    if let Some(consumed) = form_input::handle_currency_picker_key(code, app) {
        return Some(consumed);
    }

    match code {
        KeyCode::Left | KeyCode::Right => {
            // Handle Left/Right for button selection in confirmation popups
            match &mut app.mode {
                UiMode::AdminMode(AdminMode::AddSolver(ref mut state)) => {
                    state.permission = state.permission.toggle();
                    return Some(true);
                }
                UiMode::AdminMode(AdminMode::ConfirmAddSolver {
                    ref mut selected_button,
                    ..
                })
                | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, ref mut selected_button))
                | UiMode::AdminMode(AdminMode::ConfirmTakeDispute(_, ref mut selected_button))
                | UiMode::AdminMode(AdminMode::ConfirmRecoverTakenDisputes {
                    ref mut selected_button,
                    ..
                })
                | UiMode::ConfirmMostroPubkey(_, ref mut selected_button)
                | UiMode::ConfirmRelay(_, ref mut selected_button)
                | UiMode::ConfirmLnAddress(_, ref mut selected_button)
                | UiMode::ConfirmSavedLnAddressForInvoice(_, ref mut selected_button)
                | UiMode::ConfirmClearLnAddress(ref mut selected_button)
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
                        }
                        selection @ ViewingMessageButtonSelection::Three(_) => {
                            if code == KeyCode::Left {
                                selection.cycle_three_prev();
                            } else {
                                selection.cycle_three_next();
                            }
                        }
                    }
                    return Some(true);
                }
                UiMode::NewMessageNotification(
                    _,
                    Action::AddInvoice
                    | Action::AddBondInvoice
                    | Action::PayInvoice
                    | Action::PayBondInvoice
                    | Action::WaitingSellerToPay
                    | Action::WaitingBuyerInvoice,
                    ref mut invoice_state,
                ) => {
                    return Some(update_invoice_notification_action_selection(
                        code,
                        invoice_state,
                    ))
                }
                UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization {
                    dispute_id,
                    ref mut selected_button_index,
                    slash_submenu_open: false,
                    ..
                }) => {
                    let dispute_is_finalized = app
                        .admin_disputes_in_progress
                        .iter()
                        .find(|d| d.dispute_id == dispute_id.to_string())
                        .and_then(is_dispute_finalized)
                        .unwrap_or(false);
                    let bond_ui_enabled =
                        crate::util::mostro_info::instance_bonds_enabled(app.mostro_info.as_ref());

                    cycle_finalization_button(
                        selected_button_index,
                        code,
                        dispute_is_finalized,
                        bond_ui_enabled,
                    );
                    return Some(true);
                }
                _ => {}
            }
            handle_navigation(code, app, orders, disputes);
            Some(true)
        }
        KeyCode::Up | KeyCode::Down => {
            if let UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization {
                slash_submenu_open: true,
                ref mut slash_submenu_index,
                ..
            }) = app.mode
            {
                cycle_bond_slash_choice(slash_submenu_index, code);
                return Some(true);
            }

            // Handle chat message navigation when input is disabled (Disputes in Progress)
            if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
                if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                    if !app.admin_chat_input_enabled {
                        let dispute_id_key = selected_filtered_dispute(app).map(|d| d.dispute_id);
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
                    let dispute_id_key = selected_filtered_dispute(app).map(|d| d.dispute_id);
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

            // My Trades: PageUp/PageDown scroll the order chat
            if app.mode.user_my_trades_interactive() {
                if let Tab::User(UserTab::MyTrades) = app.active_tab {
                    if chat_helpers::scroll_order_chat_messages(app, code) {
                        return Some(true);
                    }
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
                ln_address_result_tx,
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
                    let dispute_id_key = selected_filtered_dispute(app).map(|d| d.dispute_id);
                    if let Some(dispute_id_key) = dispute_id_key {
                        if chat_helpers::jump_to_chat_bottom(app, &dispute_id_key) {
                            return Some(true);
                        }
                    }
                }
            }
            if app.mode.user_my_trades_interactive() {
                if let Tab::User(UserTab::MyTrades) = app.active_tab {
                    if chat_helpers::jump_to_order_chat_bottom(app) {
                        return Some(true);
                    }
                }
            }
            Some(true)
        }
        // 'q' key removed - use Exit tab instead.
        // Confirmations use Enter on the focused button to confirm and Esc to
        // cancel; the 'y'/'n' shortcuts were removed. ('n'/'N' now just falls
        // through to the generic Char(_) arm for form text entry.)
        KeyCode::Char('c') | KeyCode::Char('C') if is_creating_order_text_input(app) => {
            handle_char_input(code, app, validate_range_amount);
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

            // Handle copy Shared key (K_conv) for the Shift+K disclosure popup. Only
            // the Shared key is copyable — the signing key itself is never disclosed.
            if !key_event
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
                && copy_disclosed_shared_key_if_open(&mut app.mode)
            {
                return Some(true);
            }

            // Handle copy invoice for PayInvoice / PayBondInvoice notifications
            if let UiMode::NewMessageNotification(
                ref notification,
                Action::PayInvoice | Action::PayBondInvoice,
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

#[cfg(test)]
mod key_handler_tests {
    use super::*;
    use crate::ui::{InvoiceInputState, InvoiceNotificationActionSelection, UserRole};
    use crossterm::event::KeyModifiers;

    #[test]
    fn dispute_is_allowed_only_while_the_trade_is_under_way() {
        assert!(can_dispute_order_status(Some(Status::Active)));
        assert!(can_dispute_order_status(Some(Status::FiatSent)));

        // Nothing to dispute before the sats are locked or after the trade is closed.
        assert!(!can_dispute_order_status(Some(Status::Pending)));
        assert!(!can_dispute_order_status(Some(Status::WaitingPayment)));
        assert!(!can_dispute_order_status(Some(Status::WaitingBuyerInvoice)));
        assert!(!can_dispute_order_status(Some(Status::Success)));
        assert!(!can_dispute_order_status(Some(Status::Canceled)));
        assert!(!can_dispute_order_status(Some(Status::Expired)));
    }

    #[test]
    fn unknown_status_does_not_block_dispute() {
        // Local status can lag behind Mostro; let the daemon be the one to say no.
        assert!(can_dispute_order_status(None));
    }

    #[test]
    fn trade_action_shortcuts_are_ignored_while_a_request_is_pending() {
        // Regression (offered follow-up from #106): Shift+C/F/R shared the
        // Shift+D double-submit window — after a confirmation the UI sits in a
        // waiting mode while the row still shows an actionable status.
        let selected = Some((uuid::Uuid::new_v4(), Some(Status::Active)));
        for action in [Action::Cancel, Action::FiatSent, Action::Release] {
            assert!(
                trade_action_shortcut_next_mode(
                    &UiMode::UserMode(UserMode::WaitingAddInvoice),
                    selected,
                    action.clone(),
                )
                .is_none(),
                "{action:?} must be ignored while waiting"
            );
        }
    }

    #[test]
    fn trade_action_shortcuts_open_the_right_confirmation_when_interactive() {
        let order_id = uuid::Uuid::new_v4();
        for action in [Action::Cancel, Action::FiatSent, Action::Release] {
            match trade_action_shortcut_next_mode(
                &UiMode::UserMode(UserMode::Normal),
                Some((order_id, Some(Status::Active))),
                action.clone(),
            ) {
                Some(UiMode::ViewingMessage(view_state)) => {
                    assert_eq!(view_state.order_id, Some(order_id));
                    assert_eq!(view_state.action, action);
                }
                other => panic!("expected confirmation for {action:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn trade_action_shortcuts_stay_disabled_for_terminal_orders() {
        let next = trade_action_shortcut_next_mode(
            &UiMode::UserMode(UserMode::Normal),
            Some((uuid::Uuid::new_v4(), Some(Status::Success))),
            Action::Release,
        );
        match next {
            Some(UiMode::OperationResult(result)) => match *result {
                OperationResult::Info(msg) => {
                    assert_eq!(msg, "Release is disabled for terminal orders.")
                }
                other => panic!("expected Info, got {other:?}"),
            },
            other => panic!("expected terminal-order notice, got {other:?}"),
        }
    }

    #[test]
    fn dispute_shortcut_opens_the_confirmation_when_my_trades_is_interactive() {
        let order_id = uuid::Uuid::new_v4();
        let next = dispute_shortcut_next_mode(
            &UiMode::UserMode(UserMode::Normal),
            Some((order_id, Some(Status::Active))),
        );
        match next {
            Some(UiMode::ViewingMessage(view_state)) => {
                assert_eq!(view_state.order_id, Some(order_id));
                assert_eq!(view_state.action, Action::Dispute);
            }
            other => panic!("expected dispute confirmation, got {other:?}"),
        }
    }

    #[test]
    fn dispute_shortcut_is_ignored_while_a_dispute_request_is_pending() {
        // Regression: after confirming a dispute, spawn_dispute() puts the UI in a
        // waiting mode while the local row still reads Active/FiatSent. A second
        // Shift+D must not open another confirmation and submit a second
        // Action::Dispute.
        let selected = Some((uuid::Uuid::new_v4(), Some(Status::Active)));
        assert!(dispute_shortcut_next_mode(
            &UiMode::UserMode(UserMode::WaitingAddInvoice),
            selected,
        )
        .is_none());

        // Same while the first confirmation popup is still open.
        let popup = UiMode::ViewingMessage(build_order_action_view_state(
            uuid::Uuid::new_v4(),
            Action::Dispute,
            String::new(),
        ));
        assert!(dispute_shortcut_next_mode(&popup, selected).is_none());
    }

    #[test]
    fn dispute_shortcut_ignores_keys_without_a_selected_order() {
        assert!(dispute_shortcut_next_mode(&UiMode::UserMode(UserMode::Normal), None).is_none());
    }

    #[test]
    fn paste_shortcut_accepts_shift_insert_and_ctrl_v() {
        let shift_insert = KeyEvent::new(KeyCode::Insert, KeyModifiers::SHIFT);
        assert!(is_paste_shortcut(&shift_insert));

        let ctrl_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
        assert!(is_paste_shortcut(&ctrl_v));
    }

    #[test]
    fn observer_tab_ignores_tab_and_backtab_with_a_single_input_field() {
        let mut app = AppState::new(UserRole::Admin);
        app.active_tab = Tab::Admin(AdminTab::Observer);
        app.observer_shared_key_input = "abc".to_string();
        handle_tab_navigation(KeyCode::Tab, &mut app);
        handle_tab_navigation(KeyCode::BackTab, &mut app);
        assert_eq!(app.observer_shared_key_input, "abc");
    }

    #[test]
    fn invoice_notification_selection_toggles_with_arrows() {
        let mut state = InvoiceInputState {
            invoice_input: String::new(),
            focused: true,
            just_pasted: false,
            copied_to_clipboard: false,
            scroll_y: 0,
            action_selection: InvoiceNotificationActionSelection::Primary,
        };

        assert!(update_invoice_notification_action_selection(
            KeyCode::Right,
            &mut state
        ));
        assert_eq!(
            state.action_selection,
            InvoiceNotificationActionSelection::Cancel
        );
        assert!(update_invoice_notification_action_selection(
            KeyCode::Left,
            &mut state
        ));
        assert_eq!(
            state.action_selection,
            InvoiceNotificationActionSelection::Primary
        );
    }

    fn disclosure_mode(copied_to_clipboard: bool) -> UiMode {
        UiMode::operation_result(OperationResult::ConversationDisclosure {
            conv_hex: "a".repeat(64),
            copied_to_clipboard,
        })
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn copy_disclosed_shared_key_sets_copied_flag_when_the_write_succeeds() {
        let mut mode = disclosure_mode(false);
        assert!(copy_disclosed_shared_key_if_open_with(&mut mode, |_| true));
        let UiMode::OperationResult(result) = &mode else {
            panic!("expected OperationResult mode");
        };
        match result.as_ref() {
            OperationResult::ConversationDisclosure {
                copied_to_clipboard,
                ..
            } => assert!(*copied_to_clipboard, "C should mark the Shared key copied"),
            other => panic!("expected ConversationDisclosure, got {other:?}"),
        }
    }

    #[test]
    fn copy_disclosed_shared_key_leaves_flag_false_when_the_write_fails() {
        let mut mode = disclosure_mode(false);
        assert!(
            copy_disclosed_shared_key_if_open_with(&mut mode, |_| false),
            "the popup was still open and handled the key even though the copy failed"
        );
        let UiMode::OperationResult(result) = &mode else {
            panic!("expected OperationResult mode");
        };
        match result.as_ref() {
            OperationResult::ConversationDisclosure {
                copied_to_clipboard,
                ..
            } => assert!(
                !*copied_to_clipboard,
                "a failed clipboard write must not report success"
            ),
            other => panic!("expected ConversationDisclosure, got {other:?}"),
        }
    }

    #[test]
    fn copy_disclosed_shared_key_ignores_other_modes() {
        let mut mode = UiMode::operation_result(OperationResult::Info("hi".to_string()));
        assert!(
            !copy_disclosed_shared_key_if_open_with(&mut mode, |_| true),
            "generic Info popup must not be treated as a Shared key copy target"
        );
    }

    #[test]
    fn disclosure_copied_indicator_resets_on_keys_other_than_c() {
        let mut mode = disclosure_mode(true);
        reset_disclosure_copied_indicator(&mut mode, &key(KeyCode::Esc));
        let UiMode::OperationResult(result) = &mode else {
            panic!("expected OperationResult mode");
        };
        match result.as_ref() {
            OperationResult::ConversationDisclosure {
                copied_to_clipboard,
                ..
            } => assert!(
                !*copied_to_clipboard,
                "non-C keys must clear the copied indicator"
            ),
            other => panic!("expected ConversationDisclosure, got {other:?}"),
        }
    }

    #[test]
    fn disclosure_copied_indicator_survives_c_key() {
        let mut mode = disclosure_mode(true);
        reset_disclosure_copied_indicator(&mut mode, &key(KeyCode::Char('c')));
        let UiMode::OperationResult(result) = &mode else {
            panic!("expected OperationResult mode");
        };
        match result.as_ref() {
            OperationResult::ConversationDisclosure {
                copied_to_clipboard,
                ..
            } => assert!(*copied_to_clipboard, "C key must not clear the indicator"),
            other => panic!("expected ConversationDisclosure, got {other:?}"),
        }
    }

    #[test]
    fn disclosure_copied_indicator_clears_on_ctrl_c() {
        let mut mode = disclosure_mode(true);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        reset_disclosure_copied_indicator(&mut mode, &ctrl_c);
        let UiMode::OperationResult(result) = &mode else {
            panic!("expected OperationResult mode");
        };
        match result.as_ref() {
            OperationResult::ConversationDisclosure {
                copied_to_clipboard,
                ..
            } => assert!(
                !*copied_to_clipboard,
                "Ctrl+C must clear the indicator consistently with the Observer clear-all shortcut"
            ),
            other => panic!("expected ConversationDisclosure, got {other:?}"),
        }
    }
}
