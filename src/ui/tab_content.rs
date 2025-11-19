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

            let style = if idx == selected_idx {
                Style::default()
                    .bg(PRIMARY_COLOR)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
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

pub fn render_message_notification(f: &mut ratatui::Frame, notification: &MessageNotification) {
    let area = f.area();
    let popup_width = 60;
    let popup_height = 8;
    let popup_x = area.x + (area.width - popup_width) / 2;
    let popup_y = area.y + (area.height - popup_height) / 2;
    let popup = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    let block = Block::default()
        .title("📨 New Message")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));

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

    let order_id_str = if let Some(order_id) = notification.order_id {
        format!(
            "Order: {}",
            order_id.to_string().chars().take(8).collect::<String>()
        )
    } else {
        "Order: Unknown".to_string()
    };

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
