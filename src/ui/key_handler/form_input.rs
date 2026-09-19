use crate::ui::currencies::{filter_options, resolve_options};
use crate::ui::orders::FormField;
use crate::ui::payment_methods::{
    join_selected, parse_selected, picker_rows, toggle_method, PickerItem,
};
use crate::ui::{AppState, FormState, TakeOrderState, UiMode, UserMode};
use crossterm::event::KeyCode;

/// True when Create New Order has a field focused that should swallow typing
/// (anything except the buy/sell toggle). Includes Currency and Payment Method
/// so global shortcuts like `c` do not fire; those fields are handled by the
/// picker interceptors rather than `handle_char_input`.
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
/// Selecting a different fiat code clears `payment_method` (methods are
/// currency-specific).
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
                let previous = form.fiat_code.trim().to_ascii_uppercase();
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
                if form.fiat_code.trim().to_ascii_uppercase() != previous {
                    form.payment_method.clear();
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

/// Intercept keys for the multi-select payment-method dropdown.
///
/// Closed: Enter, Space, or typing opens the overlay. Open: ↑↓ move, Enter
/// toggles a listed method or adds a sanitized custom name, Space with an
/// empty filter also toggles, Esc closes and keeps the current selection.
/// Returns `Some(true)` when consumed, or `None` to let normal dispatch continue.
pub fn handle_payment_method_picker_key(code: KeyCode, app: &mut AppState) -> Option<bool> {
    let (open, focused_method) = match &app.mode {
        UiMode::UserMode(UserMode::CreatingOrder(form)) => (
            form.payment_method_picker.open,
            form.focused == FormField::PaymentMethod,
        ),
        _ => return None,
    };
    if !focused_method {
        return None;
    }

    let form = match &mut app.mode {
        UiMode::UserMode(UserMode::CreatingOrder(form)) => form,
        _ => return None,
    };

    if !open {
        match code {
            KeyCode::Enter | KeyCode::Char(' ') => {
                open_payment_method_picker(form);
                Some(true)
            }
            KeyCode::Char(c) if !c.is_control() => {
                open_payment_method_picker(form);
                form.payment_method_picker.filter.push(c);
                form.payment_method_picker.selected = 0;
                Some(true)
            }
            _ => None,
        }
    } else {
        let rows = picker_rows(
            &form.fiat_code,
            &form.payment_method,
            &form.payment_method_picker.filter,
        );
        match code {
            KeyCode::Up => {
                if !rows.is_empty() {
                    let n = rows.len();
                    form.payment_method_picker.selected =
                        (form.payment_method_picker.selected + n - 1) % n;
                }
                Some(true)
            }
            KeyCode::Down => {
                if !rows.is_empty() {
                    let n = rows.len();
                    form.payment_method_picker.selected =
                        (form.payment_method_picker.selected + 1) % n;
                }
                Some(true)
            }
            KeyCode::Enter => {
                apply_picker_selection(form);
                Some(true)
            }
            KeyCode::Char(' ') if form.payment_method_picker.filter.is_empty() => {
                apply_picker_selection(form);
                Some(true)
            }
            KeyCode::Esc => {
                close_payment_method_picker(form);
                Some(true)
            }
            KeyCode::Backspace => {
                form.payment_method_picker.filter.pop();
                form.payment_method_picker.selected = 0;
                Some(true)
            }
            KeyCode::Char(c) if !c.is_control() => {
                form.payment_method_picker.filter.push(c);
                form.payment_method_picker.selected = 0;
                Some(true)
            }
            _ => Some(true),
        }
    }
}

fn open_payment_method_picker(form: &mut FormState) {
    let rows = picker_rows(&form.fiat_code, &form.payment_method, "");
    let selected = parse_selected(&form.payment_method);
    let idx = rows
        .iter()
        .position(|row| match row {
            PickerItem::Listed(name) => selected.iter().any(|s| s.eq_ignore_ascii_case(name)),
            PickerItem::Custom(_) => false,
        })
        .unwrap_or(0);
    form.payment_method_picker.open = true;
    form.payment_method_picker.filter.clear();
    form.payment_method_picker.selected = idx;
}

fn close_payment_method_picker(form: &mut FormState) {
    form.payment_method_picker.open = false;
    form.payment_method_picker.filter.clear();
    form.payment_method_picker.selected = 0;
}

fn apply_picker_selection(form: &mut FormState) {
    let rows = picker_rows(
        &form.fiat_code,
        &form.payment_method,
        &form.payment_method_picker.filter,
    );
    if rows.is_empty() {
        return;
    }
    let idx = form
        .payment_method_picker
        .selected
        .min(rows.len().saturating_sub(1));
    match rows.get(idx) {
        Some(PickerItem::Listed(name)) => {
            let mut selected = parse_selected(&form.payment_method);
            toggle_method(&mut selected, name);
            form.payment_method = join_selected(&selected);
        }
        Some(PickerItem::Custom(name)) => {
            let mut selected = parse_selected(&form.payment_method);
            if !selected.iter().any(|m| m.eq_ignore_ascii_case(name)) {
                selected.push(name.clone());
            }
            form.payment_method = join_selected(&selected);
            form.payment_method_picker.filter.clear();
            form.payment_method_picker.selected = 0;
        }
        None => {}
    }
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

    fn creating_order_on_method(payment_method: &str, open: bool) -> AppState {
        let mut app = AppState::new(UserRole::User);
        let mut form = FormState::new_default_form();
        form.focused = FormField::PaymentMethod;
        form.payment_method = payment_method.to_string();
        form.payment_method_picker.open = open;
        app.mode = UiMode::UserMode(UserMode::CreatingOrder(form));
        app
    }

    fn creating_form(app: &AppState) -> &FormState {
        match &app.mode {
            UiMode::UserMode(UserMode::CreatingOrder(form)) => form,
            other => panic!("expected CreatingOrder, got {other:?}"),
        }
    }

    #[test]
    fn payment_method_picker_enter_opens_when_closed() {
        let mut app = creating_order_on_method("", false);
        assert_eq!(
            handle_payment_method_picker_key(KeyCode::Enter, &mut app),
            Some(true)
        );
        let form = creating_form(&app);
        assert!(form.payment_method_picker.open);
        assert!(form.payment_method.is_empty());
    }

    #[test]
    fn payment_method_picker_space_toggles_listed_method_while_open() {
        let mut app = creating_order_on_method("", true);
        assert_eq!(
            handle_payment_method_picker_key(KeyCode::Char(' '), &mut app),
            Some(true)
        );
        let form = creating_form(&app);
        assert!(form.payment_method_picker.open);
        assert_eq!(form.payment_method, "Cash App");
        assert_eq!(
            handle_payment_method_picker_key(KeyCode::Char(' '), &mut app),
            Some(true)
        );
        assert!(creating_form(&app).payment_method.is_empty());
    }

    #[test]
    fn payment_method_picker_enter_adds_custom_when_no_exact_match() {
        let mut app = creating_order_on_method("", true);
        match &mut app.mode {
            UiMode::UserMode(UserMode::CreatingOrder(form)) => {
                form.payment_method_picker.filter = "My Bank".to_string();
                let rows = picker_rows("USD", "", "My Bank");
                form.payment_method_picker.selected = rows.len().saturating_sub(1);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            handle_payment_method_picker_key(KeyCode::Enter, &mut app),
            Some(true)
        );
        let form = creating_form(&app);
        assert_eq!(form.payment_method, "My Bank");
        assert!(form.payment_method_picker.open);
        assert!(form.payment_method_picker.filter.is_empty());
    }

    #[test]
    fn payment_method_picker_esc_keeps_selection() {
        let mut app = creating_order_on_method("Zelle", true);
        assert_eq!(
            handle_payment_method_picker_key(KeyCode::Esc, &mut app),
            Some(true)
        );
        let form = creating_form(&app);
        assert!(!form.payment_method_picker.open);
        assert_eq!(form.payment_method, "Zelle");
    }

    #[test]
    fn currency_change_clears_payment_methods() {
        let mut app = AppState::new(UserRole::User);
        app.mostro_info = None;
        let mut form = FormState::new_default_form();
        form.focused = FormField::Currency;
        form.payment_method = "Zelle".to_string();
        form.currency_picker.open = true;
        form.currency_picker.filter = "EUR".to_string();
        form.currency_picker.selected = 0;
        app.mode = UiMode::UserMode(UserMode::CreatingOrder(form));

        assert_eq!(
            handle_currency_picker_key(KeyCode::Enter, &mut app),
            Some(true)
        );
        match &app.mode {
            UiMode::UserMode(UserMode::CreatingOrder(form)) => {
                assert_eq!(form.fiat_code, "EUR");
                assert!(form.payment_method.is_empty());
            }
            other => panic!("expected CreatingOrder, got {other:?}"),
        }
    }

    #[test]
    fn payment_method_picker_consumes_c_when_focused() {
        let mut app = creating_order_on_method("", false);
        assert_eq!(
            handle_payment_method_picker_key(KeyCode::Char('c'), &mut app),
            Some(true)
        );
        let form = creating_form(&app);
        assert!(form.payment_method_picker.open);
        assert_eq!(form.payment_method_picker.filter, "c");
    }
}
