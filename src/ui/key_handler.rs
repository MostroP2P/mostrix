use crate::ui::{
    AdminMode, AdminTab, AppState, FormState, MessageViewState, Tab, TakeOrderState, UiMode,
    UserMode, UserRole, UserTab,
};
use crate::util::order_utils::{execute_add_invoice, execute_send_msg};
use crate::SETTINGS;
use crossterm::event::{KeyCode, KeyEvent};
use dirs;
use mostro_core::prelude::*;
use std::sync::{Arc, Mutex};
use toml;
use uuid::Uuid;

/// Handle invoice input for AddInvoice notifications
/// Returns true if the key was handled and should skip further processing
pub fn handle_invoice_input(
    code: KeyCode,
    invoice_state: &mut crate::ui::InvoiceInputState,
) -> bool {
    // Clear the just_pasted flag on any key press (except Enter)
    if code != KeyCode::Enter {
        invoice_state.just_pasted = false;
    }

    // Ignore Enter if it comes immediately after paste
    if code == KeyCode::Enter && invoice_state.just_pasted {
        invoice_state.just_pasted = false;
        return true; // Skip processing this Enter key
    }

    // Handle character input
    match code {
        KeyCode::Char(c) => {
            invoice_state.invoice_input.push(c);
            true // Skip further processing
        }
        KeyCode::Backspace => {
            invoice_state.invoice_input.pop();
            true // Skip further processing
        }
        _ => false, // Let Enter and Esc fall through to app logic
    }
}

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
        | UiMode::ViewingMessage(_) => {
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
        | UiMode::ViewingMessage(_) => {
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

/// Handle Enter key - dispatches to mode-specific handlers
pub fn handle_enter_key(
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    pool: &sqlx::SqlitePool,
    client: &nostr_sdk::Client,
    _settings: &crate::settings::Settings,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &tokio::sync::mpsc::UnboundedSender<crate::ui::OrderResult>,
) {
    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::Normal),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    match std::mem::replace(&mut app.mode, default_mode.clone()) {
        UiMode::Normal
        | UiMode::UserMode(UserMode::Normal)
        | UiMode::AdminMode(AdminMode::Normal) => {
            handle_enter_normal_mode(app, orders);
        }
        UiMode::UserMode(UserMode::CreatingOrder(form)) => {
            handle_enter_creating_order(app, &form);
        }
        UiMode::UserMode(UserMode::ConfirmingOrder(_)) => {
            // Enter acts as Yes in confirmation - handled by 'y' key
            app.mode = default_mode;
        }
        UiMode::UserMode(UserMode::TakingOrder(take_state)) => {
            handle_enter_taking_order(
                app,
                take_state,
                pool,
                client,
                _settings,
                mostro_pubkey,
                order_result_tx,
            );
        }
        UiMode::UserMode(UserMode::WaitingForMostro(_))
        | UiMode::UserMode(UserMode::WaitingTakeOrder(_))
        | UiMode::UserMode(UserMode::WaitingAddInvoice) => {
            // No action while waiting
            app.mode = default_mode;
        }
        UiMode::OrderResult(_) => {
            // Close result popup, return to first tab for current role
            app.active_tab = Tab::first(app.user_role);
        }
        UiMode::NewMessageNotification(notification, action, mut invoice_state) => {
            handle_enter_message_notification(
                app,
                client,
                pool,
                &action,
                &mut invoice_state,
                mostro_pubkey,
                notification.order_id,
                order_result_tx,
            );
            // Mode is updated inside handle_enter_message_notification
        }
        UiMode::ViewingMessage(view_state) => {
            // Enter confirms the selected button (YES or NO)
            handle_enter_viewing_message(
                app,
                &view_state,
                pool,
                client,
                mostro_pubkey,
                order_result_tx,
            );
            // Mode is updated inside handle_enter_viewing_message
        }
    }
}

fn handle_enter_normal_mode(app: &mut AppState, orders: &Arc<Mutex<Vec<SmallOrder>>>) {
    // Show take order popup when Enter is pressed in Orders tab (user mode only)
    if let Tab::User(UserTab::Orders) = app.active_tab {
        let orders_lock = orders.lock().unwrap();
        if let Some(order) = orders_lock.get(app.selected_order_idx) {
            let is_range_order = order.min_amount.is_some() || order.max_amount.is_some();
            let take_state = TakeOrderState {
                order: order.clone(),
                amount_input: String::new(),
                is_range_order,
                validation_error: None,
                selected_button: true, // Default to YES
            };
            app.mode = UiMode::UserMode(UserMode::TakingOrder(take_state));
        }
    } else if let Tab::User(UserTab::Messages) = app.active_tab {
        let messages_lock = app.messages.lock().unwrap();
        if let Some(msg) = messages_lock.get(app.selected_message_idx) {
            let inner_message_kind = msg.message.get_inner_message_kind();
            let action = inner_message_kind.action.clone();
            if matches!(action, Action::AddInvoice | Action::PayInvoice) {
                // Show invoice/payment popup for actionable messages
                let notification = crate::ui::order_message_to_notification(msg);
                let action = notification.action.clone();
                let invoice_state = crate::ui::InvoiceInputState {
                    invoice_input: String::new(),
                    focused: matches!(action, Action::AddInvoice),
                    just_pasted: false,
                    copied_to_clipboard: false,
                };

                app.mode = UiMode::NewMessageNotification(notification, action, invoice_state);
            } else {
                // Show simple message view popup for other message types
                let notification = crate::ui::order_message_to_notification(msg);
                let view_state = crate::ui::MessageViewState {
                    message_content: notification.message_preview,
                    order_id: notification.order_id,
                    action: notification.action,
                    selected_button: true, // Default to YES
                };
                app.mode = UiMode::ViewingMessage(view_state);
            }
        }
    }
}

fn handle_enter_creating_order(app: &mut AppState, form: &FormState) {
    // Show confirmation popup when Enter is pressed
    if let Tab::User(UserTab::CreateNewOrder) = app.active_tab {
        app.mode = UiMode::UserMode(UserMode::ConfirmingOrder(form.clone()));
    } else {
        app.mode = UiMode::UserMode(UserMode::CreatingOrder(form.clone()));
    }
}

fn handle_enter_taking_order(
    app: &mut AppState,
    take_state: TakeOrderState,
    pool: &sqlx::SqlitePool,
    client: &nostr_sdk::Client,
    _settings: &crate::settings::Settings,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &tokio::sync::mpsc::UnboundedSender<crate::ui::OrderResult>,
) {
    // Enter confirms the selected button
    if take_state.selected_button {
        // YES selected - check validation and proceed
        if take_state.is_range_order {
            if take_state.amount_input.is_empty() {
                // Can't proceed without amount
                app.mode = UiMode::UserMode(UserMode::TakingOrder(take_state));
                return;
            }
            if take_state.validation_error.is_some() {
                // Can't proceed with invalid amount
                app.mode = UiMode::UserMode(UserMode::TakingOrder(take_state));
                return;
            }
        }
        // Proceed with taking the order
        let take_state_clone = take_state.clone();
        app.mode = UiMode::UserMode(UserMode::WaitingTakeOrder(take_state_clone.clone()));

        // Parse amount if it's a range order
        let amount = if take_state_clone.is_range_order {
            take_state_clone.amount_input.trim().parse::<i64>().ok()
        } else {
            None
        };

        // For buy orders (taking sell), we'd need invoice, but for now we'll pass None
        // TODO: Add invoice input for buy orders
        let invoice = None;

        // Spawn async task to take order
        let pool_clone = pool.clone();
        let client_clone = client.clone();
        let result_tx = order_result_tx.clone();

        tokio::spawn(async move {
            match crate::util::take_order(
                &pool_clone,
                &client_clone,
                SETTINGS.get().unwrap(),
                mostro_pubkey,
                &take_state_clone.order,
                amount,
                invoice,
            )
            .await
            {
                Ok(result) => {
                    let _ = result_tx.send(result);
                }
                Err(e) => {
                    log::error!("Failed to take order: {}", e);
                    let _ = result_tx.send(crate::ui::OrderResult::Error(e.to_string()));
                }
            }
        });
    } else {
        // NO selected - cancel
        app.mode = UiMode::Normal;
    }
}

fn handle_enter_viewing_message(
    app: &mut AppState,
    view_state: &MessageViewState,
    pool: &sqlx::SqlitePool,
    client: &nostr_sdk::Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &tokio::sync::mpsc::UnboundedSender<crate::ui::OrderResult>,
) {
    // Only proceed if YES is selected
    if !view_state.selected_button {
        app.mode = UiMode::Normal;
        return;
    }

    // Map the action from the message to the action we need to send
    let action_to_send = match view_state.action {
        Action::HoldInvoicePaymentAccepted => Action::FiatSent,
        Action::FiatSentOk => Action::Release,
        _ => {
            let _ = order_result_tx.send(crate::ui::OrderResult::Error(
                "Invalid action for send message".to_string(),
            ));
            let default_mode = match app.user_role {
                UserRole::User => UiMode::UserMode(UserMode::Normal),
                UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
            };
            app.mode = default_mode;
            return;
        }
    };

    // Get order_id from view_state
    let Some(order_id) = view_state.order_id else {
        let _ = order_result_tx.send(crate::ui::OrderResult::Error(
            "No order ID in message".to_string(),
        ));
        let default_mode = match app.user_role {
            UserRole::User => UiMode::UserMode(UserMode::Normal),
            UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
        };
        app.mode = default_mode;
        return;
    };

    // Set waiting mode (user mode only)
    app.mode = UiMode::UserMode(UserMode::WaitingAddInvoice);

    // Spawn async task to send message
    let pool_clone = pool.clone();
    let client_clone = client.clone();
    let result_tx = order_result_tx.clone();

    tokio::spawn(async move {
        match execute_send_msg(
            &order_id,
            action_to_send,
            &pool_clone,
            &client_clone,
            mostro_pubkey,
        )
        .await
        {
            Ok(_) => {
                let _ = result_tx.send(crate::ui::OrderResult::Info(
                    "Message sent successfully".to_string(),
                ));
            }
            Err(e) => {
                log::error!("Failed to send message: {}", e);
                let _ = result_tx.send(crate::ui::OrderResult::Error(e.to_string()));
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn handle_enter_message_notification(
    app: &mut AppState,
    client: &nostr_sdk::Client,
    pool: &sqlx::SqlitePool,
    action: &mostro_core::prelude::Action,
    invoice_state: &mut crate::ui::InvoiceInputState,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_id: Option<Uuid>,
    order_result_tx: &tokio::sync::mpsc::UnboundedSender<crate::ui::OrderResult>,
) {
    match action {
        Action::AddInvoice => {
            // For AddInvoice, Enter submits the invoice
            let order_result_tx_clone = order_result_tx.clone();
            if !invoice_state.invoice_input.trim().is_empty() {
                if let Some(order_id) = order_id {
                    // Set waiting mode before sending invoice
                    app.mode = UiMode::UserMode(UserMode::WaitingAddInvoice);

                    // Send invoice to Mostro
                    let invoice_state_clone = invoice_state.clone();
                    let pool_clone = pool.clone();
                    let client_clone = client.clone();
                    tokio::spawn(async move {
                        match execute_add_invoice(
                            &order_id,
                            &invoice_state_clone.invoice_input,
                            &pool_clone,
                            &client_clone,
                            mostro_pubkey,
                        )
                        .await
                        {
                            Ok(_) => {
                                let _ = order_result_tx_clone.send(crate::ui::OrderResult::Info(
                                    "Invoice sent successfully".to_string(),
                                ));
                            }
                            Err(e) => {
                                log::error!("Failed to add invoice: {}", e);
                                let _ = order_result_tx_clone
                                    .send(crate::ui::OrderResult::Error(e.to_string()));
                            }
                        }
                    });
                }
            }
        }
        Action::PayInvoice => {}
        _ => {
            let _ =
                order_result_tx.send(crate::ui::OrderResult::Error("Invalid action".to_string()));
        }
    }
}

/// Handle Esc key
pub fn handle_esc_key(app: &mut AppState) -> bool {
    // Returns true if should continue, false if should break
    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::Normal),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    match &mut app.mode {
        UiMode::UserMode(UserMode::CreatingOrder(_)) => {
            app.mode = default_mode.clone();
            true
        }
        UiMode::UserMode(UserMode::ConfirmingOrder(form)) => {
            // Cancel confirmation, go back to form
            app.mode = UiMode::UserMode(UserMode::CreatingOrder(form.clone()));
            true
        }
        UiMode::UserMode(UserMode::TakingOrder(_)) => {
            // Cancel taking order, return to normal mode
            app.mode = default_mode.clone();
            true
        }
        UiMode::UserMode(UserMode::WaitingForMostro(_))
        | UiMode::UserMode(UserMode::WaitingTakeOrder(_))
        | UiMode::UserMode(UserMode::WaitingAddInvoice) => {
            // Can't cancel while waiting
            true
        }
        UiMode::OrderResult(_) => {
            // Close result popup, return to first tab for current role
            app.mode = default_mode.clone();
            app.active_tab = Tab::first(app.user_role);
            true
        }
        UiMode::NewMessageNotification(_, _, _) => {
            // Dismiss notification
            app.mode = UiMode::Normal;
            true
        }
        UiMode::ViewingMessage(_) => {
            // Dismiss message view popup
            app.mode = UiMode::Normal;
            true
        }
        _ => false, // Break the loop
    }
}

/// Handle character input for forms
pub fn handle_char_input(
    code: KeyCode,
    app: &mut AppState,
    validate_range_amount: &dyn Fn(&mut TakeOrderState),
) {
    match code {
        KeyCode::Char(' ') => {
            if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                if form.focused == 0 {
                    // Toggle buy/sell
                    form.kind = if form.kind.to_lowercase() == "buy" {
                        "sell".to_string()
                    } else {
                        "buy".to_string()
                    };
                } else if form.focused == 3 {
                    // Toggle range mode
                    form.use_range = !form.use_range;
                }
            }
        }
        KeyCode::Char(c) => {
            if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                if form.focused == 0 {
                    // ignore typing on toggle field
                } else {
                    let target = match form.focused {
                        1 => &mut form.fiat_code,
                        2 => &mut form.amount,
                        3 => &mut form.fiat_amount,
                        4 if form.use_range => &mut form.fiat_amount_max,
                        5 => &mut form.payment_method,
                        6 => &mut form.premium,
                        7 => &mut form.invoice,
                        8 => &mut form.expiration_days,
                        _ => unreachable!(),
                    };
                    target.push(c);
                }
            } else if let UiMode::UserMode(UserMode::TakingOrder(ref mut take_state)) = app.mode {
                // Allow typing in the amount input field for range orders
                if take_state.is_range_order {
                    // Only allow digits and decimal point
                    if c.is_ascii_digit() || c == '.' {
                        take_state.amount_input.push(c);
                        // Validate after typing
                        validate_range_amount(take_state);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Handle backspace for forms
pub fn handle_backspace(app: &mut AppState, validate_range_amount: &dyn Fn(&mut TakeOrderState)) {
    if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
        if form.focused == 0 {
            // ignore
        } else {
            let target = match form.focused {
                1 => &mut form.fiat_code,
                2 => &mut form.amount,
                3 => &mut form.fiat_amount,
                4 if form.use_range => &mut form.fiat_amount_max,
                5 => &mut form.payment_method,
                6 => &mut form.premium,
                7 => &mut form.invoice,
                8 => &mut form.expiration_days,
                _ => unreachable!(),
            };
            target.pop();
        }
    } else if let UiMode::UserMode(UserMode::TakingOrder(ref mut take_state)) = app.mode {
        // Allow backspace in the amount input field
        if take_state.is_range_order {
            take_state.amount_input.pop();
            // Validate after deletion
            validate_range_amount(take_state);
        }
    }
}

/// Handle 'y' key for confirmation
pub fn handle_confirm_key(
    app: &mut AppState,
    pool: &sqlx::SqlitePool,
    client: &nostr_sdk::Client,
    _settings: &crate::settings::Settings,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &tokio::sync::mpsc::UnboundedSender<crate::ui::OrderResult>,
) -> bool {
    // Returns true if should continue (skip further processing)
    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::Normal),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    match std::mem::replace(&mut app.mode, default_mode.clone()) {
        UiMode::UserMode(UserMode::ConfirmingOrder(form)) => {
            // User confirmed, send the order
            let form_clone = form.clone();
            app.mode = UiMode::UserMode(UserMode::WaitingForMostro(form_clone.clone()));

            // Spawn async task to send order
            let pool_clone = pool.clone();
            let client_clone = client.clone();
            let result_tx = order_result_tx.clone();

            tokio::spawn(async move {
                match crate::util::send_new_order(
                    &pool_clone,
                    &client_clone,
                    SETTINGS.get().unwrap(),
                    mostro_pubkey,
                    &form_clone,
                )
                .await
                {
                    Ok(result) => {
                        let _ = result_tx.send(result);
                    }
                    Err(e) => {
                        log::error!("Failed to send order: {}", e);
                        let _ = result_tx.send(crate::ui::OrderResult::Error(e.to_string()));
                    }
                }
            });
            true
        }
        UiMode::UserMode(UserMode::TakingOrder(take_state)) => {
            // User confirmed taking the order (same as Enter key)
            // Check validation first
            if take_state.is_range_order {
                if take_state.amount_input.is_empty() {
                    // Can't proceed without amount
                    app.mode = UiMode::UserMode(UserMode::TakingOrder(take_state));
                    return true;
                }
                if take_state.validation_error.is_some() {
                    // Can't proceed with invalid amount
                    app.mode = UiMode::UserMode(UserMode::TakingOrder(take_state));
                    return true;
                }
            }
            // Proceed with taking the order
            let take_state_clone = take_state.clone();
            app.mode = UiMode::UserMode(UserMode::WaitingTakeOrder(take_state_clone.clone()));

            // Parse amount if it's a range order
            let amount = if take_state_clone.is_range_order {
                take_state_clone.amount_input.trim().parse::<i64>().ok()
            } else {
                None
            };

            // For buy orders (taking sell), we'd need invoice, but for now we'll pass None
            // TODO: Add invoice input for buy orders
            let invoice = None;

            // Spawn async task to take order
            let pool_clone = pool.clone();
            let client_clone = client.clone();
            let result_tx = order_result_tx.clone();

            tokio::spawn(async move {
                match crate::util::take_order(
                    &pool_clone,
                    &client_clone,
                    SETTINGS.get().unwrap(),
                    mostro_pubkey,
                    &take_state_clone.order,
                    amount,
                    invoice,
                )
                .await
                {
                    Ok(result) => {
                        let _ = result_tx.send(result);
                    }
                    Err(e) => {
                        log::error!("Failed to take order: {}", e);
                        let _ = result_tx.send(crate::ui::OrderResult::Error(e.to_string()));
                    }
                }
            });
            true
        }
        mode => {
            app.mode = mode;
            false
        }
    }
}

/// Handle 'n' key for cancellation
pub fn handle_cancel_key(app: &mut AppState) {
    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::Normal),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    if let UiMode::UserMode(UserMode::ConfirmingOrder(form)) = &app.mode {
        // User cancelled, go back to form
        app.mode = UiMode::UserMode(UserMode::CreatingOrder(form.clone()));
    } else if let UiMode::UserMode(UserMode::TakingOrder(_)) = &app.mode {
        // User cancelled taking the order
        app.mode = default_mode;
    }
}

/// Handle mode switching (M key in Settings tab)
fn handle_mode_switch(app: &mut AppState, _settings: &crate::settings::Settings) {
    let new_role = match app.user_role {
        UserRole::User => UserRole::Admin,
        UserRole::Admin => UserRole::User,
    };

    // Update app state
    app.switch_role(new_role);

    // Save to settings file
    // Note: We need to create a new Settings struct with updated user_mode
    // Since SETTINGS is a OnceLock, we can't modify it directly
    // We'll need to read the file, update it, and write it back
    let home_dir = dirs::home_dir().expect("Could not find home directory");
    let package_name = env!("CARGO_PKG_NAME");
    let hidden_file = home_dir
        .join(format!(".{package_name}"))
        .join("settings.toml");

    // Read current settings
    if let Ok(mut current_settings) = std::fs::read_to_string(&hidden_file).and_then(|content| {
        toml::from_str::<crate::settings::Settings>(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }) {
        // Update user_mode
        current_settings.user_mode = new_role.to_string();

        // Write back
        if let Ok(toml_string) = toml::to_string_pretty(&current_settings) {
            let _ = std::fs::write(&hidden_file, toml_string);
            log::info!("Mode switched to: {}", new_role);
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Main key event handler - dispatches to appropriate handlers
#[allow(clippy::too_many_arguments)]
pub fn handle_key_event(
    key_event: KeyEvent,
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<Dispute>>>,
    pool: &sqlx::SqlitePool,
    client: &nostr_sdk::Client,
    settings: &crate::settings::Settings,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &tokio::sync::mpsc::UnboundedSender<crate::ui::OrderResult>,
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

    // Clear "copied" indicator when any key is pressed (except C which sets it)
    if let UiMode::NewMessageNotification(_, Action::PayInvoice, ref mut invoice_state) = app.mode {
        if code != KeyCode::Char('c') && code != KeyCode::Char('C') {
            invoice_state.copied_to_clipboard = false;
        }
    }

    match code {
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
            handle_navigation(code, app, orders, disputes);
            Some(true)
        }
        KeyCode::Tab | KeyCode::BackTab => {
            handle_tab_navigation(code, app);
            Some(true)
        }
        KeyCode::Enter => {
            handle_enter_key(
                app,
                orders,
                pool,
                client,
                settings,
                mostro_pubkey,
                order_result_tx,
            );
            Some(true)
        }
        KeyCode::Esc => {
            let should_continue = handle_esc_key(app);
            Some(should_continue)
        }
        KeyCode::Char('q') => Some(false), // Break the loop
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let should_continue =
                handle_confirm_key(app, pool, client, settings, mostro_pubkey, order_result_tx);
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
                    handle_mode_switch(app, settings);
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
                    // Copy to clipboard - keep instance alive to avoid "dropped too quickly" warning
                    let copy_result = {
                        match arboard::Clipboard::new() {
                            Ok(mut clipboard) => {
                                let result = clipboard.set_text(invoice.clone());
                                // Keep clipboard in scope a bit longer
                                std::thread::sleep(std::time::Duration::from_millis(100));
                                result
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
