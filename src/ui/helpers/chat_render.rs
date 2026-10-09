use std::ops::{Range, RangeInclusive};

use ratatui::layout::Position;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Wrap};
use tui_scrollview::ScrollViewState;

use crate::ui::helpers::format_local_timestamp;
use crate::ui::{ChatParty, ChatSender, DisputeChatMessage, PRIMARY_COLOR};

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
    let date_str = format_local_timestamp(msg.timestamp, "%d-%m-%Y")
        .unwrap_or_else(|| "??-??-????".to_string());
    let time_str =
        format_local_timestamp(msg.timestamp, "%H:%M").unwrap_or_else(|| "??:??".to_string());

    let (sender_label, sender_color, is_right_aligned) = match msg.sender {
        ChatSender::Admin => ("Admin", Color::Cyan, false),
        ChatSender::Buyer => ("Buyer", Color::Green, true),
        ChatSender::Seller => ("Seller", Color::Magenta, true),
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

impl ChatScrollViewContent {
    pub(crate) fn select_messages(
        &mut self,
        selected: Option<RangeInclusive<usize>>,
        focus: Option<usize>,
    ) -> Option<Range<usize>> {
        self.select_messages_with_wrap(selected, focus, Wrap { trim: true })
    }

    pub(crate) fn select_messages_with_wrap(
        &mut self,
        selected: Option<RangeInclusive<usize>>,
        focus: Option<usize>,
        wrap: Wrap,
    ) -> Option<Range<usize>> {
        let logical_starts = self.line_start_per_message.clone();
        let mut rows = 0usize;
        let mut selected_rows = None;
        for (index, start) in logical_starts.iter().copied().enumerate() {
            let end = logical_starts
                .get(index + 1)
                .copied()
                .unwrap_or(self.lines.len());
            self.line_start_per_message[index] = rows;
            let message_start = rows;
            let in_selection = selected
                .as_ref()
                .is_some_and(|range| range.contains(&index));
            for line in &mut self.lines[start..end] {
                if in_selection && !line.spans.is_empty() && line.width() > 0 {
                    line.style = line.style.bg(PRIMARY_COLOR).fg(Color::Black);
                    for span in &mut line.spans {
                        span.style = span.style.bg(PRIMARY_COLOR).fg(Color::Black);
                    }
                }
                rows = rows.saturating_add(
                    Paragraph::new(line.clone())
                        .wrap(wrap)
                        .line_count(self.content_width.max(1)),
                );
            }
            if focus == Some(index) {
                let separator = usize::from(self.lines[end - 1].width() == 0);
                selected_rows =
                    Some(message_start..rows.saturating_sub(separator).max(message_start + 1));
            }
        }
        if !logical_starts.is_empty() {
            self.content_height = rows.min(u16::MAX as usize) as u16;
        }
        selected_rows
    }

    pub(crate) fn keep_selection_visible(
        &self,
        selected: Range<usize>,
        viewport_height: u16,
        state: &mut ScrollViewState,
    ) {
        state.set_offset(Position::new(
            0,
            self.selection_scroll_offset(selected, viewport_height, state.offset().y),
        ));
    }

    pub(crate) fn selection_scroll_offset(
        &self,
        selected: Range<usize>,
        viewport_height: u16,
        current_offset: u16,
    ) -> u16 {
        if viewport_height == 0 {
            return current_offset;
        }
        let height = usize::from(viewport_height);
        let current = usize::from(current_offset);
        let offset = if selected.start < current || selected.len() > height {
            selected.start
        } else if selected.end > current.saturating_add(height) {
            selected.end.saturating_sub(height)
        } else {
            current
        };
        let max_offset = self.content_height.saturating_sub(viewport_height);
        offset.min(usize::from(max_offset)) as u16
    }
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
            "No messages yet. Paste Shared key and press Enter to load.",
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

#[cfg(test)]
mod tests {
    use super::build_observer_scrollview_content;

    /// The UI label for the disclosed `K_conv` secret is "Shared key" (never
    /// confused with the persisted ECDH `order_chat_shared_key_hex`, which is
    /// not shown to users at all).
    #[test]
    fn observer_empty_hint_asks_for_shared_key() {
        let content = build_observer_scrollview_content(&[], 40, Some(20));
        let flat: String = content
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        assert!(
            flat.contains("Shared key"),
            "empty Observer hint must mention Shared key: {flat}"
        );
    }
}
