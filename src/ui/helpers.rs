use chrono::DateTime;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use std::fs::{self, OpenOptions};
use std::io::Write;

use super::{ChatParty, ChatSender, DisputeChatMessage, PRIMARY_COLOR};

/// Creates a centered popup area within the given area
pub fn create_centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    let (popup_width, popup_height) = (width.min(area.width), height.min(area.height));
    let [popup] = Layout::horizontal([Constraint::Length(popup_width)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::vertical([Constraint::Length(popup_height)])
        .flex(Flex::Center)
        .areas(popup);
    popup
}

/// Renders help text with a styled key binding
pub fn render_help_text(f: &mut ratatui::Frame, area: Rect, prefix: &str, key: &str, suffix: &str) {
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prefix, Style::default()),
            Span::styled(
                key,
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(suffix, Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        area,
    );
}

/// Formats an order ID for display (truncates to 8 chars)
pub fn format_order_id(order_id: Option<uuid::Uuid>) -> String {
    if let Some(id) = order_id {
        format!(
            "Order: {}",
            id.to_string().chars().take(8).collect::<String>()
        )
    } else {
        "Order: Unknown".to_string()
    }
}

/// Saves a chat message to a text file in ~/.mostrix/dispute_id.txt
/// Creates the directory and file if they don't exist, appends if they do
pub fn save_chat_message(dispute_id: &str, message: &DisputeChatMessage) {
    // Get ~/.mostrix directory path
    let home_dir = match dirs::home_dir() {
        Some(dir) => dir,
        None => {
            log::warn!("Could not find home directory, skipping chat save");
            return;
        }
    };

    let mostrix_dir = home_dir.join(".mostrix");

    // Create .mostrix directory if it doesn't exist
    if let Err(e) = fs::create_dir_all(&mostrix_dir) {
        log::warn!("Failed to create .mostrix directory: {}", e);
        return;
    }

    // Format date and time
    let (date_str, time_str) = DateTime::from_timestamp(message.timestamp, 0)
        .map(|dt| {
            let date = dt.format("%d-%m-%Y").to_string();
            let time = dt.format("%H:%M:%S").to_string();
            (date, time)
        })
        .unwrap_or_else(|| ("??-??-????".to_string(), "??:??:??".to_string()));

    // Format sender label
    let sender_label = match message.sender {
        ChatSender::Admin => "Admin",
        ChatSender::Buyer => "Buyer",
        ChatSender::Seller => "Seller",
    };

    // Format message for text file
    let formatted_message = format!(
        "{} - {} - {}\n{}\n\n",
        sender_label, date_str, time_str, message.content
    );

    // Open file in append mode (create if doesn't exist)
    let file_path = mostrix_dir.join(format!("{}.txt", dispute_id));
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(formatted_message.as_bytes()) {
                log::warn!("Failed to write chat message to file: {}", e);
            } else {
                log::debug!("Chat message saved to {:?}", file_path);
            }
        }
        Err(e) => {
            log::warn!("Failed to open chat file {:?}: {}", file_path, e);
        }
    }
}

/// Builds ListItems from chat messages for display in the chat list widget
/// Filters messages by active chat party and formats them with proper alignment
pub fn build_chat_list_items(
    messages: &[DisputeChatMessage],
    active_chat_party: ChatParty,
) -> Vec<ListItem<'_>> {
    if messages.is_empty() {
        return vec![ListItem::new(Line::from(Span::styled(
            "No messages yet. Start the conversation!",
            Style::default().fg(Color::Gray),
        )))];
    }

    messages
        .iter()
        .filter_map(|msg| {
            // Filter by active chat party
            let should_show = match msg.sender {
                ChatSender::Admin => true,
                ChatSender::Buyer => active_chat_party == ChatParty::Buyer,
                ChatSender::Seller => active_chat_party == ChatParty::Seller,
            };

            if !should_show {
                return None;
            }

            // Format date and time
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
                ChatSender::Seller => ("Seller", Color::Red, true),
            };

            // Header line: "Sender - date - time"
            let header_text = format!("{} - {} - {}", sender_label, date_str, time_str);

            // Create multi-line ListItem for this message
            let mut message_lines = Vec::new();

            if is_right_aligned {
                // Right-align buyer/seller messages
                let header_span = Span::styled(header_text, Style::default().fg(sender_color));
                message_lines.push(header_span.into_right_aligned_line());

                let message_span =
                    Span::styled(msg.content.clone(), Style::default().fg(sender_color));
                message_lines.push(message_span.into_right_aligned_line());
            } else {
                // Left-align admin messages
                message_lines.push(Line::from(vec![Span::styled(
                    header_text,
                    Style::default().fg(sender_color),
                )]));
                message_lines.push(Line::from(vec![Span::styled(
                    msg.content.clone(),
                    Style::default().fg(sender_color),
                )]));
            }

            // Add empty line for spacing
            message_lines.push(Line::from(""));

            Some(ListItem::new(message_lines))
        })
        .collect()
}

/// Renders a vertical scrollbar for the chat list on the right side of the given area
/// Calculates scroll position based on ListState selection
pub fn render_chat_scrollbar(
    f: &mut ratatui::Frame,
    area: Rect,
    total_items: usize,
    list_state: &ratatui::widgets::ListState,
) {
    if total_items == 0 {
        return;
    }

    let chat_area_height = area.height.saturating_sub(2); // Subtract borders
    let viewport_content_length = chat_area_height as usize;

    // Calculate scrollbar position from ListState selection
    // Position represents how many items are scrolled past at the top
    let selection_idx = list_state
        .selected()
        .unwrap_or(total_items.saturating_sub(1));

    // When at bottom, show max scroll position; otherwise approximate based on selection
    let scroll_position =
        if selection_idx >= total_items.saturating_sub(viewport_content_length.min(total_items)) {
            // At or near bottom: show maximum scroll position
            total_items.saturating_sub(viewport_content_length.min(total_items))
        } else {
            // Not at bottom: use selection as approximate position
            selection_idx
        };

    // Create scrollbar state
    let mut scrollbar_state = ScrollbarState::new(total_items)
        .content_length(total_items)
        .viewport_content_length(viewport_content_length.min(total_items))
        .position(scroll_position);

    // Create and render vertical scrollbar on the right
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"))
        .track_symbol(Some("│"))
        .thumb_symbol("█");

    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}
