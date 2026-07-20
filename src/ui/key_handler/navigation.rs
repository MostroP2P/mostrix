use crate::ui::helpers::active_order_chat_list_len;
use crate::ui::orders::strip_new_order_messages_and_clamp_selected;
use crate::ui::{
    AdminMode, AdminTab, AppState, FormState, Tab, UiMode, UserMode, UserRole, UserTab,
    ViewingMessageButtonSelection,
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
    // Leaving Create New Order silently keeps the draft for when the user returns.
    if let UiMode::UserMode(UserMode::CreatingOrder(form)) = &app.mode {
        if matches!(app.active_tab, Tab::User(UserTab::CreateNewOrder)) {
            save_and_leave_creating_order(app, form.clone(), true);
            return;
        }
    }
    match &mut app.mode {
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
                app.mode = UiMode::UserMode(UserMode::CreatingOrder(restore_or_new_form(app)));
            }
        }
        UiMode::UserMode(UserMode::TakingOrder(ref mut take_state)) => {
            // Switch to YES button (left side)
            take_state.selected_button = true;
        }
        UiMode::ViewingMessage(ref mut view_state) => match &mut view_state.button_selection {
            ViewingMessageButtonSelection::Two { yes_selected } => {
                *yes_selected = true;
            }
            selection @ ViewingMessageButtonSelection::Three(_) => {
                selection.cycle_three_prev();
            }
        },
        UiMode::AdminMode(AdminMode::ConfirmAddSolver {
            ref mut selected_button,
            ..
        })
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, ref mut selected_button))
        | UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute {
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
    // Leaving Create New Order silently keeps the draft for when the user returns.
    if let UiMode::UserMode(UserMode::CreatingOrder(form)) = &app.mode {
        if matches!(app.active_tab, Tab::User(UserTab::CreateNewOrder)) {
            save_and_leave_creating_order(app, form.clone(), false);
            return;
        }
    }
    match &mut app.mode {
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
                app.mode = UiMode::UserMode(UserMode::CreatingOrder(restore_or_new_form(app)));
            }
        }
        UiMode::UserMode(UserMode::TakingOrder(ref mut take_state)) => {
            // Switch to NO button (right side)
            take_state.selected_button = false;
        }
        UiMode::ViewingMessage(ref mut view_state) => match &mut view_state.button_selection {
            ViewingMessageButtonSelection::Two { yes_selected } => {
                *yes_selected = false;
            }
            selection @ ViewingMessageButtonSelection::Three(_) => {
                selection.cycle_three_next();
            }
        },
        UiMode::AdminMode(AdminMode::ConfirmAddSolver {
            ref mut selected_button,
            ..
        })
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, ref mut selected_button))
        | UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute {
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
                strip_new_order_messages_and_clamp_selected(
                    &mut messages,
                    &mut app.selected_message_idx,
                );
                let messages_len = messages.len();
                if messages_len == 0 {
                    app.selected_message_idx = 0;
                } else {
                    if app.selected_message_idx >= messages_len {
                        app.selected_message_idx = messages_len.saturating_sub(1);
                    } else if app.selected_message_idx > 0 {
                        app.selected_message_idx -= 1;
                    }
                    if let Some(msg) = messages.get_mut(app.selected_message_idx) {
                        msg.read = true;
                    }
                }
            } else if let Tab::User(UserTab::MyTrades) = app.active_tab {
                let n = active_order_chat_list_len(app);
                if n > 0 && app.selected_order_chat_idx > 0 {
                    app.selected_order_chat_idx -= 1;
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
        | UiMode::SettingsInstructionsPopup(..)
        | UiMode::OperationResult(_)
        | UiMode::NewMessageNotification(_, _, _)
        | UiMode::ViewingMessage(_)
        | UiMode::RatingOrder(_)
        | UiMode::AdminMode(AdminMode::AddSolver(_))
        | UiMode::AdminMode(AdminMode::ConfirmAddSolver { .. })
        | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, _))
        | UiMode::AdminMode(AdminMode::ConfirmTakeDispute(_, _))
        | UiMode::AdminMode(AdminMode::WaitingTakeDispute(_))
        | UiMode::AdminMode(AdminMode::WaitingAddSolver)
        | UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization { .. })
        | UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute { .. })
        | UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(_))
        | UiMode::SaveAttachmentPopup(_)
        | UiMode::ObserverSaveAttachmentPopup(_)
        | UiMode::UserSaveAttachmentPopup(_, _)
        | UiMode::UserSendAttachmentPicker(_)
        | UiMode::AddMostroPubkey(_)
        | UiMode::ConfirmMostroPubkey(_, _)
        | UiMode::AddRelay(_)
        | UiMode::ConfirmRelay(_, _)
        | UiMode::AddLnAddress(_)
        | UiMode::ConfirmLnAddress(_, _)
        | UiMode::ConfirmSavedLnAddressForInvoice(_, _)
        | UiMode::ConfirmClearLnAddress(_)
        | UiMode::AddCurrency(_)
        | UiMode::ConfirmCurrency(_, _)
        | UiMode::ConfirmClearCurrencies(_)
        | UiMode::ConfirmDeleteHistoryOrder(_, _)
        | UiMode::ConfirmBulkDeleteHistory(_)
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
                strip_new_order_messages_and_clamp_selected(
                    &mut messages,
                    &mut app.selected_message_idx,
                );
                let messages_len = messages.len();
                if messages_len == 0 {
                    app.selected_message_idx = 0;
                } else {
                    if app.selected_message_idx >= messages_len {
                        app.selected_message_idx = messages_len.saturating_sub(1);
                    } else if app.selected_message_idx < messages_len.saturating_sub(1) {
                        app.selected_message_idx += 1;
                    }
                    if let Some(msg) = messages.get_mut(app.selected_message_idx) {
                        msg.read = true;
                    }
                }
            } else if let Tab::User(UserTab::MyTrades) = app.active_tab {
                let n = active_order_chat_list_len(app);
                if n > 0 && app.selected_order_chat_idx < n.saturating_sub(1) {
                    app.selected_order_chat_idx += 1;
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
        | UiMode::SettingsInstructionsPopup(..)
        | UiMode::OperationResult(_)
        | UiMode::NewMessageNotification(_, _, _)
        | UiMode::ViewingMessage(_)
        | UiMode::RatingOrder(_)
        | UiMode::AdminMode(AdminMode::AddSolver(_))
        | UiMode::AdminMode(AdminMode::ConfirmAddSolver { .. })
        | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, _))
        | UiMode::AdminMode(AdminMode::ConfirmTakeDispute(_, _))
        | UiMode::AdminMode(AdminMode::WaitingTakeDispute(_))
        | UiMode::AdminMode(AdminMode::WaitingAddSolver)
        | UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization { .. })
        | UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute { .. })
        | UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(_))
        | UiMode::SaveAttachmentPopup(_)
        | UiMode::ObserverSaveAttachmentPopup(_)
        | UiMode::UserSaveAttachmentPopup(_, _)
        | UiMode::UserSendAttachmentPicker(_)
        | UiMode::AddMostroPubkey(_)
        | UiMode::ConfirmMostroPubkey(_, _)
        | UiMode::AddRelay(_)
        | UiMode::ConfirmRelay(_, _)
        | UiMode::AddLnAddress(_)
        | UiMode::ConfirmLnAddress(_, _)
        | UiMode::ConfirmSavedLnAddressForInvoice(_, _)
        | UiMode::ConfirmClearLnAddress(_)
        | UiMode::AddCurrency(_)
        | UiMode::ConfirmCurrency(_, _)
        | UiMode::ConfirmClearCurrencies(_)
        | UiMode::ConfirmDeleteHistoryOrder(_, _)
        | UiMode::ConfirmBulkDeleteHistory(_)
        | UiMode::ConfirmGenerateNewKeys(_)
        | UiMode::BackupNewKeys(_)
        | UiMode::ConfirmExit(_) => {
            // No navigation in these modes
        }
    }
}

/// Restore a preserved New Order draft (consuming it) or build a fresh form.
fn restore_or_new_form(app: &mut AppState) -> FormState {
    app.order_form_draft
        .take()
        .unwrap_or_else(FormState::new_default_form)
}

/// Save the current form as a draft and switch to the adjacent tab.
fn save_and_leave_creating_order(app: &mut AppState, form: FormState, to_prev: bool) {
    app.order_form_draft = Some(form);
    leave_creating_order_to_adjacent_tab(app, to_prev);
}

/// Leave the Create New Order form for the previous/next tab, returning to Normal mode.
fn leave_creating_order_to_adjacent_tab(app: &mut AppState, to_prev: bool) {
    let prev_tab = app.active_tab;
    app.active_tab = if to_prev {
        app.active_tab.prev(app.user_role)
    } else {
        app.active_tab.next(app.user_role)
    };
    handle_tab_switch(app, prev_tab);
    app.mode = UiMode::UserMode(UserMode::Normal);
}

pub(crate) fn handle_tab_switch(app: &mut AppState, prev_tab: Tab) {
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
                    strip_new_order_messages_and_clamp_selected(
                        &mut messages,
                        &mut app.selected_message_idx,
                    );
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
