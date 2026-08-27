// Notifications channel manager - handles message notifications from async tasks
use crate::settings::load_settings_from_disk;
use crate::ui::orders::BuyerInvoicePreference;
use crate::ui::orders::{
    invoice_popup_allowed_for_order_status, local_user_must_act_on_invoice_popup,
    order_message_to_notification, order_message_to_waiting_notification, OrderMessage,
};
use crate::ui::{
    AppState, InvoiceInputState, InvoiceNotificationActionSelection, MessageNotification, UiMode,
};
use mostro_core::prelude::Action;
use std::collections::HashMap;
use uuid::Uuid;

/// Check if the popup should be shown for a given notification
/// The message is guaranteed to exist in the vector because listen_for_order_messages
/// adds it before sending the notification.
fn check_if_popup_should_be_shown(notification: &MessageNotification, app: &AppState) -> bool {
    if let Some(order_id) = notification.order_id {
        if let Some(floor_ts) = app.startup_popup_floor_ts.get(&order_id) {
            if notification.timestamp <= *floor_ts {
                log::debug!(
                    "[popup] suppressed historical {:?} popup for order_id={} (notification_ts={} <= startup_floor_ts={})",
                    notification.action,
                    order_id,
                    notification.timestamp,
                    floor_ts
                );
                return false;
            }
        }
    }

    // Acquire lock on the messages vector
    let mut messages = match app.messages.lock() {
        Ok(g) => g,
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
            return false;
        }
    };
    // Check if the notification has an order_id
    if let Some(order_id) = notification.order_id {
        // Find the corresponding OrderMessage - it's guaranteed to exist because
        // listen_for_order_messages adds the message before sending the notification
        let order_msg = messages
            .iter_mut()
            .find(|m| m.order_id == Some(order_id))
            .expect("Message should exist in vector when notification is received");

        if !invoice_popup_allowed_for_order_status(&notification.action, order_msg.order_status) {
            log::debug!(
                "[popup] suppressed invoice modal for {:?} (order_status={:?})",
                notification.action,
                order_msg.order_status
            );
            return false;
        }

        // Role-aware gate: counterparty waiting on hold/bond/invoice should not get an input popup.
        // Relies on `order_msg.is_mine` hydrated in the trade-DM path (post-upsert DB read).
        if !local_user_must_act_on_invoice_popup(order_msg, &notification.action) {
            log::debug!(
                "[popup] suppressed invoice modal for {:?}: local user is not the acting party",
                notification.action,
            );
            return false;
        }

        if order_msg.auto_popup_shown {
            return false;
        } else {
            order_msg.auto_popup_shown = true;
            return true;
        }
    }
    // No order_id associated, show popup
    true
}

/// Whether the informational `payment-failed` popup should auto-open.
///
/// Mirrors the startup-floor and once-per-message dedup of
/// [`check_if_popup_should_be_shown`], but skips the invoice-input gating
/// ([`invoice_popup_allowed_for_order_status`] returns `false` for
/// `PaymentFailed`). Instead it gates to the buyer: Mostro only sends
/// `payment-failed` to the buyer, and the buyer is exactly the party who acts on
/// `AddInvoice`, so we reuse [`local_user_must_act_on_invoice_popup`] with
/// `Action::AddInvoice` as the buyer test.
fn check_if_payment_failed_popup_should_be_shown(
    notification: &MessageNotification,
    app: &AppState,
) -> bool {
    if let Some(order_id) = notification.order_id {
        if let Some(floor_ts) = app.startup_popup_floor_ts.get(&order_id) {
            if notification.timestamp <= *floor_ts {
                log::debug!(
                    "[popup] suppressed historical payment-failed popup for order_id={} (notification_ts={} <= startup_floor_ts={})",
                    order_id,
                    notification.timestamp,
                    floor_ts
                );
                return false;
            }
        }
    }

    let Some(order_id) = notification.order_id else {
        return true;
    };

    let mut messages = match app.messages.lock() {
        Ok(g) => g,
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
            return false;
        }
    };
    let Some(order_msg) = messages.iter_mut().find(|m| m.order_id == Some(order_id)) else {
        return false;
    };

    if !local_user_must_act_on_invoice_popup(order_msg, &Action::AddInvoice) {
        log::debug!(
            "[popup] suppressed payment-failed popup for order_id={order_id}: local user is not the buyer",
        );
        return false;
    }

    if order_msg.auto_popup_shown {
        false
    } else {
        // Same-action dedup only: a later `AddInvoice` carries a different action, so it
        // will not inherit this flag (see `prior_auto_popup_shown` in the trade-DM path)
        // and the buyer still gets the invoice popup once all retries fail.
        order_msg.auto_popup_shown = true;
        true
    }
}

