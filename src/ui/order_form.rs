use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::{FormState, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_order_form(f: &mut ratatui::Frame, area: Rect, form: &FormState) {
    // Calculate number of fields dynamically
    let field_count = if form.use_range { 10 } else { 9 };
    let mut constraints = vec![Constraint::Length(1)]; // spacer
    for _ in 0..field_count {
        constraints.push(Constraint::Length(3));
    }
    constraints.push(Constraint::Length(1)); // hint

    let inner_chunks = Layout::new(Direction::Vertical, constraints).split(area);

    let block = Block::default()
        .title("✨ Create New Order")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, area);

    let mut field_idx = 1;

    // Field 0: Tipo (toggle buy/sell)
    let tipo_title = Block::default()
        .title(Line::from(vec![
            Span::styled("📈 ", Style::default().fg(PRIMARY_COLOR)),
            Span::styled("Order Type", Style::default().add_modifier(Modifier::BOLD)),
        ]))
        .borders(Borders::ALL)
        .style(if form.focused == 0 {
            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
        } else {
            Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
        });
    let is_buy = form.kind.to_lowercase() == "buy";
    let tipo_line = if is_buy {
        Line::from(vec![Span::styled(
            "🟢 [ buy ]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )])
    } else {
        Line::from(vec![Span::styled(
            "🔴 [ sell ]",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )])
    };
    f.render_widget(
        Paragraph::new(tipo_line).block(tipo_title),
        inner_chunks[field_idx],
    );
    field_idx += 1;

    // Field 1: Currency
    let valuta = Paragraph::new(Line::from(form.fiat_code.clone())).block(
        Block::default()
            .title(Line::from(vec![
                Span::styled("💱 ", Style::default().fg(Color::Cyan)),
                Span::styled("Currency", Style::default().add_modifier(Modifier::BOLD)),
            ]))
            .borders(Borders::ALL)
            .style(if form.focused == 1 {
                Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
            } else {
                Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
            }),
    );
    f.render_widget(valuta, inner_chunks[field_idx]);
    field_idx += 1;

    // Field 2: Amount (sats)
    let amount = Paragraph::new(Line::from(form.amount.clone())).block(
        Block::default()
            .title(Line::from(vec![
                Span::styled("₿ ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    "Amount (sats)",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]))
            .borders(Borders::ALL)
            .style(if form.focused == 2 {
                Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
            } else {
                Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
            }),
    );
    f.render_widget(amount, inner_chunks[field_idx]);
    field_idx += 1;

    // Field 3: Fiat Amount (min or single)
    let fiat_title = if form.use_range {
        "Fiat Amount (Min)"
    } else {
        "Fiat Amount"
    };
    let qty = Paragraph::new(Line::from(form.fiat_amount.clone())).block(
        Block::default()
            .title(Line::from(vec![
                Span::styled("💰 ", Style::default().fg(Color::Yellow)),
                Span::styled(fiat_title, Style::default().add_modifier(Modifier::BOLD)),
            ]))
            .borders(Borders::ALL)
            .style(if form.focused == 3 {
                Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
            } else {
                Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
            }),
    );
    f.render_widget(qty, inner_chunks[field_idx]);
    field_idx += 1;

    // Field 4: Fiat Amount Max (if range)
    if form.use_range {
        let qty_max = Paragraph::new(Line::from(form.fiat_amount_max.clone())).block(
            Block::default()
                .title(Line::from(vec![
                    Span::styled("💰 ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        "Fiat Amount (Max)",
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]))
                .borders(Borders::ALL)
                .style(if form.focused == 4 {
                    Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                } else {
                    Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                }),
        );
        f.render_widget(qty_max, inner_chunks[field_idx]);
        field_idx += 1;
    }

    // Field 5: Payment Method
    let pm = Paragraph::new(Line::from(form.payment_method.clone())).block(
        Block::default()
            .title(Line::from(vec![
                Span::styled("💳 ", Style::default().fg(Color::Magenta)),
                Span::styled(
                    "Payment Method",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]))
            .borders(Borders::ALL)
            .style(if form.focused == 5 {
                Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
            } else {
                Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
            }),
    );
    f.render_widget(pm, inner_chunks[field_idx]);
    field_idx += 1;

    // Field 6: Premium
    let premium = Paragraph::new(Line::from(form.premium.clone())).block(
        Block::default()
            .title(Line::from(vec![
                Span::styled("📈 ", Style::default().fg(Color::Green)),
                Span::styled("Premium (%)", Style::default().add_modifier(Modifier::BOLD)),
            ]))
            .borders(Borders::ALL)
            .style(if form.focused == 6 {
                Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
            } else {
                Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
            }),
    );
    f.render_widget(premium, inner_chunks[field_idx]);
    field_idx += 1;

    // Field 7: Invoice (optional)
    let invoice = Paragraph::new(Line::from(form.invoice.clone())).block(
        Block::default()
            .title(Line::from(vec![
                Span::styled("🧾 ", Style::default().fg(Color::Blue)),
                Span::styled(
                    "Invoice (optional)",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]))
            .borders(Borders::ALL)
            .style(if form.focused == 7 {
                Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
            } else {
                Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
            }),
    );
    f.render_widget(invoice, inner_chunks[field_idx]);
    field_idx += 1;

    // Field 8: Expiration Days
    let exp = Paragraph::new(Line::from(form.expiration_days.clone())).block(
        Block::default()
            .title(Line::from(vec![
                Span::styled("⏰ ", Style::default().fg(Color::Red)),
                Span::styled(
                    "Expiration (days, 0=none)",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]))
            .borders(Borders::ALL)
            .style(if form.focused == 8 {
                Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
            } else {
                Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
            }),
    );
    f.render_widget(exp, inner_chunks[field_idx]);
    field_idx += 1;

    // Footer hint
    let hint = Paragraph::new(Line::from(vec![
        Span::styled("💡 ", Style::default().fg(Color::Cyan)),
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" submit • "),
        Span::styled(
            "Tab",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" focus • "),
        Span::styled(
            "Space",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" toggle type/range • "),
        Span::styled(
            "Esc",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" cancel"),
    ]))
    .block(Block::default());
    f.render_widget(hint, inner_chunks[field_idx]);

    // Show cursor in active text field
    let cursor_field = match form.focused {
        1 => Some((inner_chunks[2], &form.fiat_code)),
        2 => Some((inner_chunks[3], &form.amount)),
        3 => Some((inner_chunks[4], &form.fiat_amount)),
        4 if form.use_range => Some((inner_chunks[5], &form.fiat_amount_max)),
        5 => Some((
            inner_chunks[if form.use_range { 6 } else { 5 }],
            &form.payment_method,
        )),
        6 => Some((
            inner_chunks[if form.use_range { 7 } else { 6 }],
            &form.premium,
        )),
        7 => Some((
            inner_chunks[if form.use_range { 8 } else { 7 }],
            &form.invoice,
        )),
        8 => Some((
            inner_chunks[if form.use_range { 9 } else { 8 }],
            &form.expiration_days,
        )),
        _ => None,
    };
    if let Some((chunk, text)) = cursor_field {
        let x = chunk.x + 1 + text.len() as u16;
        let y = chunk.y + 1;
        f.set_cursor_position((x, y));
    }
}

pub fn render_form_initializing(f: &mut ratatui::Frame, area: Rect) {
    let paragraph = Paragraph::new(Span::raw("Initializing form...")).block(
        Block::default()
            .title("Create New Order")
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR)),
    );
    f.render_widget(paragraph, area);
}
