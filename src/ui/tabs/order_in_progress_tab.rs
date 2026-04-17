use chrono::DateTime;
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::ui::constants::{
    FOOTER_MYTRADES_END_BOTTOM, FOOTER_MYTRADES_ENTER_SEND, FOOTER_MYTRADES_PGUP_PGDN_SCROLL_CHAT,
    FOOTER_MYTRADES_SELECT_ORDER, FOOTER_MYTRADES_SHIFT_C_CANCEL,
    FOOTER_MYTRADES_SHIFT_F_FIAT_SENT, FOOTER_MYTRADES_SHIFT_I_DISABLE,
    FOOTER_MYTRADES_SHIFT_I_ENABLE, FOOTER_MYTRADES_SHIFT_R_RELEASE, FOOTER_MYTRADES_SHIFT_V_RATE,
    HELP_KEY,
};
use crate::ui::helpers::{build_active_order_chat_list, format_user_rating};
use crate::ui::UserOrderChatMessage;
use crate::ui::{AppState, UiMode, UserChatSender, UserMode};
use crate::ui::{BACKGROUND_COLOR, PRIMARY_COLOR};

/// `Order ID: …` for the sidebar — same style as disputes; shows the full id when it fits the column.
fn sidebar_order_list_label(order_id: &str, inner_width: u16) -> String {
    const PREFIX: &str = "Order ID: ";
    let w = inner_width as usize;
    if w == 0 {
        return String::new();
    }
    let full = format!("{PREFIX}{order_id}");
    if full.chars().count() <= w {
        return full;
    }
    if w <= 3 {
        return ".".repeat(w);
    }
    let head: String = full.chars().take(w.saturating_sub(3)).collect();
    format!("{head}...")
}

