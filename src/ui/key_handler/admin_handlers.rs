use crate::ui::{AdminMode, AppState, UiMode};
use crate::util::order_utils::{execute_admin_add_solver, execute_finalize_dispute};
use nostr_sdk::Client;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::ui::key_handler::confirmation::{
    create_key_input_state, handle_confirmation_enter, handle_input_to_confirmation,
};
use crate::ui::key_handler::settings::save_admin_key_to_settings;
use crate::ui::key_handler::validation::{validate_npub, validate_nsec};
use crate::ui::orders::OperationResult;
use crate::util::order_utils::execute_take_dispute;

/// Helper function to execute taking a dispute.
///
/// This avoids code duplication between Enter key and 'y' key handlers.
/// Sets the UI mode to waiting and spawns an async task to take the dispute.
pub(crate) fn execute_take_dispute_action(
    app: &mut AppState,
    dispute_id: Uuid,
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    pool: &SqlitePool,
    order_result_tx: &UnboundedSender<OperationResult>,
) {
    app.mode = UiMode::AdminMode(AdminMode::WaitingTakeDispute(dispute_id));

    // Spawn async task to take dispute
    let client_clone = client.clone();
    let result_tx = order_result_tx.clone();
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        match execute_take_dispute(&dispute_id, &client_clone, mostro_pubkey, &pool_clone).await {
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
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    order_result_tx: &UnboundedSender<OperationResult>,
) {
    // Stay on Settings tab after confirmation
    app.mode = UiMode::AdminMode(AdminMode::Normal);

    let solver_pubkey_clone = solver_pubkey.clone();
    let client_clone = client.clone();
    let result_tx = order_result_tx.clone();

    tokio::spawn(async move {
        match execute_admin_add_solver(&solver_pubkey_clone, &client_clone, mostro_pubkey).await {
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
    client: &Client,
    mostro_pubkey: nostr_sdk::PublicKey,
    pool: &SqlitePool,
    order_result_tx: &UnboundedSender<OperationResult>,
    is_settle: bool, // true = AdminSettle (pay buyer), false = AdminCancel (refund seller)
) {
    app.mode = UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(dispute_id));

    // Spawn async task to finalize dispute
    let client_clone = client.clone();
    let result_tx = order_result_tx.clone();
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        match execute_finalize_dispute(
            &dispute_id,
            &client_clone,
            mostro_pubkey,
            &pool_clone,
            is_settle,
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
                execute_add_solver_action(
                    app,
                    solver_pubkey,
                    ctx.client,
                    ctx.mostro_pubkey,
                    ctx.order_result_tx,
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
                    app.mode = UiMode::OperationResult(OperationResult::Error(e));
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
