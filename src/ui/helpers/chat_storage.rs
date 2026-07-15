use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::DateTime;

use crate::ui::{ChatParty, ChatSender, DisputeChatMessage, UserChatSender, UserOrderChatMessage};

use super::attachments::{
    legacy_placeholder_matches_filename, message_fields_from_transcript_content,
    serialize_attachment_for_transcript, try_parse_attachment_message,
};
use super::chat_render::wrap_text_to_lines;

const DISPUTES_CHAT_DIR: &str = "disputes_chat";
const ORDERS_CHAT_DIR: &str = "orders_chat";

#[derive(Clone, Copy)]
enum ChatStorageKind {
    Disputes,
    Orders,
}

impl ChatStorageKind {
    fn folder_name(self) -> &'static str {
        match self {
            ChatStorageKind::Disputes => DISPUTES_CHAT_DIR,
            ChatStorageKind::Orders => ORDERS_CHAT_DIR,
        }
    }

    fn log_label(self) -> &'static str {
        match self {
            ChatStorageKind::Disputes => "dispute chat",
            ChatStorageKind::Orders => "order chat",
        }
    }
}

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

fn parse_one_order_message_block(block: &str) -> Option<(UserChatSender, i64, String)> {
    let mut lines = block.lines();
    let header = lines.next()?;
    let parts: Vec<&str> = header.splitn(3, " - ").collect();
    if parts.len() != 3 {
        return None;
    }
    let sender = match parts[0].trim() {
        "You" => UserChatSender::You,
        "Peer" => UserChatSender::Peer,
        "Admin" | "Admin to Buyer" | "Admin to Seller" => UserChatSender::You,
        "Buyer" | "Seller" => UserChatSender::Peer,
        _ => return None,
    };
    let date = chrono::NaiveDate::parse_from_str(parts[1].trim(), "%d-%m-%Y").ok()?;
    let time = chrono::NaiveTime::parse_from_str(parts[2].trim(), "%H:%M:%S").ok()?;
    let ts = date.and_time(time).and_utc().timestamp();
    let content = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    Some((sender, ts, content))
}

fn parse_last_message_block(content: &str) -> Option<(ChatSender, Option<ChatParty>, i64, String)> {
    let blocks: Vec<&str> = content
        .split("\n\n")
        .filter(|s| !s.trim().is_empty())
        .collect();
    parse_one_message_block(blocks.last()?)
}

fn transcript_body_for_order_message(message: &UserOrderChatMessage) -> String {
    match &message.attachment {
        Some(att) => serialize_attachment_for_transcript(att),
        None => wrap_text_to_lines(&message.content, 80).join("\n"),
    }
}

fn transcript_body_for_dispute_message(message: &DisputeChatMessage) -> String {
    match &message.attachment {
        Some(att) => serialize_attachment_for_transcript(att),
        None => wrap_text_to_lines(&message.content, 80).join("\n"),
    }
}

fn order_transcript_already_has_message(
    last_sender: UserChatSender,
    last_ts: i64,
    last_body: &str,
    message: &UserOrderChatMessage,
) -> bool {
    if last_sender != message.sender || last_ts != message.timestamp {
        return false;
    }
    let body = transcript_body_for_order_message(message);
    if last_body == body {
        return true;
    }
    if let Some(att) = &message.attachment {
        if try_parse_attachment_message(last_body)
            .is_some_and(|(parsed, _)| parsed.blossom_url == att.blossom_url)
        {
            return true;
        }
        if legacy_placeholder_matches_filename(last_body, &att.filename) {
            return true;
        }
    }
    false
}

fn dispute_transcript_already_has_message(
    last_sender: ChatSender,
    last_target_party: Option<ChatParty>,
    last_ts: i64,
    last_body: &str,
    message: &DisputeChatMessage,
) -> bool {
    if last_sender != message.sender
        || last_ts != message.timestamp
        || last_target_party != message.target_party
    {
        return false;
    }
    let body = transcript_body_for_dispute_message(message);
    if last_body == body {
        return true;
    }
    if let Some(att) = &message.attachment {
        if try_parse_attachment_message(last_body)
            .is_some_and(|(parsed, _)| parsed.blossom_url == att.blossom_url)
        {
            return true;
        }
        if legacy_placeholder_matches_filename(last_body, &att.filename) {
            return true;
        }
    }
    false
}

fn chat_file_path(kind: ChatStorageKind, chat_id: &str) -> Option<PathBuf> {
    if uuid::Uuid::parse_str(chat_id).is_err() {
        return None;
    }
    let home_dir = dirs::home_dir()?;
    Some(
        home_dir
            .join(".mostrix")
            .join(kind.folder_name())
            .join(format!("{}.txt", chat_id)),
    )
}

