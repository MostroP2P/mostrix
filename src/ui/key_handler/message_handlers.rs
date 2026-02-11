use crate::ui::key_handler::EnterKeyContext;
use crate::ui::OrderResult;
use crate::ui::{AdminMode, AppState, MessageViewState, UiMode, UserMode, UserRole};
use crate::util::order_utils::{execute_add_invoice, execute_send_msg};
use mostro_core::prelude::*;
use uuid::Uuid;

/// Handle Enter key when viewing a message.
pub fn handle_enter_viewing_message(
    app: &mut AppState,
    view_state: &MessageViewState,
    ctx: &EnterKeyContext<'_>,
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
            let _ = ctx.order_result_tx.send(OrderResult::Error(
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
        let _ = ctx
            .order_result_tx
            .send(OrderResult::Error("No order ID in message".to_string()));
        let default_mode = match app.user_role {
            UserRole::User => UiMode::UserMode(UserMode::Normal),
            UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
        };
        app.mode = default_mode;
        return;
    };

    // Set waiting mode based on user role
    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::WaitingAddInvoice),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    app.mode = default_mode;

    // Spawn async task to send message
    let pool_clone = ctx.pool.clone();
    let client_clone = ctx.client.clone();
    let mostro_pubkey = ctx.mostro_pubkey;
    let result_tx = ctx.order_result_tx.clone();

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
                let _ = result_tx.send(OrderResult::Info("Message sent successfully".to_string()));
            }
            Err(e) => {
                log::error!("Failed to send message: {}", e);
                let _ = result_tx.send(OrderResult::Error(e.to_string()));
            }
        }
    });
}

/// Handle Enter key for message notifications (AddInvoice, PayInvoice, etc.)
pub fn handle_enter_message_notification(
    app: &mut AppState,
    ctx: &EnterKeyContext<'_>,
    action: &mostro_core::prelude::Action,
    invoice_state: &mut crate::ui::InvoiceInputState,
    order_id: Option<Uuid>,
) {
    match action {
        Action::AddInvoice => {
            // For AddInvoice, Enter submits the invoice
            let order_result_tx_clone = ctx.order_result_tx.clone();
            if !invoice_state.invoice_input.trim().is_empty() {
                if let Some(order_id) = order_id {
                    // Set waiting mode based on user role
                    let default_mode = match app.user_role {
                        UserRole::User => UiMode::UserMode(UserMode::WaitingAddInvoice),
                        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
                    };
                    app.mode = default_mode;

                    // Send invoice to Mostro
                    let invoice_state_clone = invoice_state.clone();
                    let pool_clone = ctx.pool.clone();
                    let client_clone = ctx.client.clone();
                    let mostro_pubkey = ctx.mostro_pubkey;
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
                                let _ = order_result_tx_clone.send(OrderResult::Info(
                                    "Invoice sent successfully".to_string(),
                                ));
                            }
                            Err(e) => {
                                log::error!("Failed to add invoice: {}", e);
                                let _ =
                                    order_result_tx_clone.send(OrderResult::Error(e.to_string()));
                            }
                        }
                    });
                }
            }
        }
        Action::PayInvoice => {}
        _ => {
            let _ = ctx
                .order_result_tx
                .send(OrderResult::Error("Invalid action".to_string()));
        }
    }
}
