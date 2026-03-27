use crate::ui::{
    AdminMode, AdminTab, AppState, FormState, Tab, UiMode, UserMode, UserRole, UserTab,
};
use crossterm::event::KeyCode;
use mostro_core::prelude::*;
use std::sync::{Arc, Mutex};

/// Handle navigation keys (Left, Right, Up, Down)
pub fn handle_navigation(
    code: KeyCode,
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<mostro_core::prelude::Dispute>>>,
) {
    match code {
        KeyCode::Left => handle_left_key(app, orders),
        KeyCode::Right => handle_right_key(app, orders),
        KeyCode::Up => handle_up_key(app, orders, disputes),
        KeyCode::Down => handle_down_key(app, orders, disputes),
        _ => {}
    }
}

fn handle_left_key(app: &mut AppState, _orders: &Arc<Mutex<Vec<SmallOrder>>>) {
    match &mut app.mode {
        // From Create New Order form → single Left press moves to previous tab (e.g. Messages)
        UiMode::UserMode(UserMode::CreatingOrder(_))
            if matches!(app.active_tab, Tab::User(UserTab::CreateNewOrder)) =>
        {
            let prev_tab = app.active_tab;
            app.active_tab = app.active_tab.prev(app.user_role);
            handle_tab_switch(app, prev_tab);
            // Leave form mode
            app.mode = UiMode::UserMode(UserMode::Normal);
        }
        // In order confirmation popup, Left should only move the selection to YES,
        // not switch tabs.
        UiMode::UserMode(UserMode::ConfirmingOrder {
            ref mut selected_button,
            ..
        }) => {
            *selected_button = true;
        }
        UiMode::Normal
        | UiMode::UserMode(UserMode::Normal)
        | UiMode::AdminMode(AdminMode::Normal)
        | UiMode::AdminMode(AdminMode::ManagingDispute) => {
            let prev_tab = app.active_tab;
            app.active_tab = app.active_tab.prev(app.user_role);
            handle_tab_switch(app, prev_tab);
            // Auto-initialize form when switching to Create New Order tab (user mode only)
            if let Tab::User(UserTab::CreateNewOrder) = app.active_tab {
                let form = FormState::new_default_form();
                app.mode = UiMode::UserMode(UserMode::CreatingOrder(form));
            }
        }
        UiMode::UserMode(UserMode::TakingOrder(ref mut take_state)) => {
            // Switch to YES button (left side)
            take_state.selected_button = true;
        }
        UiMode::ViewingMessage(ref mut view_state) => {
            // Switch to YES button (left side)
            view_state.selected_button = true;
        }
        UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, ref mut selected_button))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, ref mut selected_button))
        | UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute {
            ref mut selected_button,
            ..
        })
        | UiMode::ConfirmMostroPubkey(_, ref mut selected_button)
        | UiMode::ConfirmRelay(_, ref mut selected_button)
        | UiMode::ConfirmCurrency(_, ref mut selected_button)
        | UiMode::ConfirmClearCurrencies(ref mut selected_button)
        | UiMode::ConfirmExit(ref mut selected_button) => {
            // Switch to YES button (left side)
            *selected_button = true;
        }
        UiMode::NewMessageNotification(_, _, _) => {
            // No action in notification mode
        }
        _ => {}
    }
}

fn handle_right_key(app: &mut AppState, _orders: &Arc<Mutex<Vec<SmallOrder>>>) {
    match &mut app.mode {
        // From Create New Order form → single Right press moves to next tab (Settings)
        UiMode::UserMode(UserMode::CreatingOrder(_))
            if matches!(app.active_tab, Tab::User(UserTab::CreateNewOrder)) =>
        {
            let prev_tab = app.active_tab;
            app.active_tab = app.active_tab.next(app.user_role);
            handle_tab_switch(app, prev_tab);
            // Leave form mode
            app.mode = UiMode::UserMode(UserMode::Normal);
        }
        // In order confirmation popup, Right should only move the selection to NO,
        // not switch tabs.
        UiMode::UserMode(UserMode::ConfirmingOrder {
            ref mut selected_button,
            ..
        }) => {
            *selected_button = false;
        }
        UiMode::Normal
        | UiMode::UserMode(UserMode::Normal)
        | UiMode::AdminMode(AdminMode::Normal)
        | UiMode::AdminMode(AdminMode::ManagingDispute) => {
            let prev_tab = app.active_tab;
            app.active_tab = app.active_tab.next(app.user_role);
            handle_tab_switch(app, prev_tab);
            // Auto-initialize form when switching to Create New Order tab (user mode only)
            if let Tab::User(UserTab::CreateNewOrder) = app.active_tab {
                let form = FormState::new_default_form();
                app.mode = UiMode::UserMode(UserMode::CreatingOrder(form));
            }
        }
        UiMode::UserMode(UserMode::TakingOrder(ref mut take_state)) => {
            // Switch to NO button (right side)
            take_state.selected_button = false;
        }
        UiMode::ViewingMessage(ref mut view_state) => {
            // Switch to NO button (right side)
            view_state.selected_button = false;
        }
        UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, ref mut selected_button))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, ref mut selected_button))
        | UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute {
            ref mut selected_button,
            ..
        })
        | UiMode::ConfirmMostroPubkey(_, ref mut selected_button)
        | UiMode::ConfirmRelay(_, ref mut selected_button)
        | UiMode::ConfirmCurrency(_, ref mut selected_button)
        | UiMode::ConfirmClearCurrencies(ref mut selected_button)
        | UiMode::ConfirmExit(ref mut selected_button) => {
            // Switch to NO button (right side)
            *selected_button = false;
        }
        UiMode::NewMessageNotification(_, _, _) => {
            // No action in notification mode
        }
        _ => {}
    }
}

