use crate::models::AdminDispute;
use crate::shared::permissions::SolverPermission;
use crate::ui::helpers::hydrate_app_admin_keys_from_privkey;
use crate::ui::helpers::selected_filtered_dispute;
use crate::ui::key_handler::confirmation::{
    create_key_input_state, handle_confirmation_enter, handle_input_to_confirmation,
};
use crate::ui::key_handler::settings::try_save_admin_key_to_settings;
use crate::ui::key_handler::validation::{normalize_to_nsec, validate_npub};
use crate::ui::key_handler::EnterKeyContext;
use crate::ui::orders::OperationResult;
use crate::ui::takeover_picker::{takeover_candidates, TakeoverCandidate};
use crate::ui::{AddSolverState, AdminMode, AdminTab, AppState, Tab, UiMode, UserRole};
use crate::util::fatal::request_fatal_restart;
use crate::util::order_utils::{
    execute_admin_add_solver, execute_finalize_dispute, execute_take_dispute,
    orphan_in_progress_dispute_ids, take_error_message, AdminFinalizeAck, BondSlashChoice,
};
use mostro_core::prelude::Dispute;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Helper function to execute taking a dispute.
///
/// Shared by the Enter-key confirmation handler to avoid code duplication.
/// `takeover` comes from where the take started (Ctrl+T picker) and picks
/// the post-take navigation and the refusal wording.
/// Sets the UI mode to waiting and spawns an async task to take the dispute.
pub(crate) fn execute_take_dispute_action(
    app: &mut AppState,
    dispute_id: Uuid,
    takeover: bool,
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
    let Some(admin_keys) = ctx.admin_chat_keys.cloned() else {
        app.mode = UiMode::operation_result(OperationResult::Error(
            "Admin private key not configured".to_string(),
        ));
        return;
    };
    let client_clone = ctx.client.clone();
    let result_tx = ctx.order_result_tx.clone();
    let pool_clone = ctx.pool.clone();
    let mostro_info = ctx.mostro_info.clone();
    tokio::spawn(async move {
        match execute_take_dispute(
            &dispute_id,
            &admin_keys,
            &client_clone,
            current_mostro_pubkey,
            &pool_clone,
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(_) => {
                let _ = result_tx.send(OperationResult::DisputeTaken {
                    dispute_id,
                    message: format!("✅ Dispute {} taken successfully!", dispute_id),
                    takeover,
                });
            }
            Err(e) => {
                log::error!("Failed to take dispute {}: {}", dispute_id, e);
                let _ = result_tx.send(OperationResult::Error(take_error_message(&e, takeover)));
            }
        }
    });
}

/// Open Shift+R orphan picker when relay has `in-progress` disputes missing from local DB.
pub(crate) fn begin_recover_taken_disputes(
    app: &mut AppState,
    disputes: &Arc<Mutex<Vec<Dispute>>>,
) {
    let Ok(relay) = disputes.lock() else {
        app.mode = UiMode::operation_result(OperationResult::Error(
            "Failed to read live disputes list".to_string(),
        ));
        return;
    };
    let local_ids: HashSet<String> = app
        .admin_disputes_in_progress
        .iter()
        .map(|d| d.dispute_id.clone())
        .collect();
    let orphans = orphan_in_progress_dispute_ids(&relay, &local_ids);
    drop(relay);

    if orphans.is_empty() {
        app.mode = UiMode::operation_result(OperationResult::Info(
            "No missing taken disputes to recover (relay in-progress matches local DB)."
                .to_string(),
        ));
        return;
    }

    let checked = vec![false; orphans.len()];
    app.mode = UiMode::AdminMode(AdminMode::SelectRecoverTakenDisputes {
        candidates: orphans,
        cursor: 0,
        checked,
    });
}

