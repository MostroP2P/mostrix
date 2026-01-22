mod confirmation;
mod enter_handlers;
mod esc_handlers;
mod form_input;
mod input_helpers;
mod navigation;
mod settings;
mod validation;

use crate::ui::{AdminMode, AdminTab, AppState, Tab, TakeOrderState, UiMode, UserTab};
use crossterm::event::{KeyCode, KeyEvent};
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

// Re-export public functions
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
fn handle_admin_chat_input(app: &mut AppState, code: KeyCode) -> Option<bool> {
    if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
        if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
            // Only allow input if chat input is enabled
            if app.admin_chat_input_enabled {
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

/// Handle clipboard copy for invoice
fn handle_clipboard_copy(invoice: String) -> bool {
    let copy_result = {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                #[cfg(target_os = "linux")]
                {
                    use arboard::SetExtLinux;
                    clipboard.set().wait().text(invoice)
                }
                #[cfg(not(target_os = "linux"))]
                {
                    clipboard.set_text(invoice)
                }
            }
            Err(e) => Err(e),
        }
    };

    match copy_result {
        Ok(_) => {
            log::info!("Invoice copied to clipboard");
            true
        }
        Err(e) => {
            log::warn!("Failed to copy invoice to clipboard: {}", e);
            false
        }
    }
}

