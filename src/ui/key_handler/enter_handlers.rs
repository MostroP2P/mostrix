use crate::ui::{
    helpers::save_chat_message, AdminMode, AdminTab, AppState, FormState, MessageViewState, Tab,
    TakeOrderState, UiMode, UserMode, UserRole, UserTab,
};
use crate::util::order_utils::{
    execute_add_invoice, execute_admin_add_solver, execute_admin_cancel, execute_admin_settle,
    execute_send_msg,
};

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
    save_admin_key_to_settings, save_currency_to_settings, save_mostro_pubkey_to_settings,
    save_relay_to_settings,
};
use crate::ui::key_handler::validation::{
    validate_currency, validate_mostro_pubkey, validate_npub, validate_nsec, validate_relay,
};

/// Helper function to execute taking a dispute.
///
/// This avoids code duplication between Enter key and 'y' key handlers.
/// Sets the UI mode to waiting and spawns an async task to take the dispute.
pub(crate) fn execute_take_dispute_action(
    app: &mut AppState,
    dispute_id: uuid::Uuid,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    pool: &SqlitePool,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
) {
    app.mode = UiMode::AdminMode(AdminMode::WaitingTakeDispute(dispute_id));

    // Spawn async task to take dispute
    let client_clone = client.clone();
    let result_tx = order_result_tx.clone();
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        match crate::util::order_utils::execute_take_dispute(
            &dispute_id,
            &client_clone,
            mostro_pubkey,
            &pool_clone,
        )
        .await
        {
            Ok(_) => {
                let _ = result_tx.send(crate::ui::OrderResult::Info(format!(
                    "✅ Dispute {} taken successfully!",
                    dispute_id
                )));
            }
            Err(e) => {
                log::error!("Failed to take dispute: {}", e);
                let _ = result_tx.send(crate::ui::OrderResult::Error(e.to_string()));
            }
        }
    });
}

