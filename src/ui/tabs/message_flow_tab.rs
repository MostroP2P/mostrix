//! Messages tab: order list sidebar and trade timeline detail panel.

use chrono::{DateTime, Utc};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph};

use crate::ui::helpers;
use crate::ui::orders::{
    listing_timeline_labels, message_action_compact_label_for_message, message_action_emoji,
    message_order_kind_label, message_timeline_warning, message_timeline_warning_for_order_status,
    message_trade_timeline_step, FlowStep, StepLabel,
};
use crate::ui::{OrderMessage, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_messages_tab(
    f: &mut ratatui::Frame,
    area: Rect,
    messages: &[OrderMessage],
    selected_idx: usize,
) {
    let block = Block::default()
        .title(sidebar_title(messages))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));

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

    let columns = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(36), Constraint::Percentage(64)],
    )
    .split(inner);

    let left_chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Min(0), Constraint::Length(1)],
    )
    .split(columns[0]);

    let separator_width = left_chunks[0].width as usize;
    let items = build_sidebar_items(messages, selected_idx, separator_width);

    let list = List::new(items)
        .highlight_style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
        .highlight_symbol("▶ ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    f.render_stateful_widget(
        list,
        left_chunks[0],
        &mut ratatui::widgets::ListState::default().with_selected(Some(selected_idx)),
    );

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("↑↓", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" move · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" open · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Ctrl+H", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" help", Style::default().fg(Color::DarkGray)),
    ]))
    .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(footer, left_chunks[1]);

    render_message_timeline_panel(f, columns[1], selected_msg);
}

/// Sidebar title with total trade count and, when present, an unread badge.
fn sidebar_title(messages: &[OrderMessage]) -> Line<'static> {
    let unread = messages.iter().filter(|m| !m.read).count();
    let mut spans = vec![Span::styled(
        format!(" 📨 My Trades ({}) ", messages.len()),
        Style::default()
            .fg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD),
    )];
    if unread > 0 {
        spans.push(Span::styled("· ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(
            format!("● {unread} new "),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

/// Emoji + color for the order-kind dot shown on each sidebar row.
fn kind_dot(kind_label: &str) -> (&'static str, Color) {
    match kind_label {
        "BUY" => ("🟢", Color::Green),
        "SELL" => ("🔴", Color::Red),
        _ => ("⚪", Color::DarkGray),
    }
}

/// Builds the three-line-plus-separator sidebar rows: kind/id, action, relative time/unread.
fn build_sidebar_items(
    messages: &[OrderMessage],
    selected_idx: usize,
    separator_width: usize,
) -> Vec<ListItem<'static>> {
    let last_idx = messages.len().saturating_sub(1);
    messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
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
                Style::default().fg(Color::Gray)
            };

            let kind_label = message_order_kind_label(msg);
            let (dot, kind_color) = kind_dot(kind_label);
            let kind_style = if is_selected {
                base_style
            } else {
                Style::default().fg(kind_color).add_modifier(Modifier::BOLD)
            };

            let short_id = helpers::short_order_id(msg.order_id);
            let line1 = Line::from(vec![
                Span::styled(format!("{dot} "), kind_style),
                Span::styled(format!("{kind_label:<4} "), kind_style),
                Span::styled(short_id, base_style),
            ]);

            let action = msg.message.get_inner_message_kind().action.clone();
            let emoji = message_action_emoji(&action);
            let action_label = message_action_compact_label_for_message(msg);
            let line2 = Line::from(vec![
                Span::styled(format!("  {emoji} "), base_style),
                Span::styled(action_label.to_string(), base_style),
            ]);

            let time = helpers::relative_time_compact(msg.timestamp);
            let mut line3_spans = vec![
                Span::styled("  🕐 ", base_style),
                Span::styled(time, base_style),
            ];
            if !msg.read {
                line3_spans.push(Span::styled(
                    " · unread ",
                    Style::default().fg(Color::DarkGray),
                ));
                line3_spans.push(Span::styled("●", Style::default().fg(Color::Yellow)));
            }
            let line3 = Line::from(line3_spans);

            let mut lines = vec![line1, line2, line3];
            if idx != last_idx {
                lines.push(Line::from(Span::styled(
                    "─".repeat(separator_width.max(1)),
                    Style::default().fg(Color::DarkGray),
                )));
            }

            ListItem::new(lines)
        })
        .collect()
}

