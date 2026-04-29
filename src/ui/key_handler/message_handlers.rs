use crate::ui::key_handler::EnterKeyContext;
use crate::ui::OperationResult;
use crate::ui::{
    AdminMode, AppState, InvoiceNotificationActionSelection, MessageViewState, RatingOrderState,
    ThreeState, UiMode, UserMode, UserRole, ViewingMessageButtonSelection,
};
use crate::util::db_utils::update_order_status;
use crate::util::order_utils::{execute_add_invoice, execute_rate_user, execute_send_msg};
use mostro_core::order::Status;
use mostro_core::prelude::*;
use uuid::Uuid;

fn role_default_mode(user_role: UserRole) -> UiMode {
    match user_role {
        UserRole::User => UiMode::UserMode(UserMode::Normal),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    }
}

fn role_waiting_mode(user_role: UserRole) -> UiMode {
    match user_role {
        UserRole::User => UiMode::UserMode(UserMode::WaitingAddInvoice),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    }
}

fn should_send_cancel_from_invoice_popup(selection: InvoiceNotificationActionSelection) -> bool {
    matches!(selection, InvoiceNotificationActionSelection::Cancel)
}

fn spawn_cancel_from_notification(
    app: &mut AppState,
    ctx: &EnterKeyContext<'_>,
    order_id: Option<Uuid>,
) {
    let Some(order_id) = order_id else {
        let _ = ctx
            .order_result_tx
            .send(OperationResult::Error("No order ID in message".to_string()));
        app.mode = role_default_mode(app.user_role);
        return;
    };

    app.mode = role_waiting_mode(app.user_role);
    let pool_clone = ctx.pool.clone();
    let client_clone = ctx.client.clone();
    let mostro_pubkey = ctx.mostro_pubkey;
    let order_result_tx_clone = ctx.order_result_tx.clone();
    let mostro_info = ctx.mostro_info.clone();
    tokio::spawn(async move {
        match execute_send_msg(
            &order_id,
            Action::Cancel,
            &pool_clone,
            &client_clone,
            mostro_pubkey,
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(_) => {
                let _ = order_result_tx_clone
                    .send(OperationResult::Info("Cancel request sent.".to_string()));
            }
            Err(e) => {
                log::error!("Failed to cancel order from invoice popup: {}", e);
                let _ = order_result_tx_clone.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}

/// Handle Enter key when viewing a message.
pub fn handle_enter_viewing_message(
    app: &mut AppState,
    view_state: &MessageViewState,
    ctx: &EnterKeyContext<'_>,
) {
    let default_mode = role_default_mode(app.user_role);

    // NO / dismiss without sending
    match &view_state.button_selection {
        ViewingMessageButtonSelection::Two {
            yes_selected: false,
        } => {
            app.mode = default_mode;
            return;
        }
        ViewingMessageButtonSelection::Three(ThreeState::No) => {
            app.mode = default_mode;
            return;
        }
        _ => {}
    }

    // Map the action from the message to the action we need to send
    let action_to_send = match &view_state.action {
        Action::HoldInvoicePaymentAccepted => match &view_state.button_selection {
            ViewingMessageButtonSelection::Three(ThreeState::Yes)
            | ViewingMessageButtonSelection::Three(ThreeState::No) => Action::FiatSent,
            ViewingMessageButtonSelection::Three(ThreeState::Cancel) => Action::Cancel,
            ViewingMessageButtonSelection::Two { yes_selected } => {
                if *yes_selected {
                    Action::FiatSent
                } else {
                    app.mode = default_mode;
                    return;
                }
            }
        },
        Action::BuyerTookOrder => Action::Cancel,
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

    // Get order_id from view_state
    let Some(order_id) = view_state.order_id else {
        let _ = ctx
            .order_result_tx
            .send(OperationResult::Error("No order ID in message".to_string()));
        app.mode = role_default_mode(app.user_role);
        return;
    };

    // Set waiting mode based on user role
    app.mode = role_waiting_mode(app.user_role);

    // Spawn async task to send message
    let pool_clone = ctx.pool.clone();
    let client_clone = ctx.client.clone();
    let mostro_pubkey = ctx.mostro_pubkey;
    let result_tx = ctx.order_result_tx.clone();
    let source_action = view_state.action.clone();
    let mostro_info = ctx.mostro_info.clone();
    let sent_cooperative_cancel_request = matches!(action_to_send, Action::Cancel)
        && matches!(
            view_state.action,
            Action::HoldInvoicePaymentAccepted | Action::BuyerTookOrder
        );

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
                } else if sent_cooperative_cancel_request {
                    OperationResult::Info("Cooperative cancel request sent.".to_string())
                } else {
                    OperationResult::Info("Message sent successfully".to_string())
                };
                let _ = result_tx.send(out);
            }
            Err(e) => {
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
            if should_send_cancel_from_invoice_popup(invoice_state.action_selection) {
                spawn_cancel_from_notification(app, ctx, order_id);
                return;
            }
            // For AddInvoice, Enter submits the invoice
            let order_result_tx_clone = ctx.order_result_tx.clone();
            if !invoice_state.invoice_input.trim().is_empty() {
                if let Some(order_id) = order_id {
                    // Set waiting mode based on user role
                    let default_mode = role_waiting_mode(app.user_role);
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
        Action::PayInvoice => {
            if should_send_cancel_from_invoice_popup(invoice_state.action_selection) {
                spawn_cancel_from_notification(app, ctx, order_id);
                return;
            }
            // Primary path for PayInvoice is acknowledgement: close popup.
            app.mode = role_default_mode(app.user_role);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_selection_triggers_cancel_path() {
        assert!(should_send_cancel_from_invoice_popup(
            InvoiceNotificationActionSelection::Cancel
        ));
        assert!(!should_send_cancel_from_invoice_popup(
            InvoiceNotificationActionSelection::Primary
        ));
    }
}
