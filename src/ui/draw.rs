use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use ratatui::layout::Alignment;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::shared::permissions::SolverPermission;
use crate::ui::orders::strip_new_order_messages_and_clamp_selected;
use crate::ui::*;
use crate::util::fatal::request_fatal_restart;

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

    // Fatal restart prompt: render only the popup overlay (no additional locks).
    if app.fatal_exit_on_close {
        if let UiMode::OperationResult(result) = &app.mode {
            operation_result::render_operation_result(f, result);
        }
        return;
    }

    // Render content based on active tab and role
    let content_area = chunks[1];
    match (&app.active_tab, app.user_role) {
        (Tab::User(UserTab::Orders), UserRole::User) => tabs::orders_tab::render_orders_tab(
            f,
            content_area,
            orders,
            app.selected_order_idx,
            app,
        ),
        (Tab::User(UserTab::MyTrades), UserRole::User) => {
            tabs::order_in_progress_tab::render_order_in_progress(f, content_area, app)
        }
        (Tab::User(UserTab::Messages), UserRole::User) => {
            let mut messages = match app.messages.lock() {
                Ok(g) => g,
                Err(e) => {
                    request_fatal_restart(format!(
                        "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
                    ));
                    return;
                }
            };
            strip_new_order_messages_and_clamp_selected(
                &mut messages,
                &mut app.selected_message_idx,
            );
            tabs::message_flow_tab::render_messages_tab(
                f,
                content_area,
                &messages,
                app.selected_message_idx,
            )
        }
        (Tab::User(UserTab::MostroInfo), UserRole::User) => {
            tabs::mostro_info_tab::render_mostro_info_tab(f, content_area, app)
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
        (Tab::Admin(AdminTab::Observer), UserRole::Admin) => {
            tabs::observer_tab::render_observer_tab(f, content_area, app)
        }
        (Tab::Admin(AdminTab::MostroInfo), UserRole::Admin) => {
            tabs::mostro_info_tab::render_mostro_info_tab(f, content_area, app)
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
        let pending_count = match app.pending_notifications.lock() {
            Ok(g) => *g,
            Err(e) => {
                request_fatal_restart(format!(
                    "Mostrix encountered an internal error (poisoned pending notifications lock: {e}). Please restart the app."
                ));
                0
            }
        };
        status::render_status_bar(f, chunks[2], lines, pending_count);
    }

    // Confirmation popup overlay (user mode only)
    if let UiMode::UserMode(UserMode::ConfirmingOrder {
        form,
        selected_button,
    }) = &app.mode
    {
        order_confirm::render_order_confirm(f, form, *selected_button);
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
    // Waiting for add solver popup overlay (admin mode only)
    if let UiMode::AdminMode(AdminMode::WaitingAddSolver) = &app.mode {
        waiting::render_waiting_with_message(f, "Adding solver and waiting for confirmation...");
    }

    // Operation result popup overlay (shared)
    if let UiMode::OperationResult(result) = &app.mode {
        operation_result::render_operation_result(f, result);
    }

    // Generate new keys flow popups
    if let UiMode::ConfirmGenerateNewKeys(selected_button) = &app.mode {
        generate_keys_popup::render_confirm_generate_new_keys(
            f,
            app.user_role == UserRole::Admin,
            *selected_button,
        );
    }
    if let UiMode::BackupNewKeys(mnemonic) = &app.mode {
        generate_keys_popup::render_backup_new_keys(f, mnemonic.as_str());
    }

    // Help popup (Ctrl+H)
    if let UiMode::HelpPopup(tab, _) = &app.mode {
        help_popup::render_help_popup(f, app, *tab);
    }

    // Settings: full option reference (Shift+H)
    if let UiMode::SettingsInstructionsPopup(role, _) = &app.mode {
        help_popup::render_settings_instructions_popup(f, *role);
    }

    // Save attachment popup (Ctrl+S in dispute chat)
    if let UiMode::SaveAttachmentPopup(selected_idx) = &app.mode {
        save_attachment_popup::render_save_attachment_popup(f, app, *selected_idx);
    }

    // Observer save attachment popup (Ctrl+S in observer tab)
    if let UiMode::ObserverSaveAttachmentPopup(selected_idx) = &app.mode {
        save_attachment_popup::render_observer_save_attachment_popup(f, app, *selected_idx);
    }

    // User order chat save attachment popup (Ctrl+S on My Trades tab)
    if let UiMode::UserSaveAttachmentPopup(selected_idx) = &app.mode {
        save_attachment_popup::render_user_save_attachment_popup(f, app, *selected_idx);
    }

    // Shared settings popups
    if let UiMode::AddMostroPubkey(key_state) = &app.mode {
        key_input_popup::render_key_input_popup(
            f,
            "🌐 Add Mostro Pubkey",
            "Enter Mostro public key (64 hex chars):",
            "0123... (64 hex chars)",
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
    if let UiMode::AddLnAddress(key_state) = &app.mode {
        key_input_popup::render_key_input_popup(
            f,
            "⚡ Lightning Address (buyer)",
            "Enter Lightning address (user@domain.com):",
            "you@wallet.example.com",
            key_state,
            false,
        );
    }
    if let UiMode::ConfirmLnAddress(addr, selected_button) = &app.mode {
        admin_key_confirm::render_admin_key_confirm_with_message(
            f,
            "⚡ Confirm Lightning Address",
            addr,
            *selected_button,
            Some("Verify LNURL endpoint and save this address to settings.toml?"),
        );
    }
    if let UiMode::ConfirmClearLnAddress(selected_button) = &app.mode {
        admin_key_confirm::render_admin_key_confirm_with_message(
            f,
            "⚡ Clear Lightning Address",
            "",
            *selected_button,
            Some("Remove the saved buyer Lightning address from settings?"),
        );
    }
    if let UiMode::ConfirmSavedLnAddressForInvoice(_, selected_button) = &app.mode {
        let addr_display = crate::settings::load_settings_from_disk()
            .ok()
            .map(|s| s.ln_address.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "(none)".to_string());
        let body = format!(
            "Saved Lightning address:\n{addr_display}\n\n\
Confirm using this address from Settings as your invoice?\n\n\
Yes: fill invoice from Settings.\n\
No: paste BOLT11 or Lightning address manually."
        );
        admin_key_confirm::render_saved_ln_address_invoice_confirm(f, *selected_button, &body);
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
    if let UiMode::ConfirmDeleteHistoryOrder(order_id, selected_button) = &app.mode {
        admin_key_confirm::render_admin_key_confirm_with_message(
            f,
            "🗑 Delete Order History",
            &order_id.to_string(),
            *selected_button,
            Some("Delete selected terminal order from local database history?"),
        );
    }
    if let UiMode::ConfirmBulkDeleteHistory(selected_button) = &app.mode {
        admin_key_confirm::render_admin_key_confirm_with_message(
            f,
            "🧹 Clean Up Terminal History",
            "",
            *selected_button,
            Some("Delete all success/canceled orders from local database history?"),
        );
    }

    // Admin key input popup overlay
    if let UiMode::AdminMode(AdminMode::AddSolver(add_solver_state)) = &app.mode {
        render_add_solver_popup(f, add_solver_state);
    }
    if let UiMode::AdminMode(AdminMode::SetupAdminKey(key_state)) = &app.mode {
        key_input_popup::render_key_input_popup(
            f,
            "🔐 Setup Admin Key",
            "Enter admin private key (nsec... or hex):",
            "nsec... / hex...",
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
    if let UiMode::AdminMode(AdminMode::ConfirmAddSolver {
        solver_pubkey,
        permission,
        selected_button,
    }) = &app.mode
    {
        admin_key_confirm::render_admin_key_confirm_with_message(
            f,
            "Add Solver",
            solver_pubkey,
            *selected_button,
            Some(&format!(
                "Add this pubkey as dispute solver with '{}' permission?",
                permission.as_label()
            )),
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

    // Dispute finalization popup (bond slash submenu overlays when open)
    if let UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization {
        dispute_id,
        selected_button_index,
        bond,
        slash_submenu_open,
        slash_submenu_index,
    }) = &app.mode
    {
        dispute_finalization_popup::render_finalization_popup(
            f,
            app,
            dispute_id,
            *selected_button_index,
            *bond,
            *slash_submenu_open,
            *slash_submenu_index,
        );
    }

    // Dispute finalization confirmation popup
    if let UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute {
        dispute_id,
        is_settle,
        bond,
        selected_button,
    }) = &app.mode
    {
        dispute_finalization_confirm::render_finalization_confirm(
            f,
            app,
            dispute_id,
            *is_settle,
            *bond,
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

    if let UiMode::RatingOrder(state) = &app.mode {
        tabs::tab_content::render_rating_order(f, state);
    }

    // Non-blocking offline overlay (does not affect current mode).
    if let Some(message) = app.offline_overlay_message.as_deref() {
        offline_overlay::render_offline_overlay(f, message);
    }
}

fn render_add_solver_popup(f: &mut ratatui::Frame, add_solver_state: &AddSolverState) {
    let area = f.area();
    let popup = crate::ui::helpers::create_centered_popup(area, 84, 13);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("Add Solver")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    )
    .split(popup);

    f.render_widget(
        Paragraph::new("Enter solver pubkey (npub... or hex):")
            .style(
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        chunks[1],
    );

    let input_display: &str = if add_solver_state.key_input.key_input.is_empty() {
        "npub... / hex..."
    } else {
        add_solver_state.key_input.key_input.as_str()
    };

    f.render_widget(
        Paragraph::new(input_display)
            .style(
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().fg(PRIMARY_COLOR)),
            ),
        chunks[2],
    );

    let is_read_selected = add_solver_state.permission == SolverPermission::Read;
    let read_style = if is_read_selected {
        Style::default()
            .fg(Color::Black)
            .bg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let read_write_style = if is_read_selected {
        Style::default().fg(Color::Gray)
    } else {
        Style::default()
            .fg(Color::Black)
            .bg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD)
    };

    let permission_line = Line::from(vec![
        Span::styled("Permission: ", Style::default().fg(Color::White)),
        Span::styled(" [ Read ] ", read_style),
        Span::styled("   ", Style::default()),
        Span::styled(" [ Read-Write ] ", read_write_style),
    ]);
    f.render_widget(
        Paragraph::new(permission_line).alignment(Alignment::Center),
        chunks[4],
    );

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "(Left/Right to switch)",
            Style::default().fg(Color::DarkGray),
        )))
        .alignment(Alignment::Center),
        chunks[5],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to continue", Style::default()),
        ]))
        .alignment(Alignment::Center),
        chunks[6],
    );

    crate::ui::helpers::render_help_text(f, chunks[7], "Press ", "Esc", " to cancel");
}