/// Open the Ctrl+T take-over picker, or explain why it is empty.
pub(crate) fn begin_takeover_picker(app: &mut AppState, disputes: &Arc<Mutex<Vec<Dispute>>>) {
    let Ok(relay) = disputes.lock() else {
        app.mode = UiMode::operation_result(OperationResult::Error(
            "Failed to read live disputes list".to_string(),
        ));
        return;
    };
    let local_ids: HashSet<String> = app
        .admin_disputes_in_progress
        .iter()
        .map(|d| d.dispute_id.clone())
        .collect();
    let candidates = takeover_candidates(
        &app.solver_dms,
        &relay,
        &local_ids,
        chrono::Utc::now().timestamp(),
    );
    drop(relay);

    if candidates.is_empty() {
        app.mode = UiMode::operation_result(OperationResult::Info(
            "Nothing to take over: no in-progress dispute that Serbero wrote to you about \
             in the last 12 h is missing from your In Progress list. Serbero's messages are \
             read from trusted_dm_senders in settings.toml."
                .to_string(),
        ));
        return;
    }
    app.mode = UiMode::AdminMode(AdminMode::SelectTakeoverDispute {
        candidates,
        cursor: 0,
    });
}

/// Mode to return to after a take popup is dismissed: Disputes in Progress
/// keeps managing its dispute (chat, scroll, Shift shortcuts), others go Normal.
pub(crate) fn admin_dispute_home_mode(app: &AppState) -> UiMode {
    if matches!(app.active_tab, Tab::Admin(AdminTab::DisputesInProgress)) {
        UiMode::AdminMode(AdminMode::ManagingDispute)
    } else {
        UiMode::AdminMode(AdminMode::Normal)
    }
}

/// Enter on the take-over picker: confirm taking the highlighted dispute.
pub(crate) fn choose_takeover_candidate(
    app: &mut AppState,
    candidates: &[TakeoverCandidate],
    cursor: usize,
) {
    match candidates.get(cursor) {
        Some(candidate) => {
            app.mode = UiMode::AdminMode(AdminMode::ConfirmTakeDispute {
                dispute_id: candidate.dispute_id,
                takeover: true,
                selected_button: true,
            });
        }
        None => app.mode = admin_dispute_home_mode(app),
    }
}

/// Advance from the orphan picker into a Yes/No confirm for the selected IDs.
pub(crate) fn begin_confirm_recover_selection(
    app: &mut AppState,
    candidates: Vec<Uuid>,
    cursor: usize,
    checked: Vec<bool>,
) {
    let recover_ids = crate::ui::recover_disputes_picker::recover_ids_from_selection(
        &candidates,
        cursor,
        &checked,
    );
    if recover_ids.is_empty() {
        app.mode = UiMode::operation_result(OperationResult::Info(
            "No dispute selected to recover.".to_string(),
        ));
        return;
    }
    app.mode = UiMode::AdminMode(AdminMode::ConfirmRecoverTakenDisputes {
        candidates,
        cursor,
        checked,
        recover_ids,
        selected_button: true,
    });
}

/// Confirm Delete: remove selected dispute from local `admin_disputes` only.
pub(crate) fn begin_delete_admin_dispute(app: &mut AppState) {
    let Some(dispute) = selected_filtered_dispute(app) else {
        app.mode = UiMode::operation_result(OperationResult::Info(
            "Select a dispute in the sidebar to delete from local database.".to_string(),
        ));
        return;
    };
    app.mode = UiMode::AdminMode(AdminMode::ConfirmDeleteAdminDispute {
        dispute_id: dispute.dispute_id.clone(),
        selected_button: true,
    });
}

/// Delete the confirmed dispute from SQLite; UI list refreshes via [`OperationResult::AdminDisputeDeleted`].
pub(crate) fn execute_delete_admin_dispute_action(
    app: &mut AppState,
    dispute_id: String,
    ctx: &EnterKeyContext<'_>,
) {
    let pool = ctx.pool.clone();
    let result_tx = ctx.order_result_tx.clone();
    app.mode = UiMode::AdminMode(AdminMode::WaitingDeleteAdminDispute);

    tokio::spawn(async move {
        match AdminDispute::delete_by_dispute_id(&pool, &dispute_id).await {
            Ok(affected) if affected > 0 => {
                let _ = result_tx.send(OperationResult::AdminDisputeDeleted {
                    dispute_id: dispute_id.clone(),
                    message: format!(
                        "Deleted dispute {dispute_id} from local database.\n(Use Shift+R to re-fetch if Mostro still assigns it to you.)"
                    ),
                });
            }
            Ok(_) => {
                let _ = result_tx.send(OperationResult::Error(format!(
                    "Dispute {dispute_id} was not found in local database."
                )));
            }
            Err(e) => {
                let _ = result_tx.send(OperationResult::Error(format!(
                    "Failed to delete dispute {dispute_id} from local database: {e}"
                )));
            }
        }
    });
}

