use crate::ui::currencies::{filter_options, resolve_options};
use crate::ui::orders::FormField;
use crate::ui::{AppState, FormState, TakeOrderState, UiMode, UserMode};
use crossterm::event::KeyCode;

/// True when the create-order form has a text-editable field focused (not buy/sell toggle).
pub fn is_creating_order_text_input(app: &AppState) -> bool {
    matches!(
        app.mode,
        UiMode::UserMode(UserMode::CreatingOrder(ref form))
            if form.focused != FormField::OrderType
    )
}

/// Intercept keys for the currency dropdown on the Create New Order form.
///
/// Returns `Some(true)` when the key was consumed (either opening the picker or
/// operating it while open), or `None` to let normal key dispatch continue.
pub fn handle_currency_picker_key(code: KeyCode, app: &mut AppState) -> Option<bool> {
    let (open, focused_currency) = match &app.mode {
        UiMode::UserMode(UserMode::CreatingOrder(form)) => (
            form.currency_picker.open,
            form.focused == FormField::Currency,
        ),
        _ => return None,
    };
    if !focused_currency {
        return None;
    }

    // Accepted currencies advertised by the connected instance (empty = all).
    let accepted: Vec<String> = app
        .mostro_info
        .as_ref()
        .map(|i| i.fiat_currencies_accepted.clone())
        .unwrap_or_default();

    let form = match &mut app.mode {
        UiMode::UserMode(UserMode::CreatingOrder(form)) => form,
        _ => return None,
    };

    if !open {
        // Closed: Enter/Space or typing opens the picker.
        match code {
            KeyCode::Enter | KeyCode::Char(' ') => {
                open_currency_picker(form, &accepted);
                Some(true)
            }
            KeyCode::Char(c) if c.is_ascii_alphanumeric() => {
                open_currency_picker(form, &accepted);
                form.currency_picker.filter.push(c.to_ascii_uppercase());
                form.currency_picker.selected = 0;
                Some(true)
            }
            // Up/Down (field nav), Tab, Esc (cancel) handled by normal dispatch.
            _ => None,
        }
    } else {
        let options = resolve_options(&accepted);
        let filtered = filter_options(&options, &form.currency_picker.filter);
        match code {
            KeyCode::Up => {
                if !filtered.is_empty() {
                    let n = filtered.len();
                    form.currency_picker.selected = (form.currency_picker.selected + n - 1) % n;
                }
                Some(true)
            }
            KeyCode::Down => {
                if !filtered.is_empty() {
                    let n = filtered.len();
                    form.currency_picker.selected = (form.currency_picker.selected + 1) % n;
                }
                Some(true)
            }
            KeyCode::Enter => {
                let filter = form.currency_picker.filter.trim().to_ascii_uppercase();
                let idx = form
                    .currency_picker
                    .selected
                    .min(filtered.len().saturating_sub(1));
                // Prefer an exact code match. In unrestricted mode, a typed 3-letter
                // code that is not an exact option code wins over name-substring hits
                // (e.g. "NAD" must not become "CAD" via "Canadian Dollar").
                if let Some(choice) = filtered.iter().find(|o| o.code == filter) {
                    form.fiat_code = choice.code.clone();
                } else if accepted.is_empty() {
                    if let Some(code) = custom_currency_code(&filter) {
                        form.fiat_code = code;
                    } else if let Some(choice) = filtered.get(idx) {
                        form.fiat_code = choice.code.clone();
                    }
                } else if let Some(choice) = filtered.get(idx) {
                    form.fiat_code = choice.code.clone();
                }
                close_currency_picker(form);
                Some(true)
            }
            KeyCode::Esc => {
                close_currency_picker(form);
                Some(true)
            }
            KeyCode::Backspace => {
                form.currency_picker.filter.pop();
                form.currency_picker.selected = 0;
                Some(true)
            }
            KeyCode::Char(c) if c.is_ascii_alphanumeric() => {
                form.currency_picker.filter.push(c.to_ascii_uppercase());
                form.currency_picker.selected = 0;
                Some(true)
            }
            // Swallow everything else so the overlay stays modal.
            _ => Some(true),
        }
    }
}

fn open_currency_picker(form: &mut FormState, accepted: &[String]) {
    let options = resolve_options(accepted);
    let current = form.fiat_code.trim().to_ascii_uppercase();
    let idx = options.iter().position(|o| o.code == current).unwrap_or(0);
    form.currency_picker.open = true;
    form.currency_picker.filter.clear();
    form.currency_picker.selected = idx;
}

fn close_currency_picker(form: &mut FormState) {
    form.currency_picker.open = false;
    form.currency_picker.filter.clear();
    form.currency_picker.selected = 0;
}

/// Accept a typed ISO-4217 code (exactly three ASCII letters) when the instance
/// advertises an empty accepted list (meaning all currencies).
fn custom_currency_code(filter: &str) -> Option<String> {
    let code = filter.trim().to_ascii_uppercase();
    if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(code)
    } else {
        None
    }
}

