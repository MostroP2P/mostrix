use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use ratatui::layout::{Constraint, Direction, Layout};

use crate::ui::*;

/// Main UI draw function, extracted from `ui::mod`.
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
            tabs::orders_tab::render_orders_tab(f, content_area, orders, app.selected_order_idx)
        }
        (Tab::User(UserTab::MyTrades), UserRole::User) => {
            tabs::tab_content::render_coming_soon(f, content_area, "My Trades")
        }
        (Tab::User(UserTab::Messages), UserRole::User) => {
            let messages = app.messages.lock().unwrap();
            tabs::tab_content::render_messages_tab(
                f,
                content_area,
                &messages,
                app.selected_message_idx,
            )
        }
        (Tab::User(UserTab::Settings), UserRole::User) => tabs::settings_tab::render_settings_tab(
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
            tabs::disputes_tab::render_disputes_tab(
                f,
                content_area,
                disputes,
                app.selected_dispute_idx,
            )
        }
        (Tab::Admin(AdminTab::DisputesInProgress), UserRole::Admin) => {
            tabs::disputes_in_progress_tab::render_disputes_in_progress(f, content_area, app)
        }
        (Tab::Admin(AdminTab::Settings), UserRole::Admin) => {
            tabs::settings_tab::render_settings_tab(
                f,
                content_area,
                app.user_role,
                app.selected_settings_option,
            )
        }
        (Tab::User(UserTab::Exit), UserRole::User)
        | (Tab::Admin(AdminTab::Exit), UserRole::Admin) => {
            tabs::tab_content::render_exit_tab(f, content_area)
        }
        _ => {
            // Fallback for invalid combinations
            tabs::tab_content::render_coming_soon(f, content_area, "Unknown")
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
    if let UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization {
        dispute_id,
        selected_button_index,
    }) = &app.mode
    {
        dispute_finalization_popup::render_finalization_popup(
            f,
            app,
            dispute_id,
            *selected_button_index,
        );
    }

    // Dispute finalization confirmation popup
    if let UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute {
        dispute_id,
        is_settle,
        selected_button,
    }) = &app.mode
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
        tabs::tab_content::render_message_view(f, view_state);
    }
}
