use crate::models::AdminDispute;
use chrono::DateTime;
use mostro_core::prelude::UserInfo;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use std::fs::{self, OpenOptions};
use std::io::Write;

use std::collections::HashMap;
use std::time::Instant;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use nostr_sdk::prelude::PublicKey;
use nostr_sdk::serde_json::{from_str as json_from_str, Value};

use super::{
    AdminChatLastSeen, AdminChatUpdate, AppState, ChatAttachment, ChatAttachmentType, ChatParty,
    ChatSender, DisputeChatMessage, PRIMARY_COLOR,
};

/// Placeholder text written to transcript file for attachment messages (no blob persisted).
fn attachment_placeholder(att: &ChatAttachment) -> String {
    let kind = match att.file_type {
        ChatAttachmentType::Image => "Image",
        ChatAttachmentType::File => "File",
    };
    format!("[{}: {} - Ctrl+S to save]", kind, att.filename)
}

/// Formats user rating with star visualization
/// Rating must be in 0-5 range. Returns formatted string with stars and stats.
pub fn format_user_rating(info: Option<&UserInfo>) -> String {
    if let Some(info) = info {
        let star_count = (info.rating.round() as usize).min(5);
        let stars = "⭐".repeat(star_count);
        format!(
            "{} {:.1}/5 ({} trades completed, {} days)",
            stars, info.rating, info.reviews, info.operating_days
        )
    } else {
        "No rating available".to_string()
    }
}

/// Check if a dispute is finalized (Settled, SellerRefunded, or Released)
///
/// This is a convenience wrapper around `AdminDispute::is_finalized()` for UI code.
/// Returns `Some(true)` if finalized, `Some(false)` if not finalized.
pub fn is_dispute_finalized(selected_dispute: &AdminDispute) -> Option<bool> {
    Some(selected_dispute.is_finalized())
}

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

/// Parses one message block (header line "Sender - dd-mm-yyyy - HH:MM:SS" or
/// "Admin to Buyer/Seller - dd-mm-yyyy - HH:MM:SS", rest = content).
/// Returns (sender, target_party for Admin, timestamp, content) if valid.
fn parse_one_message_block(block: &str) -> Option<(ChatSender, Option<ChatParty>, i64, String)> {
    let mut lines = block.lines();
    let header = lines.next()?;
    let parts: Vec<&str> = header.splitn(3, " - ").collect();
    if parts.len() != 3 {
        return None;
    }
    let first = parts[0].trim();
    let (sender, target_party) = match first {
        "Admin to Buyer" => (ChatSender::Admin, Some(ChatParty::Buyer)),
        "Admin to Seller" => (ChatSender::Admin, Some(ChatParty::Seller)),
        "Admin" => (ChatSender::Admin, None),
        "Buyer" => (ChatSender::Buyer, None),
        "Seller" => (ChatSender::Seller, None),
        _ => return None,
    };
    let date_str = parts[1].trim();
    let time_str = parts[2].trim();
    let date = match chrono::NaiveDate::parse_from_str(date_str, "%d-%m-%Y") {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Malformed date '{}' in chat block: {}", date_str, e);
            return None;
        }
    };
    let time = match chrono::NaiveTime::parse_from_str(time_str, "%H:%M:%S") {
        Ok(t) => t,
        Err(e) => {
            log::warn!("Malformed time '{}' in chat block: {}", time_str, e);
            return None;
        }
    };
    let ts = date.and_time(time).and_utc().timestamp();
    let content_block = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    Some((sender, target_party, ts, content_block))
}

/// Parses the last message block from file content (blocks separated by "\n\n").
fn parse_last_message_block(content: &str) -> Option<(ChatSender, Option<ChatParty>, i64, String)> {
    let blocks: Vec<&str> = content
        .split("\n\n")
        .filter(|s| !s.trim().is_empty())
        .collect();
    parse_one_message_block(blocks.last()?)
}

