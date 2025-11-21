use crate::ui::{AppState, FormState, Tab, TakeOrderState, UiMode};
use crate::SETTINGS;
use crossterm::event::{KeyCode, KeyEvent};
use mostro_core::prelude::SmallOrder;
use std::sync::{Arc, Mutex};

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
pub fn handle_navigation(code: KeyCode, app: &mut AppState, orders: &Arc<Mutex<Vec<SmallOrder>>>) {
    match code {
        KeyCode::Left => handle_left_key(app, orders),
        KeyCode::Right => handle_right_key(app, orders),
        KeyCode::Up => handle_up_key(app, orders),
        KeyCode::Down => handle_down_key(app, orders),
        _ => {}
    }
}

fn handle_left_key(app: &mut AppState, _orders: &Arc<Mutex<Vec<SmallOrder>>>) {
    match &mut app.mode {
        UiMode::Normal => {
            let prev_tab = app.active_tab;
            app.active_tab = app.active_tab.prev();
            handle_tab_switch(app, prev_tab);
            // Exit form mode when leaving Create New Order tab
            if app.active_tab != Tab::CreateNewOrder {
                app.mode = UiMode::Normal;
            }
        }
        UiMode::TakingOrder(ref mut take_state) => {
            // Switch to YES button (left side)
            take_state.selected_button = true;
        }
        UiMode::NewMessageNotification(_, _, _) => {
            // No action in notification mode
        }
        _ => {}
    }
}

fn handle_right_key(app: &mut AppState, _orders: &Arc<Mutex<Vec<SmallOrder>>>) {
    match &mut app.mode {
        UiMode::Normal => {
            let prev_tab = app.active_tab;
            app.active_tab = app.active_tab.next();
            handle_tab_switch(app, prev_tab);
            // Auto-initialize form when switching to Create New Order tab
            if app.active_tab == Tab::CreateNewOrder {
                let form = FormState {
                    kind: "buy".to_string(),
                    fiat_code: "USD".to_string(),
                    amount: "0".to_string(),
                    premium: "0".to_string(),
                    expiration_days: "1".to_string(),
                    focused: 1,
                    ..Default::default()
                };
                app.mode = UiMode::CreatingOrder(form);
            }
        }
        UiMode::TakingOrder(ref mut take_state) => {
            // Switch to NO button (right side)
            take_state.selected_button = false;
        }
        UiMode::NewMessageNotification(_, _, _) => {
            // No action in notification mode
        }
        _ => {}
    }
}

