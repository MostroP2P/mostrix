// Notifications channel manager - handles message notifications from async tasks
use crate::ui::{AppState, InvoiceInputState, MessageNotification, UiMode};
use mostro_core::prelude::Action;

/// Check if the popup should be shown for a given notification
/// The message is guaranteed to exist in the vector because listen_for_order_messages
/// adds it before sending the notification.
fn check_if_popup_should_be_shown(notification: &MessageNotification, app: &AppState) -> bool {
    // Acquire lock on the messages vector
    let mut messages = app.messages.lock().unwrap();
    // Check if the notification has an order_id
    if let Some(order_id) = notification.order_id {
        // Find the corresponding OrderMessage - it's guaranteed to exist because
        // listen_for_order_messages adds the message before sending the notification
        let order_msg = messages
            .iter_mut()
            .find(|m| m.order_id == Some(order_id))
            .expect("Message should exist in vector when notification is received");
        
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

/// Handle message notification from the notification channel
pub fn handle_message_notification(notification: MessageNotification, app: &mut AppState) {
    // Only show popup automatically for PayInvoice and AddInvoice,
    // and only if we haven't already shown it for this message.
    match notification.action {
        Action::PayInvoice | Action::AddInvoice => {
            // Check if the popup should be shown for this notification
            let should_show_popup = check_if_popup_should_be_shown(&notification, app);
            if !should_show_popup {
                return;
            }

            // should_show_popup is true at this point, show the popup
            let invoice_state = InvoiceInputState {
                invoice_input: String::new(),
                // Only focus input for AddInvoice, PayInvoice is display-only.
                focused: matches!(notification.action, Action::AddInvoice),
                just_pasted: false,
                copied_to_clipboard: false,
            };
            let action = notification.action.clone();
            app.mode =
                UiMode::NewMessageNotification(notification, action, invoice_state);
        }
        _ => {

        }
    }
}

