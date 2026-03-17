use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::{FormState, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_order_form(f: &mut ratatui::Frame, area: Rect, form: &FormState) {
    // Calculate number of fields dynamically
    let field_count = if form.use_range { 10 } else { 9 };
    // Start with a top spacer so the form doesn't hug the frame border
    let mut constraints = vec![Constraint::Length(2)]; // spacer
    for _ in 0..field_count {
        // Give each field a bit more vertical space to improve readability
        constraints.push(Constraint::Length(4));
    }
    // Slightly taller row for the footer hint
    constraints.push(Constraint::Length(2)); // hint

    // Outer frame for the whole tab
    let block = Block::default()
        .title("✨ Create New Order")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(&block, area);

    // Work inside the inner area of the frame
    let inner = block.inner(area);

    // Horizontal layout: spacer | centered form | help panel
    let h_chunks = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Percentage(10),
            Constraint::Min(40),
            Constraint::Percentage(30),
        ],
    )
    .split(inner);

    let form_area = h_chunks[1];
    let help_area = h_chunks[2];

    // Vertical layout for the form fields inside the centered column
    let inner_chunks = Layout::new(Direction::Vertical, constraints).split(form_area);

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

    // Field 3: Fiat Amount (toggle single/range with Space)
    let fiat_title_block = Block::default()
        .title(Line::from(vec![
            Span::styled("💰 ", Style::default().fg(Color::Yellow)),
            Span::styled("Fiat Amount", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(" (Space to toggle)", Style::default().fg(Color::DarkGray)),
        ]))
        .borders(Borders::ALL)
        .style(if form.focused == 3 {
            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
        } else {
            Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
        });

    // Show toggle indicator and value
    let fiat_line = if form.use_range {
        Line::from(vec![
            Span::styled(
                "[ Range ] ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(&form.fiat_amount),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "[ Single ] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(&form.fiat_amount),
        ])
    };
    f.render_widget(
        Paragraph::new(fiat_line).block(fiat_title_block),
        inner_chunks[field_idx],
    );
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

    // Footer hint (still in the form column)
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

    // Contextual help panel on the right side
    let help_lines = build_field_help(form);
    let help_paragraph = Paragraph::new(help_lines)
        .block(
            Block::default()
                .title("Field Help")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(help_paragraph, help_area);

    // Show cursor in active text field
    let cursor_field = match form.focused {
        1 => Some((inner_chunks[2], &form.fiat_code, 0)),
        2 => Some((inner_chunks[3], &form.amount, 0)),
        3 => Some((inner_chunks[4], &form.fiat_amount, 11)), // 11 chars for "[ Single ] " or "[ Range ] "
        4 if form.use_range => Some((inner_chunks[5], &form.fiat_amount_max, 0)),
        5 => Some((
            inner_chunks[if form.use_range { 6 } else { 5 }],
            &form.payment_method,
            0,
        )),
        6 => Some((
            inner_chunks[if form.use_range { 7 } else { 6 }],
            &form.premium,
            0,
        )),
        7 => Some((
            inner_chunks[if form.use_range { 8 } else { 7 }],
            &form.invoice,
            0,
        )),
        8 => Some((
            inner_chunks[if form.use_range { 9 } else { 8 }],
            &form.expiration_days,
            0,
        )),
        _ => None,
    };
    if let Some((chunk, text, offset)) = cursor_field {
        let x = chunk.x + 1 + offset + text.len() as u16;
        let y = chunk.y + 1;
        f.set_cursor_position((x, y));
    }
}

fn build_field_help(form: &FormState) -> Vec<Line<'static>> {
    match form.focused {
        0 => vec![
            Line::from("Order Type"),
            Line::from("Choose whether you want to buy or sell bitcoin."),
            Line::from("Use Space to toggle between buy and sell orders."),
        ],
        1 => vec![
            Line::from("Currency"),
            Line::from("Enter the fiat currency code (e.g. USD, EUR)."),
            Line::from("It must be one of the currencies accepted by the Mostro instance."),
        ],
        2 => vec![
            Line::from("Amount (sats)"),
            Line::from("Amount in satoshis you want to trade."),
            Line::from("Set to 0 to create a market order, so the order will be executed at the current market price."),
        ],
        3 => vec![
            Line::from("Fiat Amount"),
            Line::from("Price of the order in fiat currency (e.g. USD, EUR, ARS, etc.)."),
            Line::from("Use Space to toggle between a single amount and a range (e.g. 100-200 USD)."),
        ],
        4 if form.use_range => vec![
            Line::from("Fiat Amount (Max)"),
            Line::from("Upper bound of the fiat amount range."),
            Line::from("Leave narrow if you only need a rough upper limit."),
        ],
        5 => vec![
            Line::from("Payment Method"),
            Line::from("Describe how you want to receive or send fiat."),
            Line::from("Use a short but recognizable label (e.g. SEPA, Bizum)."),
        ],
        6 => vec![
            Line::from("Premium (%)"),
            Line::from("Markup or discount relative to the reference price."),
            Line::from("Positive values are a premium, negative values a discount."),
        ],
        7 => vec![
            Line::from("Invoice (optional)"),
            Line::from("Pre-generated Lightning invoice, if applicable."),
            Line::from("You can leave this empty for Mostro to handle invoices."),
        ],
        8 => vec![
            Line::from("Expiration (days)"),
            Line::from("How long the order should remain active."),
            Line::from("Use 0 for no expiration."),
        ],
        _ => vec![
            Line::from("Create New Order"),
            Line::from("Fill the fields on the left and press Enter to submit."),
        ],
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
