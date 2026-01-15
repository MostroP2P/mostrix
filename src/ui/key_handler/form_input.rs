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
                if form.focused == 0 {
                    // Toggle buy/sell
                    form.kind = if form.kind.to_lowercase() == "buy" {
                        "sell".to_string()
                    } else {
                        "buy".to_string()
                    };
                } else if form.focused == 3 {
                    // Toggle range mode
                    form.use_range = !form.use_range;
                }
            }
        }
        KeyCode::Char(c) => {
            if let UiMode::UserMode(UserMode::CreatingOrder(ref mut form)) = app.mode {
                if form.focused == 0 {
                    // ignore typing on toggle field
                } else {
                    let target = match form.focused {
                        1 => &mut form.fiat_code,
                        2 => &mut form.amount,
                        3 => &mut form.fiat_amount,
                        4 if form.use_range => &mut form.fiat_amount_max,
                        5 => &mut form.payment_method,
                        6 => &mut form.premium,
                        7 => &mut form.invoice,
                        8 => &mut form.expiration_days,
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
        if form.focused == 0 {
            // ignore
        } else {
            let target = match form.focused {
                1 => &mut form.fiat_code,
                2 => &mut form.amount,
                3 => &mut form.fiat_amount,
                4 if form.use_range => &mut form.fiat_amount_max,
                5 => &mut form.payment_method,
                6 => &mut form.premium,
                7 => &mut form.invoice,
                8 => &mut form.expiration_days,
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
