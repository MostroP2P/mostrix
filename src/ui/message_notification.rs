use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

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

/// Renders AddBondInvoice (post-slash bond payout) notification popup.
fn render_add_bond_invoice(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
    invoice_state: &InvoiceInputState,
) {
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    )
    .split(popup);

    let order_id_str = helpers::format_order_id(notification.order_id);
    render_order_id_header(f, chunks[1], &order_id_str);
    render_message_preview(f, chunks[2], &notification.message_preview, false);
    if let Some(body) = notification.body.as_deref() {
        render_message_preview(f, chunks[3], body, true);
    }

    let amt: i64 = notification.sat_amount.unwrap_or_default();
    let input_label = format!("Paste your {} sats bond payout invoice:", amt);
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
            Span::styled(" to dismiss", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[9],
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
            Constraint::Length(1), // body (failed-payment explanation) or spacer
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
    if let Some(body) = notification.body.as_deref() {
        render_message_preview(f, chunks[3], body, true);
    }

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

/// Renders PayBondInvoice notification popup.
///
/// Mirrors `render_pay_invoice` but adds a yellow one-line explanation that the
/// bond sats are locked, not spent, and refunded on normal completion. Used for
/// the anti-abuse bond hold invoice that takers must pay before the trade flow
/// starts (Mostro daemon Phase 1.5+).
fn render_pay_bond_invoice(
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
            Constraint::Length(1), // bond explanatory note
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

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Locked, not spent — refunded on normal completion",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[3],
    );

    let amount_text = if notification.maker_bond_publish {
        if let Some(amount) = notification.sat_amount {
            format!("Pay bond to publish your order ({} sats):", amount)
        } else {
            "Pay bond to publish your order:".to_string()
        }
    } else if let Some(amount) = notification.sat_amount {
        format!("Bond invoice to pay ({} sats):", amount)
    } else {
        "Bond invoice to pay:".to_string()
    };

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            amount_text,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[5],
    );

    let invoice_area = create_input_area(chunks[6]);
    render_invoice_display(
        f,
        invoice_area,
        notification.invoice.as_ref(),
        invoice_state.scroll_y,
    );

    helpers::render_yes_no_buttons(
        f,
        chunks[8],
        matches!(
            invoice_state.action_selection,
            InvoiceNotificationActionSelection::Primary
        ),
        "Acknowledge",
        "Cancel Order",
    );

    if invoice_state.copied_to_clipboard {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "✓ Invoice copied to clipboard!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[9],
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
            chunks[9],
        );
    }

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
        chunks[10],
    );
}

/// Inset a rect horizontally so wrapped text does not touch popup borders.
fn inset_horizontal(area: Rect, pad: u16) -> Rect {
    if area.width <= pad.saturating_mul(2) {
        return area;
    }
    Rect {
        x: area.x.saturating_add(pad),
        y: area.y,
        width: area.width.saturating_sub(pad.saturating_mul(2)),
        height: area.height,
    }
}