fn handle_up_key(app: &mut AppState, orders: &Arc<Mutex<Vec<SmallOrder>>>) {
    match &mut app.mode {
        UiMode::Normal => {
            if app.active_tab == Tab::Orders {
                let orders_len = orders.lock().unwrap().len();
                if orders_len > 0 && app.selected_order_idx > 0 {
                    app.selected_order_idx -= 1;
                }
            } else if app.active_tab == Tab::Messages {
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
        UiMode::CreatingOrder(form) => {
            if form.focused > 0 {
                form.focused -= 1;
                // Skip field 4 if not using range (go from 5 to 3)
                if form.focused == 4 && !form.use_range {
                    form.focused = 3;
                }
            }
        }
        UiMode::ConfirmingOrder(_)
        | UiMode::TakingOrder(_)
        | UiMode::WaitingForMostro(_)
        | UiMode::WaitingTakeOrder(_)
        | UiMode::OrderResult(_)
        | UiMode::NewMessageNotification(_, _, _) => {
            // No navigation in these modes
        }
    }
}

fn handle_down_key(app: &mut AppState, orders: &Arc<Mutex<Vec<SmallOrder>>>) {
    match &mut app.mode {
        UiMode::Normal => {
            if app.active_tab == Tab::Orders {
                let orders_len = orders.lock().unwrap().len();
                if orders_len > 0 && app.selected_order_idx < orders_len.saturating_sub(1) {
                    app.selected_order_idx += 1;
                }
            } else if app.active_tab == Tab::Messages {
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
        UiMode::CreatingOrder(form) => {
            if form.focused < 8 {
                form.focused += 1;
                // Skip field 4 if not using range (go from 3 to 5)
                if form.focused == 4 && !form.use_range {
                    form.focused = 5;
                }
            }
        }
        UiMode::ConfirmingOrder(_)
        | UiMode::TakingOrder(_)
        | UiMode::WaitingForMostro(_)
        | UiMode::WaitingTakeOrder(_)
        | UiMode::OrderResult(_)
        | UiMode::NewMessageNotification(_, _, _) => {
            // No navigation in these modes
        }
    }
}

fn handle_tab_switch(app: &mut AppState, prev_tab: Tab) {
    // Clear pending notifications and mark messages as read when switching to Messages tab
    if app.active_tab == Tab::Messages && prev_tab != Tab::Messages {
        let mut pending = app.pending_notifications.lock().unwrap();
        *pending = 0;
        // Mark all messages as read when entering Messages tab
        let mut messages = app.messages.lock().unwrap();
        for msg in messages.iter_mut() {
            msg.read = true;
        }
    }
}

/// Handle Tab and BackTab keys
pub fn handle_tab_navigation(code: KeyCode, app: &mut AppState) {
    match code {
        KeyCode::Tab => {
            if let UiMode::CreatingOrder(ref mut form) = app.mode {
                form.focused = (form.focused + 1) % 9;
                // Skip field 4 if not using range
                if form.focused == 4 && !form.use_range {
                    form.focused = 5;
                }
            }
        }
        KeyCode::BackTab => {
            if let UiMode::CreatingOrder(ref mut form) = app.mode {
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
    match std::mem::replace(&mut app.mode, UiMode::Normal) {
        UiMode::Normal => {
            handle_enter_normal_mode(app, orders);
        }
        UiMode::CreatingOrder(form) => {
            handle_enter_creating_order(app, &form);
        }
        UiMode::ConfirmingOrder(_) => {
            // Enter acts as Yes in confirmation - handled by 'y' key
            app.mode = UiMode::Normal;
        }
        UiMode::TakingOrder(take_state) => {
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
        UiMode::WaitingForMostro(_) | UiMode::WaitingTakeOrder(_) => {
            // No action while waiting
            app.mode = UiMode::Normal;
        }
        UiMode::OrderResult(_) => {
            // Close result popup, return to Orders tab
            app.active_tab = Tab::Orders;
        }
        UiMode::NewMessageNotification(notification, action, mut invoice_state) => {
            handle_enter_message_notification(app, &action, &mut invoice_state);
            app.mode = UiMode::NewMessageNotification(notification, action, invoice_state);
        }
    }
}

fn handle_enter_normal_mode(app: &mut AppState, orders: &Arc<Mutex<Vec<SmallOrder>>>) {
    // Show take order popup when Enter is pressed in Orders tab
    if app.active_tab == Tab::Orders {
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
            app.mode = UiMode::TakingOrder(take_state);
        }
    }
}

fn handle_enter_creating_order(app: &mut AppState, form: &FormState) {
    // Show confirmation popup when Enter is pressed
    if app.active_tab == Tab::CreateNewOrder {
        app.mode = UiMode::ConfirmingOrder(form.clone());
    } else {
        app.mode = UiMode::CreatingOrder(form.clone());
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
                app.mode = UiMode::TakingOrder(take_state);
                return;
            }
            if take_state.validation_error.is_some() {
                // Can't proceed with invalid amount
                app.mode = UiMode::TakingOrder(take_state);
                return;
            }
        }
        // Proceed with taking the order
        let take_state_clone = take_state.clone();
        app.mode = UiMode::WaitingTakeOrder(take_state_clone.clone());

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

fn handle_enter_message_notification(
    app: &mut AppState,
    action: &mostro_core::prelude::Action,
    invoice_state: &mut crate::ui::InvoiceInputState,
) {
    match action {
        mostro_core::prelude::Action::AddInvoice => {
            // For AddInvoice, Enter submits the invoice
            if !invoice_state.invoice_input.trim().is_empty() {
                // TODO: Send invoice to Mostro
                // For now, just close and switch to Messages tab
                log::info!("Invoice submitted: {}", invoice_state.invoice_input);
                app.active_tab = Tab::Messages;
                // Clear pending notifications when viewing messages
                let mut pending = app.pending_notifications.lock().unwrap();
                *pending = 0;
                // Mark all messages as read when entering Messages tab via notification
                let mut messages = app.messages.lock().unwrap();
                for msg in messages.iter_mut() {
                    msg.read = true;
                }
            }
        }
        _ => {
            // For PayInvoice and others, Enter just closes notification
            app.active_tab = Tab::Messages;
            // Clear pending notifications when viewing messages
            let mut pending = app.pending_notifications.lock().unwrap();
            *pending = 0;
            // Mark all messages as read when entering Messages tab via notification
            let mut messages = app.messages.lock().unwrap();
            for msg in messages.iter_mut() {
                msg.read = true;
            }
        }
    }
}

/// Handle Esc key
pub fn handle_esc_key(app: &mut AppState) -> bool {
    // Returns true if should continue, false if should break
    match &mut app.mode {
        UiMode::CreatingOrder(_) => {
            app.mode = UiMode::Normal;
            true
        }
        UiMode::ConfirmingOrder(form) => {
            // Cancel confirmation, go back to form
            app.mode = UiMode::CreatingOrder(form.clone());
            true
        }
        UiMode::TakingOrder(_) => {
            // Cancel taking order, return to normal mode
            app.mode = UiMode::Normal;
            true
        }
        UiMode::WaitingForMostro(_) | UiMode::WaitingTakeOrder(_) => {
            // Can't cancel while waiting
            true
        }
        UiMode::OrderResult(_) => {
            // Close result popup, return to Orders tab
            app.mode = UiMode::Normal;
            app.active_tab = Tab::Orders;
            true
        }
        UiMode::NewMessageNotification(_, _, _) => {
            // Dismiss notification
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
            if let UiMode::CreatingOrder(ref mut form) = app.mode {
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
            if let UiMode::CreatingOrder(ref mut form) = app.mode {
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
            } else if let UiMode::TakingOrder(ref mut take_state) = app.mode {
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
    if let UiMode::CreatingOrder(ref mut form) = app.mode {
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
    } else if let UiMode::TakingOrder(ref mut take_state) = app.mode {
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
    match std::mem::replace(&mut app.mode, UiMode::Normal) {
        UiMode::ConfirmingOrder(form) => {
            // User confirmed, send the order
            let form_clone = form.clone();
            app.mode = UiMode::WaitingForMostro(form_clone.clone());

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
        UiMode::TakingOrder(take_state) => {
            // User confirmed taking the order (same as Enter key)
            // Check validation first
            if take_state.is_range_order {
                if take_state.amount_input.is_empty() {
                    // Can't proceed without amount
                    app.mode = UiMode::TakingOrder(take_state);
                    return true;
                }
                if take_state.validation_error.is_some() {
                    // Can't proceed with invalid amount
                    app.mode = UiMode::TakingOrder(take_state);
                    return true;
                }
            }
            // Proceed with taking the order
            let take_state_clone = take_state.clone();
            app.mode = UiMode::WaitingTakeOrder(take_state_clone.clone());

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
    if let UiMode::ConfirmingOrder(form) = &app.mode {
        // User cancelled, go back to form
        app.mode = UiMode::CreatingOrder(form.clone());
    } else if let UiMode::TakingOrder(_) = &app.mode {
        // User cancelled taking the order
        app.mode = UiMode::Normal;
    }
}

/// Main key event handler - dispatches to appropriate handlers
pub fn handle_key_event(
    key_event: KeyEvent,
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
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

    match code {
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
            handle_navigation(code, app, orders);
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
