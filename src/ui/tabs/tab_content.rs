use chrono::{DateTime, Utc};
use mostro_core::prelude::*;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use crate::ui::orders::{
    message_action_compact_label, message_buy_flow_step, message_order_kind_label,
    message_timeline_warning,
};
use crate::ui::{helpers, MessageViewState, OrderMessage, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_coming_soon(f: &mut ratatui::Frame, area: Rect, title: &str) {
    let paragraph = Paragraph::new(Span::raw("Coming soon")).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR)),
    );
    f.render_widget(paragraph, area);
}

/// Returns ASCII art logo for Mostro
fn get_mostro_logo() -> Vec<&'static str> {
    vec![
        "    ███╗   ███╗ ██████╗ ███████╗████████╗██████╗  ██████╗ ",
        "    ████╗ ████║██╔═══██╗██╔════╝╚══██╔══╝██╔══██╗██╔═══██╗",
        "    ██╔████╔██║██║   ██║███████╗   ██║   ██████╔╝██║   ██║",
        "    ██║╚██╔╝██║██║   ██║╚════██║   ██║   ██╔══██╗██║   ██║",
        "    ██║ ╚═╝ ██║╚██████╔╝███████║   ██║   ██║  ██║╚██████╔╝",
        "    ╚═╝     ╚═╝ ╚═════╝ ╚══════╝   ╚═╝   ╚═╝  ╚═╝ ╚═════╝ ",
        "                                                              ",
        "              ╔═══════════════════════════╗                 ",
        "              ║   Press Enter to exit     ║                 ",
        "              ╚═══════════════════════════╝                 ",
        "    ",
    ]
}