/// Loads chat messages from ~/.mostrix/dispute_id.txt if the file exists.
/// Returns messages in file order. On IO/parse error returns None and logs.
pub fn load_chat_from_file(dispute_id: &str) -> Option<Vec<DisputeChatMessage>> {
    if uuid::Uuid::parse_str(dispute_id).is_err() {
        return None;
    }
    let home_dir = dirs::home_dir()?;
    let file_path = home_dir
        .join(".mostrix")
        .join(format!("{}.txt", dispute_id));
    let content = fs::read_to_string(&file_path).ok()?;
    let mut messages = Vec::new();
    for block in content.split("\n\n").filter(|s| !s.trim().is_empty()) {
        if let Some((sender, target_party, ts, content_block)) = parse_one_message_block(block) {
            messages.push(DisputeChatMessage {
                sender,
                content: content_block,
                timestamp: ts,
                target_party,
                attachment: None,
            });
        }
    }
    if messages.is_empty() {
        return None;
    }
    Some(messages)
}

/// Get max timestamp for buyer and seller.
fn get_max_timestamp(messages: &[DisputeChatMessage]) -> (i64, i64) {
    let buyer_max = messages
        .iter()
        .filter(|m| m.sender == ChatSender::Buyer)
        .map(|m| m.timestamp)
        .max()
        .unwrap_or(0);
    let seller_max = messages
        .iter()
        .filter(|m| m.sender == ChatSender::Seller)
        .map(|m| m.timestamp)
        .max()
        .unwrap_or(0);
    (buyer_max, seller_max)
}

/// Update last-seen timestamps for buyer and seller.
/// Uses entry API to ensure entries exist before comparing, so recovered timestamps
/// from files are stored even if the HashMap was initially empty.
fn update_last_seen_timestamp(
    buyer_max_timestamp: i64,
    seller_max_timestamp: i64,
    dispute: &AdminDispute,
    admin_chat_last_seen: &mut HashMap<(String, ChatParty), AdminChatLastSeen>,
) {
    let buyer_entry = admin_chat_last_seen
        .entry((dispute.dispute_id.clone(), ChatParty::Buyer))
        .or_insert_with(|| AdminChatLastSeen {
            last_seen_timestamp: None,
        });
    if buyer_max_timestamp > buyer_entry.last_seen_timestamp.unwrap_or(0) {
        buyer_entry.last_seen_timestamp = Some(buyer_max_timestamp);
    }

    let seller_entry = admin_chat_last_seen
        .entry((dispute.dispute_id.clone(), ChatParty::Seller))
        .or_insert_with(|| AdminChatLastSeen {
            last_seen_timestamp: None,
        });
    if seller_max_timestamp > seller_entry.last_seen_timestamp.unwrap_or(0) {
        seller_entry.last_seen_timestamp = Some(seller_max_timestamp);
    }
}

/// Recover chat history from saved files for InProgress disputes (instant UI).
/// Populates `admin_dispute_chats` and advances `last_seen_timestamp` in
/// `admin_chat_last_seen` from file timestamps for incremental fetch filtering.
pub fn recover_admin_chat_from_files(
    admin_disputes_in_progress: &[AdminDispute],
    admin_dispute_chats: &mut HashMap<String, Vec<DisputeChatMessage>>,
    admin_chat_last_seen: &mut HashMap<(String, ChatParty), AdminChatLastSeen>,
) {
    use std::str::FromStr;
    for dispute in admin_disputes_in_progress {
        let is_in_progress = dispute
            .status
            .as_deref()
            .and_then(|s| mostro_core::prelude::DisputeStatus::from_str(s).ok())
            == Some(mostro_core::prelude::DisputeStatus::InProgress);
        if !is_in_progress {
            continue;
        }
        if let Some(msgs) = load_chat_from_file(&dispute.dispute_id) {
            admin_dispute_chats.insert(dispute.dispute_id.clone(), msgs.clone());
            // Get max timestamp for buyer and seller
            let (buyer_max, seller_max) = get_max_timestamp(&msgs);
            // Update last-seen timestamps for buyer and seller
            update_last_seen_timestamp(buyer_max, seller_max, dispute, admin_chat_last_seen);
        }
    }
}