/// Waiting-phase popup when the local user has no invoice/payment action yet.
fn render_waiting_phase_popup(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
    invoice_state: &InvoiceInputState,
) {
    let [inner] = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .areas(popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // order id
            Constraint::Length(1), // phase label
            Constraint::Length(1), // spacer
            Constraint::Length(5), // description (fixed height; avoids pushing buttons down)
            Constraint::Length(1), // spacer before buttons
            Constraint::Length(3), // action buttons
            Constraint::Length(1), // help text
        ],
    )
    .split(inner);

    let order_id_str = helpers::format_order_id(notification.order_id);
    render_order_id_header(f, chunks[0], &order_id_str);

    render_message_preview(f, chunks[1], &notification.message_preview, true);

    let body_text = notification
        .body
        .as_deref()
        .unwrap_or("Waiting for the counterparty. No action is required from you right now.");
    f.render_widget(
        Paragraph::new(body_text)
            .style(Style::default().fg(Color::White))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .alignment(ratatui::layout::Alignment::Center),
        inset_horizontal(chunks[3], 2),
    );

    helpers::render_yes_no_buttons(
        f,
        chunks[5],
        matches!(
            invoice_state.action_selection,
            InvoiceNotificationActionSelection::Primary
        ),
        "Ok",
        "Cancel Order",
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
        chunks[6],
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

/// Renders the informational `payment-failed` popup: order id, the retry
/// explanation carried in `notification.body`, and a dismiss hint. No input:
/// `payment-failed` is a notification only and Enter/Esc simply close it.
fn render_payment_failed(f: &mut ratatui::Frame, popup: Rect, notification: &MessageNotification) {
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(1), // order id
            Constraint::Length(1), // spacer
            Constraint::Min(3),    // wrapped retry explanation
            Constraint::Length(1), // spacer
            Constraint::Length(1), // dismiss hint
        ],
    )
    .split(popup);

    render_order_id_header(
        f,
        chunks[1],
        &helpers::format_order_id(notification.order_id),
    );

    let body = notification.body.clone().unwrap_or_else(|| {
        "Mostro could not pay your Lightning invoice. It will retry automatically.".to_string()
    });
    f.render_widget(
        Paragraph::new(body)
            .wrap(Wrap { trim: true })
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::White)),
        create_input_area(chunks[3]),
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
            Span::styled(" or ", Style::default()),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to dismiss", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[5],
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
        mostro_core::prelude::Action::AddInvoice => {
            // Extra height when explaining a post-retry replacement invoice.
            let height = if notification.body.is_some() { 21 } else { 19 };
            (90, height)
        }
        mostro_core::prelude::Action::AddBondInvoice => (90, 21),
        mostro_core::prelude::Action::PayInvoice => (90, 19),
        // Bond popup is one row taller for the "Locked, not spent" explanation line.
        mostro_core::prelude::Action::PayBondInvoice => (90, 20),
        mostro_core::prelude::Action::WaitingSellerToPay
        | mostro_core::prelude::Action::WaitingBuyerInvoice => (90, 16),
        // Informational payment-failed popup: wider/taller to fit the wrapped retry text.
        mostro_core::prelude::Action::PaymentFailed => (76, 13),
        _ => (70, 8),
    };

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let title = match action {
        mostro_core::prelude::Action::AddInvoice => {
            if notification.body.is_some() {
                "⚠️ New Invoice After Payment Failed"
            } else {
                "📝 Invoice Request"
            }
        }
        mostro_core::prelude::Action::AddBondInvoice => "⚔️ Bond Payout Invoice",
        mostro_core::prelude::Action::PayInvoice => "💳 Payment Request",
        mostro_core::prelude::Action::PayBondInvoice => "🛡️ Anti-abuse Bond Invoice",
        mostro_core::prelude::Action::WaitingSellerToPay
        | mostro_core::prelude::Action::WaitingBuyerInvoice => "📋 Trade Status",
        mostro_core::prelude::Action::PaymentFailed => "⚠️ Payment Failed",
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
        mostro_core::prelude::Action::AddBondInvoice => {
            render_add_bond_invoice(f, popup, notification, invoice_state);
        }
        mostro_core::prelude::Action::PayInvoice => {
            render_pay_invoice(f, popup, notification, invoice_state);
        }
        mostro_core::prelude::Action::PayBondInvoice => {
            render_pay_bond_invoice(f, popup, notification, invoice_state);
        }
        mostro_core::prelude::Action::WaitingSellerToPay
        | mostro_core::prelude::Action::WaitingBuyerInvoice => {
            render_waiting_phase_popup(f, popup, notification, invoice_state);
        }
        mostro_core::prelude::Action::PaymentFailed => {
            render_payment_failed(f, popup, notification);
        }
        _ => {
            render_default_notification(f, popup, notification);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render_message_notification;
    use crate::ui::{InvoiceInputState, InvoiceNotificationActionSelection, MessageNotification};
    use mostro_core::prelude::Action;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use uuid::Uuid;

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
        }
        flat
    }

    fn display_only_state() -> InvoiceInputState {
        InvoiceInputState {
            invoice_input: String::new(),
            focused: false,
            just_pasted: false,
            copied_to_clipboard: false,
            scroll_y: 0,
            action_selection: InvoiceNotificationActionSelection::Primary,
        }
    }

    #[test]
    fn payment_failed_popup_shows_retry_body_and_dismiss_hint() {
        let notification = MessageNotification {
            order_id: Some(Uuid::new_v4()),
            message_preview: "Payment Failed".to_string(),
            timestamp: 1,
            action: Action::PaymentFailed,
            sat_amount: None,
            invoice: None,
            body: Some(
                "Mostro could not pay your Lightning invoice. It will retry automatically \
                 up to 3 time(s), about 5 second(s) apart."
                    .to_string(),
            ),
            maker_bond_publish: false,
            solver_pubkey: None,
            dispute_id: None,
        };
        let state = display_only_state();

        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_message_notification(f, &notification, Action::PaymentFailed, &state))
            .unwrap();

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Payment Failed"), "title missing: {text}");
        assert!(
            text.contains("retry automatically"),
            "retry body missing: {text}"
        );
        assert!(text.contains("to dismiss"), "dismiss hint missing: {text}");
    }

    #[test]
    fn add_invoice_after_failed_payment_popup_shows_title_and_body() {
        let notification = MessageNotification {
            order_id: Some(Uuid::new_v4()),
            message_preview: "New Invoice After Failed Payment".to_string(),
            timestamp: 1,
            action: Action::AddInvoice,
            sat_amount: Some(1000),
            invoice: None,
            body: Some(
                "Previous Lightning payout failed after all retries. Paste a new invoice for your escrow sats."
                    .to_string(),
            ),
            maker_bond_publish: false,
            solver_pubkey: None,
            dispute_id: None,
        };
        let state = InvoiceInputState {
            invoice_input: String::new(),
            focused: true,
            just_pasted: false,
            copied_to_clipboard: false,
            scroll_y: 0,
            action_selection: InvoiceNotificationActionSelection::Primary,
        };

        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_message_notification(f, &notification, Action::AddInvoice, &state))
            .expect("draw");
        let text = buffer_text(terminal.backend().buffer());
        assert!(
            text.contains("New Invoice After Payment Failed"),
            "title should mark failed-payment invoice: {text}"
        );
        assert!(
            text.contains("retries") || text.contains("previous Lightning"),
            "body should explain retries: {text}"
        );
        assert!(text.contains("Submit Invoice"), "{text}");
    }
}