fn handle_up_key(
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<Dispute>>>,
) {
    match &mut app.mode {
        UiMode::Normal
        | UiMode::UserMode(UserMode::Normal)
        | UiMode::AdminMode(AdminMode::Normal)
        | UiMode::AdminMode(AdminMode::ManagingDispute) => {
            if let Tab::User(UserTab::Orders) = app.active_tab {
                let orders_len = match orders.lock() {
                    Ok(g) => g.len(),
                    Err(e) => {
                        crate::util::request_fatal_restart(format!(
                            "Mostrix encountered an internal error (poisoned orders lock: {e}). Please restart the app."
                        ));
                        return;
                    }
                };
                if orders_len > 0 && app.selected_order_idx > 0 {
                    app.selected_order_idx -= 1;
                }
            } else if let Tab::Admin(AdminTab::DisputesPending) = app.active_tab {
                // Only count disputes with "initiated" status
                use mostro_core::prelude::*;
                use std::str::FromStr;
                let disputes_lock = match disputes.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        crate::util::request_fatal_restart(format!(
                            "Mostrix encountered an internal error (poisoned disputes lock: {e}). Please restart the app."
                        ));
                        return;
                    }
                };
                let initiated_count = disputes_lock
                    .iter()
                    .filter(|d| {
                        DisputeStatus::from_str(d.status.as_str())
                            .map(|s| s == DisputeStatus::Initiated)
                            .unwrap_or(false)
                    })
                    .count();
                if initiated_count == 0 {
                    app.selected_dispute_idx = 0;
                } else {
                    // Ensure index doesn't go below 0
                    if app.selected_dispute_idx > 0 {
                        app.selected_dispute_idx -= 1;
                    } else {
                        app.selected_dispute_idx = 0;
                    }
                    // Clamp to valid range
                    app.selected_dispute_idx = app
                        .selected_dispute_idx
                        .min(initiated_count.saturating_sub(1));
                }
            } else if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                if !app.admin_disputes_in_progress.is_empty() && app.selected_in_progress_idx > 0 {
                    app.selected_in_progress_idx -= 1;
                }
            } else if let Tab::User(UserTab::Messages) = app.active_tab {
                let mut messages = match app.messages.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        crate::util::request_fatal_restart(format!(
                            "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
                        ));
                        return;
                    }
                };
                let messages_len = messages.len();
                if messages_len == 0 {
                    app.selected_message_idx = 0;
                } else {
                    if app.selected_message_idx >= messages_len {
                        app.selected_message_idx = messages_len.saturating_sub(1);
                    } else if app.selected_message_idx > 0 {
                        app.selected_message_idx -= 1;
                    }
                    // Mark selected message as read
                    if let Some(msg) = messages.get_mut(app.selected_message_idx) {
                        msg.read = true;
                    }
                }
            } else if matches!(
                app.active_tab,
                Tab::Admin(AdminTab::Settings) | Tab::User(UserTab::Settings)
            ) && app.selected_settings_option > 0
            {
                app.selected_settings_option -= 1;
            }
        }
        UiMode::UserMode(UserMode::CreatingOrder(form)) => {
            form.focused = form.focused.prev(form.use_range);
        }
        UiMode::UserMode(UserMode::ConfirmingOrder { .. })
        | UiMode::UserMode(UserMode::TakingOrder(_))
        | UiMode::UserMode(UserMode::WaitingForMostro(_))
        | UiMode::UserMode(UserMode::WaitingTakeOrder(_))
        | UiMode::UserMode(UserMode::WaitingAddInvoice)
        | UiMode::HelpPopup(..)
        | UiMode::OperationResult(_)
        | UiMode::NewMessageNotification(_, _, _)
        | UiMode::ViewingMessage(_)
        | UiMode::RatingOrder(_)
        | UiMode::AdminMode(AdminMode::AddSolver(_))
        | UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, _))
        | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, _))
        | UiMode::AdminMode(AdminMode::ConfirmTakeDispute(_, _))
        | UiMode::AdminMode(AdminMode::WaitingTakeDispute(_))
        | UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization { .. })
        | UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute { .. })
        | UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(_))
        | UiMode::SaveAttachmentPopup(_)
        | UiMode::ObserverSaveAttachmentPopup(_)
        | UiMode::AddMostroPubkey(_)
        | UiMode::ConfirmMostroPubkey(_, _)
        | UiMode::AddRelay(_)
        | UiMode::ConfirmRelay(_, _)
        | UiMode::AddCurrency(_)
        | UiMode::ConfirmCurrency(_, _)
        | UiMode::ConfirmClearCurrencies(_)
        | UiMode::ConfirmGenerateNewKeys(_)
        | UiMode::BackupNewKeys(_)
        | UiMode::ConfirmExit(_) => {
            // No navigation in these modes
        }
    }
}

