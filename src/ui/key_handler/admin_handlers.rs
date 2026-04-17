use crate::ui::key_handler::EnterKeyContext;
use crate::ui::{AdminMode, AppState, UiMode};
use crate::util::fatal::request_fatal_restart;
use crate::util::order_utils::{execute_admin_add_solver, execute_finalize_dispute};
use uuid::Uuid;

use crate::ui::helpers::hydrate_app_admin_keys_from_privkey;
use crate::ui::key_handler::confirmation::{
    create_key_input_state, handle_confirmation_enter, handle_input_to_confirmation,
};
use crate::ui::key_handler::settings::try_save_admin_key_to_settings;
use crate::ui::key_handler::validation::{validate_npub, validate_nsec};
use crate::ui::orders::OperationResult;
use crate::ui::UserRole;
use crate::util::order_utils::execute_take_dispute;

/// Helper function to execute taking a dispute.
///
/// This avoids code duplication between Enter key and 'y' key handlers.
/// Sets the UI mode to waiting and spawns an async task to take the dispute.
pub(crate) fn execute_take_dispute_action(
    app: &mut AppState,
    dispute_id: Uuid,
    ctx: &EnterKeyContext<'_>,
) {
    app.mode = UiMode::AdminMode(AdminMode::WaitingTakeDispute(dispute_id));

    let current_mostro_pubkey = if let Ok(active_pubkey) = ctx.current_mostro_pubkey.lock() {
        *active_pubkey
    } else {
        request_fatal_restart(
            "Mostrix encountered an internal error (poisoned Mostro pubkey lock). Please restart the app."
                .to_string(),
        );
        return;
    };
    // Spawn async task to take dispute
    let client_clone = ctx.client.clone();
    let result_tx = ctx.order_result_tx.clone();
    let pool_clone = ctx.pool.clone();
    let mostro_info = ctx.mostro_info.clone();
    tokio::spawn(async move {
        match execute_take_dispute(
            &dispute_id,
            &client_clone,
            current_mostro_pubkey,
            &pool_clone,
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(_) => {
                let _ = result_tx.send(OperationResult::Info(format!(
                    "✅ Dispute {} taken successfully!",
                    dispute_id
                )));
            }
            Err(e) => {
                log::error!("Failed to take dispute: {}", e);
                let _ = result_tx.send(OperationResult::Error(e.to_string()));
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
    ctx: &EnterKeyContext<'_>,
) {
    // Stay on Settings tab after confirmation
    app.mode = UiMode::AdminMode(AdminMode::Normal);

    let solver_pubkey_clone = solver_pubkey.clone();
    let client_clone = ctx.client.clone();
    let result_tx = ctx.order_result_tx.clone();

    let current_mostro_pubkey = if let Ok(active_pubkey) = ctx.current_mostro_pubkey.lock() {
        *active_pubkey
    } else {
        request_fatal_restart(
            "Mostrix encountered an internal error (poisoned Mostro pubkey lock). Please restart the app."
                .to_string(),
        );
        return;
    };

    let mostro_info = ctx.mostro_info.clone();
    tokio::spawn(async move {
        match execute_admin_add_solver(
            &solver_pubkey_clone,
            &client_clone,
            current_mostro_pubkey,
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(_) => {
                let _ = result_tx.send(OperationResult::Info(
                    "Solver added successfully".to_string(),
                ));
            }
            Err(e) => {
                log::error!("Failed to add solver: {}", e);
                let _ = result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}

/// Helper function to execute dispute finalization (settle or cancel).
///
/// This avoids code duplication between Pay Buyer and Refund Seller actions.
/// Sets the UI mode to waiting and spawns an async task to finalize the dispute.
pub(crate) fn execute_finalize_dispute_action(
    app: &mut AppState,
    dispute_id: Uuid,
    ctx: &EnterKeyContext<'_>,
    is_settle: bool, // true = AdminSettle (pay buyer), false = AdminCancel (refund seller)
) {
    app.mode = UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(dispute_id));

    let current_mostro_pubkey = if let Ok(active_pubkey) = ctx.current_mostro_pubkey.lock() {
        *active_pubkey
    } else {
        request_fatal_restart(
            "Mostrix encountered an internal error (poisoned Mostro pubkey lock). Please restart the app."
                .to_string(),
        );
        return;
    };
    // Spawn async task to finalize dispute
    let client_clone = ctx.client.clone();
    let result_tx = ctx.order_result_tx.clone();
    let pool_clone = ctx.pool.clone();
    let mostro_info = ctx.mostro_info.clone();
    tokio::spawn(async move {
        match execute_finalize_dispute(
            &dispute_id,
            &client_clone,
            current_mostro_pubkey,
            &pool_clone,
            is_settle,
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(_) => {
                let action_name = if is_settle {
                    "settled (buyer paid)"
                } else {
                    "canceled (seller refunded)"
                };
                let _ = result_tx.send(OperationResult::Info(format!(
                    "✅ Dispute {} {}!",
                    dispute_id, action_name
                )));
            }
            Err(e) => {
                log::error!("Failed to finalize dispute: {}", e);
                let _ = result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}

/// Handle Enter key for admin-specific modes (AddSolver, SetupAdminKey, etc.)
/// Kept `pub(crate)` so it can be reused by the 'y' confirmation handler
/// to avoid duplicating the AddSolver execution logic (DRY).
pub(crate) fn handle_enter_admin_mode(
    app: &mut AppState,
    mode: UiMode,
    default_mode: UiMode,
    ctx: &crate::ui::key_handler::EnterKeyContext<'_>,
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
                    app.mode = UiMode::OperationResult(OperationResult::Error(e));
                }
            }
        }
        UiMode::AdminMode(AdminMode::ConfirmAddSolver(solver_pubkey, selected_button)) => {
            if selected_button {
                // YES selected - send AddSolver message
                execute_add_solver_action(app, solver_pubkey, ctx);
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
                    app.mode = UiMode::OperationResult(OperationResult::Error(e));
                }
            }
        }
        UiMode::AdminMode(AdminMode::ConfirmAdminKey(key_string, selected_button)) => {
            if selected_button {
                match try_save_admin_key_to_settings(&key_string) {
                    Ok(()) => {
                        hydrate_app_admin_keys_from_privkey(app, &key_string);
                        if app.user_role == UserRole::Admin {
                            app.pending_admin_disputes_reload = true;
                        }
                        app.mode = default_mode;
                    }
                    Err(e) => {
                        log::error!("{e}");
                        app.mode = UiMode::OperationResult(OperationResult::Error(e));
                    }
                }
            } else {
                app.mode = handle_confirmation_enter(
                    false,
                    &key_string,
                    default_mode,
                    |_| {},
                    |input| {
                        UiMode::AdminMode(AdminMode::SetupAdminKey(create_key_input_state(input)))
                    },
                );
            }
        }
        _ => {
            // This should not happen, but handle gracefully
            app.mode = default_mode;
        }
    }
}