/// Helper function to execute adding a solver.
///
/// This avoids code duplication between Enter key and 'y' key handlers.
/// Sets the UI mode to normal and spawns an async task to add the solver.
pub(crate) fn execute_add_solver_action(
    app: &mut AppState,
    solver_pubkey: String,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
) {
    // Stay on Settings tab after confirmation
    app.mode = UiMode::AdminMode(AdminMode::Normal);

    let solver_pubkey_clone = solver_pubkey.clone();
    let client_clone = client.clone();
    let result_tx = order_result_tx.clone();
    let mostro_pubkey_clone = mostro_pubkey;

    tokio::spawn(async move {
        match execute_admin_add_solver(&solver_pubkey_clone, &client_clone, mostro_pubkey_clone)
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
}

/// Helper function to execute taking an order.
///
/// This avoids code duplication between Enter key and 'y' key handlers.
/// Validates the take_state, sets the UI mode to waiting, and spawns an async task to take the order.
pub(crate) fn execute_take_order_action(
    app: &mut AppState,
    take_state: TakeOrderState,
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
) -> bool {
    // Validate range order if needed
    if take_state.is_range_order {
        if take_state.amount_input.is_empty() {
            // Can't proceed without amount
            app.mode = UiMode::UserMode(UserMode::TakingOrder(take_state));
            return false;
        }
        if take_state.validation_error.is_some() {
            // Can't proceed with invalid amount
            app.mode = UiMode::UserMode(UserMode::TakingOrder(take_state));
            return false;
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
        let settings = match SETTINGS.get() {
            Some(s) => s,
            None => {
                let error_msg =
                    "Settings not initialized. Please restart the application.".to_string();
                log::error!("{}", error_msg);
                let _ = result_tx.send(crate::ui::OrderResult::Error(error_msg));
                return;
            }
        };
        match crate::util::take_order(
            &pool_clone,
            &client_clone,
            settings,
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

/// Handle Enter key - dispatches to mode-specific handlers
pub fn handle_enter_key(
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<mostro_core::prelude::Dispute>>>,
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
) -> bool {
    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::Normal),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    let current_mode = std::mem::replace(&mut app.mode, default_mode.clone());
    match current_mode {
        UiMode::Normal
        | UiMode::UserMode(UserMode::Normal)
        | UiMode::AdminMode(AdminMode::Normal) => {
            handle_enter_normal_mode(app, orders, disputes);
            true
        }
        UiMode::UserMode(UserMode::CreatingOrder(form)) => {
            handle_enter_creating_order(app, &form);
            true
        }
        UiMode::UserMode(UserMode::ConfirmingOrder(_)) => {
            // Enter acts as Yes in confirmation - handled by 'y' key
            app.mode = default_mode;
            true
        }
        UiMode::UserMode(UserMode::TakingOrder(take_state)) => {
            handle_enter_taking_order(
                app,
                take_state,
                pool,
                client,
                mostro_pubkey,
                order_result_tx,
            );
            true
        }
        UiMode::UserMode(UserMode::WaitingForMostro(_))
        | UiMode::UserMode(UserMode::WaitingTakeOrder(_))
        | UiMode::UserMode(UserMode::WaitingAddInvoice) => {
            // No action while waiting
            app.mode = default_mode;
            true
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
            true
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
            true
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
            true
        }
        UiMode::AdminMode(AdminMode::AddSolver(_))
        | UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, _))
        | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, _)) => {
            handle_enter_admin_mode(
                app,
                current_mode,
                default_mode,
                client,
                mostro_pubkey,
                order_result_tx,
            );
            true
        }
        UiMode::AddMostroPubkey(_)
        | UiMode::ConfirmMostroPubkey(_, _)
        | UiMode::AddRelay(_)
        | UiMode::ConfirmRelay(_, _)
        | UiMode::AddCurrency(_)
        | UiMode::ConfirmCurrency(_, _)
        | UiMode::ConfirmClearCurrencies(_)
        | UiMode::ConfirmExit(_) => {
            let should_continue =
                handle_enter_settings_mode(app, current_mode, default_mode, client);
            if !should_continue {
                return false; // Exit application
            }
            true
        }
        UiMode::AdminMode(AdminMode::ConfirmTakeDispute(dispute_id, selected_button)) => {
            if selected_button {
                // YES selected - take the dispute
                execute_take_dispute_action(
                    app,
                    dispute_id,
                    client,
                    mostro_pubkey,
                    pool,
                    order_result_tx,
                );
            } else {
                // NO selected - go back to normal mode
                app.mode = default_mode;
            }
            true
        }
        UiMode::AdminMode(AdminMode::WaitingTakeDispute(_)) => {
            // No action while waiting
            app.mode = default_mode;
            true
        }
        UiMode::AdminMode(AdminMode::ManagingDispute) => {
            // Handle Enter in Disputes in Progress tab
            if let Tab::Admin(AdminTab::DisputesInProgress) = app.active_tab {
                // IMPORTANT: Restore mode immediately to prevent any state issues
                app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);

                // Check if chat input has content and is enabled - send message
                // If input is empty, do nothing (don't disable input, don't trigger any action)
                if !app.admin_chat_input.trim().is_empty() && app.admin_chat_input_enabled {
                    if let Some(selected_dispute) = app
                        .admin_disputes_in_progress
                        .get(app.selected_in_progress_idx)
                    {
                        // Use dispute_id as the key for chat messages
                        let dispute_id_key = selected_dispute.dispute_id.clone();
                        let message_content = app.admin_chat_input.trim().to_string();
                        let timestamp = chrono::Utc::now().timestamp();

                        // Add admin's message (track which party it was sent to)
                        let admin_message = crate::ui::DisputeChatMessage {
                            sender: crate::ui::ChatSender::Admin,
                            content: message_content.clone(),
                            timestamp,
                            target_party: Some(app.active_chat_party),
                        };

                        app.admin_dispute_chats
                            .entry(dispute_id_key.clone())
                            .or_default()
                            .push(admin_message.clone());

                        // Save admin message to file (use dispute_id_key for consistency)
                        save_chat_message(&dispute_id_key, &admin_message);

                        // TODO: Send message to active chat party via Nostr DM
                        // This will be implemented when we add the DM sending logic

                        // Generate mockup response from buyer or seller
                        let mockup_responses = [
                            "I understand, let me check the payment details.",
                            "Can you please provide more information?",
                            "Yes, I have completed my part of the transaction.",
                            "I'm waiting for confirmation from my bank.",
                            "The payment was sent on time as agreed.",
                            "I have the screenshots if you need them.",
                            "No, I did not receive the payment yet.",
                            "Can we extend the deadline by 24 hours?",
                        ];

                        // Use message length to pseudo-randomly select a response
                        let response_idx = message_content.len() % mockup_responses.len();
                        let mockup_response = mockup_responses[response_idx];

                        // Add mockup response from the active chat party
                        let party_sender = match app.active_chat_party {
                            crate::ui::ChatParty::Buyer => crate::ui::ChatSender::Buyer,
                            crate::ui::ChatParty::Seller => crate::ui::ChatSender::Seller,
                        };

                        let party_message = crate::ui::DisputeChatMessage {
                            sender: party_sender,
                            content: mockup_response.to_string(),
                            timestamp: timestamp + 2, // 2 seconds later
                            target_party: None, // Buyer/Seller messages don't have a target party
                        };

                        app.admin_dispute_chats
                            .entry(dispute_id_key.clone())
                            .or_default()
                            .push(party_message.clone());

                        // Save party message to file
                        save_chat_message(&dispute_id_key, &party_message);

                        // Auto-scroll to bottom to show new messages
                        // Count visible messages (filtered by active party)
                        let visible_count = app
                            .admin_dispute_chats
                            .get(&dispute_id_key)
                            .map(|msgs| {
                                msgs.iter()
                                    .filter(|msg| {
                                        match msg.sender {
                                            crate::ui::ChatSender::Admin => {
                                                // Admin messages should only show in the chat party they were sent to
                                                msg.target_party == Some(app.active_chat_party)
                                            }
                                            crate::ui::ChatSender::Buyer => {
                                                app.active_chat_party == crate::ui::ChatParty::Buyer
                                            }
                                            crate::ui::ChatSender::Seller => {
                                                app.active_chat_party
                                                    == crate::ui::ChatParty::Seller
                                            }
                                        }
                                    })
                                    .count()
                            })
                            .unwrap_or(0);

                        if visible_count > 0 {
                            app.admin_chat_list_state
                                .select(Some(visible_count.saturating_sub(1)));
                        }
                    }

                    // Clear the input and keep focus
                    app.admin_chat_input.clear();
                    // IMPORTANT: Stay in ManagingDispute mode to keep input focus and enabled
                    app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
                    // Ensure input remains enabled after sending message
                    app.admin_chat_input_enabled = true;
                } else {
                    // If input is empty, ensure input stays enabled and mode is correct
                    app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
                    app.admin_chat_input_enabled = true;
                }
                // (finalization is now triggered by Shift+F, not Enter)
            } else {
                // Not in Disputes in Progress tab, restore mode anyway
                app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
            }
            true
        }
        UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization(
            dispute_id,
            selected_button,
        )) => {
            // Check if dispute is finalized
            let is_finalized = app
                .admin_disputes_in_progress
                .iter()
                .find(|d| d.dispute_id == dispute_id.to_string() || d.id == dispute_id.to_string())
                .map(|d| {
                    matches!(
                        d.status.as_deref(),
                        Some("Settled") | Some("SellerRefunded") | Some("Released")
                    )
                })
                .unwrap_or(false);

            // Handle Enter in finalization popup
            match selected_button {
                0 => {
                    // Pay Buyer - show confirmation popup
                    if is_finalized {
                        // Dispute already finalized, show error
                        let _ = order_result_tx.send(crate::ui::OrderResult::Error(
                            "Cannot finalize: dispute is already finalized".to_string(),
                        ));
                        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
                    } else {
                        // Show confirmation popup
                        app.mode = UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute(
                            dispute_id, true, // is_settle
                            true, // selected_button: true=Yes
                        ));
                    }
                    true
                }
                1 => {
                    // Refund Seller - show confirmation popup
                    if is_finalized {
                        // Dispute already finalized, show error
                        let _ = order_result_tx.send(crate::ui::OrderResult::Error(
                            "Cannot finalize: dispute is already finalized".to_string(),
                        ));
                        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
                    } else {
                        // Show confirmation popup
                        app.mode = UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute(
                            dispute_id, false, // is_settle
                            true,  // selected_button: true=Yes
                        ));
                    }
                    true
                }
                2 => {
                    // Exit - return to normal mode
                    app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
                    true
                }
                _ => {
                    // Invalid button, return to normal mode
                    app.mode = default_mode;
                    true
                }
            }
        }
        UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute(
            dispute_id,
            is_settle,
            selected_button,
        )) => {
            if selected_button {
                // YES selected - execute the finalization action
                execute_finalize_dispute_action(
                    app,
                    dispute_id,
                    client,
                    mostro_pubkey,
                    pool,
                    order_result_tx,
                    is_settle,
                );
            } else {
                // NO selected - go back to finalization popup
                app.mode = UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization(
                    dispute_id,
                    if is_settle { 0 } else { 1 }, // Restore the button that was selected
                ));
            }
            true
        }
        UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(_)) => {
            // No action while waiting
            app.mode = default_mode;
            true
        }
    }
}

