// Order channel manager - handles order result messages from async tasks
use crate::ui::helpers::build_active_order_chat_list;
use crate::ui::orders::{
    strip_new_order_messages_and_clamp_selected, try_placeholder_order_message_from_success,
    BuyerInvoicePreference, OrderSuccess,
};
use crate::ui::{
    AppState, InvoiceInputState, InvoiceNotificationActionSelection, MessageNotification,
    OperationResult, UiMode, UserMode,
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
    app.order_chat_static.remove(&order_id);
    app.buyer_invoice_preference.remove(&order_id);
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
            let n = build_active_order_chat_list(&messages, &app.my_trades_maker_book).len();
            if n == 0 {
                app.selected_order_chat_idx = 0;
            } else if app.selected_order_chat_idx >= n {
                app.selected_order_chat_idx = n - 1;
            }
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
    for order_id in order_ids {
        app.buyer_invoice_preference.remove(order_id);
        let key = order_id.to_string();
        app.order_chats.remove(&key);
        app.order_chat_last_seen.remove(&key);
        app.order_chat_static.remove(order_id);
        if let Ok(mut dropped) = app.dropped_user_history_order_ids.lock() {
            dropped.insert(*order_id);
        }
    }
}

/// If `Success` arrived before any DM row exists for this trade, append one placeholder so
/// **Orders In Progress** (`build_active_order_chat_list`) has a sidebar row without running
/// `sync_user_order_history_messages_from_db` (which would clobber real actions).
fn maybe_insert_my_trade_placeholder_message(app: &mut AppState, os: &OrderSuccess) {
    let Some(order_id) = os.order_id else {
        return;
    };
    if os.static_header.is_none() {
        return;
    }
    let Some(placeholder) = try_placeholder_order_message_from_success(os) else {
        return;
    };
    match app.messages.lock() {
        Ok(mut messages) => {
            if messages.iter().any(|m| m.order_id == Some(order_id)) {
                return;
            }
            messages.push(placeholder);
            messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        }
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
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
    if let OperationResult::InvoiceSubmitted {
        message,
        remember_buyer_saved_ln_address_for_order,
    } = result
    {
        if let Some(order_id) = remember_buyer_saved_ln_address_for_order {
            app.buyer_invoice_preference
                .insert(order_id, BuyerInvoicePreference::UseSavedLnAddress);
        }
        result = OperationResult::Info(message);
    }
    if let OperationResult::OrderChatAttachmentSent {
        order_id,
        chat_message,
        info_message,
    } = result
    {
        app.pending_order_attachment_sends.remove(&order_id);
        if app.sending_attachment_order_id.as_deref() == Some(order_id.as_str()) {
            app.sending_attachment_order_id = None;
        }
        crate::ui::helpers::save_order_chat_message(&order_id, &chat_message);
        app.order_chats
            .entry(order_id.clone())
            .or_default()
            .push(chat_message);
        result = OperationResult::Info(info_message);
    }
    if matches!(&result, OperationResult::Error(_)) {
        app.sending_attachment_order_id = None;
    }
    if let OperationResult::OrderChatAttachmentSendFailed { prepared, error } = result {
        let order_id = prepared.order_id.clone();
        let url = prepared.blossom_url.clone();
        let filename = prepared.filename.clone();
        app.pending_order_attachment_sends
            .insert(order_id.clone(), prepared);
        if app.sending_attachment_order_id.as_deref() == Some(order_id.as_str()) {
            app.sending_attachment_order_id = None;
        }
        result = OperationResult::Error(format!(
            "Uploaded {filename} to Blossom ({url}) but chat send failed: {error}. \
             Press Ctrl+Shift+O to retry send without re-uploading."
        ));
    }

    match &result {
        OperationResult::Success(os) => {
            if let Some(h) = &os.static_header {
                app.order_chat_static.insert(h.order_id, h.clone());
            }
            maybe_insert_my_trade_placeholder_message(app, os);
        }
        OperationResult::PaymentRequestRequired { static_header, .. } => {
            app.order_chat_static
                .insert(static_header.order_id, static_header.clone());
        }
        _ => {}
    }

    if let OperationResult::OpenInvoicePopup {
        notification,
        order_message,
    } = &result
    {
        crate::util::dm_utils::notifications_ch_mng::apply_open_invoice_popup_from_execute(
            app,
            notification.clone(),
            order_message,
        );
        return;
    }

    // Handle PaymentRequestRequired - show invoice popup for buy orders
    if let OperationResult::PaymentRequestRequired {
        order,
        invoice,
        sat_amount,
        trade_index,
        static_header: _,
        action,
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
                    app.mode = UiMode::operation_result(OperationResult::Error(
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

        // Create MessageNotification to show the invoice popup. `action` distinguishes
        // the trade hold invoice (`PayInvoice`) from the anti-abuse bond
        // (`PayBondInvoice`), so each opens its own popup variant.
        let preview = match action {
            Action::PayBondInvoice => "Bond Invoice".to_string(),
            _ => "Payment Request".to_string(),
        };
        let notification = MessageNotification {
            order_id: order.id,
            message_preview: preview,
            timestamp: chrono::Utc::now().timestamp(),
            action: action.clone(),
            sat_amount: *sat_amount,
            invoice: Some(invoice.clone()),
            body: None,
        };

        let invoice_state = InvoiceInputState {
            invoice_input: String::new(),
            focused: false,
            just_pasted: false,
            copied_to_clipboard: false,
            scroll_y: 0,
            action_selection: InvoiceNotificationActionSelection::Primary,
        };
        app.mode = UiMode::NewMessageNotification(notification, action.clone(), invoice_state);
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
                    app.mode = UiMode::operation_result(OperationResult::Error(
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
            app.mode = UiMode::operation_result(OperationResult::Error(msg));
            return;
        }
        _ => {}
    }

    // Set appropriate result mode based on current state
    match &app.mode {
        UiMode::UserMode(UserMode::WaitingTakeOrder(_)) => {
            app.mode = UiMode::operation_result(result);
        }
        UiMode::UserMode(UserMode::WaitingAddInvoice) => {
            app.mode = UiMode::operation_result(result);
        }
        UiMode::NewMessageNotification(_, action, _) => {
            // Do not replace AddInvoice/PayInvoice/PayBondInvoice popups: the take-order task
            // can finish after the DM listener already showed the invoice UI — overwriting
            // would drop the popup.
            if matches!(
                action,
                Action::AddInvoice
                    | Action::AddBondInvoice
                    | Action::PayInvoice
                    | Action::PayBondInvoice
            ) {
                app.pending_post_take_operation_result = Some(result);
            } else {
                app.mode = UiMode::operation_result(result);
            }
        }
        UiMode::ConfirmSavedLnAddressForInvoice(..) => {
            app.pending_post_take_operation_result = Some(result);
        }
        _ => {
            app.mode = UiMode::operation_result(result);
        }
    }
}