fn handle_down_key(
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<Dispute>>>,
) {
    match &mut app.mode {
        UiMode::Normal
        | UiMode::UserMode(UserMode::Normal)
        | UiMode::AdminMode(AdminMode::Normal) => {
            if let Tab::User(UserTab::Orders) = app.active_tab {
                let orders_len = match orders.lock() {
                    Ok(g) => g.len(),
                    Err(e) => {
                        crate::util::request_fatal_restart(format!(
                            "Mostrix encountered an internal error (poisoned orders lock: {e}). Please restart the app."
                        ));
                        return;
                    }
                };
                if orders_len > 0 && app.selected_order_idx < orders_len.saturating_sub(1) {
                    app.selected_order_idx += 1;
                }
            } else if let Tab::Admin(AdminTab::DisputesPending) = app.active_tab {
                // Only count disputes with "initiated" status
                use mostro_core::prelude::*;
                use std::str::FromStr;
                let disputes_lock = match disputes.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        crate::util::request_fatal_restart(format!(
                            "Mostrix encountered an internal error (poisoned disputes lock: {e}). Please restart the app."
                        ));
                        return;
                    }
                };
                let initiated_count = disputes_lock
                    .iter()
                    .filter(|d| {
                        DisputeStatus::from_str(d.status.as_str())
                            .map(|s| s == DisputeStatus::Initiated)
                            .unwrap_or(false)
                    })
                    .count();
                if initiated_count == 0 {
                    app.selected_dispute_idx = 0;
                } else {
                    // Ensure index doesn't exceed bounds
                    if app.selected_dispute_idx < initiated_count.saturating_sub(1) {
                        app.selected_dispute_idx += 1;
                    } else {
                        app.selected_dispute_idx = initiated_count.saturating_sub(1);
                    }
                    // Clamp to valid range
                    app.selected_dispute_idx = app
                        .selected_dispute_idx
                        .min(initiated_count.saturating_sub(1));
                }
            } else if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                if !app.admin_disputes_in_progress.is_empty()
                    && app.selected_in_progress_idx
                        < app.admin_disputes_in_progress.len().saturating_sub(1)
                {
                    app.selected_in_progress_idx += 1;
                }
            } else if let Tab::User(UserTab::Messages) = app.active_tab {
                let mut messages = match app.messages.lock() {
                    Ok(g) => g,
                    Err(e) => {
                        crate::util::request_fatal_restart(format!(
                            "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
                        ));
                        return;
                    }
                };
                let messages_len = messages.len();
                if messages_len == 0 {
                    app.selected_message_idx = 0;
                } else {
                    if app.selected_message_idx >= messages_len {
                        app.selected_message_idx = messages_len.saturating_sub(1);
                    } else if app.selected_message_idx < messages_len.saturating_sub(1) {
                        app.selected_message_idx += 1;
                    }
                    // Mark selected message as read
                    if let Some(msg) = messages.get_mut(app.selected_message_idx) {
                        msg.read = true;
                    }
                }
            } else if matches!(
                app.active_tab,
                Tab::Admin(AdminTab::Settings) | Tab::User(UserTab::Settings)
            ) {
                // Derive max index from actual options count (max_index = count - 1)
                let options_count = match app.user_role {
                    UserRole::Admin => crate::ui::tabs::settings_tab::ADMIN_SETTINGS_OPTIONS_COUNT,
                    UserRole::User => crate::ui::tabs::settings_tab::USER_SETTINGS_OPTIONS_COUNT,
                };
                let max_index = options_count.saturating_sub(1);
                if app.selected_settings_option < max_index {
                    app.selected_settings_option += 1;
                }
            }
        }
        UiMode::UserMode(UserMode::CreatingOrder(form)) => {
            form.focused = form.focused.next(form.use_range);
        }
        UiMode::AdminMode(AdminMode::ManagingDispute) => {
            // Navigate within disputes in progress list
            if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                if !app.admin_disputes_in_progress.is_empty()
                    && app.selected_in_progress_idx
                        < app.admin_disputes_in_progress.len().saturating_sub(1)
                {
                    app.selected_in_progress_idx += 1;
                }
            }
        }
        UiMode::UserMode(UserMode::ConfirmingOrder { .. })
        | UiMode::UserMode(UserMode::TakingOrder(_))
        | UiMode::UserMode(UserMode::WaitingForMostro(_))
        | UiMode::UserMode(UserMode::WaitingTakeOrder(_))
        | UiMode::UserMode(UserMode::WaitingAddInvoice)
        | UiMode::HelpPopup(..)
        | UiMode::OperationResult(_)
        | UiMode::NewMessageNotification(_, _, _)
        | UiMode::ViewingMessage(_)
        | UiMode::RatingOrder(_)
        | UiMode::AdminMode(AdminMode::AddSolver(_))
        | UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, _))
        | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, _))
        | UiMode::AdminMode(AdminMode::ConfirmTakeDispute(_, _))
        | UiMode::AdminMode(AdminMode::WaitingTakeDispute(_))
        | UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization { .. })
        | UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute { .. })
        | UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(_))
        | UiMode::SaveAttachmentPopup(_)
        | UiMode::ObserverSaveAttachmentPopup(_)
        | UiMode::AddMostroPubkey(_)
        | UiMode::ConfirmMostroPubkey(_, _)
        | UiMode::AddRelay(_)
        | UiMode::ConfirmRelay(_, _)
        | UiMode::AddCurrency(_)
        | UiMode::ConfirmCurrency(_, _)
        | UiMode::ConfirmClearCurrencies(_)
        | UiMode::ConfirmGenerateNewKeys(_)
        | UiMode::BackupNewKeys(_)
        | UiMode::ConfirmExit(_) => {
            // No navigation in these modes
        }
    }
}