/// Parses Mostro Mobile image_encrypted / file_encrypted JSON. Returns (ChatAttachment, display_content) or None.
fn try_parse_attachment_message(content: &str) -> Option<(ChatAttachment, String)> {
    let content = content.trim();
    if !content.starts_with('{') {
        return None;
    }
    let root: Value = json_from_str(content).ok()?;
    let obj = root.as_object()?;
    let msg_type = obj.get("type")?.as_str()?;
    let (file_type, icon) = match msg_type {
        "image_encrypted" => (ChatAttachmentType::Image, "🖼"),
        "file_encrypted" => (ChatAttachmentType::File, "📎"),
        _ => return None,
    };
    let blossom_url = obj.get("blossom_url")?.as_str()?.to_string();
    let nonce_b64 = obj.get("nonce")?.as_str()?;
    let nonce = BASE64.decode(nonce_b64.as_bytes()).ok()?;
    if nonce.len() != 12 {
        return None;
    }
    let filename = obj.get("filename")?.as_str()?.to_string();
    let mime_type = obj
        .get("mime_type")
        .and_then(|v| v.as_str())
        .map(String::from);
    let decryption_key = obj
        .get("key")
        .and_then(|v| v.as_str())
        .and_then(|s| BASE64.decode(s.as_bytes()).ok())
        .filter(|k| k.len() == 32);
    let key_hint = if decryption_key.is_some() {
        " (key provided)"
    } else {
        ""
    };
    let attachment = ChatAttachment {
        blossom_url,
        nonce,
        filename: filename.clone(),
        mime_type,
        file_type,
        decryption_key,
    };
    let display = match file_type {
        ChatAttachmentType::Image => format!("{} Image: {}{}", icon, filename, key_hint),
        ChatAttachmentType::File => format!("{} File: {}{}", icon, filename, key_hint),
    };
    Some((attachment, display))
}

/// Apply fetched admin chat updates back into the UI state and persist
/// last_seen timestamps to the database.
pub async fn apply_admin_chat_updates(
    app: &mut AppState,
    updates: Vec<AdminChatUpdate>,
    admin_chat_pubkey: Option<&PublicKey>,
    pool: &sqlx::SqlitePool,
) -> Result<(), anyhow::Error> {
    for update in updates {
        let dispute_key = update.dispute_id.clone();
        let party = update.party;

        // Get or create the chat history vector for this dispute
        let messages_vec = app
            .admin_dispute_chats
            .entry(dispute_key.clone())
            .or_default();

        // Track max timestamp to update last_seen
        let mut max_ts = app
            .admin_chat_last_seen
            .get(&(dispute_key.clone(), party))
            .and_then(|s| s.last_seen_timestamp)
            .unwrap_or(0);

        for (content, ts, sender_pubkey) in update.messages {
            // Skip messages that we sent ourselves (admin identity), since we
            // already add them locally when sending.
            if let Some(admin_pk) = admin_chat_pubkey {
                if &sender_pubkey == admin_pk {
                    if ts > max_ts {
                        max_ts = ts;
                    }
                    continue;
                }
            }

            let sender = match party {
                ChatParty::Buyer => ChatSender::Buyer,
                ChatParty::Seller => ChatSender::Seller,
            };

            // Normalize to (display content, optional attachment + filename for toast)
            let (msg_content, attachment_opt) = match try_parse_attachment_message(&content) {
                Some((attachment, display)) => {
                    let filename = attachment.filename.clone();
                    (display, Some((attachment, filename)))
                }
                None => (content.clone(), None),
            };

            // Single duplicate check (same timestamp, sender, and content)
            let is_duplicate = messages_vec.iter().any(|m: &DisputeChatMessage| {
                m.timestamp == ts && m.sender == sender && m.content == msg_content
            });
            if is_duplicate {
                if ts > max_ts {
                    max_ts = ts;
                }
                continue;
            }

            if let Some((_, filename_for_toast)) = &attachment_opt {
                app.attachment_toast = Some((
                    format!("📎 File received: {} — Ctrl+S to save", filename_for_toast),
                    Instant::now(),
                ));
            }
            let msg = match attachment_opt {
                Some((attachment, _)) => DisputeChatMessage {
                    sender,
                    content: msg_content,
                    timestamp: ts,
                    target_party: None,
                    attachment: Some(attachment),
                },
                None => DisputeChatMessage {
                    sender,
                    content: msg_content,
                    timestamp: ts,
                    target_party: None,
                    attachment: None,
                },
            };
            save_chat_message(&dispute_key, &msg);
            messages_vec.push(msg);
            if ts > max_ts {
                max_ts = ts;
            }
        }

        // Update last_seen_timestamp for this dispute/party in memory
        // Use entry API to ensure entry exists, so updates persist even for new disputes
        let entry = app
            .admin_chat_last_seen
            .entry((dispute_key.clone(), party))
            .or_insert_with(|| AdminChatLastSeen {
                last_seen_timestamp: None,
            });
        if max_ts > entry.last_seen_timestamp.unwrap_or(0) {
            entry.last_seen_timestamp = Some(max_ts);
        }

        // Persist last_seen_timestamp to the database so we can resume incremental
        // fetching after restart without scanning the full history.
        if max_ts > 0 {
            AdminDispute::update_chat_last_seen_by_dispute_id(
                pool,
                &dispute_key,
                max_ts,
                party == ChatParty::Buyer,
            )
            .await?;
        }
    }

    Ok(())
}