fn render_message_timeline_panel(f: &mut ratatui::Frame, area: Rect, selected_msg: &OrderMessage) {
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

    let step_labels = listing_timeline_labels(selected_msg);
    render_trade_stepper(
        f,
        right_chunks[1],
        message_trade_timeline_step(selected_msg),
        &step_labels,
    );

    let warning_from_status = message_timeline_warning_for_order_status(selected_msg.order_status);
    let warning_opt = warning_from_status.or_else(|| {
        message_timeline_warning(&selected_msg.message.get_inner_message_kind().action)
    });
    let warning = warning_opt.unwrap_or("Trade is on normal path").to_string();
    let warning_style = if warning_opt.is_some() {
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

    let action_text = message_action_compact_label_for_message(selected_msg);
    let details = Paragraph::new(vec![
        Line::from(Span::styled(
            "Current action",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw(action_text)),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "Trade timeline",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw(format!("1) {}", step_labels[0].as_single_line()))),
        Line::from(Span::raw(format!("2) {}", step_labels[1].as_single_line()))),
        Line::from(Span::raw(format!("3) {}", step_labels[2].as_single_line()))),
        Line::from(Span::raw(format!("4) {}", step_labels[3].as_single_line()))),
        Line::from(Span::raw(format!("5) {}", step_labels[4].as_single_line()))),
        Line::from(Span::raw(format!("6) {}", step_labels[5].as_single_line()))),
    ])
    .block(
        Block::default()
            .title("Timeline Details")
            .borders(Borders::ALL),
    )
    .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(details, right_chunks[3]);
}

fn render_trade_stepper(
    f: &mut ratatui::Frame,
    area: Rect,
    current_step: FlowStep,
    steps: &[StepLabel; 6],
) {
    let current_step = current_step.step_number();
    let step_columns = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Percentage(17),
            Constraint::Percentage(17),
            Constraint::Percentage(17),
            Constraint::Percentage(17),
            Constraint::Percentage(16),
            Constraint::Percentage(16),
        ],
    )
    .split(area);

    for (idx, step_label) in steps.iter().enumerate() {
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

        let step = Paragraph::new(vec![
            Line::from(Span::styled(format!("Step {step_number}"), style)),
            Line::from(Span::styled(step_label.top.to_string(), style)),
            Line::from(Span::styled(step_label.bottom.to_string(), style)),
        ])
        .alignment(ratatui::layout::Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
        f.render_widget(step, step_columns[idx]);
    }
}

#[cfg(test)]
mod sidebar_tests {
    use super::*;
    use mostro_core::prelude::{Action, Message};
    use nostr_sdk::Keys;

    fn sample_message(read: bool) -> OrderMessage {
        let keys = Keys::generate();
        OrderMessage {
            message: Message::new_order(None, None, None, Action::PayInvoice, None),
            timestamp: 0,
            sender: keys.public_key(),
            order_id: None,
            trade_index: 0,
            sat_amount: None,
            buyer_invoice: None,
            order_kind: None,
            is_mine: None,
            order_status: None,
            read,
            auto_popup_shown: false,
        }
    }

    fn spans_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn kind_dot_maps_buy_sell_and_unknown_to_distinct_glyphs() {
        assert_eq!(kind_dot("BUY"), ("🟢", Color::Green));
        assert_eq!(kind_dot("SELL"), ("🔴", Color::Red));
        assert_eq!(kind_dot("N/A"), ("⚪", Color::DarkGray));
    }

    #[test]
    fn sidebar_title_omits_unread_badge_when_all_read() {
        let messages = vec![sample_message(true), sample_message(true)];
        let text = spans_text(&sidebar_title(&messages));
        assert!(text.contains("My Trades (2)"));
        assert!(!text.contains("new"));
    }

    #[test]
    fn sidebar_title_shows_unread_count_badge() {
        let messages = vec![
            sample_message(false),
            sample_message(true),
            sample_message(false),
        ];
        let text = spans_text(&sidebar_title(&messages));
        assert!(text.contains("My Trades (3)"));
        assert!(text.contains("● 2 new"));
    }

    #[test]
    fn build_sidebar_items_adds_separator_between_rows_but_not_after_last() {
        let messages = vec![sample_message(false), sample_message(true)];
        let items = build_sidebar_items(&messages, 0, 20);
        assert_eq!(items.len(), 2);
        // Non-last row: kind/id + action + time + separator = 4 lines.
        assert_eq!(items[0].height(), 4);
        // Last row: no trailing separator = 3 lines.
        assert_eq!(items[1].height(), 3);
    }
}