/// Handle Enter key for admin-specific modes (AddSolver, SetupAdminKey, etc.)
/// Kept `pub(crate)` so it can be reused by the 'y' confirmation handler
/// to avoid duplicating the AddSolver execution logic (DRY).
pub(crate) fn handle_enter_admin_mode(
    app: &mut AppState,
    mode: UiMode,
    default_mode: UiMode,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
) {
    match mode {
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
                execute_add_solver_action(
                    app,
                    solver_pubkey,
                    client,
                    mostro_pubkey,
                    order_result_tx,
                );
            } else {
                // NO selected - go back to input
                app.mode =
                    UiMode::AdminMode(AdminMode::AddSolver(create_key_input_state(&solver_pubkey)));
            }
        }
        UiMode::AdminMode(AdminMode::SetupAdminKey(key_state)) => {
            match validate_nsec(&key_state.key_input) {
                Ok(_) => {
                    app.mode =
                        handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                            UiMode::AdminMode(AdminMode::ConfirmAdminKey(input, true))
                        });
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::OrderResult(crate::ui::OrderResult::Error(e));
                }
            }
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
        _ => {
            // This should not happen, but handle gracefully
            app.mode = default_mode;
        }
    }
}

/// Handle Enter key for settings-related modes (Mostro pubkey, relay, currency, etc.)
fn handle_enter_settings_mode(
    app: &mut AppState,
    mode: UiMode,
    default_mode: UiMode,
    client: &Client,
) -> bool {
    match mode {
        UiMode::AddMostroPubkey(key_state) => {
            // Validate Mostro pubkey (hex format) before proceeding to confirmation
            match validate_mostro_pubkey(&key_state.key_input) {
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
            // Validate relay URL format before proceeding to confirmation
            match validate_relay(&key_state.key_input) {
                Ok(_) => {
                    app.mode =
                        handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                            UiMode::ConfirmRelay(input, true)
                        });
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::OrderResult(crate::ui::OrderResult::Error(e));
                }
            }
        }
        UiMode::ConfirmRelay(relay_string, selected_button) => {
            app.mode = handle_confirmation_enter(
                selected_button,
                &relay_string,
                default_mode,
                save_relay_to_settings,
                |input| UiMode::AddRelay(create_key_input_state(input)),
            );

            // If YES was selected, also add the relay to the running Nostr client
            if selected_button {
                let relay_to_add = relay_string.clone();
                let client_clone = client.clone();
                tokio::spawn(async move {
                    if let Err(e) = client_clone.add_relay(relay_to_add.trim()).await {
                        log::error!("Failed to add relay at runtime: {}", e);
                    }
                });
            }
        }
        UiMode::AddCurrency(key_state) => {
            // Validate currency code before proceeding to confirmation
            match validate_currency(&key_state.key_input) {
                Ok(_) => {
                    app.mode =
                        handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                            UiMode::ConfirmCurrency(input, true)
                        });
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::OrderResult(crate::ui::OrderResult::Error(e));
                }
            }
        }
        UiMode::ConfirmCurrency(currency_string, selected_button) => {
            app.mode = handle_confirmation_enter(
                selected_button,
                &currency_string,
                default_mode,
                save_currency_to_settings,
                |input| UiMode::AddCurrency(create_key_input_state(input)),
            );
        }
        UiMode::ConfirmClearCurrencies(selected_button) => {
            if selected_button {
                // YES selected - clear currency filters
                use crate::ui::key_handler::settings::clear_currency_filters;
                clear_currency_filters();
            }
            app.mode = default_mode;
        }
        UiMode::ConfirmExit(selected_button) => {
            if selected_button {
                // YES selected - exit the application
                // Return false to break the main loop
                return false;
            } else {
                // NO selected - cancel and return to normal mode
                app.mode = default_mode;
                return true;
            }
        }
        _ => {
            // This should not happen, but handle gracefully
            app.mode = default_mode;
        }
    }
    true // Continue the loop by default
}