fn invoice_state_for_add_invoice(invoice_input: String, focused: bool) -> InvoiceInputState {
    InvoiceInputState {
        invoice_input,
        focused,
        just_pasted: false,
        copied_to_clipboard: false,
        scroll_y: 0,
        action_selection: InvoiceNotificationActionSelection::Primary,
    }
}

/// Opens AddInvoice UI: optional confirmation when settings contain a buyer Lightning address.
pub fn present_add_invoice_popup(
    buyer_invoice_preference: &mut HashMap<Uuid, BuyerInvoicePreference>,
    notification: MessageNotification,
) -> UiMode {
    let trimmed_ln = load_settings_from_disk()
        .ok()
        .map(|s| s.ln_address.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(addr) = trimmed_ln {
        if let Some(oid) = notification.order_id {
            match buyer_invoice_preference.get(&oid).copied() {
                Some(BuyerInvoicePreference::ManualInvoice) => {
                    return UiMode::NewMessageNotification(
                        notification,
                        Action::AddInvoice,
                        invoice_state_for_add_invoice(String::new(), true),
                    );
                }
                Some(BuyerInvoicePreference::UseSavedLnAddress) => {
                    return UiMode::NewMessageNotification(
                        notification,
                        Action::AddInvoice,
                        invoice_state_for_add_invoice(addr, true),
                    );
                }
                None => {
                    return UiMode::ConfirmSavedLnAddressForInvoice(notification, true);
                }
            }
        }
        return UiMode::ConfirmSavedLnAddressForInvoice(notification, true);
    }

    UiMode::NewMessageNotification(
        notification,
        Action::AddInvoice,
        invoice_state_for_add_invoice(String::new(), true),
    )
}

/// Apply Yes/No on the saved-Lightning-address confirmation before AddInvoice.
pub fn apply_saved_ln_address_invoice_choice(
    app: &mut AppState,
    notification: MessageNotification,
    use_saved: bool,
) {
    let action = Action::AddInvoice;
    if use_saved {
        let addr = load_settings_from_disk()
            .ok()
            .map(|s| s.ln_address.trim().to_string())
            .filter(|s| !s.is_empty());
        if let (Some(oid), Some(a)) = (notification.order_id, addr.as_ref()) {
            if !a.is_empty() {
                app.buyer_invoice_preference
                    .insert(oid, BuyerInvoicePreference::UseSavedLnAddress);
            }
        }
        let invoice_input = addr.unwrap_or_default();
        app.mode = UiMode::NewMessageNotification(
            notification,
            action,
            invoice_state_for_add_invoice(invoice_input, true),
        );
    } else if let Some(oid) = notification.order_id {
        app.buyer_invoice_preference
            .insert(oid, BuyerInvoicePreference::ManualInvoice);
        app.mode = UiMode::NewMessageNotification(
            notification,
            action,
            invoice_state_for_add_invoice(String::new(), true),
        );
    } else {
        app.mode = UiMode::NewMessageNotification(
            notification,
            action,
            invoice_state_for_add_invoice(String::new(), true),
        );
    }
}

fn invoice_popup_mode(
    buyer_invoice_preference: &mut HashMap<Uuid, BuyerInvoicePreference>,
    notification: MessageNotification,
) -> UiMode {
    match notification.action {
        Action::AddInvoice => present_add_invoice_popup(buyer_invoice_preference, notification),
        Action::AddBondInvoice => {
            let invoice_state = invoice_state_for_add_invoice(String::new(), true);
            UiMode::NewMessageNotification(notification, Action::AddBondInvoice, invoice_state)
        }
        Action::PayInvoice | Action::PayBondInvoice => {
            let action = notification.action.clone();
            let invoice_state = InvoiceInputState {
                invoice_input: String::new(),
                focused: false,
                just_pasted: false,
                copied_to_clipboard: false,
                scroll_y: 0,
                action_selection: InvoiceNotificationActionSelection::Primary,
            };
            UiMode::NewMessageNotification(notification, action, invoice_state)
        }
        Action::WaitingBuyerInvoice | Action::WaitingSellerToPay => {
            let action = notification.action.clone();
            let invoice_state = invoice_state_for_add_invoice(String::new(), false);
            UiMode::NewMessageNotification(notification, action, invoice_state)
        }
        _ => unreachable!(
            "apply_open_invoice_popup_from_execute only passes invoice/waiting actions"
        ),
    }
}

/// Remember the execute-path row so a later listener copy of the same DM does not
/// auto-popup again (`auto_popup_shown`), and My Trades has a sidebar entry.
fn remember_open_invoice_order_message(app: &mut AppState, order_message: &OrderMessage) {
    let Some(order_id) = order_message.order_id else {
        return;
    };
    let mut messages = match app.messages.lock() {
        Ok(g) => g,
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
            return;
        }
    };
    if let Some(existing) = messages.iter_mut().find(|m| m.order_id == Some(order_id)) {
        existing.auto_popup_shown = true;
        return;
    }
    let mut stored = order_message.clone();
    stored.auto_popup_shown = true;
    messages.push(stored);
    messages.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
}

