use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{
    helpers, InvoiceInputState, InvoiceNotificationActionSelection, MessageNotification,
    BACKGROUND_COLOR, PRIMARY_COLOR,
};

/// Renders the order ID header in a notification popup
fn render_order_id_header(f: &mut ratatui::Frame, area: Rect, order_id_str: &str) {
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            order_id_str,
            Style::default()
                .bg(BACKGROUND_COLOR)
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        area,
    );
}

/// Renders the message preview text
fn render_message_preview(f: &mut ratatui::Frame, area: Rect, preview: &str, use_white_text: bool) {
    let style = if use_white_text {
        Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
    } else {
        Style::default().bg(BACKGROUND_COLOR)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(preview, style)]))
            .alignment(ratatui::layout::Alignment::Center),
        area,
    );
}

/// Creates an input area with proper margins
fn create_input_area(chunk: Rect) -> Rect {
    if chunk.width > 2 && chunk.height > 0 {
        Rect {
            x: chunk.x.saturating_add(1),
            y: chunk.y,
            width: chunk.width.saturating_sub(2),
            height: chunk.height,
        }
    } else {
        chunk
    }
}

/// Renders the invoice input field for AddInvoice
fn render_invoice_input(f: &mut ratatui::Frame, area: Rect, invoice_state: &InvoiceInputState) {
    let input_display = if invoice_state.invoice_input.is_empty() {
        "lnbc...".to_string()
    } else {
        invoice_state.invoice_input.clone()
    };

    let input_style = if invoice_state.focused {
        Style::default()
            .fg(PRIMARY_COLOR)
            .bg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    f.render_widget(
        Paragraph::new(input_display)
            .style(input_style)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(if invoice_state.focused {
                        Style::default().fg(PRIMARY_COLOR)
                    } else {
                        Style::default()
                    }),
            ),
        area,
    );
}

/// Renders the invoice display field for PayInvoice
fn render_invoice_display(
    f: &mut ratatui::Frame,
    area: Rect,
    invoice: Option<&String>,
    scroll_y: u16,
) {
    let (invoice_text, text_color) = match invoice {
        Some(inv) if !inv.is_empty() => (inv.clone(), Color::White),
        Some(_) => (
            "⚠️  Invoice not available (empty)".to_string(),
            Color::Yellow,
        ),
        None => ("⚠️  Invoice not available".to_string(), Color::Yellow),
    };

    f.render_widget(
        Paragraph::new(invoice_text)
            .style(Style::default().fg(text_color).add_modifier(Modifier::BOLD))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .scroll((scroll_y, 0))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().fg(PRIMARY_COLOR)),
            ),
        area,
    );
}

/// Renders AddInvoice notification popup
fn render_add_invoice(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
    invoice_state: &InvoiceInputState,
) {
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(1), // order id
            Constraint::Length(1), // message preview
            Constraint::Length(1), // spacer
            Constraint::Length(1), // label
            Constraint::Length(6), // invoice input field
            Constraint::Length(1), // spacer
            Constraint::Length(3), // action buttons
            Constraint::Length(1), // help text (navigation)
            Constraint::Length(1), // help text (paste/dismiss)
        ],
    )
    .split(popup);

    let order_id_str = helpers::format_order_id(notification.order_id);
    render_order_id_header(f, chunks[1], &order_id_str);
    render_message_preview(f, chunks[2], &notification.message_preview, false);

    let amt: i64 = notification.sat_amount.unwrap_or_default();
    let input_label = format!("Paste your {} sats Lightning invoice:", amt);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            input_label,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );

    let input_area = create_input_area(chunks[5]);
    render_invoice_input(f, input_area, invoice_state);

    helpers::render_yes_no_buttons(
        f,
        chunks[7],
        matches!(
            invoice_state.action_selection,
            InvoiceNotificationActionSelection::Primary
        ),
        "Submit Invoice",
        "Cancel Order",
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Use ", Style::default()),
            Span::styled(
                "Left/Right",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to select action, ", Style::default()),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to confirm", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[8],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Paste invoice (right-click / ", Style::default()),
            Span::styled(
                "Shift+Insert",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" / ", Style::default()),
            Span::styled(
                "Ctrl+Shift+V",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("), ", Style::default()),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" dismiss", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[9],
    );
}

/// Renders PayInvoice notification popup
fn render_pay_invoice(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
    invoice_state: &InvoiceInputState,
) {
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(1), // order id
            Constraint::Length(1), // message preview
            Constraint::Length(1), // spacer
            Constraint::Length(1), // label
            Constraint::Length(6), // invoice display field
            Constraint::Length(1), // spacer
            Constraint::Length(3), // action buttons
            Constraint::Length(1), // help text line 1
            Constraint::Length(1), // help text line 2
        ],
    )
    .split(popup);

    let order_id_str = helpers::format_order_id(notification.order_id);
    render_order_id_header(f, chunks[1], &order_id_str);
    render_message_preview(f, chunks[2], &notification.message_preview, true);

    let amount_text = if let Some(amount) = notification.sat_amount {
        format!("Lightning invoice to pay ({} sats):", amount)
    } else {
        "Lightning invoice to pay:".to_string()
    };

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            amount_text,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );

    let invoice_area = create_input_area(chunks[5]);
    render_invoice_display(
        f,
        invoice_area,
        notification.invoice.as_ref(),
        invoice_state.scroll_y,
    );

    helpers::render_yes_no_buttons(
        f,
        chunks[7],
        matches!(
            invoice_state.action_selection,
            InvoiceNotificationActionSelection::Primary
        ),
        "Acknowledge",
        "Cancel Order",
    );

    // Help text - first line
    if invoice_state.copied_to_clipboard {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "✓ Invoice copied to clipboard!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[8],
        );
    } else {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Press ", Style::default()),
                Span::styled(
                    "C",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to copy invoice to clipboard. ", Style::default()),
                Span::styled(
                    "↑/↓",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" scroll, ", Style::default()),
                Span::styled(
                    "Left/Right",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" select action", Style::default()),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[8],
        );
    }

    // Help text - second line
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to confirm, ", Style::default()),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to dismiss", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[9],
    );
}

/// Renders default notification popup for other actions
fn render_default_notification(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
) {
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(1), // order id
            Constraint::Length(1), // message preview
            Constraint::Length(1), // spacer
            Constraint::Length(1), // help text
        ],
    )
    .split(popup);

    let order_id_str = helpers::format_order_id(notification.order_id);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            order_id_str,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            &notification.message_preview,
            Style::default(),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[2],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to view, ", Style::default()),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to dismiss", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );
}

/// Main function to render message notification popup
pub fn render_message_notification(
    f: &mut ratatui::Frame,
    notification: &MessageNotification,
    action: mostro_core::prelude::Action,
    invoice_state: &InvoiceInputState,
) {
    let area = f.area();
    let (popup_width, popup_height) = match action {
        mostro_core::prelude::Action::AddInvoice | mostro_core::prelude::Action::PayInvoice => {
            (90, 19)
        }
        _ => (70, 8),
    };

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let title = match action {
        mostro_core::prelude::Action::AddInvoice => "📝 Invoice Request",
        mostro_core::prelude::Action::PayInvoice => "💳 Payment Request",
        _ => "📨 New Message",
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    match action {
        mostro_core::prelude::Action::AddInvoice => {
            render_add_invoice(f, popup, notification, invoice_state);
        }
        mostro_core::prelude::Action::PayInvoice => {
            render_pay_invoice(f, popup, notification, invoice_state);
        }
        _ => {
            render_default_notification(f, popup, notification);
        }
    }
}
