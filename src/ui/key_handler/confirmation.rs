use crate::ui::UiMode;

/// Helper: Transition from input mode to confirmation mode
pub fn handle_input_to_confirmation<F>(
    input: &str,
    default_mode: UiMode,
    create_confirmation: F,
) -> UiMode
where
    F: FnOnce(String) -> UiMode,
{
    if !input.is_empty() {
        create_confirmation(input.to_string())
    } else {
        default_mode
    }
}

/// Helper: Handle Enter key in confirmation mode (YES/NO selection)
pub fn handle_confirmation_enter<F1, F2>(
    selected_button: bool,
    input_string: &str,
    default_mode: UiMode,
    save_fn: F1,
    create_input: F2,
) -> UiMode
where
    F1: FnOnce(&str),
    F2: FnOnce(&str) -> UiMode,
{
    if selected_button {
        // YES selected - save
        save_fn(input_string);
        default_mode
    } else {
        // NO selected - go back to input
        create_input(input_string)
    }
}

/// Helper: Go back from confirmation to input mode
pub fn handle_confirmation_esc<F>(input_string: &str, create_input: F) -> UiMode
where
    F: FnOnce(&str) -> UiMode,
{
    create_input(input_string)
}

/// Helper to create a KeyInputState from a string
pub fn create_key_input_state(input: &str) -> crate::ui::KeyInputState {
    crate::ui::KeyInputState {
        key_input: input.to_string(),
        focused: true,
        just_pasted: false,
    }
}
