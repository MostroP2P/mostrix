//! My Trades / order chat UI. Ctrl+H and Shift+H help overlays are styled in [`crate::ui::help_popup`].

use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap};
use tui_scrollview::{ScrollView, ScrollbarVisibility};
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

use crate::ui::constants::{
    FOOTER_CTRL_O_SEND_FILE, FOOTER_CTRL_SHIFT_O_RETRY, FOOTER_CTRL_S_SAVE_FILE,
    FOOTER_MYTRADES_END_BOTTOM, FOOTER_MYTRADES_ENTER_SEND, FOOTER_MYTRADES_PGUP_PGDN_SCROLL_CHAT,
    FOOTER_MYTRADES_SELECT_ORDER, FOOTER_MYTRADES_SHIFT_C_CANCEL, FOOTER_MYTRADES_SHIFT_D_DISPUTE,
    FOOTER_MYTRADES_SHIFT_F_FIAT_SENT, FOOTER_MYTRADES_SHIFT_I_DISABLE,
    FOOTER_MYTRADES_SHIFT_I_ENABLE, FOOTER_MYTRADES_SHIFT_K_KCONV, FOOTER_MYTRADES_SHIFT_R_RELEASE,
    FOOTER_MYTRADES_SHIFT_V_RATE, FOOTER_MYTRADES_TAB_CHAT, FOOTER_SENDING_ATTACHMENT, HELP_KEY,
};
use crate::ui::helpers::{
    active_order_chat_list_snapshot, count_order_attachments, format_local_timestamp,
    format_user_rating_compact,
};
use crate::ui::UserOrderChatMessage;
use crate::ui::{AppState, UserChatChannel, UserChatSender};
use crate::ui::{BACKGROUND_COLOR, PRIMARY_COLOR};
use mostro_core::prelude::UserInfo;

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

/// Keep a bordered chat pane with room for a sender line plus a wrapped message.
/// Widget height includes `Borders::ALL` (2 rows); inner height is this minus 2.
const ORDER_INFO_MIN_CHAT: u16 = 6;
const ORDER_INFO_MIN_FOOTER: u16 = 1;

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(Span::width).sum()
}

fn truncate_to_width(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if Span::raw(s).width() <= max {
        return s.to_string();
    }
    if max <= 3 {
        return ".".repeat(max);
    }
    let mut out = String::new();
    let mut w = 0;
    let budget = max - 3;
    for ch in s.chars() {
        let cw = Span::raw(ch.to_string()).width();
        if w + cw > budget {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push_str("...");
    out
}

fn labeled_value_line(
    label: &str,
    value: &str,
    value_style: Style,
    inner_width: u16,
) -> Line<'static> {
    let gray = Style::default().fg(Color::Gray);
    let label_text = format!("{label} ");
    let label_w = Span::raw(label_text.as_str()).width();
    let avail = (inner_width as usize).saturating_sub(label_w);
    let shown = truncate_to_width(value, avail);
    Line::from(vec![
        Span::styled(label_text, gray),
        Span::styled(shown, value_style),
    ])
}

fn packed_economics_line(
    inner_width: u16,
    amount_line: &str,
    payment_method: &str,
    premium_text: &str,
) -> Line<'static> {
    let gray = Style::default().fg(Color::Gray);
    let amount_style = Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD);
    let payment_style = Style::default().fg(Color::White);
    let premium_style = Style::default().fg(Color::Yellow);
    let amount_prefix_w = Span::raw("Amount: ").width()
        + Span::raw(amount_line).width()
        + Span::raw("  Payment: ").width();
    let premium_suffix_w = Span::raw("  Premium: ").width() + Span::raw(premium_text).width();
    let full_w = amount_prefix_w + Span::raw(payment_method).width() + premium_suffix_w;
    let include_premium = full_w <= inner_width as usize
        || (inner_width as usize).saturating_sub(amount_prefix_w) > premium_suffix_w;
    let payment_budget = if include_premium {
        (inner_width as usize)
            .saturating_sub(amount_prefix_w)
            .saturating_sub(premium_suffix_w)
    } else {
        (inner_width as usize).saturating_sub(amount_prefix_w)
    };
    let payment = truncate_to_width(payment_method, payment_budget);
    let mut spans = vec![
        Span::styled("Amount: ", gray),
        Span::styled(amount_line.to_string(), amount_style),
        Span::raw("  "),
        Span::styled("Payment: ", gray),
        Span::styled(payment, payment_style),
    ];
    if include_premium {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("Premium: ", gray));
        spans.push(Span::styled(premium_text.to_string(), premium_style));
    }
    Line::from(spans)
}