/// Cycle through 3 buttons (Pay Buyer, Refund Seller, Exit) for dispute finalization
fn cycle_finalization_button(selected_button: &mut usize, direction: KeyCode) {
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
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
    validate_range_amount: &dyn Fn(&mut TakeOrderState),
) -> Option<bool> {
    // Returns Some(true) to continue, Some(false) to break, None to continue normally
    let code = key_event.code;

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

    // Handle Shift+I to toggle chat input enabled/disabled
    if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
        if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
            if code == KeyCode::Char('i') || code == KeyCode::Char('I') {
                // Check if Shift is pressed
                if key_event
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::SHIFT)
                {
                    app.admin_chat_input_enabled = !app.admin_chat_input_enabled;
                    return Some(true);
                }
            }
        }
    }

    // Check if we're in admin chat input mode FIRST - this takes priority over all other key handling
    // (except invoice and key input which are handled earlier)
    if let Some(result) = handle_admin_chat_input(app, code) {
        return Some(result);
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
                | UiMode::ConfirmExit(ref mut selected_button) => {
                    *selected_button = !*selected_button; // Toggle between YES and NO
                    return Some(true);
                }
                UiMode::ViewingMessage(ref mut view_state) => {
                    view_state.selected_button = !view_state.selected_button; // Toggle between YES and NO
                    return Some(true);
                }
                UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization(
                    _dispute_id,
                    ref mut selected_button,
                )) => {
                    cycle_finalization_button(selected_button, code);
                    return Some(true);
                }
                _ => {}
            }
            handle_navigation(code, app, orders, disputes);
            Some(true)
        }
        KeyCode::Up | KeyCode::Down => {
            // Handle chat message navigation when input is disabled
            if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
                if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                    if !app.admin_chat_input_enabled {
                        if let Some(selected_dispute) = app
                            .admin_disputes_in_progress
                            .get(app.selected_in_progress_idx)
                        {
                            let dispute_id = &selected_dispute.id;
                            if let Some(messages) = app.admin_dispute_chats.get(dispute_id) {
                                // Filter messages by active party to get actual count
                                let visible_count = messages
                                    .iter()
                                    .filter(|msg| {
                                        matches!(
                                            (msg.sender, app.active_chat_party),
                                            (super::ChatSender::Admin, _)
                                                | (
                                                    super::ChatSender::Buyer,
                                                    super::ChatParty::Buyer
                                                )
                                                | (
                                                    super::ChatSender::Seller,
                                                    super::ChatParty::Seller
                                                )
                                        )
                                    })
                                    .count();

                                if visible_count > 0 {
                                    let current = app
                                        .admin_chat_list_state
                                        .selected()
                                        .unwrap_or(visible_count.saturating_sub(1));

                                    let new_selection = if code == KeyCode::Up {
                                        // Move up (older messages)
                                        if current > 0 {
                                            current - 1
                                        } else {
                                            0
                                        }
                                    } else {
                                        // Move down (newer messages)
                                        (current + 1).min(visible_count.saturating_sub(1))
                                    };

                                    app.admin_chat_list_state.select(Some(new_selection));
                                    return Some(true);
                                }
                            }
                        }
                    }
                }
            }
            handle_navigation(code, app, orders, disputes);
            Some(true)
        }
        KeyCode::PageUp | KeyCode::PageDown => {
            // Handle chat scrolling in ManagingDispute mode using ListState
            if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
                if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                    if let Some(selected_dispute) = app
                        .admin_disputes_in_progress
                        .get(app.selected_in_progress_idx)
                    {
                        let dispute_id = &selected_dispute.id;
                        if let Some(messages) = app.admin_dispute_chats.get(dispute_id) {
                            // Filter messages by active party to get actual count
                            let visible_count = messages
                                .iter()
                                .filter(|msg| {
                                    matches!(
                                        (msg.sender, app.active_chat_party),
                                        (super::ChatSender::Admin, _)
                                            | (super::ChatSender::Buyer, super::ChatParty::Buyer)
                                            | (super::ChatSender::Seller, super::ChatParty::Seller)
                                    )
                                })
                                .count();

                            if visible_count > 0 {
                                let current = app
                                    .admin_chat_list_state
                                    .selected()
                                    .unwrap_or(visible_count.saturating_sub(1));

                                if code == KeyCode::PageUp {
                                    // Scroll up (show older messages) - move selection up by ~10 items
                                    let new_selection = current
                                        .saturating_sub(10)
                                        .min(visible_count.saturating_sub(1));
                                    app.admin_chat_list_state.select(Some(new_selection));
                                } else {
                                    // Scroll down (show newer messages) - move selection down by ~10 items
                                    let new_selection =
                                        (current + 10).min(visible_count.saturating_sub(1));
                                    app.admin_chat_list_state.select(Some(new_selection));
                                }
                                return Some(true);
                            }
                        }
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
            let should_continue = handle_enter_key(
                app,
                orders,
                disputes,
                pool,
                client,
                mostro_pubkey,
                order_result_tx,
            );
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
                    if let Some(selected_dispute) = app
                        .admin_disputes_in_progress
                        .get(app.selected_in_progress_idx)
                    {
                        let dispute_id = &selected_dispute.id;
                        if let Some(messages) = app.admin_dispute_chats.get(dispute_id) {
                            // Filter messages by active party to get actual count
                            let visible_count = messages
                                .iter()
                                .filter(|msg| {
                                    matches!(
                                        (msg.sender, app.active_chat_party),
                                        (super::ChatSender::Admin, _)
                                            | (super::ChatSender::Buyer, super::ChatParty::Buyer)
                                            | (super::ChatSender::Seller, super::ChatParty::Seller)
                                    )
                                })
                                .count();

                            if visible_count > 0 {
                                // Jump to last message (bottom)
                                app.admin_chat_list_state
                                    .select(Some(visible_count.saturating_sub(1)));
                                return Some(true);
                            }
                        }
                    }
                }
            }
            Some(true)
        }
        // 'q' key removed - use Exit tab instead
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let should_continue =
                handle_confirm_key(app, pool, client, mostro_pubkey, order_result_tx);
            Some(should_continue)
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            handle_cancel_key(app);
            Some(true)
        }
        KeyCode::Char('m') | KeyCode::Char('M') => {
            // Switch mode when in Settings tab
            match app.active_tab {
                Tab::User(UserTab::Settings) | Tab::Admin(AdminTab::Settings) => {
                    handle_mode_switch(app);
                    Some(true)
                }
                _ => None, // Not in settings, continue normally
            }
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
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
            // Chat input is handled at the top of this function (takes priority)
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
