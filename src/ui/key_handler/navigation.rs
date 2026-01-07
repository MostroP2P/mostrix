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
        UiMode::Normal
        | UiMode::UserMode(UserMode::Normal)
        | UiMode::AdminMode(AdminMode::Normal) => {
            let prev_tab = app.active_tab;
            app.active_tab = app.active_tab.prev(app.user_role);
            handle_tab_switch(app, prev_tab);
            // Exit form mode when leaving Create New Order tab (user mode only)
            if let Tab::User(UserTab::CreateNewOrder) = app.active_tab {
                // Stay in creating order mode
            } else {
                match app.user_role {
                    UserRole::User => app.mode = UiMode::UserMode(UserMode::Normal),
                    UserRole::Admin => app.mode = UiMode::AdminMode(AdminMode::Normal),
                }
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
        | UiMode::ConfirmMostroPubkey(_, ref mut selected_button)
        | UiMode::ConfirmRelay(_, ref mut selected_button) => {
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
        UiMode::Normal
        | UiMode::UserMode(UserMode::Normal)
        | UiMode::AdminMode(AdminMode::Normal) => {
            let prev_tab = app.active_tab;
            app.active_tab = app.active_tab.next(app.user_role);
            handle_tab_switch(app, prev_tab);
            // Auto-initialize form when switching to Create New Order tab (user mode only)
            if let Tab::User(UserTab::CreateNewOrder) = app.active_tab {
                let form = FormState {
                    kind: "buy".to_string(),
                    fiat_code: "USD".to_string(),
                    amount: "0".to_string(),
                    premium: "0".to_string(),
                    expiration_days: "1".to_string(),
                    focused: 1,
                    ..Default::default()
                };
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
        | UiMode::ConfirmMostroPubkey(_, ref mut selected_button)
        | UiMode::ConfirmRelay(_, ref mut selected_button) => {
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
        | UiMode::AdminMode(AdminMode::Normal) => {
            if let Tab::User(UserTab::Orders) = app.active_tab {
                let orders_len = orders.lock().unwrap().len();
                if orders_len > 0 && app.selected_order_idx > 0 {
                    app.selected_order_idx -= 1;
                }
            } else if let Tab::Admin(AdminTab::Disputes) = app.active_tab {
                let disputes_len = disputes.lock().unwrap().len();
                if disputes_len > 0 && app.selected_dispute_idx > 0 {
                    app.selected_dispute_idx -= 1;
                }
            } else if let Tab::User(UserTab::Messages) = app.active_tab {
                let mut messages = app.messages.lock().unwrap();
                let messages_len = messages.len();
                if messages_len > 0 && app.selected_message_idx > 0 {
                    app.selected_message_idx -= 1;
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
            if form.focused > 0 {
                form.focused -= 1;
                // Skip field 4 if not using range (go from 5 to 3)
                if form.focused == 4 && !form.use_range {
                    form.focused = 3;
                }
            }
        }
        UiMode::UserMode(UserMode::ConfirmingOrder(_))
        | UiMode::UserMode(UserMode::TakingOrder(_))
        | UiMode::UserMode(UserMode::WaitingForMostro(_))
        | UiMode::UserMode(UserMode::WaitingTakeOrder(_))
        | UiMode::UserMode(UserMode::WaitingAddInvoice)
        | UiMode::OrderResult(_)
        | UiMode::NewMessageNotification(_, _, _)
        | UiMode::ViewingMessage(_)
        | UiMode::AdminMode(AdminMode::AddSolver(_))
        | UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, _))
        | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, _))
        | UiMode::AddMostroPubkey(_)
        | UiMode::ConfirmMostroPubkey(_, _)
        | UiMode::AddRelay(_)
        | UiMode::ConfirmRelay(_, _) => {
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
                let orders_len = orders.lock().unwrap().len();
                if orders_len > 0 && app.selected_order_idx < orders_len.saturating_sub(1) {
                    app.selected_order_idx += 1;
                }
            } else if let Tab::Admin(AdminTab::Disputes) = app.active_tab {
                let disputes_len = disputes.lock().unwrap().len();
                if disputes_len > 0 && app.selected_dispute_idx < disputes_len.saturating_sub(1) {
                    app.selected_dispute_idx += 1;
                }
            } else if let Tab::User(UserTab::Messages) = app.active_tab {
                let mut messages = app.messages.lock().unwrap();
                let messages_len = messages.len();
                if messages_len > 0 && app.selected_message_idx < messages_len.saturating_sub(1) {
                    app.selected_message_idx += 1;
                    // Mark selected message as read
                    if let Some(msg) = messages.get_mut(app.selected_message_idx) {
                        msg.read = true;
                    }
                }
            } else if matches!(
                app.active_tab,
                Tab::Admin(AdminTab::Settings) | Tab::User(UserTab::Settings)
            ) {
                let max_options = if app.user_role == UserRole::Admin {
                    3
                } else {
                    1
                };
                if app.selected_settings_option < max_options {
                    app.selected_settings_option += 1;
                }
            }
        }
        UiMode::UserMode(UserMode::CreatingOrder(form)) => {
            if form.focused < 8 {
                form.focused += 1;
                // Skip field 4 if not using range (go from 3 to 5)
                if form.focused == 4 && !form.use_range {
                    form.focused = 5;
                }
            }
        }
        UiMode::UserMode(UserMode::ConfirmingOrder(_))
        | UiMode::UserMode(UserMode::TakingOrder(_))
        | UiMode::UserMode(UserMode::WaitingForMostro(_))
        | UiMode::UserMode(UserMode::WaitingTakeOrder(_))
        | UiMode::UserMode(UserMode::WaitingAddInvoice)
        | UiMode::OrderResult(_)
        | UiMode::NewMessageNotification(_, _, _)
        | UiMode::ViewingMessage(_)
        | UiMode::AdminMode(AdminMode::AddSolver(_))
        | UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, _))
        | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, _))
        | UiMode::AddMostroPubkey(_)
        | UiMode::ConfirmMostroPubkey(_, _)
        | UiMode::AddRelay(_)
        | UiMode::ConfirmRelay(_, _) => {
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
            let mut pending = app.pending_notifications.lock().unwrap();
            *pending = 0;
            // Mark all messages as read when entering Messages tab
            let mut messages = app.messages.lock().unwrap();
            for msg in messages.iter_mut() {
                msg.read = true;
            }
        }
    }
}

/// Handle Tab and BackTab keys
pub fn handle_tab_navigation(code: KeyCode, app: &mut AppState) {
    match code {
        KeyCode::Tab => {
            if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                form.focused = (form.focused + 1) % 9;
                // Skip field 4 if not using range
                if form.focused == 4 && !form.use_range {
                    form.focused = 5;
                }
            }
        }
        KeyCode::BackTab => {
            if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                form.focused = if form.focused == 0 {
                    8
                } else {
                    form.focused - 1
                };
                // Skip field 4 if not using range
                if form.focused == 4 && !form.use_range {
                    form.focused = 3;
                }
            }
        }
        _ => {}
    }
}