fn load_chat_from_file_by_kind(
    kind: ChatStorageKind,
    chat_id: &str,
) -> Option<Vec<DisputeChatMessage>> {
    let file_path = chat_file_path(kind, chat_id)?;
    let content = fs::read_to_string(&file_path).ok()?;
    let mut messages = Vec::new();
    for block in content.split("\n\n").filter(|s| !s.trim().is_empty()) {
        if let Some((sender, target_party, ts, content_block)) = parse_one_message_block(block) {
            let (content, attachment) = message_fields_from_transcript_content(&content_block);
            messages.push(DisputeChatMessage {
                sender,
                content,
                timestamp: ts,
                target_party,
                attachment,
            });
        }
    }
    if messages.is_empty() {
        return None;
    }
    Some(messages)
}

fn load_order_chat_from_file_by_kind(
    kind: ChatStorageKind,
    chat_id: &str,
) -> Option<Vec<UserOrderChatMessage>> {
    let file_path = chat_file_path(kind, chat_id)?;
    let content = fs::read_to_string(&file_path).ok()?;
    let mut messages = Vec::new();
    for block in content.split("\n\n").filter(|s| !s.trim().is_empty()) {
        if let Some((sender, ts, content_block)) = parse_one_order_message_block(block) {
            let (content, attachment) = message_fields_from_transcript_content(&content_block);
            messages.push(UserOrderChatMessage {
                sender,
                content,
                timestamp: ts,
                attachment,
            });
        }
    }
    if messages.is_empty() {
        return None;
    }
    Some(messages)
}

/// Loads dispute chat messages from `~/.mostrix/disputes_chat/<dispute_id>.txt`.
pub fn load_chat_from_file(dispute_id: &str) -> Option<Vec<DisputeChatMessage>> {
    load_chat_from_file_by_kind(ChatStorageKind::Disputes, dispute_id)
}

