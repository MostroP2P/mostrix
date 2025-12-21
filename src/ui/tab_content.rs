use chrono::{DateTime, Utc};
use mostro_core::prelude::*;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use super::{MessageNotification, MessageViewState, OrderMessage, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_coming_soon(f: &mut ratatui::Frame, area: Rect, title: &str) {
    let paragraph = Paragraph::new(Span::raw("Coming soon")).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR)),
    );
    f.render_widget(paragraph, area);
}

pub fn render_messages_tab(
    f: &mut ratatui::Frame,
    area: Rect,
    messages: &[OrderMessage],
    selected_idx: usize,
) {
    let block = Block::default()
        .title("📨 Messages")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));

    if messages.is_empty() {
        let paragraph = Paragraph::new(Span::raw(
            "No messages yet. Messages related to your orders will appear here.",
        ))
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(paragraph, area);
        return;
    }

    let items: Vec<ListItem> = messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            let order_id_str = if let Some(order_id) = msg.order_id {
                format!(
                    "Order: {}",
                    order_id
                        .to_string()
                        .chars()
                        .take(order_id.to_string().len())
                        .collect::<String>()
                )
            } else {
                "Order: Unknown".to_string()
            };

            let timestamp = DateTime::<Utc>::from_timestamp(msg.timestamp as i64, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Unknown time".to_string());

            let action_str = match msg.message.get_inner_message_kind().action {
                Action::AddInvoice => "📝 Invoice Request",
                Action::PayInvoice => "💳 Payment Request",
                Action::WaitingSellerToPay => "💳 Waiting for Seller to Pay",
                Action::FiatSent => "✅ Fiat Sent",
                Action::FiatSentOk => "✅ Fiat Received",
                Action::Release | Action::Released => "🔓 Release",
                Action::Dispute | Action::DisputeInitiatedByYou => "⚠️ Dispute",
                _ => "📨 Message",
            };

            let preview = format!("{} - {} ({})", action_str, order_id_str, timestamp);

            // Determine style based on selection and read status
            let style = if idx == selected_idx {
                Style::default()
                    .bg(PRIMARY_COLOR)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else if !msg.read {
                // Unread messages are bold and white
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                // Read messages are normal white
                Style::default().fg(Color::White)
            };

            ListItem::new(Line::from(vec![Span::styled(preview, style)]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
        .highlight_symbol(">> ");

    f.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(selected_idx)),
    );
}

pub fn render_message_notification(
    f: &mut ratatui::Frame,
    notification: &MessageNotification,
    action: Action,
    invoice_state: &crate::ui::InvoiceInputState,
) {
    let area = f.area();
    // Different widths based on action type
    let popup_width = match action {
        Action::AddInvoice => 120, // Much wider to show full invoice (383 chars)
        Action::PayInvoice => 90, // Wide enough to show invoice with wrapping, but fits within terminal
        _ => 70,
    };

    // Different heights based on action type
    let popup_height = match action {
        Action::AddInvoice => 18, // More height for multi-line invoice display
        Action::PayInvoice => 18, // More height for multi-line invoice display
        _ => 8,
    };

    // Clamp popup dimensions to fit within available area to prevent overflow
    let popup_width = popup_width.min(area.width);
    let popup_height = popup_height.min(area.height);

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

    let title = match action {
        Action::AddInvoice => "📝 Invoice Request",
        Action::PayInvoice => "💳 Payment Request",
        _ => "📨 New Message",
    };

    // Clear the popup area to make it fully opaque
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));

    let order_id_str = if let Some(order_id) = notification.order_id {
        format!(
            "Order: {}",
            order_id.to_string().chars().take(8).collect::<String>()
        )
    } else {
        "Order: Unknown".to_string()
    };

    match action {
        Action::AddInvoice => {
            // Layout for AddInvoice with input field
            let chunks = Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // order id
                    Constraint::Length(1), // message preview
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // label
                    Constraint::Length(6), // invoice input field (more lines for full invoice display)
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // help text (paste instructions)
                    Constraint::Length(1), // help text (esc instructions)
                    Constraint::Length(1), // extra spacer
                ],
            )
            .split(popup);

            f.render_widget(block, popup);

            // Order ID
            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    order_id_str,
                    Style::default()
                        .bg(BACKGROUND_COLOR)
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                )]))
                .alignment(ratatui::layout::Alignment::Center),
                chunks[1],
            );

            // Message preview
            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    &notification.message_preview,
                    Style::default().bg(BACKGROUND_COLOR),
                )]))
                .alignment(ratatui::layout::Alignment::Center),
                chunks[2],
            );

            let amt: i64 = notification.sat_amount.unwrap_or_default();

            // Invoice input field label
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

            // Use the full chunks[5] area for the input field
            // Ensure we have valid dimensions and use saturating arithmetic
            let input_area = if chunks[5].width > 2 && chunks[5].height > 0 {
                Rect {
                    x: chunks[5].x.saturating_add(1),
                    y: chunks[5].y,
                    width: chunks[5].width.saturating_sub(2),
                    height: chunks[5].height,
                }
            } else {
                // Fallback to a minimal valid area if chunks[5] is too small
                chunks[5]
            };

            // Invoice input field area (larger, more visible)
            // Show full invoice with text wrapping
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

            // Use Paragraph with wrapping to show full invoice across multiple lines
            f.render_widget(
                Paragraph::new(input_display)
                    .style(input_style)
                    .wrap(ratatui::widgets::Wrap { trim: true })
                    .block(Block::default().borders(Borders::ALL).style(
                        if invoice_state.focused {
                            Style::default().fg(PRIMARY_COLOR)
                        } else {
                            Style::default()
                        },
                    )),
                input_area,
            );

            // Help text
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Paste invoice (", Style::default()),
                    Span::styled(
                        "Ctrl+Shift+V",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" or right-click), then press ", Style::default()),
                    Span::styled(
                        "Enter",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to submit", Style::default()),
                ]))
                .alignment(ratatui::layout::Alignment::Center),
                chunks[7],
            );

            // Additional help line
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Press ", Style::default()),
                    Span::styled(
                        "Esc",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to dismiss", Style::default()),
                ]))
                .alignment(ratatui::layout::Alignment::Center),
                chunks[8],
            );
        }
        Action::PayInvoice => {
            // Layout for PayInvoice showing invoice to pay (same layout as AddInvoice)
            let chunks = Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // order id
                    Constraint::Length(1), // message preview
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // label
                    Constraint::Length(6), // invoice display field (more lines for full invoice display)
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // help text line 1
                    Constraint::Length(1), // help text line 2
                ],
            )
            .split(popup);

            f.render_widget(block, popup);

            // Order ID
            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    order_id_str,
                    Style::default()
                        .bg(BACKGROUND_COLOR)
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                )]))
                .alignment(ratatui::layout::Alignment::Center),
                chunks[1],
            );

            // Message preview
            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    &notification.message_preview,
                    Style::default().bg(BACKGROUND_COLOR).fg(Color::White),
                )]))
                .alignment(ratatui::layout::Alignment::Center),
                chunks[2],
            );

            // Invoice to pay
            let amount_text = if let Some(amount) = notification.sat_amount {
                format!("Lightning invoice to pay ({} sats):", amount)
            } else {
                "Lightning invoice to pay:".to_string()
            };

            // Invoice label
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

            // Show full invoice with text wrapping (no truncation)
            // Bordered block for visual clarity - hold Shift while selecting to copy
            let (invoice_text, text_color) = match &notification.invoice {
                Some(invoice) if !invoice.is_empty() => (invoice.clone(), Color::White),
                Some(_) => (
                    "⚠️  Invoice not available (empty)".to_string(),
                    Color::Yellow,
                ),
                None => ("⚠️  Invoice not available".to_string(), Color::Yellow),
            };

            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    invoice_text,
                    Style::default().fg(text_color).add_modifier(Modifier::BOLD),
                )]))
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .style(Style::default().fg(PRIMARY_COLOR)),
                ),
                chunks[5],
            );

            // Help text - first line (show "Copied!" message if invoice was just copied)
            if invoice_state.copied_to_clipboard {
                f.render_widget(
                    Paragraph::new(Line::from(vec![Span::styled(
                        "✓ Invoice copied to clipboard!",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )]))
                    .alignment(ratatui::layout::Alignment::Center),
                    chunks[7],
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
                        Span::styled(" to copy invoice to clipboard, or ", Style::default()),
                        Span::styled(
                            "Shift",
                            Style::default()
                                .fg(PRIMARY_COLOR)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled("+click to select", Style::default()),
                    ]))
                    .alignment(ratatui::layout::Alignment::Center),
                    chunks[7],
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
                chunks[8],
            );
        }
        _ => {
            // Default layout for other notifications
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

            f.render_widget(block, popup);

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
    }
}

