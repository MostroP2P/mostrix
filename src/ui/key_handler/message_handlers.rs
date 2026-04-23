use crate::ui::key_handler::EnterKeyContext;
use crate::ui::OperationResult;
use crate::ui::{
    AdminMode, AppState, MessageViewState, RatingOrderState, UiMode, UserMode, UserRole,
    ViewingMessageButtonSelection,
};
use crate::util::db_utils::update_order_status;
use crate::util::order_utils::{execute_add_invoice, execute_rate_user, execute_send_msg};
use mostro_core::order::Status;
use mostro_core::prelude::*;
use std::fs::OpenOptions;
use std::io::Write;
use uuid::Uuid;

fn debug_log_ui(hypothesis_id: &str, location: &str, message: &str, data: serde_json::Value) {
    let payload = serde_json::json!({
        "sessionId": "715880",
        "runId": "pre-fix",
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": chrono::Utc::now().timestamp_millis()
    });
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug-715880.log")
    {
        let _ = writeln!(file, "{payload}");
    }
}

/// Handle Enter key when viewing a message.
pub fn handle_enter_viewing_message(
    app: &mut AppState,
    view_state: &MessageViewState,
    ctx: &EnterKeyContext<'_>,
) {
    // #region agent log
    debug_log_ui(
        "H1",
        "src/ui/key_handler/message_handlers.rs:handle_enter_viewing_message:entry",
        "ViewingMessage enter pressed",
        serde_json::json!({
            "source_action": format!("{:?}", view_state.action),
            "button_selection": format!("{:?}", view_state.button_selection),
            "order_id": view_state.order_id.map(|id| id.to_string()),
        }),
    );
    // #endregion

    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::Normal),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };

    // NO / dismiss without sending
    match &view_state.button_selection {
        ViewingMessageButtonSelection::Two {
            yes_selected: false,
        } => {
            app.mode = default_mode;
            return;
        }
        ViewingMessageButtonSelection::Three { selected: 1 } => {
            app.mode = default_mode;
            return;
        }
        _ => {}
    }

    // Map the action from the message to the action we need to send
    let action_to_send = match &view_state.action {
        Action::HoldInvoicePaymentAccepted => match &view_state.button_selection {
            ViewingMessageButtonSelection::Three { selected: 2 } => Action::Cancel,
            _ => Action::FiatSent,
        },
        Action::FiatSentOk => Action::Release,
        Action::CooperativeCancelInitiatedByPeer => Action::Cancel,
        // For Shift+C/F/R confirmations, the action is already the one we want to send.
        Action::Cancel | Action::FiatSent | Action::Release => view_state.action.clone(),
        _ => {
            // This view is sometimes used as a generic "view message" popup; if the message
            // doesn't map to a sendable action, just dismiss without error.
            app.mode = default_mode;
            return;
        }
    };
    // #region agent log
    debug_log_ui(
        "H1",
        "src/ui/key_handler/message_handlers.rs:handle_enter_viewing_message:action_map",
        "Mapped source action to outbound action",
        serde_json::json!({
            "source_action": format!("{:?}", view_state.action),
            "outbound_action": format!("{:?}", action_to_send),
        }),
    );
    // #endregion

    // Get order_id from view_state
    let Some(order_id) = view_state.order_id else {
        let _ = ctx
            .order_result_tx
            .send(OperationResult::Error("No order ID in message".to_string()));
        let default_mode = match app.user_role {
            UserRole::User => UiMode::UserMode(UserMode::Normal),
            UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
        };
        app.mode = default_mode;
        return;
    };

    // Set waiting mode based on user role
    let waiting_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::WaitingAddInvoice),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    app.mode = waiting_mode;

    // Spawn async task to send message
    let pool_clone = ctx.pool.clone();
    let client_clone = ctx.client.clone();
    let mostro_pubkey = ctx.mostro_pubkey;
    let result_tx = ctx.order_result_tx.clone();
    let source_action = view_state.action.clone();
    let mostro_info = ctx.mostro_info.clone();
    let sent_cooperative_cancel_from_hold_invoice =
        matches!(view_state.action, Action::HoldInvoicePaymentAccepted)
            && matches!(action_to_send, Action::Cancel);

    tokio::spawn(async move {
        match execute_send_msg(
            &order_id,
            action_to_send,
            &pool_clone,
            &client_clone,
            mostro_pubkey,
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(_) => {
                // #region agent log
                debug_log_ui(
                    "H1",
                    "src/ui/key_handler/message_handlers.rs:handle_enter_viewing_message:send_ok",
                    "Follow-up message sent successfully",
                    serde_json::json!({
                        "order_id": order_id.to_string(),
                        "source_action": format!("{:?}", source_action),
                        "sent_cooperative_cancel_from_hold_invoice": sent_cooperative_cancel_from_hold_invoice,
                    }),
                );
                // #endregion
                let out = if source_action == Action::CooperativeCancelInitiatedByPeer {
                    match update_order_status(
                        &pool_clone,
                        &order_id.to_string(),
                        Status::CooperativelyCanceled,
                    )
                    .await
                    {
                        Ok(()) => OperationResult::TradeClosed {
                            order_id,
                            message: "Cooperative cancel completed.".to_string(),
                        },
                        Err(e) => {
                            log::warn!(
                                "Failed to save CooperativelyCanceled for order {}: {}",
                                order_id,
                                e
                            );
                            OperationResult::Error(format!(
                                "Failed to mark cooperatively canceled: {e}"
                            ))
                        }
                    }
                } else if sent_cooperative_cancel_from_hold_invoice {
                    OperationResult::Info("Cooperative cancel request sent.".to_string())
                } else {
                    OperationResult::Info("Message sent successfully".to_string())
                };
                let _ = result_tx.send(out);
            }
            Err(e) => {
                // #region agent log
                debug_log_ui(
                    "H1",
                    "src/ui/key_handler/message_handlers.rs:handle_enter_viewing_message:send_err",
                    "Follow-up message failed",
                    serde_json::json!({
                        "order_id": order_id.to_string(),
                        "source_action": format!("{:?}", source_action),
                        "error": e.to_string(),
                    }),
                );
                // #endregion
                log::error!("Failed to send message: {}", e);
                let _ = result_tx.send(OperationResult::Error(e.to_string()));
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
                    // #region agent log
                    debug_log_ui(
                        "H2",
                        "src/ui/key_handler/message_handlers.rs:handle_enter_message_notification:add_invoice_submit",
                        "Submitting AddInvoice from popup",
                        serde_json::json!({
                            "order_id": order_id.to_string(),
                            "invoice_len": invoice_state.invoice_input.len(),
                            "popup_action": "AddInvoice",
                        }),
                    );
                    // #endregion
                    // Set waiting mode based on user role
                    let default_mode = match app.user_role {
                        UserRole::User => UiMode::UserMode(UserMode::WaitingAddInvoice),
                        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
                    };
                    app.pending_post_take_operation_result = None;
                    app.mode = default_mode;

                    // Send invoice to Mostro
                    let invoice_state_clone = invoice_state.clone();
                    let pool_clone = ctx.pool.clone();
                    let client_clone = ctx.client.clone();
                    let mostro_pubkey = ctx.mostro_pubkey;
                    let mostro_info = ctx.mostro_info.clone();
                    tokio::spawn(async move {
                        match execute_add_invoice(
                            &order_id,
                            &invoice_state_clone.invoice_input,
                            &pool_clone,
                            &client_clone,
                            mostro_pubkey,
                            mostro_info.as_ref(),
                        )
                        .await
                        {
                            Ok(_) => {
                                let _ = order_result_tx_clone.send(OperationResult::Info(
                                    "Invoice sent successfully".to_string(),
                                ));
                            }
                            Err(e) => {
                                log::error!("Failed to add invoice: {}", e);
                                let _ = order_result_tx_clone
                                    .send(OperationResult::Error(e.to_string()));
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
                .send(OperationResult::Error("Invalid action".to_string()));
        }
    }
}

/// Confirm and send the selected star rating (`RateUser`).
pub fn handle_enter_rating_order(
    app: &mut AppState,
    state: &RatingOrderState,
    ctx: &EnterKeyContext<'_>,
) {
    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::WaitingAddInvoice),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    app.mode = default_mode;

    let order_id = state.order_id;
    let rating = state.selected_rating;
    let pool_clone = ctx.pool.clone();
    let client_clone = ctx.client.clone();
    let mostro_pubkey = ctx.mostro_pubkey;
    let result_tx = ctx.order_result_tx.clone();
    let mostro_info = ctx.mostro_info.clone();

    tokio::spawn(async move {
        match execute_rate_user(
            &order_id,
            rating,
            &pool_clone,
            &client_clone,
            mostro_pubkey,
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(()) => {
                let _ = result_tx.send(OperationResult::Info(
                    "Rating sent successfully".to_string(),
                ));
            }
            Err(e) => {
                log::error!("Failed to send rating: {}", e);
                let _ = result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}