/// Saves a chat message to a text file in ~/.mostrix/dispute_id.txt
/// Creates the directory and file if they don't exist, appends if they do.
/// Idempotent: skips append if the last message in the file already matches (avoids duplicates when refetching from relay).
pub fn save_chat_message(dispute_id: &str, message: &DisputeChatMessage) {
    // Validate dispute_id to prevent path traversal attacks
    if uuid::Uuid::parse_str(dispute_id).is_err() {
        log::warn!(
            "Invalid dispute_id format, skipping chat save: {}",
            dispute_id
        );
        return;
    }

    let home_dir = match dirs::home_dir() {
        Some(dir) => dir,
        None => {
            log::warn!("Could not find home directory, skipping chat save");
            return;
        }
    };

    let mostrix_dir = home_dir.join(".mostrix");
    if let Err(e) = fs::create_dir_all(&mostrix_dir) {
        log::warn!("Failed to create .mostrix directory: {}", e);
        return;
    }

    let file_path = mostrix_dir.join(format!("{}.txt", dispute_id));

    // Content to write: placeholder for attachments, wrapped text for plain messages
    let content_block = match &message.attachment {
        Some(att) => attachment_placeholder(att),
        None => wrap_text_to_lines(&message.content, 80).join("\n"),
    };

    // Idempotent: skip append if last message in file already matches
    if let Ok(existing) = fs::read_to_string(&file_path) {
        if let Some((last_sender, last_target_party, last_ts, last_content)) =
            parse_last_message_block(&existing)
        {
            if last_sender == message.sender
                && last_ts == message.timestamp
                && last_content == content_block
                && last_target_party == message.target_party
            {
                return;
            }
        }
    }

    let (date_str, time_str) = DateTime::from_timestamp(message.timestamp, 0)
        .map(|dt| {
            let date = dt.format("%d-%m-%Y").to_string();
            let time = dt.format("%H:%M:%S").to_string();
            (date, time)
        })
        .unwrap_or_else(|| ("??-??-????".to_string(), "??:??:??".to_string()));

    let sender_label = match (&message.sender, message.target_party) {
        (ChatSender::Admin, Some(ChatParty::Buyer)) => "Admin to Buyer",
        (ChatSender::Admin, Some(ChatParty::Seller)) => "Admin to Seller",
        (ChatSender::Admin, None) => "Admin",
        (ChatSender::Buyer, _) => "Buyer",
        (ChatSender::Seller, _) => "Seller",
    };
    let formatted_message = format!(
        "{} - {} - {}\n{}\n\n",
        sender_label, date_str, time_str, content_block
    );

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

/// Wraps text to a maximum display width (in columns), breaking at word boundaries.
/// Uses ratatui's Span width for Unicode-aware measurement. Words longer than
/// max_width are placed on their own line.
fn wrap_text_to_lines(content: &str, max_width: u16) -> Vec<String> {
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

/// Returns true if this message should be shown in the given party's chat view.
fn message_visible_for_party(msg: &DisputeChatMessage, active_chat_party: ChatParty) -> bool {
    match msg.sender {
        ChatSender::Admin => msg.target_party.is_none_or(|p| p == active_chat_party),
        ChatSender::Buyer => active_chat_party == ChatParty::Buyer,
        ChatSender::Seller => active_chat_party == ChatParty::Seller,
    }
}

/// Returns the number of messages in the given list that are visible for the active party and have an attachment.
pub fn count_visible_attachments(
    messages: &[DisputeChatMessage],
    active_chat_party: ChatParty,
) -> usize {
    messages
        .iter()
        .filter(|msg| message_visible_for_party(msg, active_chat_party) && msg.attachment.is_some())
        .count()
}

/// Returns the currently selected chat message (by list index) for the given dispute, or None.
pub fn get_selected_chat_message<'a>(
    app: &'a AppState,
    dispute_id_key: &str,
) -> Option<&'a DisputeChatMessage> {
    let messages = app.admin_dispute_chats.get(dispute_id_key)?;
    let selected_idx = app.admin_chat_list_state.selected()?;
    let visible: Vec<&DisputeChatMessage> = messages
        .iter()
        .filter(|msg| message_visible_for_party(msg, app.active_chat_party))
        .collect();
    visible.get(selected_idx).copied()
}

