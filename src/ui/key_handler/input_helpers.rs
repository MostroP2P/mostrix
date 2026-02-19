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

/// Send an admin chat message using the per-dispute shared key.
///
/// Looks up the stored `shared_key_hex` for the given party, rebuilds the
/// shared `Keys`, and spawns an async task that wraps the message in a NIP-59
/// gift wrap addressed to the shared key's public key.
pub fn send_admin_chat_message_via_shared_key(
    dispute_id_key: &str,
    shared_key_hex: Option<&str>,
    message_content: &str,
    client: &Client,
    admin_chat_keys: Option<&Keys>,
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

    tokio::spawn(async move {
        if let Err(e) = crate::util::send_admin_chat_message_via_shared_key(
            &client,
            &admin_keys,
            &shared_keys,
            &message_content,
        )
        .await
        {
            log::error!("Failed to send admin chat message: {}", e);
        }
    });
}
