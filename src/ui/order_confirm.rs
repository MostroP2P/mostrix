use ratatui::layout::{Constraint, Direction, Flex, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{helpers, FormState, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::currencies;

pub fn render_order_confirm(f: &mut ratatui::Frame, form: &FormState, selected_button: bool) {
    let area = f.area();
    let popup_width = area.width.saturating_sub(area.width / 4);
    let popup_height = 20;
    // Center the popup using Flex::Center
    let popup = {
        let [popup] = Layout::horizontal([Constraint::Length(popup_width)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(popup_height)])
            .flex(Flex::Center)
            .areas(popup);
        popup
    };

    // Clear the popup area to make it fully opaque
    f.render_widget(Clear, popup);

    let inner_chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(2), // title
            Constraint::Length(1), // separator
            Constraint::Length(1), // kind
            Constraint::Length(1), // currency
            Constraint::Length(1), // amount
            Constraint::Length(1), // fiat amount
            Constraint::Length(1), // payment method
            Constraint::Length(1), // premium
            Constraint::Length(1), // invoice (if present)
            Constraint::Length(1), // expiration
            Constraint::Length(3), // buttons
            Constraint::Length(1), // help text
        ],
    )
    .split(popup);

    let block = Block::default()
        .title("📋 Order Confirmation")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    // Title
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Please review your order:",
            Style::default().add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[1],
    );

    // Order details (centered)
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
        Paragraph::new(Line::from(vec![
            Span::raw("Payment Method: "),
            Span::styled(&form.payment_method, Style::default().fg(PRIMARY_COLOR)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[7],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Premium: "),
            Span::styled(
                format!("{}%", form.premium),
                Style::default().fg(PRIMARY_COLOR),
            ),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[8],
    );

    if !form.invoice.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("Invoice: "),
                Span::styled(&form.invoice, Style::default().fg(PRIMARY_COLOR)),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            inner_chunks[9],
        );
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
        inner_chunks[10],
    );

    // YES/NO buttons
    helpers::render_yes_no_buttons(f, inner_chunks[11], selected_button, "✓ YES", "✗ NO");

    // Help text: use Enter/Esc for confirmation
    helpers::render_help_text(
        f,
        inner_chunks[12],
        "Press ",
        "Enter",
        " to confirm, Esc to cancel",
    );
}