/// Builds ListItems from chat messages for display in the chat list widget.
/// Filters messages by active chat party and formats them with proper alignment.
/// If `max_content_width` is Some(w), message content is wrapped to at most w
/// columns per line (word boundaries); long messages use multiple lines.
pub fn build_chat_list_items(
    messages: &[DisputeChatMessage],
    active_chat_party: ChatParty,
    max_content_width: Option<u16>,
) -> Vec<ListItem<'_>> {
    // First compute the filtered message iterator and collect into Vec
    let filtered_items: Vec<ListItem<'_>> = messages
        .iter()
        .filter_map(|msg| {
            // Filter by active chat party
            let should_show = message_visible_for_party(msg, active_chat_party);

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
            // Attachment messages: distinct color so they stand out (content already has 📎/🖼)
            let content_color = msg
                .attachment
                .as_ref()
                .map(|_| Color::Yellow)
                .unwrap_or(sender_color);

            // Header line: "Sender - date - time"
            let header_text = format!("{} - {} - {}", sender_label, date_str, time_str);

            // Create multi-line ListItem for this message
            let mut message_lines = Vec::new();

            if is_right_aligned {
                // Right-align buyer/seller messages
                let header_span = Span::styled(header_text, Style::default().fg(sender_color));
                message_lines.push(header_span.into_right_aligned_line());

                let content_lines = max_content_width
                    .map(|w| wrap_text_to_lines(&msg.content, w))
                    .unwrap_or_else(|| vec![msg.content.clone()]);
                for line in content_lines {
                    let span = Span::styled(line, Style::default().fg(content_color));
                    message_lines.push(span.into_right_aligned_line());
                }
            } else {
                // Left-align admin messages
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

            // Add empty line for spacing
            message_lines.push(Line::from(""));

            Some(ListItem::new(message_lines))
        })
        .collect();

    // If filtered list is empty, return placeholder
    if filtered_items.is_empty() {
        return vec![ListItem::new(Line::from(Span::styled(
            "No messages yet. Start the conversation!",
            Style::default().fg(Color::Gray),
        )))];
    }

    filtered_items
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
