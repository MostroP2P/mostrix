// Notifications channel manager - handles message notifications from async tasks
use crate::settings::load_settings_from_disk;
use crate::ui::orders::BuyerInvoicePreference;
use crate::ui::orders::{
    invoice_popup_allowed_for_order_status, local_user_must_act_on_invoice_popup,
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

/// Handle message notification from the notification channel
pub fn handle_message_notification(notification: MessageNotification, app: &mut AppState) {
    // Only show popup automatically for PayInvoice / PayBondInvoice / AddInvoice,
    // and only if we haven't already shown it for this message.
    match notification.action {
        Action::PayInvoice | Action::PayBondInvoice | Action::AddInvoice => {
            let should_show_popup = check_if_popup_should_be_shown(&notification, app);
            if !should_show_popup {
                return;
            }

            if matches!(notification.action, Action::AddInvoice) {
                app.mode =
                    present_add_invoice_popup(&mut app.buyer_invoice_preference, notification);
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
        _ => {}
    }
}
