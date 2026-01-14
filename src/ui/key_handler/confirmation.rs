use crate::ui::{AdminMode, AppState, UiMode, UserMode, UserRole};
use crate::SETTINGS;
use nostr_sdk::Client;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;

use crate::ui::key_handler::settings::{
    save_admin_key_to_settings, save_currency_to_settings, save_mostro_pubkey_to_settings,
    save_relay_to_settings,
};

/// Helper: Transition from input mode to confirmation mode
pub fn handle_input_to_confirmation<F>(
    input: &str,
    default_mode: UiMode,
    create_confirmation: F,
) -> UiMode
where
    F: FnOnce(String) -> UiMode,
{
    if !input.is_empty() {
        create_confirmation(input.to_string())
    } else {
        default_mode
    }
}

/// Helper: Handle Enter key in confirmation mode (YES/NO selection)
pub fn handle_confirmation_enter<F1, F2>(
    selected_button: bool,
    input_string: &str,
    default_mode: UiMode,
    save_fn: F1,
    create_input: F2,
) -> UiMode
where
    F1: FnOnce(&str),
    F2: FnOnce(&str) -> UiMode,
{
    if selected_button {
        // YES selected - save
        save_fn(input_string);
        default_mode
    } else {
        // NO selected - go back to input
        create_input(input_string)
    }
}

/// Helper: Go back from confirmation to input mode
pub fn handle_confirmation_esc<F>(input_string: &str, create_input: F) -> UiMode
where
    F: FnOnce(&str) -> UiMode,
{
    create_input(input_string)
}

/// Helper to create a KeyInputState from a string
pub fn create_key_input_state(input: &str) -> crate::ui::KeyInputState {
    crate::ui::KeyInputState {
        key_input: input.to_string(),
        focused: true,
        just_pasted: false,
    }
}

/// Handle 'y' key for confirmation
pub fn handle_confirm_key(
    app: &mut AppState,
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<crate::ui::OrderResult>,
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
        UiMode::ConfirmMostroPubkey(key_string, _) => {
            let default_mode = match app.user_role {
                UserRole::User => UiMode::UserMode(UserMode::Normal),
                UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
            };
            app.mode = handle_confirmation_enter(
                true, // 'y' key means YES
                &key_string,
                default_mode,
                save_mostro_pubkey_to_settings,
                |input| UiMode::AddMostroPubkey(create_key_input_state(input)),
            );
            true
        }
        UiMode::ConfirmRelay(relay_string, _) => {
            let default_mode = match app.user_role {
                UserRole::User => UiMode::UserMode(UserMode::Normal),
                UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
            };
            app.mode = handle_confirmation_enter(
                true, // 'y' key means YES
                &relay_string,
                default_mode,
                save_relay_to_settings,
                |input| UiMode::AddRelay(create_key_input_state(input)),
            );

            // Also add the new relay to the running Nostr client immediately
            let relay_to_add = relay_string.clone();
            let client_clone = client.clone();
            tokio::spawn(async move {
                if let Err(e) = client_clone.add_relay(relay_to_add.trim()).await {
                    log::error!("Failed to add relay at runtime: {}", e);
                }
            });
            true
        }
        UiMode::ConfirmCurrency(currency_string, _) => {
            let default_mode = match app.user_role {
                UserRole::User => UiMode::UserMode(UserMode::Normal),
                UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
            };
            app.mode = handle_confirmation_enter(
                true, // 'y' key means YES
                &currency_string,
                default_mode,
                save_currency_to_settings,
                |input| UiMode::AddCurrency(create_key_input_state(input)),
            );
            true
        }
        UiMode::ConfirmClearCurrencies(_) => {
            let default_mode = match app.user_role {
                UserRole::User => UiMode::UserMode(UserMode::Normal),
                UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
            };
            // YES selected - clear currency filters
            use crate::ui::key_handler::settings::clear_currency_filters;
            clear_currency_filters();
            app.mode = default_mode;
            true
        }
        UiMode::AdminMode(AdminMode::ConfirmAdminKey(key_string, _)) => {
            let default_mode = match app.user_role {
                UserRole::User => UiMode::UserMode(UserMode::Normal),
                UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
            };
            app.mode = handle_confirmation_enter(
                true, // 'y' key means YES
                &key_string,
                default_mode,
                save_admin_key_to_settings,
                |input| UiMode::AdminMode(AdminMode::SetupAdminKey(create_key_input_state(input))),
            );
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
    } else if let UiMode::ConfirmMostroPubkey(key_string, _) = &app.mode {
        app.mode = handle_confirmation_esc(key_string, |input| {
            UiMode::AddMostroPubkey(create_key_input_state(input))
        });
    } else if let UiMode::ConfirmRelay(relay_string, _) = &app.mode {
        app.mode = handle_confirmation_esc(relay_string, |input| {
            UiMode::AddRelay(create_key_input_state(input))
        });
    } else if let UiMode::ConfirmCurrency(currency_string, _) = &app.mode {
        // Cancel currency confirmation, go back to AddCurrency input
        app.mode = handle_confirmation_esc(currency_string, |input| {
            UiMode::AddCurrency(create_key_input_state(input))
        });
    } else if let UiMode::ConfirmClearCurrencies(_) = &app.mode {
        // Cancel clear-all confirmation, just return to default mode
        app.mode = default_mode;
    } else if let UiMode::AdminMode(AdminMode::ConfirmAddSolver(solver_pubkey, _)) = &app.mode {
        app.mode = handle_confirmation_esc(solver_pubkey, |input| {
            UiMode::AdminMode(AdminMode::AddSolver(create_key_input_state(input)))
        });
    } else if let UiMode::AdminMode(AdminMode::ConfirmAdminKey(key_string, _)) = &app.mode {
        app.mode = handle_confirmation_esc(key_string, |input| {
            UiMode::AdminMode(AdminMode::SetupAdminKey(create_key_input_state(input)))
        });
    }
}
