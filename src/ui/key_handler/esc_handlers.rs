use crate::ui::key_handler::confirmation::{create_key_input_state, handle_confirmation_esc};
use crate::ui::{AdminMode, AdminTab, AppState, Tab, UiMode, UserMode, UserRole, UserTab};

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
            // Close result popup
            // If we're on Settings tab, stay there; otherwise return to first tab
            if !matches!(
                app.active_tab,
                Tab::Admin(AdminTab::Settings) | Tab::User(UserTab::Settings)
            ) {
                app.active_tab = Tab::first(app.user_role);
            }
            app.mode = default_mode.clone();
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
        UiMode::AdminMode(AdminMode::AddSolver(_))
        | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
        | UiMode::AddMostroPubkey(_)
        | UiMode::AddRelay(_) => {
            // Dismiss key input popup
            app.mode = default_mode.clone();
            true
        }
        UiMode::AdminMode(AdminMode::ConfirmAddSolver(solver_pubkey, _)) => {
            // Cancel confirmation, go back to input
            app.mode = handle_confirmation_esc(solver_pubkey, |input| {
                UiMode::AdminMode(AdminMode::AddSolver(create_key_input_state(input)))
            });
            true
        }
        UiMode::AdminMode(AdminMode::ConfirmAdminKey(key_string, _)) => {
            app.mode = handle_confirmation_esc(key_string, |input| {
                UiMode::AdminMode(AdminMode::SetupAdminKey(create_key_input_state(input)))
            });
            true
        }
        UiMode::ConfirmMostroPubkey(key_string, _) => {
            app.mode = handle_confirmation_esc(key_string, |input| {
                UiMode::AddMostroPubkey(create_key_input_state(input))
            });
            true
        }
        UiMode::ConfirmRelay(relay_string, _) => {
            app.mode = handle_confirmation_esc(relay_string, |input| {
                UiMode::AddRelay(create_key_input_state(input))
            });
            true
        }
        _ => false, // Break the loop
    }
}