pub fn render_message_view(f: &mut ratatui::Frame, view_state: &MessageViewState) {
    let area = f.area();
    let popup_width = area.width.saturating_sub(area.width / 4);

    // Check if we need YES/NO buttons (only for FiatSent or Release actions)
    let show_buttons = matches!(
        view_state.action,
        Action::HoldInvoicePaymentAccepted | Action::FiatSentOk
    );

    // Adjust popup height based on whether we show buttons
    let popup_height = if show_buttons {
        14 // Need space for button blocks with borders
    } else {
        10 // Simpler layout without buttons
    };

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

    // Adjust layout constraints based on whether we show buttons
    let constraints = if show_buttons {
        vec![
            Constraint::Length(1), // spacer
            Constraint::Length(1), // title
            Constraint::Length(1), // separator
            Constraint::Length(1), // order id
            Constraint::Length(1), // message content
            Constraint::Length(1), // spacer
            Constraint::Length(3), // buttons (need space for borders)
            Constraint::Length(1), // help text
        ]
    } else {
        vec![
            Constraint::Length(1), // spacer
            Constraint::Length(1), // title
            Constraint::Length(1), // separator
            Constraint::Length(1), // order id
            Constraint::Length(1), // message content
            Constraint::Length(1), // spacer
            Constraint::Length(1), // exit text
        ]
    };

    let inner_chunks = Layout::new(Direction::Vertical, constraints).split(popup);

    let block = Block::default()
        .title("📨 Message")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    // Order ID
    let order_id_str = if let Some(order_id) = view_state.order_id {
        format!(
            "Order: {}",
            order_id
                .to_string()
                .chars()
                .take(order_id.to_string().len())
                .collect::<String>()
        )
    } else {
        "Order: Unknown".to_string()
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            order_id_str,
            Style::default()
                .bg(BACKGROUND_COLOR)
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[3],
    );

    // Message content
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            &view_state.message_content,
            Style::default().bg(BACKGROUND_COLOR),
        )]))
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true }),
        inner_chunks[4],
    );

    if show_buttons {
        // Yes/No buttons - center them in the popup
        let button_area = inner_chunks[6];

        // Calculate button width (each button + separator)
        let button_width = 15; // Width for each button
        let separator_width = 1;
        let total_button_width = (button_width * 2) + separator_width;

        // Center the buttons horizontally
        let button_x = button_area.x + (button_area.width.saturating_sub(total_button_width)) / 2;
        let centered_button_area = Rect {
            x: button_x,
            y: button_area.y,
            width: total_button_width.min(button_area.width),
            height: button_area.height,
        };

        let button_chunks = Layout::new(
            Direction::Horizontal,
            [
                Constraint::Length(button_width),
                Constraint::Length(separator_width), // separator
                Constraint::Length(button_width),
            ],
        )
        .split(centered_button_area);

        // YES button
        let yes_style = if view_state.selected_button {
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        };

        let yes_block = Block::default().borders(Borders::ALL).style(yes_style);
        f.render_widget(yes_block, button_chunks[0]);

        let yes_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
            .margin(1)
            .split(button_chunks[0]);

        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "✓ YES",
                Style::default()
                    .fg(if view_state.selected_button {
                        Color::Black
                    } else {
                        Color::Green
                    })
                    .add_modifier(Modifier::BOLD),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            yes_inner[0],
        );

        // NO button
        let no_style = if !view_state.selected_button {
            Style::default()
                .bg(Color::Red)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        };

        let no_block = Block::default().borders(Borders::ALL).style(no_style);
        f.render_widget(no_block, button_chunks[2]);

        let no_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
            .margin(1)
            .split(button_chunks[2]);

        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "✗ NO",
                Style::default()
                    .fg(if !view_state.selected_button {
                        Color::Black
                    } else {
                        Color::Red
                    })
                    .add_modifier(Modifier::BOLD),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            no_inner[0],
        );

        // Help text for buttons
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Use ", Style::default()),
                Span::styled(
                    "Left/Right",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to select, ", Style::default()),
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
            inner_chunks[7],
        );
    } else {
        // Simple exit text for other actions
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Press ", Style::default()),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" or ", Style::default()),
                Span::styled(
                    "Return",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to exit", Style::default()),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            inner_chunks[6],
        );
    }
}