fn handle_tab_switch(app: &mut AppState, prev_tab: Tab) {
    // Clear pending notifications and mark messages as read when switching to Messages tab (user mode only)
    if let Tab::User(UserTab::Messages) = app.active_tab {
        if let Tab::User(UserTab::Messages) = prev_tab {
            // Already on Messages tab, do nothing
        } else {
            match app.pending_notifications.lock() {
                Ok(mut pending) => {
                    *pending = 0;
                }
                Err(e) => {
                    crate::util::request_fatal_restart(format!(
                        "Mostrix encountered an internal error (poisoned pending notifications lock: {e}). Please restart the app."
                    ));
                    return;
                }
            }
            // Mark all messages as read when entering Messages tab
            match app.messages.lock() {
                Ok(mut messages) => {
                    for msg in messages.iter_mut() {
                        msg.read = true;
                    }
                }
                Err(e) => {
                    crate::util::request_fatal_restart(format!(
                        "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
                    ));
                    return;
                }
            }
        }
    }
    // Set mode to ManagingDispute when switching to Disputes in Progress tab
    if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
        if let Tab::Admin(AdminTab::DisputesInProgress) = prev_tab {
            // Already on Disputes in Progress tab, do nothing
        } else {
            app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
        }
    } else if let Tab::Admin(AdminTab::DisputesInProgress) = prev_tab {
        // Switching away from Disputes in Progress tab, reset to Normal mode
        if matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute)) {
            app.mode = UiMode::AdminMode(AdminMode::Normal);
        }
    }

    // Clear transient observer state when leaving Observer tab
    if let Tab::Admin(AdminTab::Observer) = prev_tab {
        app.clear_observer_secrets();
    }
}

/// Handle Tab and BackTab keys
pub fn handle_tab_navigation(code: KeyCode, app: &mut AppState) {
    match code {
        KeyCode::Tab => {
            if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                app.active_chat_party = match app.active_chat_party {
                    crate::ui::ChatParty::Buyer => crate::ui::ChatParty::Seller,
                    crate::ui::ChatParty::Seller => crate::ui::ChatParty::Buyer,
                };
                // Reset scroll/selection when switching parties (will be set in render)
                app.admin_chat_selected_message_idx = None;
            } else if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                form.focused = form.focused.next(form.use_range);
            }
        }
        KeyCode::BackTab => {
            if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                app.active_chat_party = match app.active_chat_party {
                    crate::ui::ChatParty::Buyer => crate::ui::ChatParty::Seller,
                    crate::ui::ChatParty::Seller => crate::ui::ChatParty::Buyer,
                };
                // Reset scroll/selection when switching parties (will be set in render)
                app.admin_chat_selected_message_idx = None;
            } else if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                form.focused = form.focused.prev(form.use_range);
            }
        }
        _ => {}
    }
}