fn order_info_economics_lines(
    inner_width: u16,
    amount_line: &str,
    payment_method: &str,
    premium_text: &str,
    allow_stack: bool,
) -> Vec<Line<'static>> {
    let gray = Style::default().fg(Color::Gray);
    let one_line = Line::from(vec![
        Span::styled("Amount: ", gray),
        Span::styled(
            amount_line.to_string(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("Payment: ", gray),
        Span::styled(
            payment_method.to_string(),
            Style::default().fg(Color::White),
        ),
        Span::raw("  "),
        Span::styled("Premium: ", gray),
        Span::styled(premium_text.to_string(), Style::default().fg(Color::Yellow)),
    ]);
    if inner_width > 0 && spans_width(&one_line.spans) <= inner_width as usize {
        return vec![one_line];
    }
    if allow_stack {
        let amount_style = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
        vec![
            labeled_value_line("Amount:", amount_line, amount_style, inner_width),
            labeled_value_line(
                "Payment:",
                payment_method,
                Style::default().fg(Color::White),
                inner_width,
            ),
            labeled_value_line(
                "Premium:",
                premium_text,
                Style::default().fg(Color::Yellow),
                inner_width,
            ),
        ]
    } else {
        vec![packed_economics_line(
            inner_width,
            amount_line,
            payment_method,
            premium_text,
        )]
    }
}

fn order_info_rating_lines(
    inner_width: u16,
    buyer: Option<&UserInfo>,
    seller: Option<&UserInfo>,
) -> Vec<Line<'static>> {
    let yellow = Style::default().fg(Color::Yellow);
    match (buyer, seller) {
        (Some(b), Some(s)) => {
            let combined = format!(
                "Buyer: {}  Seller: {}",
                format_user_rating_compact(b),
                format_user_rating_compact(s)
            );
            if inner_width > 0 && Span::raw(combined.as_str()).width() <= inner_width as usize {
                vec![Line::from(Span::styled(combined, yellow))]
            } else {
                vec![
                    labeled_value_line(
                        "Buyer Rating:",
                        &format_user_rating_compact(b),
                        yellow,
                        inner_width,
                    ),
                    labeled_value_line(
                        "Seller Rating:",
                        &format_user_rating_compact(s),
                        yellow,
                        inner_width,
                    ),
                ]
            }
        }
        (Some(b), None) => vec![labeled_value_line(
            "Buyer Rating:",
            &format_user_rating_compact(b),
            yellow,
            inner_width,
        )],
        (None, Some(s)) => vec![labeled_value_line(
            "Seller Rating:",
            &format_user_rating_compact(s),
            yellow,
            inner_width,
        )],
        (None, None) => vec![],
    }
}

#[cfg(test)]
fn order_info_economics_and_ratings(
    inner_width: u16,
    amount_line: &str,
    payment_method: &str,
    premium_text: &str,
    buyer: Option<&UserInfo>,
    seller: Option<&UserInfo>,
) -> Vec<Line<'static>> {
    let mut lines =
        order_info_economics_lines(inner_width, amount_line, payment_method, premium_text, true);
    lines.extend(order_info_rating_lines(inner_width, buyer, seller));
    lines
}

struct OrderInfoEconomics<'a> {
    amount_line: &'a str,
    payment_method: &'a str,
    premium_text: &'a str,
    buyer: Option<&'a UserInfo>,
    seller: Option<&'a UserInfo>,
}

fn assemble_order_info_header(
    max_body: usize,
    identity_id: Line<'static>,
    context_line: Line<'static>,
    prefer_context: bool,
    inner_width: u16,
    economics: OrderInfoEconomics<'_>,
) -> Vec<Line<'static>> {
    let economics_stacked = order_info_economics_lines(
        inner_width,
        economics.amount_line,
        economics.payment_method,
        economics.premium_text,
        true,
    );
    let ratings = order_info_rating_lines(inner_width, economics.buyer, economics.seller);
    let mut full = vec![identity_id.clone(), context_line.clone()];
    full.extend(economics_stacked.iter().cloned());
    full.extend(ratings.iter().cloned());
    if full.len() <= max_body {
        return full;
    }

    let mut lines = Vec::new();
    if prefer_context {
        lines.push(context_line.clone());
    }
    let room = max_body.saturating_sub(lines.len());
    if economics_stacked.len() <= room {
        lines.extend(economics_stacked);
    } else if room >= 1 {
        lines.extend(order_info_economics_lines(
            inner_width,
            economics.amount_line,
            economics.payment_method,
            economics.premium_text,
            false,
        ));
    }
    let room = max_body.saturating_sub(lines.len());
    if ratings.len() <= room {
        lines.extend(ratings);
    }
    if lines.len() < max_body {
        if prefer_context {
            lines.insert(0, identity_id);
        } else {
            lines.push(identity_id);
            if lines.len() < max_body {
                lines.push(context_line);
            }
        }
    }
    lines.truncate(max_body);
    lines
}

