use chrono::{DateTime, Utc};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::{MessageNotification, OrderMessage, BACKGROUND_COLOR, PRIMARY_COLOR};

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
                    order_id.to_string().chars().take(8).collect::<String>()
                )
            } else {
                "Order: Unknown".to_string()
            };

            let timestamp = DateTime::<Utc>::from_timestamp(msg.timestamp as i64, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Unknown time".to_string());

            let action_str = match msg.message.get_inner_message_kind().action {
                mostro_core::prelude::Action::AddInvoice => "📝 Invoice Request",
                mostro_core::prelude::Action::PayInvoice => "💳 Payment Request",
                mostro_core::prelude::Action::FiatSent => "✅ Fiat Sent",
                mostro_core::prelude::Action::FiatSentOk => "✅ Fiat Received",
                mostro_core::prelude::Action::Release | mostro_core::prelude::Action::Released => {
                    "🔓 Release"
                }
                mostro_core::prelude::Action::Dispute
                | mostro_core::prelude::Action::DisputeInitiatedByYou => "⚠️ Dispute",
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
    action: mostro_core::prelude::Action,
    invoice_state: &crate::ui::InvoiceInputState,
) {
    let area = f.area();
    // Different widths based on action type
    let popup_width = match action {
        mostro_core::prelude::Action::AddInvoice => 80, // Wider for invoice input
        _ => 70,
    };
    
    // Different heights based on action type
    let popup_height = match action {
        mostro_core::prelude::Action::AddInvoice => 15, // More space for input field and help text
        mostro_core::prelude::Action::PayInvoice => 10,  // Need space for invoice display
        _ => 8,
    };
    
    let popup_x = area.x + (area.width - popup_width) / 2;
    let popup_y = area.y + (area.height - popup_height) / 2;
    let popup = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    let title = match action {
        mostro_core::prelude::Action::AddInvoice => "📝 Invoice Request",
        mostro_core::prelude::Action::PayInvoice => "💳 Payment Request",
        _ => "📨 New Message",
    };

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
        mostro_core::prelude::Action::AddInvoice => {
            // Layout for AddInvoice with input field
            let chunks = Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // order id
                    Constraint::Length(1), // message preview
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // label
                    Constraint::Length(3), // invoice input field (more space)
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
                    Style::default(),
                )]))
                .alignment(ratatui::layout::Alignment::Center),
                chunks[2],
            );

            // Invoice input field label
            let input_label = "Paste your Lightning invoice:";
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(input_label, Style::default().fg(PRIMARY_COLOR).add_modifier(Modifier::BOLD)),
                ]))
                .alignment(ratatui::layout::Alignment::Center),
                chunks[4],
            );

            // Invoice input field area (larger, more visible)
            let input_display = if invoice_state.invoice_input.is_empty() {
                "lnbc..."
            } else {
                &invoice_state.invoice_input
            };
            
            let input_style = if invoice_state.focused {
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            // Use the full chunks[5] area for the input field
            let input_area = Rect {
                x: chunks[5].x + 1,
                y: chunks[5].y,
                width: chunks[5].width.saturating_sub(2),
                height: chunks[5].height,
            };

            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    input_display,
                    input_style,
                )]))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .style(if invoice_state.focused {
                            Style::default().fg(PRIMARY_COLOR)
                        } else {
                            Style::default()
                        }),
                ),
                input_area,
            );

            // Help text
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Paste invoice (", Style::default()),
                    Span::styled(
                        "Ctrl+V",
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
        mostro_core::prelude::Action::PayInvoice => {
            // Layout for PayInvoice showing invoice to pay
            let chunks = Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // order id
                    Constraint::Length(1), // message preview
                    Constraint::Length(1), // spacer
                    Constraint::Length(3), // invoice display
                    Constraint::Length(1), // spacer
                    Constraint::Length(1), // help text
                ],
            )
            .split(popup);

            f.render_widget(block, popup);

            // Order ID
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

            // Message preview
            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    &notification.message_preview,
                    Style::default(),
                )]))
                .alignment(ratatui::layout::Alignment::Center),
                chunks[2],
            );

            // Invoice to pay
            if let Some(invoice) = &notification.buyer_invoice {
                let amount_text = if let Some(amount) = notification.sat_amount {
                    format!("Amount: {} sats", amount)
                } else {
                    "Amount: See invoice".to_string()
                };

                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(amount_text, Style::default().fg(PRIMARY_COLOR)),
                    ])),
                    chunks[4],
                );

                // Show invoice (truncated if too long)
                let invoice_display = if invoice.len() > 60 {
                    format!("{}...", &invoice[..60])
                } else {
                    invoice.clone()
                };

                f.render_widget(
                    Paragraph::new(Line::from(vec![Span::styled(
                        invoice_display,
                        Style::default().fg(Color::Yellow),
                    )]))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .style(Style::default().fg(PRIMARY_COLOR)),
                    )
                    .wrap(ratatui::widgets::Wrap { trim: true }),
                    Rect {
                        x: chunks[4].x,
                        y: chunks[4].y + 1,
                        width: chunks[4].width,
                        height: 2,
                    },
                );
            }

            // Help text
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
                chunks[6],
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
