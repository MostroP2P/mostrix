use crate::ui::{AdminMode, AppState, UiMode, UserMode, UserRole};
use crate::util::dm_utils::apply_saved_ln_address_invoice_choice;

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

/// Handle 'n' key for cancellation
pub fn handle_cancel_key(app: &mut AppState) {
    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::Normal),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    if let UiMode::UserMode(UserMode::ConfirmingOrder { form, .. }) = &app.mode {
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
    } else if let UiMode::ConfirmLnAddress(addr, _) = &app.mode {
        app.mode = handle_confirmation_esc(addr, |input| {
            UiMode::AddLnAddress(create_key_input_state(input))
        });
    } else if let UiMode::ConfirmSavedLnAddressForInvoice(notification, _) = &app.mode {
        apply_saved_ln_address_invoice_choice(app, notification.clone(), false);
    } else if let UiMode::ConfirmClearLnAddress(_) = &app.mode {
        app.mode = default_mode;
    } else if let UiMode::ConfirmCurrency(currency_string, _) = &app.mode {
        // Cancel currency confirmation, go back to AddCurrency input
        app.mode = handle_confirmation_esc(currency_string, |input| {
            UiMode::AddCurrency(create_key_input_state(input))
        });
    } else if let UiMode::ConfirmClearCurrencies(_) = &app.mode {
        // Cancel clear-all confirmation, just return to default mode
        app.mode = default_mode;
    } else if matches!(
        app.mode,
        UiMode::ConfirmDeleteHistoryOrder(_, _) | UiMode::ConfirmBulkDeleteHistory(_)
    ) {
        app.mode = default_mode;
    } else if let UiMode::AdminMode(AdminMode::ConfirmAddSolver {
        solver_pubkey,
        permission,
        ..
    }) = &app.mode
    {
        app.mode = handle_confirmation_esc(solver_pubkey, |input| {
            UiMode::AdminMode(AdminMode::AddSolver(crate::ui::AddSolverState {
                key_input: create_key_input_state(input),
                permission: *permission,
            }))
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
