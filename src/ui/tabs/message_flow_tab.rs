//! Messages tab: order list sidebar and trade timeline detail panel.

use chrono::{DateTime, Utc};
use mostro_core::prelude::*;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::ui::orders::{
    message_action_compact_label, message_buy_flow_step, message_order_kind_label,
    message_timeline_warning,
};
use crate::ui::{OrderMessage, BACKGROUND_COLOR, PRIMARY_COLOR};

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
    .block(
        Block::default()
            .title("Selected Trade")
            .borders(Borders::ALL),
    );
    f.render_widget(header, right_chunks[0]);

    render_buy_stepper(f, right_chunks[1], message_buy_flow_step(selected_action));

    let warning = message_timeline_warning(selected_action)
        .unwrap_or("Trade is on normal path")
        .to_string();
    let warning_style = if message_timeline_warning(selected_action).is_some() {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
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
    .block(
        Block::default()
            .title("Timeline Details")
            .borders(Borders::ALL),
    )
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
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else if step_number == current_step {
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let indicator = if step_number <= current_step {
            "[x]"
        } else {
            "[ ]"
        };
        let step = Paragraph::new(vec![
            Line::from(Span::styled(
                format!("{indicator} Step {step_number}"),
                style,
            )),
            Line::from(Span::styled(step_name.to_string(), style)),
        ])
        .alignment(ratatui::layout::Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
        f.render_widget(step, step_columns[idx]);
    }
}
