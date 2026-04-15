use chrono::DateTime;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

use crate::ui::{ChatParty, ChatSender, DisputeChatMessage};

use super::chat_visibility::message_visible_for_party;

/// Wraps text to a maximum display width (in columns), breaking at word boundaries.
/// Uses ratatui's Span width for Unicode-aware measurement. Words longer than
/// max_width are placed on their own line.
pub(crate) fn wrap_text_to_lines(content: &str, max_width: u16) -> Vec<String> {
    let max_width = max_width as usize;
    if max_width == 0 {
        return vec![content.to_string()];
    }
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in content.split_whitespace() {
        let word_width = Span::raw(word).width();
        let space_width = if current_width > 0 { 1 } else { 0 };

        if word_width > max_width {
            if !current_line.is_empty() {
                lines.push(std::mem::take(&mut current_line));
                current_width = 0;
            }
            lines.push(word.to_string());
        } else if current_width + space_width + word_width > max_width {
            if !current_line.is_empty() {
                lines.push(std::mem::take(&mut current_line));
            }
            current_line = word.to_string();
            current_width = word_width;
        } else {
            if current_width > 0 {
                current_line.push(' ');
                current_width += 1;
            }
            current_line.push_str(word);
            current_width += word_width;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(content.to_string());
    }
    lines
}

/// Formats a single message as display lines (header + content + blank). Used by list and scrollview.
fn format_message_lines(
    msg: &DisputeChatMessage,
    max_content_width: Option<u16>,
) -> Vec<Line<'static>> {
    let (date_str, time_str) = DateTime::from_timestamp(msg.timestamp, 0)
        .map(|dt| {
            let date = dt.format("%d-%m-%Y").to_string();
            let time = dt.format("%H:%M").to_string();
            (date, time)
        })
        .unwrap_or_else(|| ("??-??-????".to_string(), "??:??".to_string()));

    let (sender_label, sender_color, is_right_aligned) = match msg.sender {
        ChatSender::Admin => ("Admin", Color::Cyan, false),
        ChatSender::Buyer => ("Buyer", Color::Green, true),
        ChatSender::Seller => ("Seller", Color::Magenta, false),
    };
    let content_color = msg
        .attachment
        .as_ref()
        .map(|_| Color::Yellow)
        .unwrap_or(sender_color);

    let header_text = format!("{} - {} - {}", sender_label, date_str, time_str);
    let mut message_lines = Vec::new();

    if is_right_aligned {
        let header_span = Span::styled(header_text, Style::default().fg(sender_color));
        message_lines.push(header_span.into_right_aligned_line());
        let content_lines = max_content_width
            .map(|w| wrap_text_to_lines(&msg.content, w))
            .unwrap_or_else(|| vec![msg.content.clone()]);
        for line in content_lines {
            message_lines.push(
                Span::styled(line, Style::default().fg(content_color)).into_right_aligned_line(),
            );
        }
    } else {
        message_lines.push(Line::from(vec![Span::styled(
            header_text,
            Style::default().fg(sender_color),
        )]));
        let content_lines = max_content_width
            .map(|w| wrap_text_to_lines(&msg.content, w))
            .unwrap_or_else(|| vec![msg.content.clone()]);
        for line in content_lines {
            message_lines.push(Line::from(vec![Span::styled(
                line,
                Style::default().fg(content_color),
            )]));
        }
    }
    message_lines.push(Line::from(""));
    message_lines
}

/// Builds `ListItem`s from chat messages for display in the dispute chat list widget.
pub fn build_chat_list_items(
    messages: &[DisputeChatMessage],
    active_chat_party: ChatParty,
    max_content_width: Option<u16>,
) -> Vec<ListItem<'_>> {
    let filtered_items: Vec<ListItem<'_>> = messages
        .iter()
        .filter(|msg| message_visible_for_party(msg, active_chat_party))
        .map(|msg| ListItem::new(format_message_lines(msg, max_content_width)))
        .collect();

    if filtered_items.is_empty() {
        return vec![ListItem::new(Line::from(Span::styled(
            "No messages yet. Start the conversation!",
            Style::default().fg(Color::Gray),
        )))];
    }

    filtered_items
}

/// Content for the dispute chat ScrollView: all lines, dimensions, and line start index per message.
pub struct ChatScrollViewContent {
    pub lines: Vec<Line<'static>>,
    pub content_height: u16,
    pub content_width: u16,
    pub line_start_per_message: Vec<usize>,
}

/// Builds scrollview content: flat lines, height, width, and line_start_per_message for the visible messages.
pub fn build_chat_scrollview_content(
    messages: &[DisputeChatMessage],
    active_chat_party: ChatParty,
    content_width: u16,
    max_content_width: Option<u16>,
) -> ChatScrollViewContent {
    let mut lines = Vec::new();
    let mut line_start_per_message = Vec::new();

    for msg in messages
        .iter()
        .filter(|m| message_visible_for_party(m, active_chat_party))
    {
        line_start_per_message.push(lines.len());
        lines.extend(format_message_lines(msg, max_content_width));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No messages yet. Start the conversation!",
            Style::default().fg(Color::Gray),
        )));
    }

    let content_height = lines.len().min(u16::MAX as usize) as u16;
    ChatScrollViewContent {
        lines,
        content_height,
        content_width,
        line_start_per_message,
    }
}

/// Builds scrollview content for the observer tab (no party filtering).
pub fn build_observer_scrollview_content(
    messages: &[DisputeChatMessage],
    content_width: u16,
    max_content_width: Option<u16>,
) -> ChatScrollViewContent {
    let mut lines = Vec::new();
    let mut line_start_per_message = Vec::new();

    for msg in messages {
        line_start_per_message.push(lines.len());
        lines.extend(format_message_lines(msg, max_content_width));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No messages yet. Paste a shared key and press Enter to load.",
            Style::default().fg(Color::Gray),
        )));
    }

    let content_height = lines.len().min(u16::MAX as usize) as u16;
    ChatScrollViewContent {
        lines,
        content_height,
        content_width,
        line_start_per_message,
    }
}
