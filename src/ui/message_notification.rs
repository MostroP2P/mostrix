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

/// QR or scrollable bolt11 text for the PayInvoice / PayBondInvoice region.
enum InvoiceVisual {
    Qr(helpers::QrView),
    Text { fallback_hint: bool },
}

struct PayInvoiceLayout {
    popup_width: u16,
    popup_height: u16,
    invoice_rows: u16,
    visual: InvoiceVisual,
    /// QR always; text when the full 3-row-button card does not fit.
    compact: bool,
}

impl PayInvoiceLayout {
    fn showing_qr(&self) -> bool {
        matches!(self.visual, InvoiceVisual::Qr(_))
    }

    fn use_compact(&self) -> bool {
        self.showing_qr() || self.compact
    }

    fn qr_fallback(&self) -> bool {
        matches!(
            self.visual,
            InvoiceVisual::Text {
                fallback_hint: true
            }
        )
    }
}

fn invoice_visual(
    invoice: Option<&str>,
    show_qr: bool,
    max_width: u16,
    max_height: u16,
) -> InvoiceVisual {
    if show_qr {
        if let Some(raw) = invoice.filter(|s| !s.is_empty()) {
            let payload = helpers::qr_payload(raw);
            if let Some(view) = helpers::encode_qr_fitting(&payload, max_width, max_height) {
                return InvoiceVisual::Qr(view);
            }
            return InvoiceVisual::Text {
                fallback_hint: true,
            };
        }
    }
    InvoiceVisual::Text {
        fallback_hint: false,
    }
}

