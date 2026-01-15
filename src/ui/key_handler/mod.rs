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
use nostr_sdk::Client;
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

#[allow(clippy::too_many_arguments)]
/// Main key event handler - dispatches to appropriate handlers
pub fn handle_key_event(
    key_event: KeyEvent,
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<Dispute>>>,
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
    validate_range_amount: &dyn Fn(&mut TakeOrderState),
) -> Option<bool> {
    // Returns Some(true) to continue, Some(false) to break, None to continue normally
    let code = key_event.code;

    // Handle invoice input first (before other key handling)
    if let UiMode::NewMessageNotification(
        _,
        mostro_core::prelude::Action::AddInvoice,
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

    match code {
        KeyCode::Left | KeyCode::Right => {
            // Handle Left/Right for button selection in confirmation popups
            match &mut app.mode {
                UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, ref mut selected_button))
                | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, ref mut selected_button))
                | UiMode::ConfirmMostroPubkey(_, ref mut selected_button)
                | UiMode::ConfirmRelay(_, ref mut selected_button)
                | UiMode::ConfirmCurrency(_, ref mut selected_button)
                | UiMode::ConfirmClearCurrencies(ref mut selected_button) => {
                    *selected_button = !*selected_button; // Toggle between YES and NO
                    return Some(true);
                }
                UiMode::ViewingMessage(ref mut view_state) => {
                    view_state.selected_button = !view_state.selected_button; // Toggle between YES and NO
                    return Some(true);
                }
                _ => {}
            }
            handle_navigation(code, app, orders, disputes);
            Some(true)
        }
        KeyCode::Up | KeyCode::Down => {
            handle_navigation(code, app, orders, disputes);
            Some(true)
        }
        KeyCode::Tab | KeyCode::BackTab => {
            handle_tab_navigation(code, app);
            Some(true)
        }
        KeyCode::Enter => {
            handle_enter_key(app, orders, pool, client, mostro_pubkey, order_result_tx);
            Some(true)
        }
        KeyCode::Esc => {
            let should_continue = handle_esc_key(app);
            Some(should_continue)
        }
        KeyCode::Char('q') => Some(false), // Break the loop
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
                    // Copy to clipboard using proper arboard methods
                    let copy_result = {
                        match arboard::Clipboard::new() {
                            Ok(mut clipboard) => {
                                // Use builder pattern with wait() on Linux for proper clipboard handling
                                #[cfg(target_os = "linux")]
                                {
                                    use arboard::SetExtLinux;
                                    clipboard.set().wait().text(invoice.clone())
                                }
                                #[cfg(not(target_os = "linux"))]
                                {
                                    clipboard.set_text(invoice.clone())
                                }
                            }
                            Err(e) => Err(e),
                        }
                    };

                    match copy_result {
                        Ok(_) => {
                            log::info!("Invoice copied to clipboard");
                            invoice_state.copied_to_clipboard = true;
                        }
                        Err(e) => {
                            log::warn!("Failed to copy invoice to clipboard: {}", e);
                        }
                    }
                }
            }
            Some(true)
        }
        KeyCode::Char(_) | KeyCode::Backspace => {
            handle_char_input(code, app, validate_range_amount);
            if code == KeyCode::Backspace {
                handle_backspace(app, validate_range_amount);
            }
            Some(true)
        }
        _ => None,
    }
}
