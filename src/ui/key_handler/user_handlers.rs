use crate::ui::{AppState, FormState, Tab, TakeOrderState, UiMode, UserMode, UserRole, UserTab};
use crate::SETTINGS;
use nostr_sdk::Client;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;

/// Handle Enter key when creating an order.
pub fn handle_enter_creating_order(app: &mut AppState, form: &FormState) {
    // Show confirmation popup when Enter is pressed
    if let Tab::User(UserTab::CreateNewOrder) = app.active_tab {
        app.mode = UiMode::UserMode(UserMode::ConfirmingOrder(form.clone()));
    } else {
        app.mode = UiMode::UserMode(UserMode::CreatingOrder(form.clone()));
    }
}

/// Handle Enter key when taking an order.
pub fn handle_enter_taking_order(
    app: &mut AppState,
    take_state: TakeOrderState,
    ctx: &crate::ui::key_handler::EnterKeyContext<'_>,
) {
    // Enter confirms the selected button
    if take_state.selected_button {
        // YES selected - execute take order action
        execute_take_order_action(
            app,
            take_state,
            ctx.pool,
            ctx.client,
            ctx.mostro_pubkey,
            ctx.order_result_tx,
        );
    } else {
        // NO selected - cancel and return to the appropriate normal mode
        let default_mode = match app.user_role {
            UserRole::User => UiMode::UserMode(UserMode::Normal),
            UserRole::Admin => UiMode::AdminMode(crate::ui::AdminMode::Normal),
        };
        app.mode = default_mode;
    }
}

/// Execute taking an order.
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