/// Re-send `AdminTakeDispute` for the explicitly selected orphan dispute IDs.
pub(crate) fn execute_recover_taken_disputes_action(
    app: &mut AppState,
    recover_ids: Vec<Uuid>,
    ctx: &EnterKeyContext<'_>,
) {
    if recover_ids.is_empty() {
        app.mode = UiMode::operation_result(OperationResult::Info(
            "No dispute selected to recover.".to_string(),
        ));
        return;
    }

    let Some(admin_keys) = ctx.admin_chat_keys.cloned() else {
        app.mode = UiMode::operation_result(OperationResult::Error(
            "Admin private key not configured".to_string(),
        ));
        return;
    };
    let current_mostro_pubkey = if let Ok(active_pubkey) = ctx.current_mostro_pubkey.lock() {
        *active_pubkey
    } else {
        request_fatal_restart(
            "Mostrix encountered an internal error (poisoned Mostro pubkey lock). Please restart the app."
                .to_string(),
        );
        return;
    };

    app.mode = UiMode::AdminMode(AdminMode::WaitingRecoverTakenDisputes);
    let client_clone = ctx.client.clone();
    let result_tx = ctx.order_result_tx.clone();
    let pool_clone = ctx.pool.clone();
    let mostro_info = ctx.mostro_info.clone();
    let recover_count = recover_ids.len();
    tokio::spawn(async move {
        let mut recovered = 0usize;
        let mut rejected = 0usize;
        let mut failed = 0usize;
        let mut recovered_ids: Vec<String> = Vec::new();
        let mut rejected_lines: Vec<String> = Vec::new();
        let mut failed_lines: Vec<String> = Vec::new();

        for dispute_id in recover_ids {
            match execute_take_dispute(
                &dispute_id,
                &admin_keys,
                &client_clone,
                current_mostro_pubkey,
                &pool_clone,
                mostro_info.as_ref(),
            )
            .await
            {
                Ok(()) => {
                    recovered += 1;
                    recovered_ids.push(dispute_id.to_string());
                    log::info!(
                        "✅ Recovered dispute {} taken successfully and saved locally",
                        dispute_id
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    log::error!("Failed to recover dispute {}: {}", dispute_id, msg);
                    if let Some(reason) =
                        msg.strip_prefix(&format!("Mostro rejected take dispute {}: ", dispute_id))
                    {
                        rejected += 1;
                        rejected_lines.push(format!("⛔ {dispute_id}\n   {reason}"));
                    } else if msg.contains("Mostro rejected take dispute") {
                        rejected += 1;
                        rejected_lines.push(format!("⛔ {dispute_id}\n   {msg}"));
                    } else {
                        failed += 1;
                        failed_lines.push(format!("❌ {dispute_id}\n   {msg}"));
                    }
                }
            }
        }

        let mut summary = if recovered > 0 {
            format!(
                "✅ Recovered {recovered}/{recover_count} · ⛔ rejected {rejected} · ❌ failed {failed}"
            )
        } else if rejected > 0 && failed == 0 {
            format!("⛔ Mostro rejected all {recover_count} selected dispute(s)")
        } else {
            format!(
                "ℹ️ No disputes recovered ({recover_count} requested) · ⛔ rejected {rejected} · ❌ failed {failed}"
            )
        };
        if !recovered_ids.is_empty() {
            summary.push_str("\n\nRecovered:");
            for id in &recovered_ids {
                summary.push_str(&format!("\n✨ {id}"));
            }
        }
        if !rejected_lines.is_empty() {
            summary.push_str("\n\nRejected by Mostro:");
            for line in &rejected_lines {
                summary.push('\n');
                summary.push_str(line);
            }
        }
        if !failed_lines.is_empty() {
            summary.push_str("\n\nFailed:");
            for line in &failed_lines {
                summary.push('\n');
                summary.push_str(line);
            }
        }
        // Keep "taken successfully" wording when anything was restored so the
        // admin disputes list reloads via apply_order_result.
        if recovered > 0 {
            summary.push_str("\n\nDispute(s) taken successfully and saved locally.");
        }
        let result = if recovered == 0 && (rejected > 0 || failed > 0) {
            OperationResult::Error(summary)
        } else {
            OperationResult::Info(summary)
        };
        let _ = result_tx.send(result);
    });
}

/// Helper function to execute adding a solver.
///
/// Shared by the Enter-key confirmation handler to avoid code duplication.
/// `takeover` comes from where the take started (Ctrl+T picker) and picks
/// the post-take navigation and the refusal wording.
/// Sets the UI mode to waiting and spawns an async task to add the solver.
pub(crate) fn execute_add_solver_action(
    app: &mut AppState,
    solver_pubkey: String,
    permission: SolverPermission,
    ctx: &EnterKeyContext<'_>,
) {
    let Some(admin_keys) = ctx.admin_chat_keys.cloned() else {
        app.mode = UiMode::operation_result(OperationResult::Error(
            "Admin private key not configured".to_string(),
        ));
        return;
    };
    app.mode = UiMode::AdminMode(AdminMode::WaitingAddSolver);

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
            &solver_pubkey,
            permission,
            &admin_keys,
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
    bond: BondSlashChoice,
) {
    let Some(admin_keys) = ctx.admin_chat_keys.cloned() else {
        app.mode = UiMode::operation_result(OperationResult::Error(
            "Admin private key not configured".to_string(),
        ));
        return;
    };
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
            bond,
            &admin_keys,
            &client_clone,
            current_mostro_pubkey,
            &pool_clone,
            is_settle,
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(ack) => {
                let msg = match ack {
                    AdminFinalizeAck::Confirmed => {
                        bond.finalize_success_message(dispute_id, is_settle)
                    }
                    AdminFinalizeAck::AlreadyCooperativelyCanceled => {
                        format!(
                            "Dispute finalized\n\nOutcome:\nCooperative cancellation accepted — seller refunded\n\nDispute ID:\n{dispute_id}"
                        )
                    }
                };
                let _ = result_tx.send(OperationResult::Info(msg));
            }
            Err(e) => {
                log::error!("Failed to finalize dispute: {}", e);
                let _ = result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}

/// Handle Enter key for admin-specific modes (AddSolver, SetupAdminKey, etc.)
/// Kept `pub(crate)` so it can be reused by the Enter confirmation handler
/// to avoid duplicating the AddSolver execution logic (DRY).
pub(crate) fn handle_enter_admin_mode(
    app: &mut AppState,
    mode: UiMode,
    default_mode: UiMode,
    ctx: &crate::ui::key_handler::EnterKeyContext<'_>,
) {
    match mode {
        UiMode::AdminMode(AdminMode::AddSolver(add_solver_state)) => {
            // Validate npub before proceeding to confirmation
            match validate_npub(&add_solver_state.key_input.key_input) {
                Ok(_) => {
                    app.mode = handle_input_to_confirmation(
                        &add_solver_state.key_input.key_input,
                        default_mode,
                        |input| {
                            UiMode::AdminMode(AdminMode::ConfirmAddSolver {
                                solver_pubkey: input,
                                permission: add_solver_state.permission,
                                selected_button: true,
                            })
                        },
                    );
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::operation_result(OperationResult::Error(e));
                }
            }
        }
        UiMode::AdminMode(AdminMode::ConfirmAddSolver {
            solver_pubkey,
            permission,
            selected_button,
        }) => {
            if selected_button {
                // YES selected - send AddSolver message
                execute_add_solver_action(app, solver_pubkey, permission, ctx);
            } else {
                // NO selected - go back to input
                app.mode = UiMode::AdminMode(AdminMode::AddSolver(AddSolverState {
                    key_input: create_key_input_state(&solver_pubkey),
                    permission,
                }));
            }
        }
        UiMode::AdminMode(AdminMode::SetupAdminKey(key_state)) => {
            match normalize_to_nsec(&key_state.key_input) {
                Ok(normalized) => {
                    app.mode = handle_input_to_confirmation(&normalized, default_mode, |input| {
                        UiMode::AdminMode(AdminMode::ConfirmAdminKey(input, true))
                    });
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::operation_result(OperationResult::Error(e));
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
                        app.mode = UiMode::operation_result(OperationResult::Error(e));
                    }
                }
            } else {
                app.mode = handle_confirmation_enter(
                    false,
                    &key_string,
                    default_mode,
                    |_| Ok(()),
                    |input| {
                        UiMode::AdminMode(AdminMode::SetupAdminKey(create_key_input_state(input)))
                    },
                )
                .unwrap_or_else(|e| UiMode::operation_result(OperationResult::Error(e)));
            }
        }
        _ => {
            // This should not happen, but handle gracefully
            app.mode = default_mode;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{AdminTab, Tab};

    #[test]
    fn begin_confirm_recover_selection_opens_yes_no_with_selected_ids() {
        let mut app = AppState::new(UserRole::Admin);
        app.active_tab = Tab::Admin(AdminTab::DisputesInProgress);
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        begin_confirm_recover_selection(&mut app, vec![a, b], 0, vec![false, true]);

        match &app.mode {
            UiMode::AdminMode(AdminMode::ConfirmRecoverTakenDisputes {
                recover_ids,
                selected_button,
                ..
            }) => {
                assert_eq!(recover_ids, &vec![b]);
                assert!(*selected_button);
            }
            other => panic!("expected confirm mode, got {other:?}"),
        }
    }

    fn serbero_app(dispute: Uuid) -> (AppState, Arc<Mutex<Vec<Dispute>>>) {
        use crate::util::solver_dms::{index_by_dispute, SolverDm};
        let mut app = AppState::new(UserRole::Admin);
        app.solver_dms = index_by_dispute(vec![SolverDm {
            event_id: "e1".into(),
            sender_pubkey: String::new(),
            recipient_pubkey: String::new(),
            dispute_id: Some(dispute.to_string()),
            subject: "handed off: conflicting_claims".into(),
            text: String::new(),
            created_at: chrono::Utc::now().timestamp() - 60,
        }]);
        let relay = vec![Dispute {
            id: dispute,
            status: "in-progress".into(),
            ..Default::default()
        }];
        (app, Arc::new(Mutex::new(relay)))
    }

    #[test]
    fn ctrl_t_lists_in_progress_disputes_serbero_wrote_about() {
        let dispute = Uuid::from_u128(42);
        let (mut app, relay) = serbero_app(dispute);

        begin_takeover_picker(&mut app, &relay);

        match &app.mode {
            UiMode::AdminMode(AdminMode::SelectTakeoverDispute { candidates, cursor }) => {
                assert_eq!(candidates.len(), 1);
                assert_eq!(candidates[0].dispute_id, dispute);
                assert_eq!(*cursor, 0);
            }
            other => panic!("expected take-over picker, got {other:?}"),
        }
    }

    #[test]
    fn ctrl_t_skips_disputes_already_taken_locally() {
        let dispute = Uuid::from_u128(42);
        let (mut app, relay) = serbero_app(dispute);
        app.admin_disputes_in_progress = vec![AdminDispute {
            dispute_id: dispute.to_string(),
            ..Default::default()
        }];

        begin_takeover_picker(&mut app, &relay);

        match &app.mode {
            UiMode::OperationResult(result) => match result.as_ref() {
                OperationResult::Info(msg) => assert!(msg.contains("trusted_dm_senders"), "{msg}"),
                other => panic!("expected info, got {other:?}"),
            },
            other => panic!("expected info popup, got {other:?}"),
        }
    }

    #[test]
    fn choosing_a_candidate_asks_to_confirm_the_take() {
        let dispute = Uuid::from_u128(42);
        let (mut app, relay) = serbero_app(dispute);
        begin_takeover_picker(&mut app, &relay);
        let UiMode::AdminMode(AdminMode::SelectTakeoverDispute { candidates, cursor }) =
            app.mode.clone()
        else {
            panic!("picker not open");
        };

        choose_takeover_candidate(&mut app, &candidates, cursor);

        assert!(matches!(
            app.mode,
            UiMode::AdminMode(AdminMode::ConfirmTakeDispute {
                dispute_id,
                takeover: true,
                selected_button: true,
            }) if dispute_id == dispute
        ));
    }

    #[test]
    fn begin_confirm_recover_falls_back_to_cursor_when_none_checked() {
        let mut app = AppState::new(UserRole::Admin);
        let a = Uuid::from_u128(10);
        let b = Uuid::from_u128(20);
        begin_confirm_recover_selection(&mut app, vec![a, b], 1, vec![false, false]);

        match &app.mode {
            UiMode::AdminMode(AdminMode::ConfirmRecoverTakenDisputes { recover_ids, .. }) => {
                assert_eq!(recover_ids, &vec![b]);
            }
            other => panic!("expected confirm mode, got {other:?}"),
        }
    }
}