/// Renders the Exit tab content with ASCII art logo
pub fn render_exit_tab(f: &mut ratatui::Frame, area: Rect) {
    let logo_lines = get_mostro_logo();

    // Create a layout to center the logo vertically
    let inner_area = Block::default()
        .title("Exit")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR))
        .inner(area);

    let logo_height = logo_lines.len() as u16;
    let available_height = inner_area.height;

    // Calculate vertical centering
    let start_y = if logo_height < available_height {
        inner_area.y + (available_height.saturating_sub(logo_height)) / 2
    } else {
        inner_area.y
    };

    // Render the block
    f.render_widget(
        Block::default()
            .title("Exit")
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR)),
        area,
    );

    // Render ASCII art logo line by line
    for (idx, line) in logo_lines.iter().enumerate() {
        let y = start_y + idx as u16;
        if y < inner_area.y + inner_area.height {
            // Center the line horizontally
            let line_width = line.chars().count() as u16;
            let start_x = if line_width < inner_area.width {
                inner_area.x + (inner_area.width.saturating_sub(line_width)) / 2
            } else {
                inner_area.x
            };

            let centered_rect = Rect {
                x: start_x,
                y,
                width: line_width.min(inner_area.width),
                height: 1,
            };

            // Style different parts of the logo
            let spans: Vec<Span> = if line.contains('█') {
                // Style the ASCII art logo (block characters) with primary color
                line.chars()
                    .map(|c| {
                        if c == '█' {
                            Span::styled(
                                c.to_string(),
                                Style::default()
                                    .fg(PRIMARY_COLOR)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw(c.to_string())
                        }
                    })
                    .collect()
            } else if line.contains('╔') || line.contains('║') || line.contains('╚') {
                // Style the box with primary color
                line.chars()
                    .map(|c| {
                        if ['╔', '║', '╚', '═', '╗', '╝'].contains(&c) {
                            Span::styled(
                                c.to_string(),
                                Style::default()
                                    .fg(PRIMARY_COLOR)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw(c.to_string())
                        }
                    })
                    .collect()
            } else {
                vec![Span::raw(*line)]
            };

            f.render_widget(Paragraph::new(Line::from(spans)), centered_rect);
        }
    }
}

pub fn render_messages_tab(
    f: &mut ratatui::Frame,
    area: Rect,
    messages: &[OrderMessage],
    selected_idx: usize,
) {
    let block = Block::default()
        .title("Messages")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));

    f.render_widget(block.clone(), area);
    let inner = block.inner(area);

    if messages.is_empty() {
        let paragraph = Paragraph::new(Span::raw(
            "No messages yet. Messages related to your orders will appear here.",
        ))
        .block(Block::default())
        .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(paragraph, inner);
        return;
    }

    let selected_idx = selected_idx.min(messages.len().saturating_sub(1));
    let selected_msg = &messages[selected_idx];
    let selected_action = selected_msg.message.get_inner_message_kind().action.clone();

    let columns = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(36), Constraint::Percentage(64)],
    )
    .split(inner);

    let left_chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Min(0), Constraint::Length(2)],
    )
    .split(columns[0]);

    let items: Vec<ListItem> = messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            let order_short = if let Some(order_id) = msg.order_id {
                order_id.to_string().chars().take(8).collect::<String>()
            } else {
                "unknown".to_string()
            };
            let kind = message_order_kind_label(msg);
            let action = msg.message.get_inner_message_kind().action.clone();
            let action_label = message_action_compact_label(&action);

            let timestamp = DateTime::<Utc>::from_timestamp(msg.timestamp, 0)
                .map(|dt| dt.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "Unknown time".to_string());

            let is_selected = idx == selected_idx;
            let base_style = if is_selected {
                Style::default()
                    .bg(PRIMARY_COLOR)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else if !msg.read {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let kind_style = if is_selected {
                base_style
            } else if kind == "BUY" {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if kind == "SELL" {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                base_style
            };

            let line1 = Line::from(vec![
                Span::styled(format!("{order_short:8} "), base_style),
                Span::styled(format!("{kind:4} "), kind_style),
                Span::styled(action_label.to_string(), base_style),
            ]);
            let line2 = Line::from(vec![Span::styled(format!("  {timestamp}"), base_style)]);
            ListItem::new(vec![line1, line2])
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title("Orders")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        )
        .highlight_style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
        .highlight_symbol(">>")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    f.render_stateful_widget(
        list,
        left_chunks[0],
        &mut ratatui::widgets::ListState::default().with_selected(Some(selected_idx)),
    );

    let help = Paragraph::new(Line::from(vec![
        Span::styled("Up/Down", Style::default().fg(PRIMARY_COLOR)),
        Span::raw(" move   "),
        Span::styled("Enter", Style::default().fg(PRIMARY_COLOR)),
        Span::raw(" open action"),
    ]))
    .alignment(ratatui::layout::Alignment::Center)
    .block(Block::default().borders(Borders::ALL).title("Controls"));
    f.render_widget(help, left_chunks[1]);

    render_message_timeline_panel(f, columns[1], selected_msg, &selected_action);
}

fn render_message_timeline_panel(
    f: &mut ratatui::Frame,
    area: Rect,
    selected_msg: &OrderMessage,
    selected_action: &Action,
) {
    let right_chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(3),
            Constraint::Min(0),
        ],
    )
    .split(area);

    let order_id = selected_msg
        .order_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "Unknown order id".to_string());
    let timestamp = DateTime::<Utc>::from_timestamp(selected_msg.timestamp, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown time".to_string());

    let header = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("Order {order_id}"),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw(format!("Last message at {timestamp}"))),
    ])
    .block(Block::default().title("Selected Trade").borders(Borders::ALL));
    f.render_widget(header, right_chunks[0]);

    render_buy_stepper(f, right_chunks[1], message_buy_flow_step(selected_action));

    let warning = message_timeline_warning(selected_action)
        .unwrap_or("Trade is on normal path")
        .to_string();
    let warning_style = if message_timeline_warning(selected_action).is_some() {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let state = Paragraph::new(Line::from(Span::styled(warning, warning_style)))
        .alignment(ratatui::layout::Alignment::Center)
        .block(Block::default().title("State").borders(Borders::ALL));
    f.render_widget(state, right_chunks[2]);

    let action_text = message_action_compact_label(selected_action);
    let details = Paragraph::new(vec![
        Line::from(Span::styled(
            "Current action",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw(action_text)),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "Buy flow",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw("1) Paste Invoice")),
        Line::from(Span::raw("2) Wait for Seller")),
        Line::from(Span::raw("3) Chat with Seller")),
        Line::from(Span::raw("4) Send Fiat")),
        Line::from(Span::raw("5) Receive Sats")),
    ])
    .block(Block::default().title("Timeline Details").borders(Borders::ALL))
    .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(details, right_chunks[3]);
}

fn render_buy_stepper(f: &mut ratatui::Frame, area: Rect, current_step: usize) {
    let steps = [
        "Paste Invoice",
        "Wait for Seller",
        "Chat with Seller",
        "Send Fiat",
        "Receive Sats",
    ];
    let step_columns = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ],
    )
    .split(area);

    for (idx, step_name) in steps.iter().enumerate() {
        let step_number = idx + 1;
        let style = if step_number < current_step {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else if step_number == current_step {
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let indicator = if step_number <= current_step { "[x]" } else { "[ ]" };
        let step = Paragraph::new(vec![
            Line::from(Span::styled(format!("{indicator} Step {step_number}"), style)),
            Line::from(Span::styled(step_name.to_string(), style)),
        ])
        .alignment(ratatui::layout::Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
        f.render_widget(step, step_columns[idx]);
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
