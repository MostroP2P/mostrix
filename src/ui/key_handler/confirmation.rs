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

/// Helper: Handle Enter key in confirmation mode (YES/NO selection).
///
/// On YES, `save_fn` runs first. `Ok(default_mode)` is returned only after a
/// successful save so callers can gate runtime side effects on persistence.
pub fn handle_confirmation_enter<F1, F2>(
    selected_button: bool,
    input_string: &str,
    default_mode: UiMode,
    save_fn: F1,
    create_input: F2,
) -> Result<UiMode, String>
where
    F1: FnOnce(&str) -> Result<(), String>,
    F2: FnOnce(&str) -> UiMode,
{
    if selected_button {
        save_fn(input_string)?;
        Ok(default_mode)
    } else {
        Ok(create_input(input_string))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yes_returns_default_mode_after_successful_save() {
        let mode = handle_confirmation_enter(
            true,
            "abc",
            UiMode::Normal,
            |_| Ok(()),
            |_| panic!("NO callback must not run on YES"),
        )
        .expect("successful save returns default mode");
        assert!(matches!(mode, UiMode::Normal));
    }

    #[test]
    fn yes_returns_err_when_save_fails() {
        let err = handle_confirmation_enter(
            true,
            "abc",
            UiMode::Normal,
            |_| Err("disk full".to_string()),
            |_| panic!("NO callback must not run on YES"),
        )
        .expect_err("failed save must not yield default mode");
        assert_eq!(err, "disk full");
    }

    #[test]
    fn no_skips_save_and_returns_input_mode() {
        let mut went_back = false;
        handle_confirmation_enter(
            false,
            "abc",
            UiMode::Normal,
            |_| panic!("save must not run on NO"),
            |_| {
                went_back = true;
                UiMode::Normal
            },
        )
        .expect("NO path does not persist");
        assert!(went_back);
    }
}
