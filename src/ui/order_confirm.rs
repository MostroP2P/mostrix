use ratatui::layout::{Constraint, Direction, Flex, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{helpers, FormState, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::currencies;
use crate::ui::helpers::{display_width, wrap_to_width};

/// Max wrapped lines for payment method / invoice so the popup stays bounded.
const MAX_WRAPPED_DETAIL_LINES: usize = 4;

pub fn render_order_confirm(f: &mut ratatui::Frame, form: &FormState, selected_button: bool) {
    let area = f.area();
    let popup_width = area.width.saturating_sub(area.width / 4);
    let inner_width = popup_width.saturating_sub(2); // exclude border columns
    let content_width = inner_width.saturating_sub(2).max(1) as usize; // side padding

    let payment_lines = labeled_wrapped_lines(
        "Payment Method: ",
        &form.payment_method,
        content_width,
        Style::default().fg(PRIMARY_COLOR),
    );
    let invoice_lines = if form.invoice.is_empty() {
        Vec::new()
    } else {
        labeled_wrapped_lines(
            "Invoice: ",
            &form.invoice,
            content_width,
            Style::default().fg(PRIMARY_COLOR),
        )
    };

    // Fixed chrome (spacer/title/sep/kind/currency/amount/fiat/premium/exp/privacy/buttons/help)
    // + wrapped payment (+ optional invoice) lines.
    let fixed_rows: u16 = 1 + 2 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 3 + 1;
    let popup_height = (fixed_rows + payment_lines.len() as u16 + invoice_lines.len() as u16 + 2) // outer border
        .min(area.height)
        .max(10);

    let popup = {
        let [popup] = Layout::horizontal([Constraint::Length(popup_width)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(popup_height)])
            .flex(Flex::Center)
            .areas(popup);
        popup
    };

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("📋 Order Confirmation")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let mut constraints = vec![
        Constraint::Length(1), // spacer
        Constraint::Length(2), // title
        Constraint::Length(1), // separator
        Constraint::Length(1), // kind
        Constraint::Length(1), // currency
        Constraint::Length(1), // amount
        Constraint::Length(1), // fiat amount
    ];
    constraints.push(Constraint::Length(payment_lines.len() as u16));
    constraints.push(Constraint::Length(1)); // premium
    if !invoice_lines.is_empty() {
        constraints.push(Constraint::Length(invoice_lines.len() as u16));
    }
    constraints.extend([
        Constraint::Length(1), // expiration
        Constraint::Length(1), // privacy
        Constraint::Length(3), // buttons
        Constraint::Length(1), // help text
    ]);

    let inner_chunks = Layout::new(Direction::Vertical, constraints).split(inner);

    // Title
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Please review your order:",
            Style::default().add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[1],
    );

    let kind_str = if form.kind.to_lowercase() == "buy" {
        "🟢 Buy"
    } else {
        "🔴 Sell"
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Order Type: "),
            Span::styled(kind_str, Style::default().fg(PRIMARY_COLOR)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[3],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Currency: "),
            Span::styled(
                form.fiat_code.to_ascii_uppercase(),
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                {
                    let name = currencies::name_for(&form.fiat_code);
                    if name.is_empty() {
                        String::new()
                    } else {
                        format!("  {name}")
                    }
                },
                Style::default().fg(Color::Gray),
            ),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[4],
    );

    let amount_str = if form.amount.is_empty() || form.amount == "0" {
        "market".to_string()
    } else {
        format!("{} sats", form.amount)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Amount: "),
            Span::styled(amount_str, Style::default().fg(PRIMARY_COLOR)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[5],
    );

    let fiat_str = if form.use_range && !form.fiat_amount_max.is_empty() {
        format!(
            "{}-{} {}",
            form.fiat_amount, form.fiat_amount_max, form.fiat_code
        )
    } else {
        format!("{} {}", form.fiat_amount, form.fiat_code)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Fiat Amount: "),
            Span::styled(fiat_str, Style::default().fg(PRIMARY_COLOR)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[6],
    );

    f.render_widget(
        Paragraph::new(payment_lines).alignment(ratatui::layout::Alignment::Center),
        inner_chunks[7],
    );

    let mut idx = 8;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Premium: "),
            Span::styled(
                format!("{}%", form.premium),
                Style::default().fg(PRIMARY_COLOR),
            ),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[idx],
    );
    idx += 1;

    if !invoice_lines.is_empty() {
        f.render_widget(
            Paragraph::new(invoice_lines).alignment(ratatui::layout::Alignment::Center),
            inner_chunks[idx],
        );
        idx += 1;
    }

    let exp_str = match form.expiration_days.trim().parse::<i64>() {
        Ok(n) if n >= 1 => format!("{n} days"),
        Ok(0) => "0 (invalid — min 1 day)".to_string(),
        _ if form.expiration_days.trim().is_empty() => "—".to_string(),
        _ => form.expiration_days.clone(),
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Expiration: "),
            Span::styled(exp_str, Style::default().fg(PRIMARY_COLOR)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[idx],
    );
    idx += 1;

    let privacy_str = if form.full_privacy {
        "Full privacy (no reputation)"
    } else {
        "Reputation"
    };
    let privacy_color = if form.full_privacy {
        Color::Yellow
    } else {
        PRIMARY_COLOR
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Privacy: "),
            Span::styled(privacy_str, Style::default().fg(privacy_color)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[idx],
    );
    idx += 1;

    helpers::render_yes_no_buttons(f, inner_chunks[idx], selected_button, "✓ YES", "✗ NO");
    idx += 1;

    helpers::render_help_text(
        f,
        inner_chunks[idx],
        "Press ",
        "Enter",
        " to confirm, Esc to cancel",
    );
}

/// Build centered detail lines: first line is `label` + start of value; further
/// lines continue the value. Hard-wraps so unbroken runs cannot escape the frame.
fn labeled_wrapped_lines(
    label: &str,
    value: &str,
    width: usize,
    value_style: Style,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let label_w = display_width(label);
    let first_budget = width.saturating_sub(label_w).max(1);

    let mut chars = value.chars().peekable();
    let mut first = String::new();
    let mut w = 0usize;
    while let Some(&ch) = chars.peek() {
        let cw = display_width(&ch.to_string());
        if w + cw > first_budget {
            break;
        }
        first.push(chars.next().expect("peeked char"));
        w += cw;
    }
    let rest: String = chars.collect();

    let mut value_chunks = vec![first];
    if !rest.is_empty() {
        value_chunks.extend(wrap_to_width(&rest, width));
    }

    if value_chunks.len() > MAX_WRAPPED_DETAIL_LINES {
        value_chunks.truncate(MAX_WRAPPED_DETAIL_LINES);
        if let Some(last) = value_chunks.last_mut() {
            *last = crate::ui::helpers::truncate_to_width(last, width);
        }
    }

    value_chunks
        .into_iter()
        .enumerate()
        .map(|(i, chunk)| {
            if i == 0 {
                Line::from(vec![
                    Span::raw(label.to_string()),
                    Span::styled(chunk, value_style),
                ])
            } else {
                Line::from(Span::styled(chunk, value_style))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_contains(buf: &ratatui::buffer::Buffer, needle: &str) -> bool {
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
            flat.push('\n');
        }
        flat.contains(needle)
    }

    /// Flatten one row of the buffer (symbols only).
    fn row_text(buf: &ratatui::buffer::Buffer, y: u16) -> String {
        let mut s = String::new();
        for x in 0..buf.area.width {
            s.push_str(buf[(x, y)].symbol());
        }
        s
    }

    #[test]
    fn render_buy_market_order_confirmation() {
        let mut form = FormState::new_default_form();
        form.payment_method = "SEPA".to_string();
        form.fiat_amount = "100".to_string();
        form.amount = "0".to_string();

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_order_confirm(f, &form, true))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Order Confirmation"));
        assert!(buffer_contains(buf, "Buy"));
        assert!(buffer_contains(buf, "USD"));
        assert!(buffer_contains(buf, "market"));
        assert!(buffer_contains(buf, "Reputation"));
        assert!(buffer_contains(buf, "YES"));
        assert!(buffer_contains(buf, "NO"));
    }

    #[test]
    fn render_sell_range_with_invoice() {
        let mut form = FormState::new_default_form();
        form.kind = "sell".to_string();
        form.payment_method = "Cash".to_string();
        form.fiat_amount = "50".to_string();
        form.fiat_amount_max = "150".to_string();
        form.use_range = true;
        form.amount = "10000".to_string();
        form.invoice = "lnbc1testinvoice".to_string();

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_order_confirm(f, &form, false))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Order Confirmation"));
        assert!(buffer_contains(buf, "Sell"));
        assert!(buffer_contains(buf, "USD"));
        assert!(buffer_contains(buf, "50-150"));
        assert!(buffer_contains(buf, "10000 sats"));
        assert!(buffer_contains(buf, "lnbc1testinvoice"));
        assert!(buffer_contains(buf, "YES"));
        assert!(buffer_contains(buf, "NO"));
    }

    #[test]
    fn render_full_privacy_on_confirmation() {
        let mut form = FormState::new_default_form();
        form.payment_method = "SEPA".to_string();
        form.fiat_amount = "100".to_string();
        form.full_privacy = true;

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_order_confirm(f, &form, true))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Full privacy"));
    }

    #[test]
    fn long_payment_method_stays_inside_popup_border() {
        let mut form = FormState::new_default_form();
        form.kind = "sell".to_string();
        form.fiat_code = "EUR".to_string();
        form.fiat_amount = "100".to_string();
        form.payment_method = format!(
            "HalCash, Bank Transfer, this is a very long text payment method to make a a test, {}",
            "bdsajdsajdsaoidjsadiosaisajfsoefjesdafjidofis"
        );

        let width = 80u16;
        let backend = TestBackend::new(width, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_order_confirm(f, &form, true))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Payment Method"));
        // Popup is centered at 3/4 of terminal width; its right border must not be
        // overwritten by payment-method text on any row.
        let popup_w = width.saturating_sub(width / 4);
        let popup_x = (width - popup_w) / 2;
        let right_border_x = popup_x + popup_w - 1;
        for y in 0..buf.area.height {
            let cell = &buf[(right_border_x, y)];
            let sym = cell.symbol();
            // Border glyphs or empty background — never a payment-method letter.
            if sym.chars().any(|c| c.is_ascii_alphanumeric()) {
                let row = row_text(buf, y);
                panic!("payment text escaped right border at y={y}: {row:?}");
            }
        }
    }
}
