use crate::ui::{helpers::save_chat_message, AppState, ChatSender, DisputeChatMessage};
use crate::ui::{InvoiceInputState, KeyInputState};
use crossterm::event::KeyCode;
use nostr_sdk::prelude::{Client, Keys};
/// Trait for input states that can handle text input
trait TextInputState {
    fn get_input_mut(&mut self) -> &mut String;
    fn get_just_pasted_mut(&mut self) -> &mut bool;
}

impl TextInputState for crate::ui::InvoiceInputState {
    fn get_input_mut(&mut self) -> &mut String {
        &mut self.invoice_input
    }
    fn get_just_pasted_mut(&mut self) -> &mut bool {
        &mut self.just_pasted
    }
}

impl TextInputState for crate::ui::KeyInputState {
    fn get_input_mut(&mut self) -> &mut String {
        &mut self.key_input
    }
    fn get_just_pasted_mut(&mut self) -> &mut bool {
        &mut self.just_pasted
    }
}

/// Generic handler for text input (invoice or key input)
/// Returns true if the key was handled and should skip further processing
fn handle_text_input<T: TextInputState>(
    code: KeyCode,
    state: &mut T,
    ignore_enter_after_paste: bool,
) -> bool {
    // Clear the just_pasted flag on any key press (except Enter)
    if code != KeyCode::Enter {
        *state.get_just_pasted_mut() = false;
    }

    // Optionally ignore Enter if it comes immediately after paste
    if ignore_enter_after_paste && code == KeyCode::Enter && *state.get_just_pasted_mut() {
        *state.get_just_pasted_mut() = false;
        return true; // Skip processing this Enter key
    }

    // Handle character input
    match code {
        KeyCode::Char(c) => {
            state.get_input_mut().push(c);
            true // Skip further processing
        }
        KeyCode::Backspace => {
            state.get_input_mut().pop();
            true // Skip further processing
        }
        _ => false, // Let Enter and Esc fall through to app logic
    }
}

/// Handle invoice input for AddInvoice notifications
/// Returns true if the key was handled and should skip further processing
///
/// Does **not** swallow Enter after paste: bracketed paste / clipboard paste
/// already strip newlines from the pasted text, so the next Enter is the user's
/// intentional submit. Swallowing it forced a double-Enter regression.
pub fn handle_invoice_input(code: KeyCode, invoice_state: &mut InvoiceInputState) -> bool {
    handle_text_input(code, invoice_state, false)
}

/// Handle key input for admin key input popups (AddSolver, SetupAdminKey)
/// Returns true if the key was handled and should skip further processing
pub fn handle_key_input(code: KeyCode, key_state: &mut KeyInputState) -> bool {
    handle_text_input(code, key_state, false)
}

/// Prepare admin chat message for sending via inputbox in admin disputes in progress tab.
pub fn prepare_admin_chat_message(
    dispute_id_key: &str,
    message_content: &str,
    app: &mut AppState,
) -> String {
    // Use dispute_id as the key for chat messages
    let dispute_id_key = dispute_id_key.to_string();
    let message_content = message_content.trim().to_string();
    let timestamp = chrono::Utc::now().timestamp();

    // Add admin's message (track which party it was sent to)
    let admin_message = DisputeChatMessage {
        sender: ChatSender::Admin,
        content: message_content.clone(),
        timestamp,
        target_party: Some(app.active_chat_party),
        attachment: None,
    };

    app.admin_dispute_chats
        .entry(dispute_id_key.clone())
        .or_default()
        .push(admin_message.clone());

    // Save admin message to file (use dispute_id_key for consistency)
    save_chat_message(&dispute_id_key, &admin_message);

    // Return dispute_id_key for further processing
    dispute_id_key
}

/// Send an admin chat message using the per-dispute ECDH shared key.
///
/// Looks up the stored `shared_key_hex` for the given party, rebuilds the
/// ECDH `Keys`, and spawns an async task that wraps the message as kind 14
/// (`K_sign` / `K_conv` via `send_admin_chat_message_via_shared_key`).
pub fn send_admin_chat_message_via_shared_key(
    dispute_id_key: &str,
    shared_key_hex: Option<&str>,
    message_content: &str,
    client: &Client,
    admin_chat_keys: Option<&Keys>,
    mostro_instance: Option<crate::util::MostroInstanceInfo>,
) {
    let Some(admin_keys) = admin_chat_keys else {
        log::warn!(
            "Admin chat keys not available; cannot send message for dispute {}",
            dispute_id_key
        );
        return;
    };

    let Some(hex) = shared_key_hex else {
        log::warn!(
            "Missing shared key for dispute {} when sending chat message",
            dispute_id_key
        );
        return;
    };

    let Some(shared_keys) = crate::util::chat_utils::keys_from_shared_hex(hex) else {
        log::warn!("Invalid shared key hex for dispute {}", dispute_id_key);
        return;
    };

    let client = client.clone();
    let admin_keys = admin_keys.clone();
    let message_content = message_content.trim().to_string();
    let dispute_id_key = dispute_id_key.to_string();

    tokio::spawn(async move {
        match crate::util::send_admin_chat_message_via_shared_key(
            &client,
            &admin_keys,
            &shared_keys,
            &message_content,
            mostro_instance.as_ref(),
        )
        .await
        {
            Ok(()) => log::info!("Admin chat message sent for dispute {}", dispute_id_key),
            Err(e) => log::error!(
                "Failed to send admin chat message for dispute {}: {}",
                dispute_id_key,
                e
            ),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::handle_invoice_input;
    use crate::ui::{InvoiceInputState, InvoiceNotificationActionSelection};
    use crossterm::event::KeyCode;

    fn focused_invoice(input: &str) -> InvoiceInputState {
        InvoiceInputState {
            invoice_input: input.to_string(),
            focused: true,
            just_pasted: false,
            copied_to_clipboard: false,
            scroll_y: 0,
            action_selection: InvoiceNotificationActionSelection::Primary,
        }
    }

    #[test]
    fn enter_is_not_swallowed_after_paste_flag() {
        // Paste paths used to set `just_pasted`, which made the next Enter a no-op
        // (double-Enter to submit). Enter must fall through to the submit handler.
        let mut state = focused_invoice("lnbc1...");
        state.just_pasted = true;
        assert!(
            !handle_invoice_input(KeyCode::Enter, &mut state),
            "Enter after paste must not be consumed by invoice input handling"
        );
    }

    #[test]
    fn chars_and_backspace_still_handled() {
        let mut state = focused_invoice("ab");
        assert!(handle_invoice_input(KeyCode::Char('c'), &mut state));
        assert_eq!(state.invoice_input, "abc");
        assert!(handle_invoice_input(KeyCode::Backspace, &mut state));
        assert_eq!(state.invoice_input, "ab");
    }
}
