use chrono::DateTime;
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::ui::constants::*;
use crate::ui::helpers::build_observer_scrollview_content;
use crate::ui::{AppState, UiMode, UserMode, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Render the user My Trades tab with active trades + peer chat.
pub fn render_my_trades_tab(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let chunks = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(20), Constraint::Percentage(80)],
    )
    .split(area);
    let sidebar_area = chunks[0];
    let main_area = chunks[1];

    let trades = &app.my_trade_orders;
    let selected_idx = if trades.is_empty() {
        0
    } else {
        app.selected_my_trade_idx
            .min(trades.len().saturating_sub(1))
    };

    let trades_block = Block::default()
        .title("My Active Trades")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));
    if trades.is_empty() {
        f.render_widget(
            Paragraph::new("No active trades yet")
                .block(trades_block)
                .alignment(ratatui::layout::Alignment::Center),
            sidebar_area,
        );
    } else {
        let items: Vec<ListItem> = trades
            .iter()
            .enumerate()
            .map(|(idx, order)| {
                let style = if idx == selected_idx {
                    Style::default().bg(PRIMARY_COLOR).fg(Color::Black)
                } else {
                    Style::default().fg(Color::White)
                };
                let id = order.id.as_deref().unwrap_or("unknown");
                let short_id = if id.len() > 20 {
                    format!("{}...", &id[..20])
                } else {
                    id.to_string()
                };
                ListItem::new(Line::from(Span::styled(short_id, style)))
            })
            .collect();
        f.render_widget(List::new(items).block(trades_block), sidebar_area);
    }

    if let Some(order) = trades.get(selected_idx) {
        let main_chunks = {
            let available_width = main_area.width.saturating_sub(4).max(1) as usize;
            let input_lines = if app.user_chat_input.is_empty() {
                1
            } else {
                let mut lines = 0usize;
                let mut current_width = 0usize;
                for word in app.user_chat_input.split_whitespace() {
                    let word_width = Span::raw(word).width();
                    let space_width = if current_width > 0 { 1 } else { 0 };
                    if current_width + space_width + word_width > available_width {
                        lines += 1;
                        current_width = word_width;
                    } else {
                        current_width += space_width + word_width;
                    }
                }
                if current_width > 0 {
                    lines += 1;
                }
                lines.max(1)
            };
            let input_height = (input_lines.min(10) as u16) + 2;

            Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(7),            // Header
                    Constraint::Min(0),               // Chat
                    Constraint::Length(input_height), // Input
                    Constraint::Length(2),            // Footer
                ],
            )
            .split(main_area)
        };

        let order_id = order.id.as_deref().unwrap_or("unknown").to_string();
        let created_str = order
            .created_at
            .and_then(|ts| DateTime::from_timestamp(ts, 0))
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let counterparty = order
            .counterparty_pubkey
            .as_deref()
            .map(|pk| {
                if pk.len() > 16 {
                    format!("{}...{}", &pk[..8], &pk[pk.len() - 8..])
                } else {
                    pk.to_string()
                }
            })
            .unwrap_or_else(|| "Unknown".to_string());
        let shared_state = if order.shared_key_hex.is_some() {
            "Ready"
        } else {
            "Missing peer key"
        };

        let header_lines = vec![
            Line::from(vec![
                Span::styled("Order ID: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    &order_id,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    order.status.as_deref().unwrap_or("Unknown"),
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Amount: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{} sats", order.amount),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("Fiat: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{} {}", order.fiat_amount, order.fiat_code),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("  "),
                Span::styled("Created: ", Style::default().fg(Color::Gray)),
                Span::styled(created_str, Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled("Counterparty: ", Style::default().fg(Color::Gray)),
                Span::styled(counterparty, Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("Shared Key: ", Style::default().fg(Color::Gray)),
                Span::styled(shared_state, Style::default().fg(Color::White)),
            ]),
        ];
        f.render_widget(
            Paragraph::new(header_lines)
                .block(
                    Block::default()
                        .title(Span::styled(
                            "Trade Info",
                            Style::default()
                                .fg(PRIMARY_COLOR)
                                .add_modifier(Modifier::BOLD),
                        ))
                        .borders(Borders::ALL)
                        .style(Style::default().bg(BACKGROUND_COLOR)),
                )
                .alignment(ratatui::layout::Alignment::Left),
            main_chunks[0],
        );

        let messages = app.user_trade_chats.get(&order_id);
        let chat_area = main_chunks[1];
        let inner_width = Block::default()
            .borders(Borders::ALL)
            .inner(chat_area)
            .width;
        let content_width = inner_width.saturating_sub(1).max(1);
        let max_content_width = (content_width / 2).max(1);
        let content = build_observer_scrollview_content(
            messages.map(|m| m.as_slice()).unwrap_or(&[]),
            content_width,
            Some(max_content_width),
        );
        let visible_count = content.line_start_per_message.len();
        app.user_chat_line_starts = content.line_start_per_message.clone();

        if visible_count > 0 {
            if let Some((ref last_order_id, last_count)) = app.user_chat_scroll_tracker {
                if *last_order_id == order_id && visible_count > last_count {
                    app.user_chat_scrollview_state.scroll_to_bottom();
                    app.user_chat_selected_message_idx = Some(visible_count.saturating_sub(1));
                }
            }
            app.user_chat_scroll_tracker = Some((order_id.clone(), visible_count));
            let sel = app.user_chat_selected_message_idx;
            if sel.is_none_or(|idx| idx >= visible_count.saturating_sub(1)) {
                app.user_chat_selected_message_idx = Some(visible_count.saturating_sub(1));
            }
        } else {
            app.user_chat_selected_message_idx = None;
            app.user_chat_scroll_tracker = Some((order_id.clone(), 0));
        }

        let chat_title = if visible_count > 0 {
            format!("Chat ({} messages)", visible_count)
        } else {
            "Chat (no messages)".to_string()
        };
        let chat_block = Block::default()
            .title(chat_title)
            .borders(Borders::ALL)
            .style(Style::default());
        let inner_area = chat_block.inner(chat_area);
        f.render_widget(chat_block, chat_area);

        let mut scroll_view = ScrollView::new(Size::new(
            content.content_width,
            content.content_height.max(1),
        ))
        .vertical_scrollbar_visibility(ScrollbarVisibility::Always);
        scroll_view.render_widget(
            Paragraph::new(content.lines).wrap(ratatui::widgets::Wrap { trim: true }),
            Rect::new(0, 0, content.content_width, content.content_height.max(1)),
        );
        f.render_stateful_widget(scroll_view, inner_area, &mut app.user_chat_scrollview_state);

        let is_input_focused = matches!(app.mode, UiMode::UserMode(UserMode::ManagingTradeChat));
        let is_input_enabled = app.user_chat_input_enabled;
        let input_title = if is_input_focused && is_input_enabled {
            "Message (typing enabled)"
        } else if is_input_focused && !is_input_enabled {
            "Message (disabled - Shift+I to enable)"
        } else {
            "Message"
        };
        let input_border_style = if is_input_focused && is_input_enabled {
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        f.render_widget(
            Paragraph::new(app.user_chat_input.as_str())
                .block(
                    Block::default()
                        .title(input_title)
                        .borders(Borders::ALL)
                        .border_style(input_border_style),
                )
                .wrap(ratatui::widgets::Wrap { trim: true }),
            main_chunks[2],
        );

        let line1 = if main_chunks[3].width < 90 {
            format!(
                "{} | {} | {} | {} | {}",
                HELP_KEY,
                FOOTER_ENTER_SEND,
                FOOTER_SHIFT_I_INPUT_TOGGLE,
                FOOTER_PGUP_PGDN_SCROLL_CHAT,
                FOOTER_END_BOTTOM
            )
        } else {
            format!(
                "{} | {} | {} | {} | {} | {}",
                HELP_KEY,
                FOOTER_ENTER_SEND,
                FOOTER_SHIFT_I_INPUT_TOGGLE,
                FOOTER_PGUP_PGDN_SCROLL_CHAT,
                FOOTER_END_BOTTOM,
                FOOTER_UP_DOWN_SELECT_TRADE
            )
        };
        let line2 = if order.shared_key_hex.is_none() {
            Some("Chat send disabled until counterparty key is available".to_string())
        } else {
            None
        };
        if let Some(line2) = line2 {
            let footer_chunks = Layout::new(
                Direction::Vertical,
                [Constraint::Length(1), Constraint::Length(1)],
            )
            .split(main_chunks[3]);
            f.render_widget(Paragraph::new(line1), footer_chunks[0]);
            f.render_widget(
                Paragraph::new(line2).style(Style::default().fg(Color::Yellow)),
                footer_chunks[1],
            );
        } else {
            f.render_widget(Paragraph::new(line1), main_chunks[3]);
        }

        app.selected_my_trade_idx = selected_idx;
    } else {
        f.render_widget(
            Paragraph::new("Select a trade from the sidebar")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .style(Style::default().bg(BACKGROUND_COLOR)),
                )
                .alignment(ratatui::layout::Alignment::Center),
            main_area,
        );
        app.selected_my_trade_idx = 0;
    }
}
