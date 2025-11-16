use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::{FormState, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_order_confirm(f: &mut ratatui::Frame, form: &FormState) {
    let area = f.area();
    let popup_width = area.width.saturating_sub(area.width / 4);
    let popup_height = 20;
    let popup_x = area.x + (area.width - popup_width) / 2;
    let popup_y = area.y + (area.height - popup_height) / 2;
    let popup = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

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
            Constraint::Length(1), // separator
            Constraint::Length(1), // confirmation prompt
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
            Span::styled(&form.fiat_code, Style::default().fg(PRIMARY_COLOR)),
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

    let exp_str = if form.expiration_days.is_empty() || form.expiration_days == "0" {
        "No expiration".to_string()
    } else {
        format!("{} days", form.expiration_days)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Expiration: "),
            Span::styled(exp_str, Style::default().fg(PRIMARY_COLOR)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[10],
    );

    // Confirmation prompt
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Y",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to confirm or "),
            Span::styled(
                "N",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to cancel"),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[12],
    );
}
