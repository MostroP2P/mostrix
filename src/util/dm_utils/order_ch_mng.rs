// Order channel manager - handles order result messages from async tasks
use crate::ui::helpers::build_active_order_chat_list;
use crate::ui::orders::{
    strip_new_order_messages_and_clamp_selected, try_placeholder_order_message_from_success,
    BuyerInvoicePreference, OrderSuccess,
};
use crate::ui::{
    AppState, ChatParty, InvoiceInputState, InvoiceNotificationActionSelection,
    MessageNotification, OperationResult, UiMode, UserMode,
};
use crate::util::chat_listener::untrack_dispute_chat_parties;
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

fn remove_admin_dispute_from_app_state(app: &mut AppState, dispute_id: &str) {
    untrack_dispute_chat_parties(dispute_id);
    app.admin_disputes_in_progress
        .retain(|d| d.dispute_id != dispute_id);
    app.admin_dispute_chats.remove(dispute_id);
    app.admin_chat_last_seen
        .remove(&(dispute_id.to_string(), ChatParty::Buyer));
    app.admin_chat_last_seen
        .remove(&(dispute_id.to_string(), ChatParty::Seller));
    if app.selected_dispute_id.as_deref() == Some(dispute_id) {
        app.selected_dispute_id = None;
        app.admin_chat_input.clear();
        app.admin_chat_selected_message_idx = None;
        app.admin_chat_scroll_tracker = None;
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
            messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
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
    if let OperationResult::AdminDisputeDeleted {
        dispute_id,
        message,
    } = result
    {
        remove_admin_dispute_from_app_state(app, &dispute_id);
        result = OperationResult::Info(message);
    }
    if let OperationResult::SessionRestored { message } = result {
        result = OperationResult::Info(message);
    }
    if let OperationResult::OrdersRefreshed { message } = result {
        result = OperationResult::Info(message);
    }
    if let OperationResult::InvoiceSubmitted {
        message,
        order_id,
        remember_buyer_saved_ln_address_for_order,
    } = result
    {
        if remember_buyer_saved_ln_address_for_order {
            app.buyer_invoice_preference
                .insert(order_id, BuyerInvoicePreference::UseSavedLnAddress);
        }
        app.orders_needing_replacement_invoice.remove(&order_id);
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
    if let OperationResult::OrderChatAttachmentError { order_id, error } = result {
        if app.sending_attachment_order_id.as_deref() == Some(&order_id) {
            app.sending_attachment_order_id = None;
        }
        result = OperationResult::Error(error);
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
        // New-order bond invoice path succeeds here and returns before the
        // WaitingForMostro match below — clear the draft now so it cannot restore
        // a duplicate form after the maker pays the bond.
        if matches!(app.mode, UiMode::UserMode(UserMode::WaitingForMostro(_))) {
            app.order_form_draft = None;
        }

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
            maker_bond_publish: order.status == Some(mostro_core::order::Status::WaitingMakerBond),
            solver_pubkey: None,
            dispute_id: None,
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
        OperationResult::ObserverChatLoaded {
            generation,
            messages,
        } => {
            if generation != app.observer_fetch_generation {
                return;
            }
            app.observer_loading = false;
            app.observer_error = None;
            app.observer_messages = messages;
            return;
        }
        OperationResult::ObserverChatError {
            generation,
            message: msg,
        } => {
            if generation != app.observer_fetch_generation {
                return;
            }
            app.observer_loading = false;
            app.observer_error = Some(msg.clone());
            app.mode = UiMode::operation_result(OperationResult::Error(msg));
            return;
        }
        _ => {}
    }

    // Set appropriate result mode based on current state
    match &app.mode {
        UiMode::UserMode(UserMode::WaitingForMostro(form)) => {
            match &result {
                // Order published — discard the draft.
                // (PaymentRequestRequired is handled earlier and returns before this match.)
                OperationResult::Success(_) => {
                    app.order_form_draft = None;
                }
                // Submit failed — keep the form so the user can resume editing.
                OperationResult::Error(_) => {
                    app.order_form_draft = Some(form.clone());
                }
                _ => {}
            }
            app.mode = UiMode::operation_result(result);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::orders::{
        order_message_to_notification, OrderChatStaticHeader, OrderMessage, TakeOrderState,
    };
    use crate::ui::{FormState, UserRole};
    use mostro_core::prelude::{Message, Payload, SmallOrder, Status};
    use nostr_sdk::prelude::Keys;

    #[test]
    fn failed_new_order_keeps_form_draft() {
        let mut app = AppState::new(UserRole::User);
        let mut form = FormState::new_default_form();
        form.fiat_amount = "42".to_string();
        form.payment_method = "Bizum".to_string();
        app.mode = UiMode::UserMode(UserMode::WaitingForMostro(form.clone()));
        app.order_form_draft = Some(form.clone());

        handle_operation_result(OperationResult::Error("relay timeout".into()), &mut app);

        let draft = app.order_form_draft.expect("draft should survive failure");
        assert_eq!(draft.fiat_amount, "42");
        assert_eq!(draft.payment_method, "Bizum");
        assert!(matches!(app.mode, UiMode::OperationResult(_)));
    }

    #[test]
    fn successful_new_order_clears_form_draft() {
        let mut app = AppState::new(UserRole::User);
        let form = FormState::new_default_form();
        app.mode = UiMode::UserMode(UserMode::WaitingForMostro(form.clone()));
        app.order_form_draft = Some(form);

        handle_operation_result(OperationResult::Success(OrderSuccess::default()), &mut app);

        assert!(app.order_form_draft.is_none());
        assert!(matches!(app.mode, UiMode::OperationResult(_)));
    }

    #[test]
    fn enter_while_waiting_then_success_clears_draft() {
        // Mirrors handle_enter_key: mem::replace then restore waiting mode (no-op Enter).
        // Success must still hit the WaitingForMostro cleanup and drop the draft.
        let mut app = AppState::new(UserRole::User);
        let form = FormState::new_default_form();
        app.mode = UiMode::UserMode(UserMode::WaitingForMostro(form.clone()));
        app.order_form_draft = Some(form);

        let default_mode = UiMode::UserMode(UserMode::Normal);
        let current_mode = std::mem::replace(&mut app.mode, default_mode);
        match current_mode {
            mode @ (UiMode::UserMode(UserMode::WaitingForMostro(_))
            | UiMode::UserMode(UserMode::WaitingTakeOrder(_))
            | UiMode::UserMode(UserMode::WaitingAddInvoice)) => {
                app.mode = mode;
            }
            other => panic!("expected waiting mode, got {other:?}"),
        }
        assert!(matches!(
            app.mode,
            UiMode::UserMode(UserMode::WaitingForMostro(_))
        ));

        handle_operation_result(OperationResult::Success(OrderSuccess::default()), &mut app);
        assert!(
            app.order_form_draft.is_none(),
            "draft must clear when Enter kept WaitingForMostro until Success"
        );
    }

    #[test]
    fn success_after_leaving_waiting_mode_leaves_draft() {
        // Documents the Enter bug: if waiting mode was dropped to Normal, Success
        // bypasses draft cleanup and the submitted form remains reusable.
        let mut app = AppState::new(UserRole::User);
        let form = FormState::new_default_form();
        app.order_form_draft = Some(form);
        app.mode = UiMode::UserMode(UserMode::Normal);

        handle_operation_result(OperationResult::Success(OrderSuccess::default()), &mut app);
        assert!(app.order_form_draft.is_some());
    }

    #[test]
    fn payment_request_from_waiting_new_order_clears_form_draft() {
        // PaymentRequestRequired returns before the WaitingForMostro match; draft
        // must still be cleared so a successful bond-invoice create cannot restore.
        let mut app = AppState::new(UserRole::User);
        let form = FormState::new_default_form();
        app.mode = UiMode::UserMode(UserMode::WaitingForMostro(form.clone()));
        app.order_form_draft = Some(form);

        let order_id = uuid::Uuid::new_v4();
        handle_operation_result(
            OperationResult::PaymentRequestRequired {
                order: SmallOrder {
                    id: Some(order_id),
                    status: Some(Status::WaitingMakerBond),
                    ..Default::default()
                },
                invoice: "lnbc1test".to_string(),
                sat_amount: Some(1000),
                trade_index: 1,
                static_header: OrderChatStaticHeader {
                    order_id,
                    kind: None,
                    created_at: None,
                    trade_index: 1,
                    initiator_trade_pubkey: "pk".to_string(),
                    is_mine: true,
                    solver_pubkey: None,
                    dispute_id: None,
                },
                action: Action::PayBondInvoice,
            },
            &mut app,
        );

        assert!(app.order_form_draft.is_none());
        assert!(matches!(app.mode, UiMode::NewMessageNotification(_, _, _)));
    }

    #[test]
    fn take_add_invoice_from_waiting_opens_invoice_popup_not_created_success() {
        let mut app = AppState::new(UserRole::User);
        let order_id = uuid::Uuid::new_v4();
        app.mode = UiMode::UserMode(UserMode::WaitingTakeOrder(TakeOrderState {
            order: SmallOrder {
                id: Some(order_id),
                kind: Some(mostro_core::order::Kind::Sell),
                ..Default::default()
            },
            amount_input: "100".to_string(),
            is_range_order: true,
            validation_error: None,
            selected_button: true,
        }));

        let sender = Keys::generate().public_key();
        let message = Message::new_order(
            Some(order_id),
            Some(1),
            Some(2),
            Action::AddInvoice,
            Some(Payload::Order(SmallOrder {
                id: Some(order_id),
                kind: Some(mostro_core::order::Kind::Sell),
                status: Some(Status::WaitingBuyerInvoice),
                ..Default::default()
            })),
        );
        let order_message = OrderMessage {
            message,
            timestamp: 1,
            sender,
            order_id: Some(order_id),
            trade_index: 2,
            sat_amount: Some(21_000),
            buyer_invoice: None,
            order_kind: Some(mostro_core::order::Kind::Sell),
            is_mine: Some(false),
            order_status: Some(Status::WaitingBuyerInvoice),
            order_snapshot: None,
            read: true,
            auto_popup_shown: true,
        };
        let notification = order_message_to_notification(&order_message);

        handle_operation_result(
            OperationResult::OpenInvoicePopup {
                notification,
                order_message: Box::new(order_message),
            },
            &mut app,
        );

        match &app.mode {
            UiMode::NewMessageNotification(n, action, _) => {
                assert_eq!(*action, Action::AddInvoice);
                assert_eq!(n.order_id, Some(order_id));
            }
            other => panic!("expected AddInvoice popup, got {other:?}"),
        }
        assert!(
            !matches!(app.mode, UiMode::OperationResult(_)),
            "take AddInvoice must not show the create-order success overlay"
        );
    }

    #[test]
    fn stale_observer_chat_loaded_does_not_replace_newer_or_cleared_state() {
        use crate::ui::chat::{ChatSender, DisputeChatMessage};

        let dummy = |content: &str| DisputeChatMessage {
            sender: ChatSender::Buyer,
            content: content.to_string(),
            timestamp: 1,
            target_party: None,
            attachment: None,
        };

        let mut app = AppState::new(UserRole::Admin);
        let gen_a = app.begin_observer_fetch();
        app.clear_observer_secrets();
        handle_operation_result(
            OperationResult::ObserverChatLoaded {
                generation: gen_a,
                messages: vec![dummy("from-a")],
            },
            &mut app,
        );
        assert!(
            app.observer_messages.is_empty(),
            "cleared Observer must ignore the late fetch for the previous K_conv"
        );

        let gen_b = app.begin_observer_fetch();
        handle_operation_result(
            OperationResult::ObserverChatLoaded {
                generation: gen_a,
                messages: vec![dummy("from-a")],
            },
            &mut app,
        );
        assert!(
            app.observer_messages.is_empty(),
            "in-flight B must not be overwritten by late A"
        );
        assert!(app.observer_loading);

        handle_operation_result(
            OperationResult::ObserverChatLoaded {
                generation: gen_b,
                messages: vec![dummy("from-b")],
            },
            &mut app,
        );
        assert_eq!(app.observer_messages.len(), 1);
        assert_eq!(app.observer_messages[0].content, "from-b");
        assert!(!app.observer_loading);
    }

    #[test]
    fn stale_observer_chat_error_does_not_raise_popup() {
        let mut app = AppState::new(UserRole::Admin);
        let gen_a = app.begin_observer_fetch();
        app.clear_observer_secrets();
        handle_operation_result(
            OperationResult::ObserverChatError {
                generation: gen_a,
                message: "relay timeout".into(),
            },
            &mut app,
        );
        assert!(app.observer_error.is_none());
        assert!(!matches!(app.mode, UiMode::OperationResult(_)));
    }
}
