use chrono::{DateTime, Utc};
use mostro_core::prelude::*;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use super::{helpers, MessageViewState, OrderMessage, BACKGROUND_COLOR, PRIMARY_COLOR};

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

pub fn render_disputes_in_progress(f: &mut ratatui::Frame, area: Rect, app: &super::AppState) {
    let chunks = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(30), Constraint::Percentage(70)],
    )
    .split(area);

    let sidebar_area = chunks[0];
    let main_area = chunks[1];

    // 1. Sidebar - Dispute List
    let disputes_block = Block::default()
        .title("Disputes in Progress")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));

    if app.admin_disputes_in_progress.is_empty() {
        let empty_msg = Paragraph::new("No disputes in progress")
            .block(disputes_block)
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(empty_msg, sidebar_area);
    } else {
        let items: Vec<ListItem> = app
            .admin_disputes_in_progress
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let style = if i == app.selected_in_progress_idx {
                    Style::default().bg(PRIMARY_COLOR).fg(Color::Black)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(vec![Span::styled(
                    format!("ID: {}...", &d.id[..8]),
                    style,
                )]))
            })
            .collect();

        let list = List::new(items).block(disputes_block);
        f.render_widget(list, sidebar_area);
    }

    // 2. Main Area
    if let Some(selected_dispute) = app
        .admin_disputes_in_progress
        .get(app.selected_in_progress_idx)
    {
        let main_chunks = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(3), // Header
                Constraint::Length(3), // Party Tabs
                Constraint::Min(0),    // Chat
                Constraint::Length(3), // Input
                Constraint::Length(1), // Footer
            ],
        )
        .split(main_area);

        // Header
        let header_text = format!(
            "Dispute ID: {} | Type: {:?} | Status: {:?}",
            selected_dispute.id, selected_dispute.kind, selected_dispute.status
        );
        let header = Paragraph::new(header_text).block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(PRIMARY_COLOR)),
        );
        f.render_widget(header, main_chunks[0]);

        // Party Tabs
        let buyer_style = if app.active_chat_party == super::ChatParty::Buyer {
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        let seller_style = if app.active_chat_party == super::ChatParty::Seller {
            Style::default()
                .bg(Color::Red)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red)
        };

        let party_tabs_area = main_chunks[1];
        let party_chunks = Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        )
        .split(party_tabs_area);

        f.render_widget(
            Paragraph::new("BUYER")
                .block(Block::default().borders(Borders::ALL).style(buyer_style))
                .alignment(ratatui::layout::Alignment::Center),
            party_chunks[0],
        );
        f.render_widget(
            Paragraph::new("SELLER")
                .block(Block::default().borders(Borders::ALL).style(seller_style))
                .alignment(ratatui::layout::Alignment::Center),
            party_chunks[1],
        );

        // Chat History (Filtered)
        let messages_lock = app.messages.lock().unwrap();
        // For admin disputes, we need to filter messages that belong to this dispute ID
        // AND are either from/to the active chat party.
        // NOTE: Admin messages usually have the order_id set.
        let chat_party_pubkey = match app.active_chat_party {
            super::ChatParty::Buyer => selected_dispute.buyer_pubkey.as_ref(),
            super::ChatParty::Seller => selected_dispute.seller_pubkey.as_ref(),
        };

        let filtered_messages: Vec<ListItem> = messages_lock
            .iter()
            .filter(|m| {
                let is_same_order =
                    m.order_id.map(|id| id.to_string()) == Some(selected_dispute.id.clone());
                let _is_correct_party = if let Some(party_pk) = chat_party_pubkey {
                    m.sender.to_string() == *party_pk
                        || m.message.get_inner_message_kind().action == Action::AdminTookDispute
                // Placeholder check
                } else {
                    false
                };
                is_same_order // For now, just show all messages for this order ID
            })
            .map(|m| {
                let sender_label = if m.sender.to_string() == selected_dispute.initiator_pubkey {
                    "Initiator"
                } else {
                    "Other"
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{}] ", sender_label),
                        Style::default().fg(PRIMARY_COLOR),
                    ),
                    Span::raw(format!("{:?}", m.message.get_inner_message_kind().action)),
                ]))
            })
            .collect();

        let chat_list = List::new(filtered_messages).block(
            Block::default()
                .title(format!("Chat with {}", app.active_chat_party))
                .borders(Borders::ALL),
        );
        f.render_widget(chat_list, main_chunks[2]);

        // Input Area
        let input = Paragraph::new(app.admin_chat_input.as_str()).block(
            Block::default()
                .title("Message")
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Yellow)),
        );
        f.render_widget(input, main_chunks[3]);

        // Footer
        let footer = Paragraph::new("Tab: Switch Party | Enter: Send | S: Settle (Full Buyer) | X: Settle (Full Seller) | C: Cancel Order");
        f.render_widget(footer, main_chunks[4]);
    } else {
        let no_selection = Paragraph::new("Select a dispute from the sidebar")
            .block(Block::default().borders(Borders::ALL))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(no_selection, main_area);
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

    // Center the popup
    let popup = helpers::create_centered_popup(area, popup_width, popup_height);

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
    let order_id_str = helpers::format_order_id(view_state.order_id);
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