/// Apply invoice or waiting-phase popup after a synchronous protocol reply (bond payout, etc.).
pub fn apply_open_invoice_popup_from_execute(
    app: &mut AppState,
    notification: MessageNotification,
    order_message: &OrderMessage,
) {
    remember_open_invoice_order_message(app, order_message);
    let gate_action = match notification.action {
        Action::WaitingBuyerInvoice | Action::WaitingSellerToPay => Action::AddInvoice,
        ref other => other.clone(),
    };
    if !invoice_popup_allowed_for_order_status(&gate_action, order_message.order_status) {
        return;
    }
    if local_user_must_act_on_invoice_popup(order_message, &gate_action) {
        let mut actionable = notification;
        if matches!(
            actionable.action,
            Action::WaitingBuyerInvoice | Action::WaitingSellerToPay
        ) {
            actionable = order_message_to_notification(order_message);
            actionable.action = Action::AddInvoice;
        }
        app.mode = invoice_popup_mode(&mut app.buyer_invoice_preference, actionable);
        return;
    }
    let waiting = order_message_to_waiting_notification(order_message);
    let action = waiting.action.clone();
    app.mode = UiMode::NewMessageNotification(
        waiting,
        action,
        invoice_state_for_add_invoice(String::new(), false),
    );
}

/// Handle message notification from the notification channel
pub fn handle_message_notification(notification: MessageNotification, app: &mut AppState) {
    if let Some(order_id) = notification.order_id {
        if let Some(header) = app.order_chat_static.get_mut(&order_id) {
            if notification.solver_pubkey.is_some() {
                header.solver_pubkey.clone_from(&notification.solver_pubkey);
            }
            if notification.dispute_id.is_some() {
                header.dispute_id.clone_from(&notification.dispute_id);
            }
        }
    }

    // Only show popup automatically for PayInvoice / PayBondInvoice / AddInvoice,
    // and only if we haven't already shown it for this message.
    match notification.action {
        Action::PayInvoice
        | Action::PayBondInvoice
        | Action::AddInvoice
        | Action::AddBondInvoice => {
            // Remember post-retry replacement-invoice asks so Enter can reopen the popup
            // after Esc (Mostro does not resend `add-invoice`).
            if matches!(notification.action, Action::AddInvoice) && notification.body.is_some() {
                if let Some(order_id) = notification.order_id {
                    app.orders_needing_replacement_invoice.insert(order_id);
                }
            }

            let should_show_popup = check_if_popup_should_be_shown(&notification, app);
            if !should_show_popup {
                return;
            }

            if matches!(notification.action, Action::AddInvoice) {
                app.mode =
                    present_add_invoice_popup(&mut app.buyer_invoice_preference, notification);
            } else if matches!(notification.action, Action::AddBondInvoice) {
                let invoice_state = invoice_state_for_add_invoice(String::new(), true);
                app.mode = UiMode::NewMessageNotification(
                    notification,
                    Action::AddBondInvoice,
                    invoice_state,
                );
            } else {
                // PayInvoice (trade hold) or PayBondInvoice (anti-abuse bond): both use the
                // same display-only InvoiceInputState. The popup variant is selected by the
                // action stored on the notification.
                let invoice_state = InvoiceInputState {
                    invoice_input: String::new(),
                    focused: false,
                    just_pasted: false,
                    copied_to_clipboard: false,
                    scroll_y: 0,
                    action_selection: InvoiceNotificationActionSelection::Primary,
                };
                let action = notification.action.clone();
                app.mode = UiMode::NewMessageNotification(notification, action, invoice_state);
            }
        }
        // Informational only: `payment-failed` does not change order status and needs no
        // input. Show a dismissible popup explaining the automatic retries to the buyer.
        Action::PaymentFailed => {
            if !check_if_payment_failed_popup_should_be_shown(&notification, app) {
                return;
            }
            app.mode = UiMode::NewMessageNotification(
                notification,
                Action::PaymentFailed,
                invoice_state_for_add_invoice(String::new(), false),
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::handle_message_notification;
    use crate::ui::orders::OrderMessage;
    use crate::ui::{AppState, MessageNotification, OrderChatStaticHeader, UiMode, UserRole};
    use mostro_core::prelude::{Action, Kind, Message, Payload, PaymentFailedInfo, Status};
    use nostr_sdk::prelude::Keys;
    use uuid::Uuid;

    fn notification(
        order_id: Uuid,
        action: Action,
        solver_pubkey: Option<&str>,
        dispute_id: Option<&str>,
    ) -> MessageNotification {
        MessageNotification {
            order_id: Some(order_id),
            message_preview: String::new(),
            timestamp: 1,
            action,
            sat_amount: None,
            invoice: None,
            body: None,
            maker_bond_publish: false,
            solver_pubkey: solver_pubkey.map(str::to_string),
            dispute_id: dispute_id.map(str::to_string),
        }
    }

    /// A stored `payment-failed` row for `order_id`, at `settled-hold-invoice`.
    /// `kind` + `is_mine` decide whether the local user is the buyer.
    fn payment_failed_row(order_id: Uuid, kind: Kind, is_mine: bool) -> OrderMessage {
        let keys = Keys::generate();
        OrderMessage {
            message: Message::new_order(
                Some(order_id),
                None,
                None,
                Action::PaymentFailed,
                Some(Payload::PaymentFailed(PaymentFailedInfo {
                    payment_attempts: 3,
                    payment_retries_interval: 5,
                })),
            ),
            timestamp: 10,
            sender: keys.public_key(),
            order_id: Some(order_id),
            trade_index: 1,
            read: false,
            sat_amount: None,
            buyer_invoice: None,
            order_kind: Some(kind),
            is_mine: Some(is_mine),
            order_status: Some(Status::SettledHoldInvoice),
            order_snapshot: None,
            auto_popup_shown: false,
        }
    }

    fn payment_failed_notification(order_id: Uuid) -> MessageNotification {
        let mut n = notification(order_id, Action::PaymentFailed, None, None);
        n.timestamp = 10;
        n.body = Some("It will retry automatically.".to_string());
        n
    }

    fn is_payment_failed_popup(mode: &UiMode) -> bool {
        matches!(
            mode,
            UiMode::NewMessageNotification(_, Action::PaymentFailed, _)
        )
    }

    #[test]
    fn payment_failed_opens_popup_for_buy_maker_buyer() {
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        // Buy listing + we are maker => we are the buyer.
        app.messages
            .lock()
            .unwrap()
            .push(payment_failed_row(order_id, Kind::Buy, true));

        handle_message_notification(payment_failed_notification(order_id), &mut app);

        assert!(is_payment_failed_popup(&app.mode));
    }

    #[test]
    fn payment_failed_opens_popup_for_sell_taker_buyer() {
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        // Sell listing + we are taker => we are the buyer.
        app.messages
            .lock()
            .unwrap()
            .push(payment_failed_row(order_id, Kind::Sell, false));

        handle_message_notification(payment_failed_notification(order_id), &mut app);

        assert!(is_payment_failed_popup(&app.mode));
    }

    #[test]
    fn payment_failed_suppressed_for_seller() {
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        let baseline = std::mem::discriminant(&app.mode);
        // Buy listing + we are taker => we are the seller, not the buyer.
        app.messages
            .lock()
            .unwrap()
            .push(payment_failed_row(order_id, Kind::Buy, false));

        handle_message_notification(payment_failed_notification(order_id), &mut app);

        assert!(!is_payment_failed_popup(&app.mode));
        assert_eq!(std::mem::discriminant(&app.mode), baseline);
    }

    #[test]
    fn payment_failed_popup_deduplicates_repeated_dms() {
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        app.messages
            .lock()
            .unwrap()
            .push(payment_failed_row(order_id, Kind::Sell, false));

        handle_message_notification(payment_failed_notification(order_id), &mut app);
        assert!(is_payment_failed_popup(&app.mode));

        // Dismiss, then a duplicate/replayed DM must not reopen the popup.
        app.mode = UiMode::Normal;
        handle_message_notification(payment_failed_notification(order_id), &mut app);
        assert!(!is_payment_failed_popup(&app.mode));
    }

    #[test]
    fn payment_failed_suppressed_below_startup_floor() {
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        app.messages
            .lock()
            .unwrap()
            .push(payment_failed_row(order_id, Kind::Sell, false));
        // Historical DM at/below the startup floor must not auto-open a popup.
        app.startup_popup_floor_ts.insert(order_id, 10);

        handle_message_notification(payment_failed_notification(order_id), &mut app);

        assert!(!is_payment_failed_popup(&app.mode));
    }

    fn is_add_invoice_popup(mode: &UiMode) -> bool {
        matches!(
            mode,
            UiMode::NewMessageNotification(_, Action::AddInvoice, _)
                | UiMode::ConfirmSavedLnAddressForInvoice(_, _)
        )
    }

    /// Canonical post-retry state: order stays `settled-hold-invoice` while Mostro
    /// asks the buyer for a replacement Lightning invoice.
    fn add_invoice_row_at_settled(
        order_id: Uuid,
        kind: Kind,
        is_mine: bool,
        auto_popup_shown: bool,
    ) -> OrderMessage {
        let keys = Keys::generate();
        OrderMessage {
            message: Message::new_order(
                Some(order_id),
                None,
                None,
                Action::AddInvoice,
                Some(Payload::Order(mostro_core::prelude::SmallOrder {
                    id: Some(order_id),
                    kind: Some(kind),
                    status: Some(Status::SettledHoldInvoice),
                    amount: 50_000,
                    fiat_code: "EUR".to_string(),
                    fiat_amount: 100,
                    payment_method: "sepa".to_string(),
                    ..Default::default()
                })),
            ),
            timestamp: 20,
            sender: keys.public_key(),
            order_id: Some(order_id),
            trade_index: 1,
            read: false,
            sat_amount: Some(50_000),
            buyer_invoice: None,
            order_kind: Some(kind),
            is_mine: Some(is_mine),
            order_status: Some(Status::SettledHoldInvoice),
            order_snapshot: None,
            auto_popup_shown,
        }
    }

    #[test]
    fn add_invoice_at_settled_hold_invoice_opens_popup_for_buyer() {
        // Protocol happy path after all payment retries fail: Mostro sends
        // `add-invoice` while status remains `settled-hold-invoice`.
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        // Sell listing + taker = buyer.
        app.messages
            .lock()
            .unwrap()
            .push(add_invoice_row_at_settled(
                order_id,
                Kind::Sell,
                false,
                false,
            ));

        let mut n = notification(order_id, Action::AddInvoice, None, None);
        n.timestamp = 20;
        n.sat_amount = Some(50_000);
        handle_message_notification(n, &mut app);

        assert!(
            is_add_invoice_popup(&app.mode),
            "AddInvoice at SettledHoldInvoice must open the invoice popup for the buyer"
        );
    }

    #[test]
    fn add_invoice_after_payment_failed_still_opens_for_buyer() {
        // After the informational payment-failed popup was shown, a later
        // `add-invoice` (different action) must still open — `auto_popup_shown`
        // must not carry across actions.
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        app.messages
            .lock()
            .unwrap()
            .push(payment_failed_row(order_id, Kind::Sell, false));

        handle_message_notification(payment_failed_notification(order_id), &mut app);
        assert!(is_payment_failed_popup(&app.mode));

        // Simulate the trade-DM path replacing the row with AddInvoice and
        // resetting auto_popup_shown (same-action-only preserve).
        app.mode = UiMode::Normal;
        {
            let mut messages = app.messages.lock().unwrap();
            messages.retain(|m| m.order_id != Some(order_id));
            messages.push(add_invoice_row_at_settled(
                order_id,
                Kind::Sell,
                false,
                false,
            ));
        }

        let mut n = notification(order_id, Action::AddInvoice, None, None);
        n.timestamp = 20;
        n.sat_amount = Some(50_000);
        handle_message_notification(n, &mut app);

        assert!(
            is_add_invoice_popup(&app.mode),
            "AddInvoice after payment-failed must still open for the buyer"
        );
    }

    #[test]
    fn add_invoice_after_retries_opens_popup_even_if_local_status_is_success() {
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        let keys = Keys::generate();
        app.messages.lock().unwrap().push(OrderMessage {
            message: Message::new_order(
                Some(order_id),
                None,
                None,
                Action::AddInvoice,
                Some(Payload::Order(mostro_core::prelude::SmallOrder {
                    id: Some(order_id),
                    kind: Some(Kind::Sell),
                    status: Some(Status::SettledHoldInvoice),
                    amount: 139_859,
                    fiat_code: "EUR".to_string(),
                    fiat_amount: 100,
                    payment_method: "Bizum".to_string(),
                    ..Default::default()
                })),
            ),
            timestamp: 20,
            sender: keys.public_key(),
            order_id: Some(order_id),
            trade_index: 1,
            read: false,
            sat_amount: Some(139_859),
            buyer_invoice: None,
            order_kind: Some(Kind::Sell),
            is_mine: Some(false),
            // `released` used to be stored as Success; the payout-retry
            // `add-invoice` must still open for the buyer.
            order_status: Some(Status::Success),
            order_snapshot: None,
            auto_popup_shown: false,
        });

        handle_message_notification(
            notification(order_id, Action::AddInvoice, None, None),
            &mut app,
        );

        assert!(is_add_invoice_popup(&app.mode));
    }

    #[test]
    fn dispute_metadata_survives_later_notifications() {
        let order_id = Uuid::new_v4();
        let mut app = AppState::new(UserRole::User);
        app.order_chat_static.insert(
            order_id,
            OrderChatStaticHeader {
                order_id,
                kind: Some(Kind::Buy),
                created_at: None,
                trade_index: 1,
                initiator_trade_pubkey: "initiator".to_string(),
                is_mine: false,
                solver_pubkey: None,
                dispute_id: None,
            },
        );

        handle_message_notification(
            notification(
                order_id,
                Action::DisputeInitiatedByYou,
                None,
                Some("dispute-id"),
            ),
            &mut app,
        );
        handle_message_notification(
            notification(
                order_id,
                Action::AdminTookDispute,
                Some("solver-pubkey"),
                None,
            ),
            &mut app,
        );
        handle_message_notification(
            notification(order_id, Action::FiatSent, None, None),
            &mut app,
        );

        let header = app.order_chat_static.get(&order_id).expect("static header");
        assert_eq!(header.dispute_id.as_deref(), Some("dispute-id"));
        assert_eq!(header.solver_pubkey.as_deref(), Some("solver-pubkey"));
    }
}
