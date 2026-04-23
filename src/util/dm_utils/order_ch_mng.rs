// Order channel manager - handles order result messages from async tasks
use crate::ui::orders::{strip_new_order_messages_and_clamp_selected, OrderSuccess};
use crate::ui::{
    AppState, InvoiceInputState, MessageNotification, OperationResult, UiMode, UserMode,
};
use mostro_core::prelude::Action;
use uuid::Uuid;

fn remove_closed_trade_from_messages_tab(app: &mut AppState, order_id: Uuid) {
    match app.messages.lock() {
        Ok(mut messages) => {
            messages.retain(|m| m.order_id != Some(order_id));
            strip_new_order_messages_and_clamp_selected(
                &mut messages,
                &mut app.selected_message_idx,
            );
        }
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
        }
    }
    match app.active_order_trade_indices.lock() {
        Ok(mut indices) => {
            indices.remove(&order_id);
        }
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
            ));
        }
    }
}

fn remove_many_orders_from_messages_tab(app: &mut AppState, order_ids: &[Uuid]) {
    let id_set: std::collections::HashSet<Uuid> = order_ids.iter().copied().collect();
    match app.messages.lock() {
        Ok(mut messages) => {
            messages.retain(|m| m.order_id.map(|id| !id_set.contains(&id)).unwrap_or(true));
            strip_new_order_messages_and_clamp_selected(
                &mut messages,
                &mut app.selected_message_idx,
            );
        }
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
        }
    }
    match app.active_order_trade_indices.lock() {
        Ok(mut indices) => {
            for order_id in order_ids {
                indices.remove(order_id);
            }
        }
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
            ));
        }
    }
}

/// Handle order result from the order result channel
pub fn handle_operation_result(mut result: OperationResult, app: &mut AppState) {
    if let OperationResult::TradeClosed { order_id, message } = result {
        remove_closed_trade_from_messages_tab(app, order_id);
        result = OperationResult::Info(message);
    }
    if let OperationResult::OrderHistoryDeleted {
        deleted_order_ids,
        message,
    } = result
    {
        remove_many_orders_from_messages_tab(app, &deleted_order_ids);
        result = OperationResult::Info(message);
    }

    // Handle PaymentRequestRequired - show invoice popup for buy orders
    if let OperationResult::PaymentRequestRequired {
        order,
        invoice,
        sat_amount,
        trade_index,
    } = &result
    {
        // Track trade_index
        if let Some(order_id) = order.id {
            match app.active_order_trade_indices.lock() {
                Ok(mut indices) => {
                    indices.insert(order_id, *trade_index);
                }
                Err(e) => {
                    crate::util::request_fatal_restart(format!(
                        "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
                    ));
                    app.fatal_exit_on_close = true;
                    app.mode = UiMode::OperationResult(OperationResult::Error(
                        "Internal error. Please restart Mostrix.".to_string(),
                    ));
                    return;
                }
            }
            log::info!(
                "Tracking order {} with trade_index {}",
                order_id,
                trade_index
            );
        }

        // Create MessageNotification to show PayInvoice popup
        let notification = MessageNotification {
            order_id: order.id,
            message_preview: "Payment Request".to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            action: Action::PayInvoice,
            sat_amount: *sat_amount,
            invoice: Some(invoice.clone()),
        };

        // Create invoice state (not focused since this is display-only)
        let invoice_state = InvoiceInputState {
            invoice_input: String::new(),
            focused: false,
            just_pasted: false,
            copied_to_clipboard: false,
            scroll_y: 0,
        };
        // Reuse pay invoice popup for buy orders when taking an order
        app.mode = UiMode::NewMessageNotification(notification, Action::PayInvoice, invoice_state);
        return;
    }

    // Track trade_index for taken orders
    if let OperationResult::Success(OrderSuccess {
        order_id,
        trade_index,
        ..
    }) = &result
    {
        if let (Some(order_id), Some(trade_index)) = (order_id, trade_index) {
            match app.active_order_trade_indices.lock() {
                Ok(mut indices) => {
                    indices.insert(*order_id, *trade_index);
                }
                Err(e) => {
                    crate::util::request_fatal_restart(format!(
                        "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
                    ));
                    app.fatal_exit_on_close = true;
                    app.mode = UiMode::OperationResult(OperationResult::Error(
                        "Internal error. Please restart Mostrix.".to_string(),
                    ));
                    return;
                }
            }
            log::info!(
                "Tracking order {} with trade_index {}",
                order_id,
                trade_index
            );
        }
    }

    // Handle observer chat results directly (don't show popup)
    match result {
        OperationResult::ObserverChatLoaded(messages) => {
            app.observer_loading = false;
            app.observer_error = None;
            app.observer_messages = messages;
            return;
        }
        OperationResult::ObserverChatError(msg) => {
            app.observer_loading = false;
            app.observer_error = Some(msg.clone());
            app.mode = UiMode::OperationResult(OperationResult::Error(msg));
            return;
        }
        _ => {}
    }

    // Set appropriate result mode based on current state
    match &app.mode {
        UiMode::UserMode(UserMode::WaitingTakeOrder(_)) => {
            app.mode = UiMode::OperationResult(result);
        }
        UiMode::UserMode(UserMode::WaitingAddInvoice) => {
            app.mode = UiMode::OperationResult(result);
        }
        UiMode::NewMessageNotification(_, action, _) => {
            // Do not replace AddInvoice/PayInvoice popups: the take-order task can finish after
            // the DM listener already showed the invoice UI — overwriting would drop the popup.
            if matches!(action, Action::AddInvoice | Action::PayInvoice) {
                app.pending_post_take_operation_result = Some(result);
            } else {
                app.mode = UiMode::OperationResult(result);
            }
        }
        _ => {
            app.mode = UiMode::OperationResult(result);
        }
    }
}