/// Handle character input for forms
pub fn handle_char_input(
    code: KeyCode,
    app: &mut AppState,
    validate_range_amount: &dyn Fn(&mut TakeOrderState),
) {
    match code {
        KeyCode::Char(' ') => {
            if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                match form.focused {
                    FormField::OrderType => {
                        // Toggle buy/sell
                        form.kind = if form.kind.to_lowercase() == "buy" {
                            "sell".to_string()
                        } else {
                            "buy".to_string()
                        };
                    }
                    FormField::FiatAmount => {
                        // Toggle range mode
                        form.use_range = !form.use_range;
                    }
                    FormField::PaymentMethod => {
                        // Payment method descriptions may contain spaces.
                        form.payment_method.push(' ');
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Char(c) => {
            if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                if form.focused == FormField::OrderType {
                    // ignore typing on toggle field
                } else {
                    let target = match form.focused {
                        FormField::Currency => &mut form.fiat_code,
                        FormField::AmountSats => &mut form.amount,
                        FormField::FiatAmount => &mut form.fiat_amount,
                        FormField::FiatAmountMax => {
                            if form.use_range {
                                &mut form.fiat_amount_max
                            } else {
                                &mut form.fiat_amount
                            }
                        }
                        FormField::PaymentMethod => &mut form.payment_method,
                        FormField::Premium => &mut form.premium,
                        FormField::Invoice => &mut form.invoice,
                        FormField::ExpirationDays => &mut form.expiration_days,
                        _ => unreachable!(),
                    };
                    target.push(c);
                }
            } else if let UiMode::UserMode(UserMode::TakingOrder(ref mut take_state)) = app.mode {
                // Allow typing in the amount input field for range orders
                if take_state.is_range_order {
                    // Only allow digits and decimal point
                    if c.is_ascii_digit() || c == '.' {
                        take_state.amount_input.push(c);
                        // Validate after typing
                        validate_range_amount(take_state);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Handle backspace for forms
pub fn handle_backspace(app: &mut AppState, validate_range_amount: &dyn Fn(&mut TakeOrderState)) {
    if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
        if form.focused == FormField::OrderType {
            // ignore
        } else {
            let target = match form.focused {
                FormField::Currency => &mut form.fiat_code,
                FormField::AmountSats => &mut form.amount,
                FormField::FiatAmount => &mut form.fiat_amount,
                FormField::FiatAmountMax => {
                    if form.use_range {
                        &mut form.fiat_amount_max
                    } else {
                        &mut form.fiat_amount
                    }
                }
                FormField::PaymentMethod => &mut form.payment_method,
                FormField::Premium => &mut form.premium,
                FormField::Invoice => &mut form.invoice,
                FormField::ExpirationDays => &mut form.expiration_days,
                _ => unreachable!(),
            };
            target.pop();
        }
    } else if let UiMode::UserMode(UserMode::TakingOrder(ref mut take_state)) = app.mode {
        // Allow backspace in the amount input field
        if take_state.is_range_order {
            take_state.amount_input.pop();
            // Validate after deletion
            validate_range_amount(take_state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{FormState, UserRole};

    #[test]
    fn creating_order_text_input_excludes_order_type_toggle() {
        let mut app = AppState::new(UserRole::User);
        let mut form = FormState::new_default_form();
        form.focused = FormField::PaymentMethod;
        app.mode = UiMode::UserMode(UserMode::CreatingOrder(form));
        assert!(is_creating_order_text_input(&app));

        if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
            form.focused = FormField::OrderType;
        }
        assert!(!is_creating_order_text_input(&app));
    }

    #[test]
    fn custom_currency_code_accepts_three_letter_iso() {
        assert_eq!(custom_currency_code("kwd").as_deref(), Some("KWD"));
        assert_eq!(custom_currency_code("  BHD ").as_deref(), Some("BHD"));
        assert_eq!(custom_currency_code("JO").as_deref(), None);
        assert_eq!(custom_currency_code("USDT").as_deref(), None);
        assert_eq!(custom_currency_code("12A").as_deref(), None);
    }

    #[test]
    fn currency_picker_enter_accepts_unlisted_code_when_all_currencies_allowed() {
        let mut app = AppState::new(UserRole::User);
        app.mostro_info = None; // no accepted list → all currencies
        let mut form = FormState::new_default_form();
        form.focused = FormField::Currency;
        form.currency_picker.open = true;
        form.currency_picker.filter = "KWD".to_string();
        form.currency_picker.selected = 0;
        app.mode = UiMode::UserMode(UserMode::CreatingOrder(form));

        assert_eq!(
            handle_currency_picker_key(KeyCode::Enter, &mut app),
            Some(true)
        );
        match &app.mode {
            UiMode::UserMode(UserMode::CreatingOrder(form)) => {
                assert_eq!(form.fiat_code, "KWD");
                assert!(!form.currency_picker.open);
            }
            other => panic!("expected CreatingOrder, got {other:?}"),
        }
    }

    #[test]
    fn currency_picker_enter_prefers_custom_nad_over_canadian_dollar_name_hit() {
        // "NAD" is a valid ISO code not in CURRENCIES, but also a substring of
        // "Canadian Dollar" — unrestricted Enter must keep NAD, not assign CAD.
        let mut app = AppState::new(UserRole::User);
        app.mostro_info = None;
        let mut form = FormState::new_default_form();
        form.focused = FormField::Currency;
        form.currency_picker.open = true;
        form.currency_picker.filter = "NAD".to_string();
        form.currency_picker.selected = 0;
        app.mode = UiMode::UserMode(UserMode::CreatingOrder(form));

        assert_eq!(
            handle_currency_picker_key(KeyCode::Enter, &mut app),
            Some(true)
        );
        match &app.mode {
            UiMode::UserMode(UserMode::CreatingOrder(form)) => {
                assert_eq!(form.fiat_code, "NAD");
            }
            other => panic!("expected CreatingOrder, got {other:?}"),
        }
    }
}
