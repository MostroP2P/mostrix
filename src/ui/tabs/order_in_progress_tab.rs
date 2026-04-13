use std::collections::HashMap;

use chrono::DateTime;
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::ui::constants::HELP_KEY;
use crate::ui::{AppState, UiMode, UserChatSender, UserMode};
use crate::ui::UserOrderChatMessage;
use crate::ui::{BACKGROUND_COLOR, PRIMARY_COLOR};
use mostro_core::prelude::{Payload, Status};

#[derive(Clone)]
struct OrderChatListItem {
    order_id: String,
    status: Option<Status>,
    amount: Option<i64>,
    fiat: Option<(i64, String)>,
}

fn status_from_message(msg: &crate::ui::OrderMessage) -> Option<Status> {
    msg.order_status
}

fn is_order_chat_actionable(status: Option<Status>) -> bool {
    matches!(
        status,
        Some(Status::WaitingPayment)
            | Some(Status::WaitingBuyerInvoice)
            | Some(Status::SettledHoldInvoice)
            | Some(Status::InProgress)
            | Some(Status::Active)
            | Some(Status::FiatSent)
    )
}

fn build_active_orders(messages: &[crate::ui::OrderMessage]) -> Vec<OrderChatListItem> {
    let mut by_order: HashMap<String, OrderChatListItem> = HashMap::new();
    for msg in messages {
        let Some(order_id) = msg.order_id else {
            continue;
        };
        let key = order_id.to_string();
        by_order
            .entry(key.clone())
            .and_modify(|entry| {
                entry.status = status_from_message(msg).or(entry.status);
                if entry.amount.is_none() {
                    if let Some(Payload::Order(order)) =
                        &msg.message.get_inner_message_kind().payload
                    {
                        entry.amount = Some(order.amount);
                        entry.fiat = Some((order.fiat_amount, order.fiat_code.clone()));
                    }
                }
            })
            .or_insert_with(|| {
                let mut amount = None;
                let mut fiat = None;
                if let Some(Payload::Order(order)) = &msg.message.get_inner_message_kind().payload {
                    amount = Some(order.amount);
                    fiat = Some((order.fiat_amount, order.fiat_code.clone()));
                }
                OrderChatListItem {
                    order_id: key,
                    status: status_from_message(msg),
                    amount,
                    fiat,
                }
            });
    }

    let mut rows: Vec<OrderChatListItem> = by_order
        .into_values()
        .filter(|row| is_order_chat_actionable(row.status))
        .collect();
    rows.sort_by(|a, b| a.order_id.cmp(&b.order_id));
    rows
}

fn build_order_chat_content(
    messages: &[UserOrderChatMessage],
    content_width: u16,
) -> (Vec<Line<'static>>, u16, Vec<usize>) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    for msg in messages {
        starts.push(lines.len());
        let sender = msg.sender;
        let label = match sender {
            UserChatSender::You => "You",
            UserChatSender::Peer => "Peer",
        };
        let color = match sender {
            UserChatSender::You => Color::Cyan,
            UserChatSender::Peer => Color::Green,
        };
        let ts = DateTime::from_timestamp(msg.timestamp, 0)
            .map(|dt| dt.format("%d-%m-%Y %H:%M").to_string())
            .unwrap_or_else(|| "unknown time".to_string());
        lines.push(Line::from(vec![Span::styled(
            format!("{label} - {ts}"),
            Style::default().fg(color),
        )]));
        lines.push(Line::from(Span::styled(
            msg.content.clone(),
            Style::default().fg(color),
        )));
        lines.push(Line::from(""));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No messages yet. Start the conversation!",
            Style::default().fg(Color::Gray),
        )));
    }
    (lines, content_width.max(1), starts)
}