fn build_order_chat_content(
    messages: &[UserOrderChatMessage],
    content_width: u16,
) -> (Vec<Line<'static>>, u16, Vec<usize>) {
    fn wrap_text_to_lines(content: &str, max_width: u16) -> Vec<String> {
        if max_width == 0 {
            return vec![String::new()];
        }
        let max = max_width as usize;
        let mut wrapped = Vec::new();
        let mut current = String::new();

        fn chunks_for_word(word: &str, max: usize) -> Vec<String> {
            if word.chars().count() <= max {
                return vec![word.to_string()];
            }
            word.chars()
                .collect::<Vec<_>>()
                .chunks(max)
                .map(|chunk| chunk.iter().collect())
                .collect()
        }

        for word in content.split_whitespace() {
            for chunk in chunks_for_word(word, max) {
                let chunk_len = chunk.chars().count();
                let pending_len = if current.is_empty() {
                    chunk_len
                } else {
                    current.chars().count() + 1 + chunk_len
                };
                if pending_len > max && !current.is_empty() {
                    wrapped.push(current);
                    current = chunk;
                } else if current.is_empty() {
                    current = chunk;
                } else {
                    current.push(' ');
                    current.push_str(&chunk);
                }
            }
        }
        if wrapped.is_empty() && current.is_empty() && !content.is_empty() {
            return vec![content.to_string()];
        }
        if !current.is_empty() {
            wrapped.push(current);
        }
        if wrapped.is_empty() {
            wrapped.push(String::new());
        }
        wrapped
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    let max_content_width = (content_width / 2).max(1);
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
        let header = Span::styled(format!("{label} - {ts}"), Style::default().fg(color));
        let wrapped_lines = wrap_text_to_lines(&msg.content, max_content_width);
        let peer_is_right_aligned = matches!(sender, UserChatSender::Peer);
        if peer_is_right_aligned {
            lines.push(header.into_right_aligned_line());
            for line in wrapped_lines {
                lines
                    .push(Span::styled(line, Style::default().fg(color)).into_right_aligned_line());
            }
        } else {
            lines.push(Line::from(header));
            for line in wrapped_lines {
                lines.push(Line::from(Span::styled(line, Style::default().fg(color))));
            }
        }
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
    let active_orders = build_active_order_chat_list(&messages_snapshot);

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
            Paragraph::new("No active orders yet")
                .block(sidebar_block)
                .alignment(ratatui::layout::Alignment::Center),
            sidebar_area,
        );
        let empty_main_chunks = Layout::new(
            Direction::Vertical,
            [Constraint::Min(0), Constraint::Length(1)],
        )
        .split(main_area);
        f.render_widget(
            Paragraph::new("Select an order from sidebar when available.").block(
                Block::default()
                    .title("Order Chat")
                    .borders(Borders::ALL)
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            ),
            empty_main_chunks[0],
        );
        f.render_widget(Paragraph::new(HELP_KEY), empty_main_chunks[1]);
        return;
    }

    let sidebar_text_width = sidebar_block.inner(sidebar_area).width.max(1);
    let items: Vec<ListItem> = active_orders
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let style = if idx == selected_idx {
                Style::default().bg(PRIMARY_COLOR).fg(Color::Black)
            } else {
                Style::default().fg(Color::White)
            };
            let label = sidebar_order_list_label(&row.order_id, sidebar_text_width);
            ListItem::new(Line::from(Span::styled(label, style)))
        })
        .collect();
    f.render_widget(List::new(items).block(sidebar_block), sidebar_area);

    let selected = &active_orders[selected_idx];
    let input_height: u16 = 3;

    let status_label = selected
        .status
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let order_kind = selected.kind.as_deref().unwrap_or("Unknown");
    let created_str = selected
        .created_at
        .and_then(|ts| DateTime::from_timestamp(ts, 0))
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let truncate_pubkey = |pubkey: &str| -> String {
        if pubkey.len() > 16 {
            format!("{}...{}", &pubkey[..8], &pubkey[pubkey.len() - 8..])
        } else {
            pubkey.to_string()
        }
    };
    let initiator_pubkey_display = selected
        .initiator_pubkey
        .as_deref()
        .map(truncate_pubkey)
        .unwrap_or_else(|| "Unknown".to_string());
    let initiator_role = match selected.is_mine {
        Some(true) => "Maker",
        Some(false) => "Taker",
        None => "Initiator",
    };
    let trade_id = selected
        .trade_index
        .map(|t| t.to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let payment_method = selected.payment_method.as_deref().unwrap_or("Unknown");
    let premium_text = selected
        .premium
        .map(|p| format!("{p}%"))
        .unwrap_or_else(|| "Unknown".to_string());
    let amount_line = match (selected.amount, &selected.fiat) {
        (Some(sats), Some((fiat_amount, fiat_code))) => {
            format!("{sats} sats | {fiat_amount} {fiat_code}")
        }
        (Some(sats), None) => format!("{sats} sats"),
        _ => "amount N/A".to_string(),
    };

    // TODO(My Trades header): Wire "Privacy:", "Buyer -", "Seller -" from trade privacy / full-privacy
    // signals once available on DM payloads or local `orders` (see dispute UI + `Order::is_full_privacy_order`).
    // Omit that row until then — avoid static "Unknown" placeholders.

    let mut header_lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("Order ID: ", Style::default().fg(Color::Gray)),
            Span::styled(
                selected.order_id.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Trade ID: ", Style::default().fg(Color::Gray)),
            Span::styled(
                trade_id,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Type: ", Style::default().fg(Color::Gray)),
            Span::styled(
                order_kind.to_string(),
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Status: ", Style::default().fg(Color::Gray)),
            Span::styled(status_label, Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(
                format!("Initiator: {initiator_role} "),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(initiator_pubkey_display, Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("Created: ", Style::default().fg(Color::Gray)),
            Span::styled(created_str, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Amount: ", Style::default().fg(Color::Gray)),
            Span::styled(
                amount_line,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let gray = Style::default().fg(Color::Gray);
    let yellow = Style::default().fg(Color::Yellow);
    let mut payment_row: Vec<Span> = Vec::new();
    let mut any_rating = false;
    if let Some(ref info) = selected.buyer_reputation {
        payment_row.push(Span::styled("Buyer Rating: ", gray));
        payment_row.push(Span::styled(format_user_rating(Some(info)), yellow));
        any_rating = true;
    }
    if let Some(ref info) = selected.seller_reputation {
        if any_rating {
            payment_row.push(Span::raw("  |  "));
        }
        payment_row.push(Span::styled("Seller Rating: ", gray));
        payment_row.push(Span::styled(format_user_rating(Some(info)), yellow));
        any_rating = true;
    }
    if any_rating {
        payment_row.push(Span::raw("  |  "));
    }
    payment_row.push(Span::styled("Payment: ", gray));
    payment_row.push(Span::styled(
        payment_method.to_string(),
        Style::default().fg(Color::White),
    ));
    payment_row.push(Span::raw("  "));
    payment_row.push(Span::styled("Premium: ", gray));
    payment_row.push(Span::styled(premium_text, yellow));
    header_lines.push(Line::from(payment_row));

    let header_height = header_lines.len() as u16;

    let wants_two_line_footer = main_area.width >= 90;
    let can_fit_two_line_footer = main_area
        .height
        .saturating_sub(header_height.saturating_add(input_height))
        >= 2;
    let footer_height: u16 = if wants_two_line_footer && can_fit_two_line_footer {
        2
    } else {
        1
    };
    let allow_two_line_footer = footer_height >= 2;

    let main_chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(header_height),
            Constraint::Min(0),
            Constraint::Length(input_height),
            Constraint::Length(footer_height),
        ],
    )
    .split(main_area);
    f.render_widget(
        Paragraph::new(header_lines).block(
            Block::default()
                .title(Span::styled(
                    "📋 Order Info",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ))
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
    let (chat_lines, content_width, line_starts) = build_order_chat_content(
        &chat_messages,
        main_chunks[1].width.saturating_sub(4).max(1),
    );
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

    // Footer (width-aware, similar to disputes footer)
    let footer_area = main_chunks[3];
    let footer_width = footer_area.width;
    let (footer_line1, footer_line2) = if footer_width < 50 {
        (HELP_KEY.to_string(), None)
    } else if footer_width < 90 || !allow_two_line_footer {
        // Single-line compact footer
        let base = if app.order_chat_input_enabled {
            format!(
                "{} | {} | {} | {} | {}",
                HELP_KEY,
                FOOTER_MYTRADES_SELECT_ORDER,
                FOOTER_MYTRADES_ENTER_SEND,
                FOOTER_MYTRADES_SHIFT_I_DISABLE,
                FOOTER_MYTRADES_SHIFT_C_CANCEL
            )
        } else {
            format!(
                "{} | {} | {} | {}",
                HELP_KEY,
                FOOTER_MYTRADES_SELECT_ORDER,
                FOOTER_MYTRADES_SHIFT_I_ENABLE,
                FOOTER_MYTRADES_SHIFT_C_CANCEL
            )
        };
        (base, None)
    } else {
        // Two-line rich footer when wide enough
        if app.order_chat_input_enabled {
            (
                format!(
                    "{} | {} | {} | {} | {} | {}",
                    HELP_KEY,
                    FOOTER_MYTRADES_SELECT_ORDER,
                    FOOTER_MYTRADES_ENTER_SEND,
                    FOOTER_MYTRADES_SHIFT_I_DISABLE,
                    FOOTER_MYTRADES_SHIFT_C_CANCEL,
                    FOOTER_MYTRADES_SHIFT_F_FIAT_SENT
                ),
                Some(format!(
                    "{} | {} | {} | {}",
                    FOOTER_MYTRADES_PGUP_PGDN_SCROLL_CHAT,
                    FOOTER_MYTRADES_END_BOTTOM,
                    FOOTER_MYTRADES_SHIFT_R_RELEASE,
                    FOOTER_MYTRADES_SHIFT_V_RATE
                )),
            )
        } else {
            (
                format!(
                    "{} | {} | {} | {} | {}",
                    HELP_KEY,
                    FOOTER_MYTRADES_SELECT_ORDER,
                    FOOTER_MYTRADES_SHIFT_I_ENABLE,
                    FOOTER_MYTRADES_SHIFT_C_CANCEL,
                    FOOTER_MYTRADES_SHIFT_F_FIAT_SENT
                ),
                Some(format!(
                    "{} | {} | {} | {}",
                    FOOTER_MYTRADES_PGUP_PGDN_SCROLL_CHAT,
                    FOOTER_MYTRADES_END_BOTTOM,
                    FOOTER_MYTRADES_SHIFT_R_RELEASE,
                    FOOTER_MYTRADES_SHIFT_V_RATE
                )),
            )
        }
    };

    if let Some(line2) = footer_line2 {
        let footer_chunks = Layout::new(
            Direction::Vertical,
            [Constraint::Length(1), Constraint::Length(1)],
        )
        .split(footer_area);
        f.render_widget(Paragraph::new(footer_line1), footer_chunks[0]);
        f.render_widget(Paragraph::new(line2), footer_chunks[1]);
    } else {
        f.render_widget(Paragraph::new(footer_line1), footer_area);
    }
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
