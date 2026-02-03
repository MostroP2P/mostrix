use crate::ui::{helpers::save_chat_message, AppState, ChatSender, DisputeChatMessage};
use crate::ui::{InvoiceInputState, KeyInputState};
use crossterm::event::KeyCode;
use nostr_sdk::prelude::{Client, Keys, PublicKey};
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
fn handle_text_input<T: TextInputState>(code: KeyCode, state: &mut T) -> bool {
    // Clear the just_pasted flag on any key press (except Enter)
    if code != KeyCode::Enter {
        *state.get_just_pasted_mut() = false;
    }

    // Ignore Enter if it comes immediately after paste
    if code == KeyCode::Enter && *state.get_just_pasted_mut() {
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
pub fn handle_invoice_input(code: KeyCode, invoice_state: &mut InvoiceInputState) -> bool {
    handle_text_input(code, invoice_state)
}

/// Handle key input for admin key input popups (AddSolver, SetupAdminKey)
/// Returns true if the key was handled and should skip further processing
pub fn handle_key_input(code: KeyCode, key_state: &mut KeyInputState) -> bool {
    handle_text_input(code, key_state)
}

/// Prepare admin chat message for sending via inputbox in admin disputes in progress tab
pub fn prepare_admin_chat_message(dispute_id_key: &str, app: &mut AppState) -> String {
    // Use dispute_id as the key for chat messages
    let dispute_id_key = dispute_id_key.to_string();
    let message_content = app.admin_chat_input.trim().to_string();
    let timestamp = chrono::Utc::now().timestamp();

    // Add admin's message (track which party it was sent to)
    let admin_message = DisputeChatMessage {
        sender: ChatSender::Admin,
        content: message_content.clone(),
        timestamp,
        target_party: Some(app.active_chat_party),
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

/// Try to send admin chat message to the counterparty's pubkey.
/// Handles missing keys, invalid pubkey, and logs errors; calls util to perform the actual send.
pub fn send_admin_chat_message_to_pubkey(
    dispute_id_key: &str,
    counterparty_pubkey: Option<&str>,
    message_content: &str,
    client: &Client,
    admin_chat_keys: Option<&Keys>,
) {
    if let (Some(admin_keys), Some(counterparty_pubkey_str)) =
        (admin_chat_keys, counterparty_pubkey)
    {
        if let Ok(recipient_pubkey) = PublicKey::parse(counterparty_pubkey_str) {
            if let Err(e) =
                futures::executor::block_on(crate::util::send_admin_chat_message_to_pubkey(
                    client,
                    admin_keys,
                    &recipient_pubkey,
                    message_content.trim(),
                ))
            {
                log::error!("Failed to send admin chat message: {}", e);
            }
        } else {
            log::warn!("Invalid counterparty pubkey for dispute {}", dispute_id_key);
        }
    } else if counterparty_pubkey.is_none() {
        log::warn!(
            "Missing counterparty pubkey for dispute {} when sending chat message",
            dispute_id_key
        );
    }
}