pub fn render_order_in_progress(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let messages_snapshot = match app.messages.lock() {
        Ok(g) => g.clone(),
        Err(_) => Vec::new(),
    };
    let active_orders = build_active_orders(&messages_snapshot);

    let chunks = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(22), Constraint::Percentage(78)],
    )
    .split(area);
    let sidebar_area = chunks[0];
    let main_area = chunks[1];

    let selected_idx = if active_orders.is_empty() {
        0
    } else {
        app.selected_order_chat_idx
            .min(active_orders.len().saturating_sub(1))
    };

    let sidebar_block = Block::default()
        .title("Orders In Progress")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));
    if active_orders.is_empty() {
        f.render_widget(
            Paragraph::new("No actionable orders yet")
                .block(sidebar_block)
                .alignment(ratatui::layout::Alignment::Center),
            sidebar_area,
        );
        f.render_widget(
            Paragraph::new("Select an order from sidebar when available.\nCtrl+H: Help").block(
                Block::default()
                    .title("Order Chat")
                    .borders(Borders::ALL)
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            ),
            main_area,
        );
        return;
    }

    let items: Vec<ListItem> = active_orders
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let style = if idx == selected_idx {
                Style::default().bg(PRIMARY_COLOR).fg(Color::Black)
            } else {
                Style::default().fg(Color::White)
            };
            let short_id = if row.order_id.len() > 16 {
                format!("{}...", &row.order_id[..16])
            } else {
                row.order_id.clone()
            };
            ListItem::new(Line::from(Span::styled(short_id, style)))
        })
        .collect();
    f.render_widget(List::new(items).block(sidebar_block), sidebar_area);

    let selected = &active_orders[selected_idx];
    let main_chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(5),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ],
    )
    .split(main_area);

    let status_label = selected
        .status
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let amount_line = match (selected.amount, &selected.fiat) {
        (Some(sats), Some((fiat_amount, fiat_code))) => {
            format!("{sats} sats | {fiat_amount} {fiat_code}")
        }
        (Some(sats), None) => format!("{sats} sats"),
        _ => "amount N/A".to_string(),
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Order: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(selected.order_id.clone()),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(status_label),
            ]),
            Line::from(vec![
                Span::styled("Amount: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(amount_line),
            ]),
        ])
        .block(
            Block::default()
                .title("Order details")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        ),
        main_chunks[0],
    );

    let chat_messages = app
        .order_chats
        .get(&selected.order_id)
        .cloned()
        .unwrap_or_default();
    let mut scroll_view = ScrollView::new(Size::new(
        main_chunks[1].width.saturating_sub(2),
        main_chunks[1].height.saturating_sub(2),
    ))
    .scrollbars_visibility(ScrollbarVisibility::Always);
    let (chat_lines, content_width, line_starts) =
        build_order_chat_content(&chat_messages, main_chunks[1].width.saturating_sub(4).max(1));
    app.order_chat_line_starts = line_starts;
    scroll_view.render_widget(
        Paragraph::new(chat_lines.clone()).wrap(Wrap { trim: true }),
        Rect::new(0, 0, content_width.max(1), (chat_lines.len() as u16).max(1)),
    );
    f.render_stateful_widget(
        scroll_view,
        main_chunks[1],
        &mut app.order_chat_scrollview_state,
    );

    let input_active =
        matches!(app.mode, UiMode::UserMode(UserMode::Normal)) && app.order_chat_input_enabled;
    f.render_widget(
        Paragraph::new(app.order_chat_input.clone())
            .wrap(Wrap { trim: false })
            .style(if input_active {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            })
            .block(
                Block::default()
                    .title(if app.order_chat_input_enabled {
                        "Message"
                    } else {
                        "Message (disabled: Shift+I)"
                    })
                    .borders(Borders::ALL),
            ),
        main_chunks[2],
    );

    let footer = if app.order_chat_input_enabled {
        format!(
            "↑↓: Select order | Enter: Send | Shift+I: Disable | {}",
            HELP_KEY
        )
    } else {
        format!("↑↓: Select order | Shift+I: Enable | {}", HELP_KEY)
    };
    f.render_widget(Paragraph::new(footer), main_chunks[3]);
}

pub fn push_local_order_chat_message(
    app: &mut AppState,
    order_id: &str,
    content: String,
    is_local_sender: bool,
) -> UserOrderChatMessage {
    let msg = UserOrderChatMessage {
        sender: if is_local_sender {
            UserChatSender::You
        } else {
            UserChatSender::Peer
        },
        content,
        timestamp: chrono::Utc::now().timestamp(),
        attachment: None,
    };
    app.order_chats
        .entry(order_id.to_string())
        .or_default()
        .push(msg.clone());
    msg
}
