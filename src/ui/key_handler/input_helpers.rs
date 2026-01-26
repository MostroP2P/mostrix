use crate::ui::{InvoiceInputState, KeyInputState};
use crossterm::event::KeyCode;
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