fn handle_enter_normal_mode(
    app: &mut AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<mostro_core::prelude::Dispute>>>,
) {
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
    } else if let Tab::Admin(AdminTab::DisputesPending) = app.active_tab {
        // Show take dispute confirmation popup when Enter is pressed in Disputes tab (admin mode only)
        let disputes_lock = disputes.lock().unwrap();
        // Filter to only get "initiated" disputes
        let initiated_disputes: Vec<(usize, &Dispute)> = disputes_lock
            .iter()
            .enumerate()
            .filter(|(_, dispute)| dispute.status == "initiated")
            .collect();

        if let Some((_original_idx, dispute)) = initiated_disputes.get(app.selected_dispute_idx) {
            // Only allow taking disputes with "Initiated" status
            // (We already filtered, so this should always be true)
            app.mode = UiMode::AdminMode(AdminMode::ConfirmTakeDispute(dispute.id, true));
            // Default to YES
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
        Tab::Admin(AdminTab::Exit) | Tab::User(UserTab::Exit)
    ) {
        // Show exit confirmation popup
        app.mode = UiMode::ConfirmExit(true); // true = Yes button selected by default
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
            2 => {
                // Add Currency Filter (Common for both roles)
                app.mode = UiMode::AddCurrency(key_state);
            }
            3 => {
                // Clear Currency Filters (Common for both roles) - show confirmation
                app.mode = UiMode::ConfirmClearCurrencies(true);
            }
            4 if app.user_role == UserRole::Admin => {
                // Add Solver (Admin only)
                app.mode = UiMode::AdminMode(AdminMode::AddSolver(key_state));
            }
            5 if app.user_role == UserRole::Admin => {
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
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
) {
    // Enter confirms the selected button
    if take_state.selected_button {
        // YES selected - execute take order action
        execute_take_order_action(
            app,
            take_state,
            pool,
            client,
            mostro_pubkey,
            order_result_tx,
        );
    } else {
        // NO selected - cancel and return to the appropriate normal mode
        let default_mode = match app.user_role {
            UserRole::User => UiMode::UserMode(UserMode::Normal),
            UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
        };
        app.mode = default_mode;
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

/// Helper function to execute dispute finalization (settle or cancel).
///
/// This avoids code duplication between Pay Buyer and Refund Seller actions.
/// Sets the UI mode to waiting and spawns an async task to finalize the dispute.
pub(crate) fn execute_finalize_dispute_action(
    app: &mut AppState,
    dispute_id: uuid::Uuid,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    pool: &SqlitePool,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
    is_settle: bool, // true = AdminSettle (pay buyer), false = AdminCancel (refund seller)
) {
    app.mode = UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(dispute_id));

    // Spawn async task to finalize dispute
    let client_clone = client.clone();
    let result_tx = order_result_tx.clone();
    let pool_clone = pool.clone();
    let dispute_id_str = dispute_id.to_string();
    tokio::spawn(async move {
        let result = if is_settle {
            execute_admin_settle(&dispute_id, &client_clone, mostro_pubkey).await
        } else {
            execute_admin_cancel(&dispute_id, &client_clone, mostro_pubkey).await
        };

        match result {
            Ok(_) => {
                // Update dispute status in database
                let db_update_result = if is_settle {
                    crate::models::AdminDispute::set_status_settled(&pool_clone, &dispute_id_str)
                        .await
                } else {
                    crate::models::AdminDispute::set_status_seller_refunded(
                        &pool_clone,
                        &dispute_id_str,
                    )
                    .await
                };

                if let Err(e) = db_update_result {
                    log::error!("Failed to update dispute status in database: {}", e);
                    // Still show success message since the action was sent successfully
                }

                let action_name = if is_settle {
                    "settled (buyer paid)"
                } else {
                    "canceled (seller refunded)"
                };
                let _ = result_tx.send(crate::ui::OrderResult::Info(format!(
                    "✅ Dispute {} {}!",
                    dispute_id, action_name
                )));
            }
            Err(e) => {
                log::error!("Failed to finalize dispute: {}", e);
                let _ = result_tx.send(crate::ui::OrderResult::Error(e.to_string()));
            }
        }
    });
}
