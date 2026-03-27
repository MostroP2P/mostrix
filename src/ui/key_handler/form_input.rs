use crate::ui::orders::FormField;
use crate::ui::{AppState, TakeOrderState, UiMode, UserMode};
use crossterm::event::KeyCode;

/// Handle character input for forms
pub fn handle_char_input(
    code: KeyCode,
    app: &mut AppState,
    validate_range_amount: &dyn Fn(&mut TakeOrderState),
) {
    match code {
        KeyCode::Char(' ') => {
            if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                if form.focused == FormField::OrderType {
                    // Toggle buy/sell
                    form.kind = if form.kind.to_lowercase() == "buy" {
                        "sell".to_string()
                    } else {
                        "buy".to_string()
                    };
                } else if form.focused == FormField::FiatAmount {
                    // Toggle range mode
                    form.use_range = !form.use_range;
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
