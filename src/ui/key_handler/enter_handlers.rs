use crate::ui::{
    AdminMode, AdminTab, AppState, FormState, MessageViewState, Tab, TakeOrderState, UiMode,
    UserMode, UserRole, UserTab,
};
use crate::util::order_utils::{execute_add_invoice, execute_admin_add_solver, execute_send_msg};
use crate::SETTINGS;
use mostro_core::prelude::*;
use nostr_sdk::Client;
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::ui::key_handler::confirmation::{
    create_key_input_state, handle_confirmation_enter, handle_input_to_confirmation,
};
use crate::ui::key_handler::settings::{
    save_admin_key_to_settings, save_mostro_pubkey_to_settings, save_relay_to_settings,
};
use crate::ui::key_handler::validation::validate_npub;

/// Handle Enter key - dispatches to mode-specific handlers
pub fn handle_enter_key(
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    pool: &SqlitePool,
    client: &Client,
    _settings: &crate::settings::Settings,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
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
            // Close result popup
            // If we're on Settings tab, stay there; otherwise return to first tab
            if !matches!(
                app.active_tab,
                Tab::Admin(AdminTab::Settings) | Tab::User(UserTab::Settings)
            ) {
                app.active_tab = Tab::first(app.user_role);
            }
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
        UiMode::AdminMode(AdminMode::AddSolver(key_state)) => {
            // Validate npub before proceeding to confirmation
            match validate_npub(&key_state.key_input) {
                Ok(_) => {
                    app.mode =
                        handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                            UiMode::AdminMode(AdminMode::ConfirmAddSolver(input, true))
                        });
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::OrderResult(crate::ui::OrderResult::Error(e));
                }
            }
        }
        UiMode::AdminMode(AdminMode::ConfirmAddSolver(solver_pubkey, selected_button)) => {
            if selected_button {
                // YES selected - send AddSolver message
                let solver_pubkey_clone = solver_pubkey.clone();
                let client_clone = client.clone();
                let result_tx = order_result_tx.clone();
                let mostro_pubkey_clone = mostro_pubkey;

                // Stay on Settings tab after confirmation
                app.mode = UiMode::AdminMode(AdminMode::Normal);

                tokio::spawn(async move {
                    match execute_admin_add_solver(
                        &solver_pubkey_clone,
                        &client_clone,
                        mostro_pubkey_clone,
                    )
                    .await
                    {
                        Ok(_) => {
                            let _ = result_tx.send(crate::ui::OrderResult::Info(
                                "Solver added successfully".to_string(),
                            ));
                        }
                        Err(e) => {
                            log::error!("Failed to add solver: {}", e);
                            let _ = result_tx.send(crate::ui::OrderResult::Error(e.to_string()));
                        }
                    }
                });
            } else {
                // NO selected - go back to input
                app.mode =
                    UiMode::AdminMode(AdminMode::AddSolver(create_key_input_state(&solver_pubkey)));
            }
        }
        UiMode::AdminMode(AdminMode::SetupAdminKey(key_state)) => {
            app.mode = handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                UiMode::AdminMode(AdminMode::ConfirmAdminKey(input, true))
            });
        }
        UiMode::AdminMode(AdminMode::ConfirmAdminKey(key_string, selected_button)) => {
            app.mode = handle_confirmation_enter(
                selected_button,
                &key_string,
                default_mode,
                save_admin_key_to_settings,
                |input| UiMode::AdminMode(AdminMode::SetupAdminKey(create_key_input_state(input))),
            );
        }
        UiMode::AddMostroPubkey(key_state) => {
            // Validate npub before proceeding to confirmation
            match validate_npub(&key_state.key_input) {
                Ok(_) => {
                    app.mode =
                        handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                            UiMode::ConfirmMostroPubkey(input, true)
                        });
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::OrderResult(crate::ui::OrderResult::Error(e));
                }
            }
        }
        UiMode::ConfirmMostroPubkey(key_string, selected_button) => {
            app.mode = handle_confirmation_enter(
                selected_button,
                &key_string,
                default_mode,
                save_mostro_pubkey_to_settings,
                |input| UiMode::AddMostroPubkey(create_key_input_state(input)),
            );
        }
        UiMode::AddRelay(key_state) => {
            app.mode = handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                UiMode::ConfirmRelay(input, true)
            });
        }
        UiMode::ConfirmRelay(relay_string, selected_button) => {
            app.mode = handle_confirmation_enter(
                selected_button,
                &relay_string,
                default_mode,
                save_relay_to_settings,
                |input| UiMode::AddRelay(create_key_input_state(input)),
            );
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
    } else if matches!(
        app.active_tab,
        Tab::Admin(AdminTab::Settings) | Tab::User(UserTab::Settings)
    ) {
        // Open key input popup based on selected option
        let key_state = create_key_input_state("");

        match app.selected_settings_option {
            0 => {
                // Add Mostro Pubkey (Common for both roles)
                app.mode = UiMode::AddMostroPubkey(key_state);
            }
            1 => {
                // Add Relay (Common for both roles)
                app.mode = UiMode::AddRelay(key_state);
            }
            2 if app.user_role == UserRole::Admin => {
                // Add Solver (Admin only)
                app.mode = UiMode::AdminMode(AdminMode::AddSolver(key_state));
            }
            3 if app.user_role == UserRole::Admin => {
                // Setup Admin Key (Admin only)
                app.mode = UiMode::AdminMode(AdminMode::SetupAdminKey(key_state));
            }
            _ => {}
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
    pool: &SqlitePool,
    client: &Client,
    _settings: &crate::settings::Settings,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
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
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
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
    client: &Client,
    pool: &SqlitePool,
    action: &mostro_core::prelude::Action,
    invoice_state: &mut crate::ui::InvoiceInputState,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_id: Option<Uuid>,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
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