fn build_order_chat_content(
    messages: &[UserOrderChatMessage],
    content_width: u16,
    channel: UserChatChannel,
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
            UserChatSender::Peer => match channel {
                UserChatChannel::Peer => "Peer",
                UserChatChannel::Solver => "Solver",
            },
        };
        let color = match sender {
            UserChatSender::You => Color::Cyan,
            UserChatSender::Peer => Color::Green,
        };
        let content_color = if msg.attachment.is_some() {
            Color::Yellow
        } else {
            color
        };
        let ts = format_local_timestamp(msg.timestamp, "%d-%m-%Y %H:%M")
            .unwrap_or_else(|| "unknown time".to_string());
        let header = Span::styled(format!("{label} - {ts}"), Style::default().fg(color));
        let wrapped_lines = wrap_text_to_lines(&msg.content, max_content_width);
        let peer_is_right_aligned = matches!(sender, UserChatSender::Peer);
        if peer_is_right_aligned {
            lines.push(header.into_right_aligned_line());
            for line in wrapped_lines {
                lines.push(
                    Span::styled(line, Style::default().fg(content_color))
                        .into_right_aligned_line(),
                );
            }
        } else {
            lines.push(Line::from(header));
            for line in wrapped_lines {
                lines.push(Line::from(Span::styled(
                    line,
                    Style::default().fg(content_color),
                )));
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

fn trailing_order_chat_input(input: &str, visible_width: u16) -> String {
    let max_width = visible_width as usize;
    if max_width == 0 || input.is_empty() {
        return String::new();
    }
    if Span::raw(input).width() <= max_width {
        return input.to_string();
    }

    let mut best_start = input.len();
    for (idx, _) in input.grapheme_indices(true).rev() {
        let tail = &input[idx..];
        if Span::raw(tail).width() <= max_width {
            best_start = idx;
        } else {
            break;
        }
    }
    input[best_start..].to_string()
}

pub fn render_order_in_progress(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let active_orders = active_order_chat_list_snapshot(app);

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
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
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
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(PRIMARY_COLOR))
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
    let static_h = Uuid::parse_str(&selected.order_id)
        .ok()
        .and_then(|id| app.order_chat_static.get(&id));
    let solver_available = selected.solver_pubkey.is_some()
        || static_h
            .and_then(|header| header.solver_pubkey.as_ref())
            .is_some();
    if !solver_available {
        app.active_user_chat_channel = UserChatChannel::Peer;
    }
    let active_channel = app.active_user_chat_channel;
    let order_kind = static_h
        .and_then(|h| h.kind.map(|k| k.to_string()))
        .unwrap_or_else(|| "Unknown".to_string());
    let created_str = static_h
        .and_then(|h| h.created_at)
        .and_then(|ts| format_local_timestamp(ts, "%Y-%m-%d %H:%M:%S"))
        .unwrap_or_else(|| "Unknown".to_string());
    let truncate_pubkey = |pubkey: &str| -> String {
        if pubkey.len() > 16 {
            format!("{}...{}", &pubkey[..8], &pubkey[pubkey.len() - 8..])
        } else {
            pubkey.to_string()
        }
    };
    let initiator_pubkey_display = static_h
        .map(|h| truncate_pubkey(&h.initiator_trade_pubkey))
        .unwrap_or_else(|| "Unknown".to_string());
    let initiator_role = match static_h.map(|h| h.is_mine) {
        Some(true) => "Maker",
        Some(false) => "Taker",
        None => "Initiator",
    };
    let trade_id = static_h
        .map(|h| h.trade_index.to_string())
        .or_else(|| selected.trade_index.map(|t| t.to_string()))
        .unwrap_or_else(|| "Unknown".to_string());
    let payment_method = selected.payment_method.as_deref().unwrap_or("Unknown");
    let premium_text = selected
        .premium
        .map(|p| format!("{p}%"))
        .unwrap_or_else(|| "Unknown".to_string());
    let amount_line = match (selected.amount, &selected.fiat) {
        (Some(sats), Some((fiat_amount, fiat_code))) if sats > 0 => {
            format!("{sats} sats | {fiat_amount} {fiat_code}")
        }
        (Some(sats), None) if sats > 0 => format!("{sats} sats"),
        (_, Some((fiat_amount, fiat_code))) => {
            format!("{fiat_amount} {fiat_code}")
        }
        _ => "amount N/A".to_string(),
    };

    // TODO(My Trades header): Wire "Privacy:", "Buyer -", "Seller -" from trade privacy / full-privacy
    // signals once available on DM payloads or local `orders` (see dispute UI + `Order::is_full_privacy_order`).
    // Omit that row until then — avoid static "Unknown" placeholders.

    let order_id_display = static_h
        .map(|h| h.order_id.to_string())
        .unwrap_or_else(|| selected.order_id.clone());
    let dispute_id = selected
        .dispute_id
        .as_deref()
        .or_else(|| static_h.and_then(|header| header.dispute_id.as_deref()));
    let context_line = if let Some(dispute_id) = dispute_id {
        Line::from(vec![
            Span::styled("Dispute ID: ", Style::default().fg(Color::Gray)),
            Span::styled(
                dispute_id.to_string(),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!("Initiator: {initiator_role} "),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(initiator_pubkey_display, Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("Created: ", Style::default().fg(Color::Gray)),
            Span::styled(created_str, Style::default().fg(Color::Yellow)),
        ])
    };
    let identity_id = Line::from(vec![
        Span::styled("Order ID: ", Style::default().fg(Color::Gray)),
        Span::styled(
            order_id_display,
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
            order_kind,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("Status: ", Style::default().fg(Color::Gray)),
        Span::styled(status_label, Style::default().add_modifier(Modifier::BOLD)),
    ]);
    let header_inner_width = main_area.width.saturating_sub(2);

    // Borders::ALL consumes 2 rows. Cap the header so chat/input/footer still fit on
    // short terminals (a 60×15 post-chrome pane must keep a usable chat row).
    let max_header = main_area
        .height
        .saturating_sub(
            input_height
                .saturating_add(ORDER_INFO_MIN_FOOTER)
                .saturating_add(ORDER_INFO_MIN_CHAT),
        )
        .max(3);
    let max_body = max_header.saturating_sub(2) as usize;
    let header_lines = assemble_order_info_header(
        max_body,
        identity_id,
        context_line,
        dispute_id.is_some(),
        header_inner_width,
        OrderInfoEconomics {
            amount_line: &amount_line,
            payment_method,
            premium_text: &premium_text,
            buyer: selected.buyer_reputation.as_ref(),
            seller: selected.seller_reputation.as_ref(),
        },
    );
    let header_height = (header_lines.len() as u16)
        .saturating_add(2)
        .min(max_header)
        .max(3);

    let spare_below_header_input = main_area
        .height
        .saturating_sub(header_height.saturating_add(input_height));
    // Footer grows only when chat still keeps ORDER_INFO_MIN_CHAT rows.
    let can_fit_three_line_footer = spare_below_header_input >= 3 + ORDER_INFO_MIN_CHAT;
    let can_fit_two_line_footer = spare_below_header_input >= 2 + ORDER_INFO_MIN_CHAT;
    // Prefer 3 rows for My Trades hints (many shortcuts); wrap needs one Paragraph over full height
    // — never split into per-line widgets of height 1 or wrapped text has nowhere to go.
    let footer_height: u16 = if main_area.width < 50 {
        1
    } else if can_fit_three_line_footer {
        3
    } else if can_fit_two_line_footer {
        2
    } else {
        1
    };
    let footer_height =
        footer_height.saturating_add(if app.attachment_toast.is_some() { 1 } else { 0 });

    let file_count = if active_channel == UserChatChannel::Peer {
        count_order_attachments(app, &selected.order_id)
    } else {
        0
    };
    let mut attach_hints = if active_channel == UserChatChannel::Peer {
        FOOTER_CTRL_O_SEND_FILE.to_string()
    } else {
        String::new()
    };
    if file_count > 0 {
        attach_hints.push_str(FOOTER_CTRL_S_SAVE_FILE);
    }
    if active_channel == UserChatChannel::Peer
        && app
            .pending_order_attachment_sends
            .contains_key(&selected.order_id)
    {
        attach_hints.push_str(FOOTER_CTRL_SHIFT_O_RETRY);
    }
    if active_channel == UserChatChannel::Peer
        && app.sending_attachment_order_id.as_deref() == Some(selected.order_id.as_str())
    {
        attach_hints.push_str(FOOTER_SENDING_ATTACHMENT);
    }
    if solver_available {
        attach_hints.push_str(" | ");
        attach_hints.push_str(FOOTER_MYTRADES_TAB_CHAT);
    }
    let attach_hints = attach_hints.as_str();

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
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(PRIMARY_COLOR))
                .style(Style::default().bg(BACKGROUND_COLOR)),
        ),
        main_chunks[0],
    );

    let chat_messages = match active_channel {
        UserChatChannel::Peer => app.order_chats.get(&selected.order_id),
        UserChatChannel::Solver => app.user_dispute_chats.get(&selected.order_id),
    }
    .cloned()
    .unwrap_or_default();
    let message_count = chat_messages.len();
    let chat_title = if message_count > 0 {
        if file_count > 0 {
            format!(
                "{} Chat ({} messages, {} file(s))",
                active_channel, message_count, file_count
            )
        } else {
            format!("{} Chat ({} messages)", active_channel, message_count)
        }
    } else {
        format!("{} Chat (no messages)", active_channel)
    };
    let chat_area = main_chunks[1];
    let chat_block = Block::default()
        .title(chat_title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));
    let chat_inner = chat_block.inner(chat_area);
    f.render_widget(chat_block, chat_area);

    // Match disputes/observer chat: content width reserves one column for the vertical scrollbar.
    let content_width = chat_inner.width.saturating_sub(1).max(1);
    let (chat_lines, _, line_starts) =
        build_order_chat_content(&chat_messages, content_width, active_channel);
    app.order_chat_line_starts = line_starts;
    let content_height = chat_lines.len().min(u16::MAX as usize) as u16;

    if message_count > 0 {
        let order_id_key = selected.order_id.clone();
        if let Some((ref prev_id, prev_channel, last_count)) = app.order_chat_scroll_tracker {
            if *prev_id == order_id_key && prev_channel == active_channel {
                if message_count > last_count {
                    app.order_chat_scrollview_state.scroll_to_bottom();
                }
            } else {
                app.order_chat_scrollview_state.scroll_to_bottom();
            }
        } else {
            app.order_chat_scrollview_state.scroll_to_bottom();
        }
        app.order_chat_scroll_tracker = Some((order_id_key, active_channel, message_count));
    } else {
        app.order_chat_scroll_tracker = Some((selected.order_id.clone(), active_channel, 0));
    }

    let mut scroll_view = ScrollView::new(Size::new(content_width, content_height.max(1)))
        .vertical_scrollbar_visibility(ScrollbarVisibility::Always);
    let content_rect = Rect::new(0, 0, content_width, content_height.max(1));
    scroll_view.render_widget(
        Paragraph::new(chat_lines).wrap(Wrap { trim: true }),
        content_rect,
    );
    f.render_stateful_widget(
        scroll_view,
        chat_inner,
        &mut app.order_chat_scrollview_state,
    );

    let input_active = app.mode.user_my_trades_interactive() && app.order_chat_input_enabled;
    let input_block = Block::default()
        .title(if app.order_chat_input_enabled {
            "Message"
        } else {
            "Message (disabled: Shift+I)"
        })
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR));
    let visible_input = trailing_order_chat_input(
        &app.order_chat_input,
        input_block.inner(main_chunks[2]).width,
    );
    f.render_widget(
        Paragraph::new(visible_input)
            .wrap(Wrap { trim: false })
            .style(if input_active {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            })
            .block(input_block),
        main_chunks[2],
    );

    // Footer: one Paragraph over the full footer rect so `wrap` can use every reserved row.
    let footer_area = main_chunks[3];
    let footer_width = footer_area.width;
    let base_footer_lines: u16 = if footer_width < 50 {
        1
    } else if can_fit_three_line_footer {
        3
    } else if can_fit_two_line_footer {
        2
    } else {
        1
    };
    let has_toast = app.attachment_toast.is_some();
    let hint_lines = base_footer_lines;

    let footer_body: Text<'static> = if footer_width < 50 {
        Text::raw(format!("{HELP_KEY}{attach_hints}"))
    } else if hint_lines >= 3 {
        if app.order_chat_input_enabled {
            Text::from(vec![
                Line::from(format!(
                    "{} | {} | {} | {}",
                    HELP_KEY,
                    FOOTER_MYTRADES_SELECT_ORDER,
                    FOOTER_MYTRADES_ENTER_SEND,
                    FOOTER_MYTRADES_SHIFT_I_DISABLE,
                )),
                Line::from(format!(
                    "{} | {} | {} | {}",
                    FOOTER_MYTRADES_SHIFT_C_CANCEL,
                    FOOTER_MYTRADES_SHIFT_D_DISPUTE,
                    FOOTER_MYTRADES_SHIFT_F_FIAT_SENT,
                    FOOTER_MYTRADES_SHIFT_R_RELEASE,
                )),
                Line::from(format!(
                    "{} | {} | {} | {}{}",
                    FOOTER_MYTRADES_PGUP_PGDN_SCROLL_CHAT,
                    FOOTER_MYTRADES_END_BOTTOM,
                    FOOTER_MYTRADES_SHIFT_V_RATE,
                    FOOTER_MYTRADES_SHIFT_K_KCONV,
                    attach_hints,
                )),
            ])
        } else {
            Text::from(vec![
                Line::from(format!(
                    "{} | {} | {}",
                    HELP_KEY, FOOTER_MYTRADES_SELECT_ORDER, FOOTER_MYTRADES_SHIFT_I_ENABLE,
                )),
                Line::from(format!(
                    "{} | {} | {} | {}",
                    FOOTER_MYTRADES_SHIFT_C_CANCEL,
                    FOOTER_MYTRADES_SHIFT_D_DISPUTE,
                    FOOTER_MYTRADES_SHIFT_F_FIAT_SENT,
                    FOOTER_MYTRADES_SHIFT_R_RELEASE,
                )),
                Line::from(format!(
                    "{} | {} | {} | {}{}",
                    FOOTER_MYTRADES_PGUP_PGDN_SCROLL_CHAT,
                    FOOTER_MYTRADES_END_BOTTOM,
                    FOOTER_MYTRADES_SHIFT_V_RATE,
                    FOOTER_MYTRADES_SHIFT_K_KCONV,
                    attach_hints,
                )),
            ])
        }
    } else if hint_lines >= 2 {
        if app.order_chat_input_enabled {
            Text::from(vec![
                Line::from(format!(
                    "{} | {} | {} | {}",
                    HELP_KEY,
                    FOOTER_MYTRADES_SELECT_ORDER,
                    FOOTER_MYTRADES_ENTER_SEND,
                    FOOTER_MYTRADES_SHIFT_I_DISABLE,
                )),
                Line::from(format!(
                    "{} | {} | {} | {} | {}{}",
                    FOOTER_MYTRADES_SHIFT_C_CANCEL,
                    FOOTER_MYTRADES_SHIFT_D_DISPUTE,
                    FOOTER_MYTRADES_SHIFT_F_FIAT_SENT,
                    FOOTER_MYTRADES_SHIFT_R_RELEASE,
                    FOOTER_MYTRADES_SHIFT_V_RATE,
                    attach_hints,
                )),
            ])
        } else {
            Text::from(vec![
                Line::from(format!(
                    "{} | {} | {} | {} | {}",
                    HELP_KEY,
                    FOOTER_MYTRADES_SELECT_ORDER,
                    FOOTER_MYTRADES_SHIFT_I_ENABLE,
                    FOOTER_MYTRADES_SHIFT_C_CANCEL,
                    FOOTER_MYTRADES_SHIFT_D_DISPUTE,
                )),
                Line::from(format!(
                    "{} | {} | {} | {}{}",
                    FOOTER_MYTRADES_SHIFT_F_FIAT_SENT,
                    FOOTER_MYTRADES_PGUP_PGDN_SCROLL_CHAT,
                    FOOTER_MYTRADES_SHIFT_R_RELEASE,
                    FOOTER_MYTRADES_SHIFT_V_RATE,
                    attach_hints,
                )),
            ])
        }
    } else {
        let base = if app.order_chat_input_enabled {
            format!(
                "{} | {} | {} | {} | {} | {}",
                HELP_KEY,
                FOOTER_MYTRADES_SELECT_ORDER,
                FOOTER_MYTRADES_ENTER_SEND,
                FOOTER_MYTRADES_SHIFT_I_DISABLE,
                FOOTER_MYTRADES_SHIFT_C_CANCEL,
                FOOTER_MYTRADES_SHIFT_D_DISPUTE
            )
        } else {
            format!(
                "{} | {} | {} | {} | {}",
                HELP_KEY,
                FOOTER_MYTRADES_SELECT_ORDER,
                FOOTER_MYTRADES_SHIFT_I_ENABLE,
                FOOTER_MYTRADES_SHIFT_C_CANCEL,
                FOOTER_MYTRADES_SHIFT_D_DISPUTE
            )
        };
        Text::raw(format!("{base}{attach_hints}"))
    };

    if has_toast {
        let chunks = Layout::new(
            Direction::Vertical,
            [Constraint::Length(1), Constraint::Length(hint_lines.max(1))],
        )
        .split(footer_area);
        let (toast_msg, _) = app.attachment_toast.as_ref().unwrap();
        f.render_widget(
            Paragraph::new(toast_msg.as_str()).style(Style::default().fg(Color::Yellow)),
            chunks[0],
        );
        f.render_widget(
            Paragraph::new(footer_body).wrap(Wrap { trim: true }),
            chunks[1],
        );
    } else {
        f.render_widget(
            Paragraph::new(footer_body).wrap(Wrap { trim: true }),
            footer_area,
        );
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

#[cfg(test)]
mod tests {
    use super::{render_order_in_progress, trailing_order_chat_input};
    use crate::ui::helpers::OrderChatListItem;
    use crate::ui::key_handler::handle_tab_navigation;
    use crate::ui::{
        AppState, OrderChatStaticHeader, Tab, UiMode, UserChatChannel, UserChatSender, UserMode,
        UserOrderChatMessage, UserRole, UserTab,
    };
    use crossterm::event::KeyCode;
    use mostro_core::prelude::{Status, UserInfo};
    use ratatui::backend::TestBackend;
    use ratatui::text::Span;
    use ratatui::Terminal;
    use uuid::Uuid;

    #[test]
    fn trailing_input_keeps_full_text_when_it_fits() {
        assert_eq!(
            trailing_order_chat_input("short message", 20),
            "short message"
        );
    }

    #[test]
    fn trailing_input_shows_latest_text_when_overflowing() {
        let input = "abcdefghijklmnopqrstuvwxyz";

        assert_eq!(trailing_order_chat_input(input, 10), "qrstuvwxyz");
    }

    #[test]
    fn trailing_input_respects_unicode_display_width() {
        let input = "abc你好def";
        let visible = trailing_order_chat_input(input, 6);

        assert_eq!(visible, "好def");
        assert!(Span::raw(visible.as_str()).width() <= 6);
    }

    #[test]
    fn trailing_input_keeps_decomposed_character_together() {
        let input = "abcde\u{0301}fgh";
        let visible = trailing_order_chat_input(input, 4);

        assert_eq!(visible, "e\u{0301}fgh");
        assert!(Span::raw(visible.as_str()).width() <= 4);
    }

    #[test]
    fn trailing_input_keeps_zwj_emoji_sequence_together() {
        let input = "abc👩\u{200d}💻def";
        let visible = trailing_order_chat_input(input, 5);

        assert_eq!(visible, "👩\u{200d}💻def");
        assert!(Span::raw(visible.as_str()).width() <= 5);
    }

    fn buffer_contains(buf: &ratatui::buffer::Buffer, needle: &str) -> bool {
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
            flat.push('\n');
        }
        flat.contains(needle)
    }

    #[test]
    fn render_order_input_shows_trailing_suffix_when_overflowing() {
        let order_id = Uuid::nil().to_string();
        let mut app = AppState::new(UserRole::User);
        app.mode = UiMode::UserMode(UserMode::Normal);
        app.my_trades_maker_book.push(OrderChatListItem {
            order_id,
            status: Some(Status::Pending),
            amount: Some(1000),
            fiat: Some((10, "USD".to_string())),
            trade_index: Some(1),
            payment_method: Some("cash".to_string()),
            premium: Some(0),
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            buyer_reputation: None,
            seller_reputation: None,
            solver_pubkey: None,
            dispute_id: None,
        });
        app.order_chat_input = format!(
            "hidden-prefix-that-should-scroll-away-{}-visible-suffix",
            "x".repeat(80)
        );

        // Keep the message input usable on a terminal that is both narrow and short.
        let backend = TestBackend::new(60, 15);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_order_in_progress(frame, frame.area(), &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert!(buffer_contains(buffer, "visible-suffix"));
        assert!(!buffer_contains(
            buffer,
            "hidden-prefix-that-should-scroll-away"
        ));
    }

    #[test]
    fn render_solver_chat_keeps_solver_messages_visible() {
        let order_id = Uuid::nil().to_string();
        let mut app = AppState::new(UserRole::User);
        app.mode = UiMode::UserMode(UserMode::Normal);
        app.active_user_chat_channel = UserChatChannel::Solver;
        app.order_chat_static.insert(
            Uuid::nil(),
            OrderChatStaticHeader {
                order_id: Uuid::nil(),
                kind: None,
                created_at: Some(1),
                trade_index: 1,
                initiator_trade_pubkey: "trade-pubkey".to_string(),
                is_mine: false,
                solver_pubkey: Some("solver-pubkey".to_string()),
                dispute_id: Some("11111111-2222-3333-4444-555555555555".to_string()),
            },
        );
        app.my_trades_maker_book.push(OrderChatListItem {
            order_id: order_id.clone(),
            status: Some(Status::Dispute),
            amount: Some(1000),
            fiat: Some((10, "USD".to_string())),
            trade_index: Some(1),
            payment_method: Some("cash".to_string()),
            premium: Some(0),
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            buyer_reputation: None,
            seller_reputation: None,
            solver_pubkey: Some("solver-pubkey".to_string()),
            dispute_id: None,
        });
        app.user_dispute_chats.insert(
            order_id,
            vec![UserOrderChatMessage {
                sender: UserChatSender::Peer,
                content: "Please send the payment receipt".to_string(),
                timestamp: 1,
                attachment: None,
            }],
        );

        let backend = TestBackend::new(60, 15);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_order_in_progress(frame, frame.area(), &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert!(buffer_contains(buffer, "Solver Chat (1 messages)"));
        assert!(buffer_contains(buffer, "Dispute ID:"));
        assert!(buffer_contains(buffer, "Please send the"));
        assert!(buffer_contains(buffer, "payment receipt"));
        assert!(!buffer_contains(buffer, "Ctrl+O: Send file"));
    }

    #[test]
    fn render_order_info_shows_amount_payment_premium_and_ratings() {
        let order_id = Uuid::nil().to_string();
        let mut app = AppState::new(UserRole::User);
        app.mode = UiMode::UserMode(UserMode::Normal);
        app.order_chat_static.insert(
            Uuid::nil(),
            OrderChatStaticHeader {
                order_id: Uuid::nil(),
                kind: None,
                created_at: Some(1),
                trade_index: 1,
                initiator_trade_pubkey: "trade-pubkey".to_string(),
                is_mine: false,
                solver_pubkey: None,
                dispute_id: None,
            },
        );
        app.my_trades_maker_book.push(OrderChatListItem {
            order_id,
            status: Some(Status::Success),
            amount: Some(1000),
            fiat: Some((10, "USD".to_string())),
            trade_index: Some(1),
            payment_method: Some("cash".to_string()),
            premium: Some(2),
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            buyer_reputation: Some(UserInfo {
                rating: 4.5,
                reviews: 12,
                operating_days: 30,
            }),
            seller_reputation: Some(UserInfo {
                rating: 3.0,
                reviews: 4,
                operating_days: 10,
            }),
            solver_pubkey: None,
            dispute_id: None,
        });

        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_order_in_progress(frame, frame.area(), &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert!(buffer_contains(buffer, "Amount:"));
        assert!(buffer_contains(buffer, "1000 sats | 10 USD"));
        assert!(buffer_contains(buffer, "4.5/5"));
        assert!(buffer_contains(buffer, "3.0/5"));
        assert!(buffer_contains(buffer, "Payment:"));
        assert!(buffer_contains(buffer, "cash"));
        assert!(buffer_contains(buffer, "Premium:"));
        assert!(buffer_contains(buffer, "2%"));
    }

    #[test]
    fn render_order_info_keeps_chat_on_short_narrow_viewport() {
        let order_id = Uuid::nil().to_string();
        let mut app = AppState::new(UserRole::User);
        app.mode = UiMode::UserMode(UserMode::Normal);
        app.order_chat_static.insert(
            Uuid::nil(),
            OrderChatStaticHeader {
                order_id: Uuid::nil(),
                kind: None,
                created_at: Some(1),
                trade_index: 1,
                initiator_trade_pubkey: "trade-pubkey".to_string(),
                is_mine: false,
                solver_pubkey: None,
                dispute_id: None,
            },
        );
        app.my_trades_maker_book.push(OrderChatListItem {
            order_id: order_id.clone(),
            status: Some(Status::Active),
            amount: Some(1000),
            fiat: Some((10, "USD".to_string())),
            trade_index: Some(1),
            payment_method: Some("SEPA bank transfer to ES9121000418450200051332".to_string()),
            premium: Some(2),
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            buyer_reputation: Some(UserInfo {
                rating: 4.5,
                reviews: 12,
                operating_days: 30,
            }),
            seller_reputation: Some(UserInfo {
                rating: 3.0,
                reviews: 4,
                operating_days: 10,
            }),
            solver_pubkey: None,
            dispute_id: None,
        });
        app.order_chats.insert(
            order_id,
            vec![UserOrderChatMessage {
                sender: UserChatSender::Peer,
                content: "invoice posted please pay".to_string(),
                timestamp: 1,
                attachment: None,
            }],
        );

        let backend = TestBackend::new(60, 15);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_order_in_progress(frame, frame.area(), &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(buffer_contains(buffer, "Amount:"));
        assert!(buffer_contains(buffer, "Payment:"));
        assert!(buffer_contains(buffer, "SEPA"));
        assert!(buffer_contains(buffer, "Peer Chat"));
        assert!(
            buffer_contains(buffer, "invoice")
                || buffer_contains(buffer, "posted")
                || buffer_contains(buffer, "pay")
        );
    }

    #[test]
    fn order_info_economics_stack_when_narrower_than_combined_line() {
        let buyer = UserInfo {
            rating: 4.5,
            reviews: 12,
            operating_days: 30,
        };
        let lines = super::order_info_economics_and_ratings(
            40,
            "1000 sats | 10 USD",
            "SEPA bank transfer",
            "2%",
            Some(&buyer),
            None,
        );
        let joined: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("Amount:"));
        assert!(joined.contains("Payment:"));
        assert!(joined.contains("Premium:"));
        assert!(joined.matches("Amount:").count() == 1);
        assert!(joined.contains("Buyer Rating:"));
        assert!(!joined.contains("Payment: SEPA bank transfer  Premium:"));
    }

    #[test]
    fn render_footer_lists_dispute_shortcut_next_to_cancel() {
        let order_id = Uuid::nil().to_string();
        let mut app = AppState::new(UserRole::User);
        app.mode = UiMode::UserMode(UserMode::Normal);
        app.my_trades_maker_book.push(OrderChatListItem {
            order_id,
            status: Some(Status::Active),
            amount: Some(1000),
            fiat: Some((10, "USD".to_string())),
            trade_index: Some(1),
            payment_method: Some("cash".to_string()),
            premium: Some(0),
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            buyer_reputation: None,
            seller_reputation: None,
            solver_pubkey: None,
            dispute_id: None,
        });

        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_order_in_progress(frame, frame.area(), &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();

        assert!(buffer_contains(
            buffer,
            "Shift+C: Cancel order | Shift+D: Dispute"
        ));
    }

    #[test]
    fn tab_switches_to_solver_chat_only_after_assignment() {
        let mut app = AppState::new(UserRole::User);
        app.active_tab = Tab::User(UserTab::MyTrades);
        app.my_trades_maker_book.push(OrderChatListItem {
            order_id: Uuid::nil().to_string(),
            status: Some(Status::Dispute),
            amount: None,
            fiat: None,
            trade_index: Some(1),
            payment_method: None,
            premium: None,
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            buyer_reputation: None,
            seller_reputation: None,
            solver_pubkey: None,
            dispute_id: None,
        });

        handle_tab_navigation(KeyCode::Tab, &mut app);
        assert_eq!(app.active_user_chat_channel, UserChatChannel::Peer);

        app.my_trades_maker_book[0].solver_pubkey = Some("solver".to_string());
        handle_tab_navigation(KeyCode::Tab, &mut app);
        assert_eq!(app.active_user_chat_channel, UserChatChannel::Solver);
        handle_tab_navigation(KeyCode::BackTab, &mut app);
        assert_eq!(app.active_user_chat_channel, UserChatChannel::Peer);
    }
}
