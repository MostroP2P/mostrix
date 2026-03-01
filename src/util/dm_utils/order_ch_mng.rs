// Order channel manager - handles order result messages from async tasks
use crate::ui::orders::OrderSuccess;
use crate::ui::{
    AppState, InvoiceInputState, MessageNotification, OperationResult, UiMode, UserMode,
};
use mostro_core::prelude::Action;

/// Handle order result from the order result channel
pub fn handle_order_result(result: OperationResult, app: &mut AppState) {
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
            let mut indices = app.active_order_trade_indices.lock().unwrap();
            indices.insert(order_id, *trade_index);
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
            let mut indices = app.active_order_trade_indices.lock().unwrap();
            indices.insert(*order_id, *trade_index);
            log::info!(
                "Tracking order {} with trade_index {}",
                order_id,
                trade_index
            );
        }
    }

    // Set appropriate result mode based on current state
    match app.mode {
        UiMode::UserMode(UserMode::WaitingTakeOrder(_)) => {
            app.mode = UiMode::OperationResult(result);
        }
        UiMode::UserMode(UserMode::WaitingAddInvoice) => {
            app.mode = UiMode::OperationResult(result);
        }
        UiMode::NewMessageNotification(_, _, _) => {
            // If we have a notification, replace it with the result
            app.mode = UiMode::OperationResult(result);
        }
        _ => {
            app.mode = UiMode::OperationResult(result);
        }
    }
}