/// Persist one user order chat message into `~/.mostrix/orders_chat/<order_id>.txt`.
pub fn save_order_chat_message(order_id: &str, message: &UserOrderChatMessage) {
    let file_path = match chat_file_path(ChatStorageKind::Orders, order_id) {
        Some(path) => path,
        None => {
            log::warn!("Invalid order chat id format, skipping save: {}", order_id);
            return;
        }
    };
    let Some(chat_dir) = file_path.parent() else {
        log::warn!("Failed to resolve order chat folder for id {}", order_id);
        return;
    };
    if let Err(e) = fs::create_dir_all(chat_dir) {
        log::warn!("Failed to create order chat folder {:?}: {}", chat_dir, e);
        return;
    }

    let content_block = transcript_body_for_order_message(message);
    if let Ok(existing) = fs::read_to_string(&file_path) {
        let blocks: Vec<&str> = existing
            .split("\n\n")
            .filter(|s| !s.trim().is_empty())
            .collect();
        if let Some(last_block) = blocks.last() {
            if let Some((last_sender, last_ts, last_content)) =
                parse_one_order_message_block(last_block)
            {
                if order_transcript_already_has_message(
                    last_sender,
                    last_ts,
                    &last_content,
                    message,
                ) {
                    return;
                }
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
    let sender_label = match message.sender {
        UserChatSender::You => "You",
        UserChatSender::Peer => "Peer",
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
                log::warn!("Failed to write order chat message to file: {}", e);
            } else {
                log::debug!("Saved order chat message to {:?}", file_path);
            }
        }
        Err(e) => log::warn!("Failed to open order chat file {:?}: {}", file_path, e),
    }
}

/// Load cached user order chat from `~/.mostrix/orders_chat/<order_id>.txt`.
pub fn load_order_chat_from_file(order_id: &str) -> Option<Vec<UserOrderChatMessage>> {
    load_order_chat_from_file_by_kind(ChatStorageKind::Orders, order_id)
}

/// Max message timestamp from the on-disk order chat transcript (cursor for relay hydrate).
pub fn order_chat_since_from_file(order_id: &str) -> Option<i64> {
    load_order_chat_from_file(order_id).and_then(|msgs| msgs.iter().map(|m| m.timestamp).max())
}

/// Per-party max timestamps from the on-disk dispute transcript (cursor for relay hydrate).
///
/// Returns `(buyer_since, seller_since)`; a side with no messages yields `None`.
pub fn dispute_chat_since_from_file(dispute_id: &str) -> (Option<i64>, Option<i64>) {
    match load_chat_from_file(dispute_id) {
        Some(msgs) => {
            let (buyer_max, seller_max) = max_party_timestamps(&msgs);
            (
                (buyer_max > 0).then_some(buyer_max),
                (seller_max > 0).then_some(seller_max),
            )
        }
        None => (None, None),
    }
}

/// Saves a dispute chat message to a text file in `~/.mostrix/disputes_chat/<dispute_id>.txt`.
pub fn save_chat_message(dispute_id: &str, message: &DisputeChatMessage) {
    save_chat_message_by_kind(ChatStorageKind::Disputes, dispute_id, message);
}

fn save_chat_message_by_kind(kind: ChatStorageKind, chat_id: &str, message: &DisputeChatMessage) {
    let file_path = match chat_file_path(kind, chat_id) {
        Some(path) => path,
        None => {
            log::warn!(
                "Invalid {} id format, skipping save: {}",
                kind.log_label(),
                chat_id
            );
            return;
        }
    };
    let Some(chat_dir) = file_path.parent() else {
        log::warn!(
            "Failed to resolve {} folder for id {}",
            kind.log_label(),
            chat_id
        );
        return;
    };
    if let Err(e) = fs::create_dir_all(chat_dir) {
        log::warn!(
            "Failed to create {} folder {:?}: {}",
            kind.log_label(),
            chat_dir,
            e
        );
        return;
    }

    let content_block = transcript_body_for_dispute_message(message);

    if let Ok(existing) = fs::read_to_string(&file_path) {
        if let Some((last_sender, last_target_party, last_ts, last_content)) =
            parse_last_message_block(&existing)
        {
            if dispute_transcript_already_has_message(
                last_sender,
                last_target_party,
                last_ts,
                &last_content,
                message,
            ) {
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

    let sender_label = match kind {
        ChatStorageKind::Disputes => match (&message.sender, message.target_party) {
            (ChatSender::Admin, Some(ChatParty::Buyer)) => "Admin to Buyer",
            (ChatSender::Admin, Some(ChatParty::Seller)) => "Admin to Seller",
            (ChatSender::Admin, None) => "Admin",
            (ChatSender::Buyer, _) => "Buyer",
            (ChatSender::Seller, _) => "Seller",
        },
        ChatStorageKind::Orders => match message.sender {
            ChatSender::Admin => "You",
            ChatSender::Buyer | ChatSender::Seller => "Peer",
        },
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
                log::warn!(
                    "Failed to write {} message to file: {}",
                    kind.log_label(),
                    e
                );
            } else {
                log::debug!("Saved {} message to {:?}", kind.log_label(), file_path);
            }
        }
        Err(e) => {
            log::warn!(
                "Failed to open {} file {:?}: {}",
                kind.log_label(),
                file_path,
                e
            );
        }
    }
}

pub(crate) fn max_party_timestamps(messages: &[DisputeChatMessage]) -> (i64, i64) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        ChatAttachment, ChatAttachmentType, ChatSender, DisputeChatMessage, UserChatSender,
        UserOrderChatMessage,
    };

    use super::super::attachments::serialize_attachment_for_transcript;

    #[test]
    fn transcript_body_roundtrip_via_message_fields() {
        let att = ChatAttachment {
            blossom_url: "blossom://host/hash".to_string(),
            filename: "doc.pdf".to_string(),
            mime_type: None,
            file_type: ChatAttachmentType::File,
            decryption_key: None,
        };
        let json = serialize_attachment_for_transcript(&att);
        let (content, restored) = message_fields_from_transcript_content(&json);
        let restored = restored.expect("attachment");
        assert_eq!(restored.blossom_url, att.blossom_url);
        assert!(content.contains("doc.pdf"));

        let msg = UserOrderChatMessage {
            sender: UserChatSender::Peer,
            content,
            timestamp: 1_700_000_000,
            attachment: Some(restored),
        };
        assert_eq!(transcript_body_for_order_message(&msg), json);
    }

    #[test]
    fn legacy_placeholder_load_has_no_attachment_until_relay() {
        let (content, attachment) =
            message_fields_from_transcript_content("[Image: pic.png - Ctrl+S to save]");
        assert!(attachment.is_none());
        assert_eq!(content, "[Image: pic.png - Ctrl+S to save]");
    }

    #[test]
    fn parse_order_block_supports_legacy_sender_labels() {
        let block = "Admin to Buyer - 10-10-2024 - 01:02:03\nhello";
        let parsed = parse_one_order_message_block(block).expect("valid parsed block");
        assert_eq!(parsed.0, UserChatSender::You);
        assert_eq!(parsed.2, "hello");
    }

    #[test]
    fn parse_last_message_returns_most_recent_block() {
        let file_data = concat!(
            "Buyer - 10-10-2024 - 01:02:03\nfirst\n\n",
            "Admin to Buyer - 11-10-2024 - 01:02:03\nsecond\n\n"
        );
        let parsed = parse_last_message_block(file_data).expect("last message parsed");
        assert_eq!(parsed.0, ChatSender::Admin);
        assert_eq!(parsed.1, Some(ChatParty::Buyer));
        assert_eq!(parsed.3, "second");
    }

    #[test]
    fn max_party_timestamps_tracks_each_side() {
        let msgs = vec![
            DisputeChatMessage {
                sender: ChatSender::Buyer,
                content: "a".to_string(),
                timestamp: 10,
                target_party: None,
                attachment: None,
            },
            DisputeChatMessage {
                sender: ChatSender::Seller,
                content: "b".to_string(),
                timestamp: 20,
                target_party: None,
                attachment: None,
            },
            DisputeChatMessage {
                sender: ChatSender::Buyer,
                content: "c".to_string(),
                timestamp: 30,
                target_party: None,
                attachment: None,
            },
        ];
        let (buyer, seller) = max_party_timestamps(&msgs);
        assert_eq!(buyer, 30);
        assert_eq!(seller, 20);
    }
}