/// Size the pay popup from the QR matrix when it fits; otherwise keep the text card.
fn pay_invoice_popup_layout(
    area: Rect,
    invoice: Option<&str>,
    show_qr: bool,
    bond: bool,
) -> PayInvoiceLayout {
    const PREFERRED_WIDTH: u16 = 90;
    const TEXT_INVOICE_ROWS: u16 = 6;
    const TEXT_HEIGHT_PAY: u16 = 19;
    const TEXT_HEIGHT_BOND: u16 = 20;
    // Compact chrome: spacer, [bond note], amount, Ack/Cancel strip, help.
    // Order id lives in the Block title so the strip can replace that row.
    const QR_CHROME_PAY: u16 = 4;
    const QR_CHROME_BOND: u16 = 5;
    const TEXT_FIXED_PAY: u16 = 11;
    const TEXT_FIXED_BOND: u16 = 12;

    let chrome = if bond { QR_CHROME_BOND } else { QR_CHROME_PAY };
    let max_qr_w = area.width.saturating_sub(4);
    let max_qr_h = area.height.saturating_sub(chrome);
    let visual = invoice_visual(invoice, show_qr, max_qr_w, max_qr_h);

    match visual {
        InvoiceVisual::Qr(view) => {
            // Size the card to the code + help line, not a 90-col slab.
            const QR_MIN_WIDTH: u16 = 64;
            let popup_width = QR_MIN_WIDTH
                .max(view.width.saturating_add(4))
                .min(area.width);
            let popup_height = chrome.saturating_add(view.height).min(area.height).max(6);
            PayInvoiceLayout {
                popup_width,
                popup_height,
                invoice_rows: view.height,
                visual: InvoiceVisual::Qr(view),
                compact: true,
            }
        }
        visual @ InvoiceVisual::Text { .. } => {
            let preferred_h = if bond {
                TEXT_HEIGHT_BOND
            } else {
                TEXT_HEIGHT_PAY
            };
            let full_fixed = if bond {
                TEXT_FIXED_BOND
            } else {
                TEXT_FIXED_PAY
            };
            let popup_height = preferred_h.min(area.height).max(6);
            let compact = popup_height < full_fixed.saturating_add(TEXT_INVOICE_ROWS);
            let invoice_rows = if compact {
                let chrome = if bond { QR_CHROME_BOND } else { QR_CHROME_PAY };
                popup_height.saturating_sub(chrome).max(1)
            } else {
                TEXT_INVOICE_ROWS
            };
            PayInvoiceLayout {
                popup_width: PREFERRED_WIDTH.min(area.width),
                popup_height,
                invoice_rows,
                visual,
                compact,
            }
        }
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

/// Draws a half-block QR, or the wrapped bolt11 when QR is off or does not fit.
fn render_invoice_display(
    f: &mut ratatui::Frame,
    area: Rect,
    invoice: Option<&String>,
    scroll_y: u16,
    visual: &InvoiceVisual,
) {
    match visual {
        InvoiceVisual::Qr(view) => {
            let qr_area = Rect {
                x: area.x + area.width.saturating_sub(view.width) / 2,
                y: area.y + area.height.saturating_sub(view.height) / 2,
                width: view.width.min(area.width),
                height: view.height.min(area.height),
            };
            f.render_widget(Paragraph::new(view.lines.clone()), qr_area);
        }
        InvoiceVisual::Text { fallback_hint } => {
            let (invoice_text, text_color) = match invoice {
                Some(inv) if !inv.is_empty() => (inv.clone(), Color::White),
                Some(_) => (
                    "⚠️  Invoice not available (empty)".to_string(),
                    Color::Yellow,
                ),
                None => ("⚠️  Invoice not available".to_string(), Color::Yellow),
            };

            let mut block = Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(PRIMARY_COLOR));
            if *fallback_hint {
                block = block.title("QR needs a taller terminal");
            }

            f.render_widget(
                Paragraph::new(invoice_text)
                    .style(Style::default().fg(text_color).add_modifier(Modifier::BOLD))
                    .wrap(ratatui::widgets::Wrap { trim: true })
                    .scroll((scroll_y, 0))
                    .block(block),
                area,
            );
        }
    }
}

fn render_pay_help(
    f: &mut ratatui::Frame,
    help1: Rect,
    help2: Rect,
    invoice_state: &InvoiceInputState,
    showing_qr: bool,
    qr_fallback: bool,
) {
    if invoice_state.copied_to_clipboard {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "✓ Invoice copied to clipboard!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            help1,
        );
    } else if qr_fallback {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("QR needs a taller terminal. Press ", Style::default()),
                Span::styled(
                    "C",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to copy, ", Style::default()),
                Span::styled(
                    "↑/↓",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" scroll", Style::default()),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            help1,
        );
    } else {
        let mut spans = vec![
            Span::styled("Press ", Style::default()),
            Span::styled(
                "C",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to copy. ", Style::default()),
            Span::styled(
                "SPACE",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if showing_qr {
                    " for text. "
                } else {
                    " for QR. "
                },
                Style::default(),
            ),
        ];
        if showing_qr {
            spans.push(Span::styled(
                "Left/Right",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(" select action", Style::default()));
        } else {
            spans.push(Span::styled(
                "↑/↓",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(" scroll, ", Style::default()));
            spans.push(Span::styled(
                "Left/Right",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(" select action", Style::default()));
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).alignment(ratatui::layout::Alignment::Center),
            help1,
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
        help2,
    );
}

fn pay_amount_label(notification: &MessageNotification, bond: bool) -> String {
    if bond {
        if notification.maker_bond_publish {
            if let Some(amount) = notification.sat_amount {
                format!("Pay bond to publish your order ({} sats):", amount)
            } else {
                "Pay bond to publish your order:".to_string()
            }
        } else if let Some(amount) = notification.sat_amount {
            format!("Bond invoice to pay ({} sats):", amount)
        } else {
            "Bond invoice to pay:".to_string()
        }
    } else if let Some(amount) = notification.sat_amount {
        format!("Lightning invoice to pay ({} sats):", amount)
    } else {
        "Lightning invoice to pay:".to_string()
    }
}

fn render_centered_label(f: &mut ratatui::Frame, area: Rect, text: &str, color: Color) {
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            text,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        area,
    );
}

/// Compact QR (or short-terminal text) layout: order id is in the title.
fn render_pay_qr_compact(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
    invoice_state: &InvoiceInputState,
    layout: &PayInvoiceLayout,
    bond: bool,
) {
    let mut constraints = vec![
        Constraint::Length(1), // spacer / top border
    ];
    if bond {
        constraints.push(Constraint::Length(1)); // locked note
    }
    constraints.push(Constraint::Length(1)); // amount
    constraints.push(Constraint::Length(layout.invoice_rows));
    constraints.push(Constraint::Length(1)); // ack/cancel strip
    constraints.push(Constraint::Length(1)); // help

    let chunks = Layout::new(Direction::Vertical, constraints).split(popup);
    let mut idx = 1;
    if bond {
        render_centered_label(
            f,
            chunks[idx],
            "Locked, not spent — refunded on normal completion",
            Color::Yellow,
        );
        idx += 1;
    }
    render_centered_label(
        f,
        chunks[idx],
        &pay_amount_label(notification, bond),
        PRIMARY_COLOR,
    );
    idx += 1;
    render_invoice_display(
        f,
        create_input_area(chunks[idx]),
        notification.invoice.as_ref(),
        invoice_state.scroll_y,
        &layout.visual,
    );
    idx += 1;
    helpers::render_compact_action_strip(
        f,
        chunks[idx],
        matches!(
            invoice_state.action_selection,
            InvoiceNotificationActionSelection::Primary
        ),
        "Acknowledge",
        "Cancel Order",
    );
    idx += 1;
    render_pay_qr_help(
        f,
        chunks[idx],
        invoice_state,
        layout.showing_qr(),
        layout.qr_fallback(),
    );
}

fn render_pay_qr_help(
    f: &mut ratatui::Frame,
    area: Rect,
    invoice_state: &InvoiceInputState,
    showing_qr: bool,
    qr_fallback: bool,
) {
    if invoice_state.copied_to_clipboard {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "✓ Invoice copied to clipboard!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            area,
        );
        return;
    }
    let mut spans = Vec::new();
    if qr_fallback {
        spans.push(Span::styled(
            "QR needs a taller terminal. ",
            Style::default(),
        ));
    }
    spans.extend([
        Span::styled(
            "C",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" copy  ", Style::default()),
        Span::styled(
            "SPACE",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if showing_qr { " text  " } else { " QR  " },
            Style::default(),
        ),
        Span::styled(
            "X",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel  ", Style::default()),
        Span::styled(
            "←/→",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            "Enter",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ack  ", Style::default()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" dismiss", Style::default()),
    ]);
    f.render_widget(
        Paragraph::new(Line::from(spans)).alignment(ratatui::layout::Alignment::Center),
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

/// Renders AddInvoice notification popup.
///
/// When `notification.body` is set (post-retry replacement invoice), skips the
/// redundant preview and uses `body_rows` / `input_rows` from
/// [`add_invoice_popup_layout`] so the explanation can wrap without a fixed clip.
fn render_add_invoice(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
    invoice_state: &InvoiceInputState,
    body_rows: u16,
    input_rows: u16,
) {
    // Post-retry replacement invoice carries a body: skip the redundant preview
    // (title already says it) and wrap the explanation into `body_rows`.
    let has_failed_payment_body = notification.body.is_some() && body_rows > 0;
    let chunks = if has_failed_payment_body {
        Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(1),          // spacer
                Constraint::Length(1),          // order id
                Constraint::Length(body_rows),  // wrapped explanation
                Constraint::Length(1),          // spacer
                Constraint::Length(1),          // label
                Constraint::Length(input_rows), // invoice input field
                Constraint::Length(1),          // spacer
                Constraint::Length(3),          // action buttons
                Constraint::Length(1),          // help text (navigation)
                Constraint::Length(1),          // help text (paste/dismiss)
            ],
        )
        .split(popup)
    } else {
        Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(1),          // spacer
                Constraint::Length(1),          // order id
                Constraint::Length(1),          // message preview
                Constraint::Length(1),          // spacer
                Constraint::Length(1),          // label
                Constraint::Length(input_rows), // invoice input field
                Constraint::Length(1),          // spacer
                Constraint::Length(3),          // action buttons
                Constraint::Length(1),          // help text (navigation)
                Constraint::Length(1),          // help text (paste/dismiss)
            ],
        )
        .split(popup)
    };

    let order_id_str = helpers::format_order_id(notification.order_id);
    render_order_id_header(f, chunks[1], &order_id_str);

    if has_failed_payment_body {
        if let Some(body) = notification.body.as_deref() {
            f.render_widget(
                Paragraph::new(body)
                    .wrap(Wrap { trim: true })
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::White)),
                inset_horizontal(chunks[2], 2),
            );
        }
    } else {
        render_message_preview(f, chunks[2], &notification.message_preview, false);
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

/// Renders the PayInvoice popup: half-block QR when it fits, otherwise the text card.
fn render_pay_invoice(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
    invoice_state: &InvoiceInputState,
    layout: &PayInvoiceLayout,
) {
    if layout.use_compact() {
        render_pay_qr_compact(f, popup, notification, invoice_state, layout, false);
        return;
    }
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(1), // order id
            Constraint::Length(1), // message preview
            Constraint::Length(1), // spacer
            Constraint::Length(1), // label
            Constraint::Length(layout.invoice_rows),
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
        &layout.visual,
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

    render_pay_help(
        f,
        chunks[8],
        chunks[9],
        invoice_state,
        layout.showing_qr(),
        layout.qr_fallback(),
    );
}

/// Renders PayBondInvoice notification popup.
///
/// Mirrors `render_pay_invoice` (half-block QR, or text when the code does not
/// fit) and adds a yellow one-line explanation that the bond sats are locked,
/// not spent, and refunded on normal completion. Used for the anti-abuse bond
/// hold invoice that takers must pay before the trade flow starts (Mostro
/// daemon Phase 1.5+).
fn render_pay_bond_invoice(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
    invoice_state: &InvoiceInputState,
    layout: &PayInvoiceLayout,
) {
    if layout.use_compact() {
        render_pay_qr_compact(f, popup, notification, invoice_state, layout, true);
        return;
    }
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(1), // order id
            Constraint::Length(1), // message preview
            Constraint::Length(1), // bond explanatory note
            Constraint::Length(1), // spacer
            Constraint::Length(1), // label
            Constraint::Length(layout.invoice_rows),
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
        &layout.visual,
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

    render_pay_help(
        f,
        chunks[9],
        chunks[10],
        invoice_state,
        layout.showing_qr(),
        layout.qr_fallback(),
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

/// Columns available for wrapped body text inside a bordered popup with horizontal inset.
fn body_text_width(popup_width: u16, horizontal_inset: u16) -> u16 {
    popup_width
        .saturating_sub(2) // block borders
        .saturating_sub(horizontal_inset.saturating_mul(2))
        .max(1)
}

/// How many rows a notification body needs at `width`, honoring hard `\n` breaks.
fn count_wrapped_body_rows(body: &str, width: u16) -> u16 {
    let width = width.max(1);
    let mut total = 0u16;
    for segment in body.split('\n') {
        let rows = if segment.trim().is_empty() {
            1
        } else {
            helpers::wrap_text_to_lines(segment, width).len().max(1) as u16
        };
        total = total.saturating_add(rows);
    }
    total.max(1)
}

/// Popup width/height plus body/input row counts for AddInvoice (optional replacement body).
fn add_invoice_popup_layout(
    area: Rect,
    body: Option<&str>,
) -> (
    u16,
    u16,
    u16, /* body_rows */
    u16, /* input_rows */
) {
    const PREFERRED_WIDTH: u16 = 90;
    const BODY_INSET: u16 = 2;
    const MIN_INPUT: u16 = 3;
    const MAX_INPUT: u16 = 6;
    // spacer + order id + spacer + label + spacer + buttons + 2 help lines
    const FIXED_CHROME: u16 = 10;

    let popup_width = PREFERRED_WIDTH.min(area.width).max(20);
    let (body_rows, input_rows) = if let Some(body) = body {
        let text_width = body_text_width(popup_width, BODY_INSET);
        let needed = count_wrapped_body_rows(body, text_width);
        let mut body_rows = needed.clamp(2, 8);
        let mut input_rows = MAX_INPUT;
        let mut total = FIXED_CHROME
            .saturating_add(body_rows)
            .saturating_add(input_rows);
        if total > area.height {
            let overflow = total.saturating_sub(area.height);
            let shrink_input = overflow.min(input_rows.saturating_sub(MIN_INPUT));
            input_rows = input_rows.saturating_sub(shrink_input);
            total = total.saturating_sub(shrink_input);
        }
        if total > area.height {
            let overflow = total.saturating_sub(area.height);
            body_rows = body_rows.saturating_sub(overflow).max(1);
        }
        (body_rows, input_rows)
    } else {
        let mut input_rows = MAX_INPUT;
        // No body slot: preview row (1) replaces body.
        let mut total = FIXED_CHROME.saturating_add(1).saturating_add(input_rows);
        if total > area.height {
            let overflow = total.saturating_sub(area.height);
            let shrink_input = overflow.min(input_rows.saturating_sub(MIN_INPUT));
            input_rows = input_rows.saturating_sub(shrink_input);
            total = total.saturating_sub(shrink_input);
        }
        let _ = total;
        (0, input_rows)
    };

    let popup_height = if body.is_some() {
        FIXED_CHROME
            .saturating_add(body_rows)
            .saturating_add(input_rows)
    } else {
        FIXED_CHROME.saturating_add(1).saturating_add(input_rows)
    }
    .min(area.height)
    .max(8);

    (popup_width, popup_height, body_rows, input_rows)
}

/// Popup width/height plus body row count for the informational payment-failed popup.
fn payment_failed_popup_layout(area: Rect, body: &str) -> (u16, u16, u16) {
    const PREFERRED_WIDTH: u16 = 76;
    const BODY_INSET: u16 = 1; // create_input_area pads by 1
                               // spacer + order id + spacer + spacer + dismiss
    const FIXED_CHROME: u16 = 5;

    let popup_width = PREFERRED_WIDTH.min(area.width).max(24);
    let text_width = body_text_width(popup_width, BODY_INSET);
    let needed = count_wrapped_body_rows(body, text_width);
    let mut body_rows = needed.clamp(2, 10);
    let mut total = FIXED_CHROME.saturating_add(body_rows);
    if total > area.height {
        let overflow = total.saturating_sub(area.height);
        body_rows = body_rows.saturating_sub(overflow).max(1);
        total = FIXED_CHROME.saturating_add(body_rows);
    }
    let popup_height = total.min(area.height).max(6);
    (popup_width, popup_height, body_rows)
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
///
/// `body_rows` is allocated by [`payment_failed_popup_layout`] from the popup width
/// and available terminal height so wrapped retry text is not clipped.
fn render_payment_failed(
    f: &mut ratatui::Frame,
    popup: Rect,
    notification: &MessageNotification,
    body_rows: u16,
) {
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1),         // spacer
            Constraint::Length(1),         // order id
            Constraint::Length(1),         // spacer
            Constraint::Length(body_rows), // wrapped retry explanation
            Constraint::Length(1),         // spacer
            Constraint::Length(1),         // dismiss hint
        ],
    )
    .split(popup);

    render_order_id_header(
        f,
        chunks[1],
        &helpers::format_order_id(notification.order_id),
    );

    let body = notification.body.clone().unwrap_or_else(|| {
        "Lightning payout to your invoice failed.\n\nMostro will retry automatically.\n\n\
         Your sats remain locked in escrow.\nNo action needed yet."
            .to_string()
    });
    f.render_widget(
        Paragraph::new(body)
            .wrap(Wrap { trim: true })
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::White)),
        inset_horizontal(chunks[3], 2),
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

fn compact_pay_title(bond: bool, order_id: Option<uuid::Uuid>) -> String {
    let base = if bond {
        "🛡️ Anti-abuse Bond Invoice"
    } else {
        "💳 Payment Request"
    };
    format!("{base} · {}", helpers::short_order_id(order_id))
}

/// Render a message-notification popup.
///
/// Post-retry [`mostro_core::prelude::Action::AddInvoice`] (body present) and
/// [`mostro_core::prelude::Action::PaymentFailed`] choose popup size from terminal
/// width/height so wrapped body text degrades gracefully on narrow or short screens.
/// [`mostro_core::prelude::Action::PayInvoice`] / [`mostro_core::prelude::Action::PayBondInvoice`]
/// size to a half-block QR when it fits, a quadrant QR on narrow terminals and
/// a compact sextant QR on short ones, otherwise keep the text card; other
/// actions keep fixed preferred dimensions.
pub fn render_message_notification(
    f: &mut ratatui::Frame,
    notification: &MessageNotification,
    action: mostro_core::prelude::Action,
    invoice_state: &InvoiceInputState,
) {
    let area = f.area();
    let pay_layout = match action {
        mostro_core::prelude::Action::PayInvoice => Some(pay_invoice_popup_layout(
            area,
            notification.invoice.as_deref(),
            invoice_state.show_qr,
            false,
        )),
        mostro_core::prelude::Action::PayBondInvoice => Some(pay_invoice_popup_layout(
            area,
            notification.invoice.as_deref(),
            invoice_state.show_qr,
            true,
        )),
        _ => None,
    };
    let (popup_width, popup_height, add_invoice_body_rows, add_invoice_input_rows, pf_body_rows) =
        match action {
            mostro_core::prelude::Action::AddInvoice => {
                let (w, h, body_rows, input_rows) =
                    add_invoice_popup_layout(area, notification.body.as_deref());
                (w, h, body_rows, input_rows, 0)
            }
            mostro_core::prelude::Action::AddBondInvoice => (90, 21, 0, 6, 0),
            mostro_core::prelude::Action::PayInvoice => {
                let layout = pay_layout.as_ref().expect("PayInvoice layout");
                (layout.popup_width, layout.popup_height, 0, 6, 0)
            }
            mostro_core::prelude::Action::PayBondInvoice => {
                let layout = pay_layout.as_ref().expect("PayBondInvoice layout");
                (layout.popup_width, layout.popup_height, 0, 6, 0)
            }
            mostro_core::prelude::Action::WaitingSellerToPay
            | mostro_core::prelude::Action::WaitingBuyerInvoice => (90, 16, 0, 6, 0),
            mostro_core::prelude::Action::PaymentFailed => {
                let body = notification.body.as_deref().unwrap_or(
                    "Lightning payout to your invoice failed.\n\nMostro will retry automatically.\n\n\
                     Your sats remain locked in escrow.\nNo action needed yet.",
                );
                let (w, h, body_rows) = payment_failed_popup_layout(area, body);
                (w, h, 0, 6, body_rows)
            }
            _ => (70, 8, 0, 6, 0),
        };

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let compact_pay = pay_layout
        .as_ref()
        .is_some_and(|layout| layout.use_compact());
    let title = match action {
        mostro_core::prelude::Action::AddInvoice => {
            if notification.body.is_some() {
                "⚠️ New Invoice After Payment Failed".to_string()
            } else {
                "📝 Invoice Request".to_string()
            }
        }
        mostro_core::prelude::Action::AddBondInvoice => "⚔️ Bond Payout Invoice".to_string(),
        mostro_core::prelude::Action::PayInvoice | mostro_core::prelude::Action::PayBondInvoice
            if compact_pay =>
        {
            compact_pay_title(
                matches!(action, mostro_core::prelude::Action::PayBondInvoice),
                notification.order_id,
            )
        }
        mostro_core::prelude::Action::PayInvoice => "💳 Payment Request".to_string(),
        mostro_core::prelude::Action::PayBondInvoice => "🛡️ Anti-abuse Bond Invoice".to_string(),
        mostro_core::prelude::Action::WaitingSellerToPay
        | mostro_core::prelude::Action::WaitingBuyerInvoice => "📋 Trade Status".to_string(),
        mostro_core::prelude::Action::PaymentFailed => "⚠️ Payment Failed".to_string(),
        _ => "📨 New Message".to_string(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    match action {
        mostro_core::prelude::Action::AddInvoice => {
            render_add_invoice(
                f,
                popup,
                notification,
                invoice_state,
                add_invoice_body_rows,
                add_invoice_input_rows,
            );
        }
        mostro_core::prelude::Action::AddBondInvoice => {
            render_add_bond_invoice(f, popup, notification, invoice_state);
        }
        mostro_core::prelude::Action::PayInvoice => {
            render_pay_invoice(
                f,
                popup,
                notification,
                invoice_state,
                pay_layout.as_ref().expect("PayInvoice layout"),
            );
        }
        mostro_core::prelude::Action::PayBondInvoice => {
            render_pay_bond_invoice(
                f,
                popup,
                notification,
                invoice_state,
                pay_layout.as_ref().expect("PayBondInvoice layout"),
            );
        }
        mostro_core::prelude::Action::WaitingSellerToPay
        | mostro_core::prelude::Action::WaitingBuyerInvoice => {
            render_waiting_phase_popup(f, popup, notification, invoice_state);
        }
        mostro_core::prelude::Action::PaymentFailed => {
            render_payment_failed(f, popup, notification, pf_body_rows.max(1));
        }
        _ => {
            render_default_notification(f, popup, notification);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render_message_notification;
    use crate::ui::{InvoiceInputState, MessageNotification};
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

    /// Collapses row padding so wrap-across-line phrases still match.
    fn buffer_text_collapsed(buf: &ratatui::buffer::Buffer) -> String {
        buffer_text(buf)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn display_only_state() -> InvoiceInputState {
        InvoiceInputState::display_only()
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
                "Lightning payout to your invoice failed.\n\nMostro will retry automatically:\nUp to 3 attempt(s), every 5 second(s) apart\n\n\
                 Your sats remain locked in escrow.\nNo action needed yet."
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
                "Lightning payout failed after all retries.\nPaste a new invoice to receive your escrow sats."
                    .to_string(),
            ),
            maker_bond_publish: false,
            solver_pubkey: None,
            dispute_id: None,
        };
        let state = InvoiceInputState::for_input(String::new(), true);

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
        // Body must not be truncated: both lines of the explanation are visible.
        assert!(
            text.contains("Lightning payout failed after all retries"),
            "first body line missing: {text}"
        );
        assert!(
            text.contains("Paste a new invoice to receive your escrow sats"),
            "second body line missing/truncated: {text}"
        );
        // Redundant preview (near-duplicate of the title) must not appear.
        assert!(
            !text.contains("New Invoice After Failed Payment"),
            "redundant preview should be omitted when body is shown: {text}"
        );
        assert!(text.contains("Submit Invoice"), "{text}");
    }

    #[test]
    fn payment_failed_popup_wraps_body_on_narrow_terminal() {
        let body = "Lightning payout to your invoice failed.\n\nMostro will retry automatically:\nUp to 3 attempt(s), every 5 second(s) apart\n\n\
             Your sats remain locked in escrow.\nNo action needed yet.";
        let notification = MessageNotification {
            order_id: Some(Uuid::new_v4()),
            message_preview: "Payment Failed".to_string(),
            timestamp: 1,
            action: Action::PaymentFailed,
            sat_amount: None,
            invoice: None,
            body: Some(body.to_string()),
            maker_bond_publish: false,
            solver_pubkey: None,
            dispute_id: None,
        };
        let state = display_only_state();

        // Narrow: body must wrap across several rows without losing key phrases.
        let backend = TestBackend::new(40, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_message_notification(f, &notification, Action::PaymentFailed, &state))
            .unwrap();
        let text = buffer_text_collapsed(terminal.backend().buffer());
        assert!(text.contains("Payment Failed"), "title missing: {text}");
        assert!(
            text.contains("retry automatically"),
            "wrapped body clipped on narrow width: {text}"
        );
        assert!(
            text.contains("Lightning") && text.contains("invoice"),
            "wrapped body missing invoice phrase: {text}"
        );
        assert!(text.contains("to dismiss"), "dismiss hint missing: {text}");
    }

    #[test]
    fn payment_failed_popup_fits_short_terminal() {
        let body = "Lightning payout to your invoice failed.\n\nMostro will retry automatically:\nUp to 3 attempt(s), every 5 second(s) apart\n\n\
             Your sats remain locked in escrow.\nNo action needed yet.";
        let notification = MessageNotification {
            order_id: Some(Uuid::new_v4()),
            message_preview: "Payment Failed".to_string(),
            timestamp: 1,
            action: Action::PaymentFailed,
            sat_amount: None,
            invoice: None,
            body: Some(body.to_string()),
            maker_bond_publish: false,
            solver_pubkey: None,
            dispute_id: None,
        };
        let state = display_only_state();

        let backend = TestBackend::new(76, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_message_notification(f, &notification, Action::PaymentFailed, &state))
            .unwrap();
        let text = buffer_text_collapsed(terminal.backend().buffer());
        assert!(text.contains("Payment Failed"), "title missing: {text}");
        // Short height may drop later wrap lines; keep distinctive early phrase + dismiss.
        assert!(
            text.contains("Lightning"),
            "body should remain visible on short height: {text}"
        );
        assert!(text.contains("dismiss"), "dismiss hint missing: {text}");
    }

    #[test]
    fn add_invoice_replacement_body_wraps_on_narrow_terminal() {
        let notification = MessageNotification {
            order_id: Some(Uuid::new_v4()),
            message_preview: "New Invoice After Failed Payment".to_string(),
            timestamp: 1,
            action: Action::AddInvoice,
            sat_amount: Some(1000),
            invoice: None,
            body: Some(
                "Lightning payout failed after all retries. Paste a new invoice to receive your escrow sats."
                    .to_string(),
            ),
            maker_bond_publish: false,
            solver_pubkey: None,
            dispute_id: None,
        };
        let state = InvoiceInputState::for_input(String::new(), true);

        let backend = TestBackend::new(48, 28);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_message_notification(f, &notification, Action::AddInvoice, &state))
            .expect("draw");
        let text = buffer_text_collapsed(terminal.backend().buffer());
        assert!(
            text.contains("New Invoice After Payment Failed"),
            "title missing: {text}"
        );
        assert!(
            text.contains("Lightning payout failed after all retries"),
            "wrapped body clipped on narrow width: {text}"
        );
        assert!(
            text.contains("Paste a new invoice"),
            "second body phrase missing: {text}"
        );
        assert!(text.contains("Submit Invoice"), "{text}");
    }

    #[test]
    fn add_invoice_replacement_body_fits_short_terminal() {
        let notification = MessageNotification {
            order_id: Some(Uuid::new_v4()),
            message_preview: "New Invoice After Failed Payment".to_string(),
            timestamp: 1,
            action: Action::AddInvoice,
            sat_amount: Some(1000),
            invoice: None,
            body: Some(
                "Lightning payout failed after all retries.\nPaste a new invoice to receive your escrow sats."
                    .to_string(),
            ),
            maker_bond_publish: false,
            solver_pubkey: None,
            dispute_id: None,
        };
        let state = InvoiceInputState::for_input(String::new(), true);

        let backend = TestBackend::new(90, 16);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_message_notification(f, &notification, Action::AddInvoice, &state))
            .expect("draw");
        let text = buffer_text_collapsed(terminal.backend().buffer());
        assert!(
            text.contains("New Invoice After Payment Failed"),
            "title missing: {text}"
        );
        assert!(
            text.contains("Lightning payout failed"),
            "body should remain visible on short height: {text}"
        );
        assert!(text.contains("Submit Invoice"), "{text}");
    }

    #[test]
    fn count_wrapped_body_rows_honors_hard_breaks_and_width() {
        let wide = super::count_wrapped_body_rows("one line only", 40);
        assert_eq!(wide, 1);
        let hard = super::count_wrapped_body_rows("first line\nsecond line", 40);
        assert_eq!(hard, 2);
        let wrapped = super::count_wrapped_body_rows(
            "Lightning payout failed after all retries. Paste a new invoice.",
            20,
        );
        assert!(wrapped >= 3, "expected multi-line wrap, got {wrapped}");
    }

    fn pay_notification(action: Action, invoice: &str) -> MessageNotification {
        MessageNotification {
            order_id: Some(Uuid::new_v4()),
            message_preview: "Pay the invoice".to_string(),
            timestamp: 1,
            action,
            sat_amount: Some(1000),
            invoice: Some(invoice.to_string()),
            body: None,
            maker_bond_publish: false,
            solver_pubkey: None,
            dispute_id: None,
        }
    }

    fn draw_pay(
        width: u16,
        height: u16,
        notification: &MessageNotification,
        state: &InvoiceInputState,
    ) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let action = notification.action.clone();
        terminal
            .draw(|f| render_message_notification(f, notification, action, state))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn buffer_has_half_block(buf: &ratatui::buffer::Buffer) -> bool {
        buffer_text(buf).contains('▀')
    }

    fn buffer_has_sextant(buf: &ratatui::buffer::Buffer) -> bool {
        buffer_text(buf)
            .chars()
            .any(|c| ('\u{1FB00}'..='\u{1FB3B}').contains(&c) || matches!(c, '▌' | '▐'))
    }

    fn buffer_has_qr_glyph(buf: &ratatui::buffer::Buffer) -> bool {
        buffer_has_half_block(buf) || buffer_has_sextant(buf)
    }

    fn buffer_has_qr_colors(buf: &ratatui::buffer::Buffer) -> bool {
        let dark = ratatui::style::Color::Rgb(0, 0, 0);
        let light = ratatui::style::Color::Rgb(255, 255, 255);
        let mut saw_dark = false;
        let mut saw_light = false;
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let style = buf[(x, y)].style();
                if style.fg == Some(dark) || style.bg == Some(dark) {
                    saw_dark = true;
                }
                if style.fg == Some(light) || style.bg == Some(light) {
                    saw_light = true;
                }
            }
        }
        saw_dark && saw_light
    }

    fn buffer_has_bg(buf: &ratatui::buffer::Buffer, bg: ratatui::style::Color) -> bool {
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].style().bg == Some(bg) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn pay_invoice_qr_renders_on_tall_terminal() {
        let notification = pay_notification(Action::PayInvoice, "lnbc1test");
        let state = InvoiceInputState::display_only();
        let buf = draw_pay(90, 40, &notification, &state);
        let text = buffer_text(&buf);
        assert!(buffer_has_half_block(&buf), "QR glyphs missing: {text}");
        assert!(buffer_has_qr_colors(&buf), "QR black/white missing: {text}");
        assert!(text.contains("SPACE"), "toggle hint missing: {text}");
        assert!(
            text.contains("Acknowledge"),
            "QR mode must show the primary action: {text}"
        );
        assert!(
            text.contains("Cancel Order"),
            "QR mode must show Cancel Order: {text}"
        );
        let short_id = crate::ui::helpers::short_order_id(notification.order_id);
        assert!(
            text.contains(&short_id),
            "order id should be in the QR title: {text}"
        );
        assert!(
            !text.contains("lnbc1test"),
            "raw invoice should be hidden in QR view: {text}"
        );
    }

    #[test]
    fn pay_invoice_space_pref_shows_text() {
        let notification = pay_notification(Action::PayInvoice, "lnbc1testinvoice");
        let mut state = InvoiceInputState::display_only();
        state.show_qr = false;
        let buf = draw_pay(90, 24, &notification, &state);
        let text = buffer_text(&buf);
        assert!(
            text.contains("lnbc1testinvoice"),
            "text view should show invoice: {text}"
        );
        assert!(
            !buffer_has_qr_glyph(&buf),
            "text view should not draw QR: {text}"
        );
        assert!(text.contains("SPACE"), "toggle hint missing: {text}");
    }

    #[test]
    fn pay_invoice_qr_renders_on_standard_terminal_when_it_fits() {
        let notification = pay_notification(Action::PayInvoice, "lnbc1test");
        let state = InvoiceInputState::display_only();
        let buf = draw_pay(80, 24, &notification, &state);
        let text = buffer_text(&buf);
        assert!(
            buffer_has_qr_glyph(&buf),
            "a fitting QR must still render on 80x24: {text}"
        );
        assert!(
            !text.contains("taller terminal"),
            "must not fall back to text when the QR fits: {text}"
        );
        assert!(
            text.contains("Acknowledge") && text.contains("Cancel Order"),
            "compact QR must keep Ack/Cancel visible: {text}"
        );
    }

    #[test]
    fn pay_invoice_qr_renders_typical_bolt11_on_standard_terminal() {
        let invoice = format!("lnbc1{}", "a".repeat(340));
        let notification = pay_notification(Action::PayInvoice, &invoice);
        let state = InvoiceInputState::display_only();
        let buf = draw_pay(80, 24, &notification, &state);
        let text = buffer_text(&buf);
        assert!(
            buffer_has_qr_glyph(&buf),
            "a typical bolt11 must show a compact QR on 80x24: {text}"
        );
        assert!(
            !text.contains("taller terminal"),
            "must not fall back to text on 80x24: {text}"
        );
    }

    #[test]
    fn pay_invoice_falls_back_to_text_on_tiny_terminal() {
        let invoice = format!("lnbc1{}", "a".repeat(400));
        let notification = pay_notification(Action::PayInvoice, &invoice);
        let state = InvoiceInputState::display_only();
        let buf = draw_pay(80, 10, &notification, &state);
        let text = buffer_text(&buf);
        assert!(
            !buffer_has_qr_glyph(&buf),
            "QR must not render when it cannot fit: {text}"
        );
        assert!(
            text.contains("taller terminal"),
            "fallback hint missing: {text}"
        );
        assert!(
            text.contains("Acknowledge") && text.contains("Cancel Order"),
            "short-terminal text fallback must keep Ack/Cancel visible: {text}"
        );
    }

    #[test]
    fn pay_invoice_qr_selection_is_visibly_distinct() {
        let notification = pay_notification(Action::PayInvoice, "lnbc1test");
        let primary = InvoiceInputState::display_only();
        let mut cancel = InvoiceInputState::display_only();
        cancel.action_selection = crate::ui::InvoiceNotificationActionSelection::Cancel;
        let buf_primary = draw_pay(80, 24, &notification, &primary);
        let buf_cancel = draw_pay(80, 24, &notification, &cancel);
        assert!(
            buffer_has_qr_glyph(&buf_primary),
            "test needs a QR on 80x24"
        );
        assert!(
            buffer_has_bg(&buf_primary, ratatui::style::Color::Green)
                && !buffer_has_bg(&buf_primary, ratatui::style::Color::Red),
            "Acknowledge should be the only highlighted action when Primary is selected"
        );
        assert!(
            buffer_has_bg(&buf_cancel, ratatui::style::Color::Red)
                && !buffer_has_bg(&buf_cancel, ratatui::style::Color::Green),
            "Cancel Order should be the only highlighted action when Cancel is selected"
        );
    }

    #[test]
    fn pay_bond_invoice_qr_renders_on_tall_terminal() {
        let notification = pay_notification(Action::PayBondInvoice, "lnbc1bond");
        let state = InvoiceInputState::display_only();
        let buf = draw_pay(90, 40, &notification, &state);
        let text = buffer_text(&buf);
        assert!(
            buffer_has_half_block(&buf),
            "bond QR glyphs missing: {text}"
        );
        assert!(text.contains("Locked"), "bond note missing: {text}");
    }

    #[test]
    fn pay_bond_invoice_qr_renders_on_standard_terminal_when_it_fits() {
        let notification = pay_notification(Action::PayBondInvoice, "lnbc1bond");
        let state = InvoiceInputState::display_only();
        let buf = draw_pay(80, 24, &notification, &state);
        let text = buffer_text(&buf);
        assert!(
            buffer_has_qr_glyph(&buf),
            "a fitting bond QR must still render on 80x24: {text}"
        );
        assert!(text.contains("Locked"), "bond note missing: {text}");
        assert!(
            text.contains("Acknowledge") && text.contains("Cancel Order"),
            "bond QR must keep Ack/Cancel visible: {text}"
        );
    }

    #[test]
    fn pay_bond_invoice_qr_renders_typical_bolt11_on_standard_terminal() {
        let invoice = format!("lnbcrt1{}", "a".repeat(340));
        let notification = pay_notification(Action::PayBondInvoice, &invoice);
        let state = InvoiceInputState::display_only();
        let buf = draw_pay(80, 26, &notification, &state);
        let text = buffer_text(&buf);
        assert!(
            buffer_has_qr_glyph(&buf),
            "a typical bond invoice must show a compact QR when the extra bond line fits: {text}"
        );
        assert!(text.contains("Locked"), "bond note missing: {text}");
        assert!(
            text.contains("Acknowledge") && text.contains("Cancel Order"),
            "bond QR must keep Ack/Cancel visible: {text}"
        );
    }
}
