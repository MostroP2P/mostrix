use crate::ui::{AdminMode, AppState, UiMode, UserMode, UserRole};

use crate::ui::key_handler::admin_handlers::{
    execute_take_dispute_action, handle_enter_admin_mode,
};
use crate::ui::key_handler::async_tasks::{
    spawn_add_relay_task, spawn_refresh_mostro_info_task, spawn_send_new_order_task,
};
use crate::ui::key_handler::user_handlers::execute_take_order_action;

use crate::ui::key_handler::settings::{
    clear_currency_filters, save_admin_key_to_settings, save_currency_to_settings,
    save_mostro_pubkey_to_settings, save_relay_to_settings,
};
use nostr_sdk::prelude::PublicKey;
use std::str::FromStr;

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
    ctx: &crate::ui::key_handler::EnterKeyContext<'_>,
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
            let runtime_settings = match crate::settings::load_settings_from_disk() {
                Ok(s) => s,
                Err(e) => {
                    app.mode = UiMode::OperationResult(crate::ui::OperationResult::Error(format!(
                        "Failed to load settings for order creation: {}",
                        e
                    )));
                    return true;
                }
            };
            spawn_send_new_order_task(
                ctx.pool.clone(),
                ctx.client.clone(),
                runtime_settings,
                ctx.mostro_pubkey,
                form_clone,
                ctx.order_result_tx.clone(),
            );
            true
        }
        UiMode::UserMode(UserMode::TakingOrder(take_state)) => {
            // User confirmed taking the order (same as Enter key)
            execute_take_order_action(
                app,
                take_state,
                ctx.pool,
                ctx.client,
                ctx.mostro_pubkey,
                ctx.order_result_tx,
            );
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

            // Spawn task to refresh Mostro instance info using the new pubkey (no disk round-trip);
            // UI stays responsive.
            let new_pubkey = match PublicKey::from_str(&key_string) {
                Ok(pk) => pk,
                Err(e) => {
                    log::error!("Invalid pubkey after confirmation: {}", e);
                    app.mostro_info = None;
                    return true;
                }
            };
            if let Ok(mut active_pubkey) = ctx.current_mostro_pubkey.lock() {
                *active_pubkey = new_pubkey;
            } else {
                log::warn!("Failed to update runtime Mostro pubkey after confirmation");
            }
            spawn_refresh_mostro_info_task(
                ctx.client.clone(),
                new_pubkey,
                ctx.mostro_info_tx.clone(),
            );
            app.mode = crate::ui::UiMode::OperationResult(crate::ui::OperationResult::Info(
                "Fetching Mostro instance info...".to_string(),
            ));
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
            spawn_add_relay_task(ctx.client.clone(), relay_string.clone());
            true
        }
        UiMode::ConfirmCurrency(currency_string, _) => {
            let default_mode = match app.user_role {
                UserRole::User => UiMode::UserMode(UserMode::Normal),
                UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
            };
            // Persist to settings.toml.
            app.mode = handle_confirmation_enter(
                true, // 'y' key means YES
                &currency_string,
                default_mode,
                save_currency_to_settings,
                |input| UiMode::AddCurrency(create_key_input_state(input)),
            );
            // Update in-memory cache so UI-side filtering takes effect immediately.
            let upper = currency_string.trim().to_uppercase();
            if !upper.is_empty() && !app.currencies_filter.contains(&upper) {
                app.currencies_filter.push(upper);
            }
            true
        }
        UiMode::ConfirmClearCurrencies(_) => {
            let default_mode = match app.user_role {
                UserRole::User => UiMode::UserMode(UserMode::Normal),
                UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
            };
            // YES selected - clear currency filters
            clear_currency_filters();
            // Clear in-memory cache as well.
            app.currencies_filter.clear();
            app.mode = default_mode;
            true
        }
        UiMode::ConfirmExit(_) => {
            // 'y' key means YES - exit the application
            // Return false to break the main loop
            false
        }
        UiMode::AdminMode(AdminMode::ConfirmAddSolver(solver_pubkey, _)) => {
            // Delegate to the same handler used for Enter to keep logic DRY
            // (synthesize a mode with YES selected)
            let mode = UiMode::AdminMode(AdminMode::ConfirmAddSolver(solver_pubkey, true));
            handle_enter_admin_mode(app, mode, default_mode, ctx);
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
        UiMode::AdminMode(AdminMode::ConfirmTakeDispute(dispute_id, _)) => {
            // 'y' key means YES - always take the dispute (same as Enter key with YES selected)
            // This mirrors ConfirmAddSolver behavior: forced-YES input always triggers the action
            execute_take_dispute_action(
                app,
                dispute_id,
                ctx.client,
                ctx.mostro_pubkey,
                ctx.pool,
                ctx.order_result_tx,
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
    } else if let UiMode::AdminMode(AdminMode::ConfirmTakeDispute(_, _)) = &app.mode {
        // User cancelled taking the dispute
        let default_mode = match app.user_role {
            UserRole::User => UiMode::UserMode(UserMode::Normal),
            UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
        };
        app.mode = default_mode;
    }
}
