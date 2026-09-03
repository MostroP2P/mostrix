use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};

use chrono::DateTime;
use nostr_sdk::prelude::EventId;

use crate::ui::{ChatParty, ChatSender, DisputeChatMessage, UserChatSender, UserOrderChatMessage};
use crate::util::chat_utils::clamp_chat_since_cursor_now;

use super::attachments::{
    legacy_placeholder_matches_filename, message_fields_from_transcript_content,
    serialize_attachment_for_transcript, try_parse_attachment_message,
};
use super::chat_render::wrap_text_to_lines;

const DISPUTES_CHAT_DIR: &str = "disputes_chat";
const ORDERS_CHAT_DIR: &str = "orders_chat";
const USER_DISPUTES_CHAT_DIR: &str = "user_disputes_chat";

#[derive(Clone, Copy)]
enum ChatStorageKind {
    Disputes,
    Orders,
    UserDisputes,
}

impl ChatStorageKind {
    fn folder_name(self) -> &'static str {
        match self {
            ChatStorageKind::Disputes => DISPUTES_CHAT_DIR,
            ChatStorageKind::Orders => ORDERS_CHAT_DIR,
            ChatStorageKind::UserDisputes => USER_DISPUTES_CHAT_DIR,
        }
    }

    fn log_label(self) -> &'static str {
        match self {
            ChatStorageKind::Disputes => "dispute chat",
            ChatStorageKind::Orders => "order chat",
            ChatStorageKind::UserDisputes => "user dispute chat",
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
///
/// Returns `true` when the message is durably represented on disk (newly written
/// or already present as the last transcript block). Returns `false` on I/O failure.
pub fn save_order_chat_message(order_id: &str, message: &UserOrderChatMessage) -> bool {
    save_user_chat_message_by_kind(ChatStorageKind::Orders, order_id, message)
}

fn save_user_chat_message_by_kind(
    kind: ChatStorageKind,
    chat_id: &str,
    message: &UserOrderChatMessage,
) -> bool {
    let file_path = match chat_file_path(kind, chat_id) {
        Some(path) => path,
        None => {
            log::warn!(
                "Invalid {} id format, skipping save: {}",
                kind.log_label(),
                chat_id
            );
            return false;
        }
    };
    let Some(chat_dir) = file_path.parent() else {
        log::warn!(
            "Failed to resolve {} folder for id {}",
            kind.log_label(),
            chat_id
        );
        return false;
    };
    if let Err(e) = fs::create_dir_all(chat_dir) {
        log::warn!(
            "Failed to create {} folder {:?}: {}",
            kind.log_label(),
            chat_dir,
            e
        );
        return false;
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
                    return true;
                }
            }
        }
    }
    let formatted_message = format_order_transcript_block(message, &content_block);
    append_transcript_block(&file_path, &formatted_message, kind.log_label())
}

/// Rewrite the full order-chat transcript (used when upgrading a placeholder in place).
///
/// Returns `true` on successful atomic replace; `false` on I/O failure (caller should
/// not treat the in-memory upgrade as durable).
pub fn rewrite_order_chat_messages(order_id: &str, messages: &[UserOrderChatMessage]) -> bool {
    rewrite_user_chat_messages_by_kind(ChatStorageKind::Orders, order_id, messages)
}

fn rewrite_user_chat_messages_by_kind(
    kind: ChatStorageKind,
    chat_id: &str,
    messages: &[UserOrderChatMessage],
) -> bool {
    let file_path = match chat_file_path(kind, chat_id) {
        Some(path) => path,
        None => {
            log::warn!(
                "Invalid {} id format, skipping rewrite: {}",
                kind.log_label(),
                chat_id
            );
            return false;
        }
    };
    let Some(chat_dir) = file_path.parent() else {
        log::warn!(
            "Failed to resolve {} folder for id {}",
            kind.log_label(),
            chat_id
        );
        return false;
    };
    if let Err(e) = fs::create_dir_all(chat_dir) {
        log::warn!(
            "Failed to create {} folder {:?}: {}",
            kind.log_label(),
            chat_dir,
            e
        );
        return false;
    }
    let mut body = String::new();
    for message in messages {
        let content_block = transcript_body_for_order_message(message);
        body.push_str(&format_order_transcript_block(message, &content_block));
    }
    write_transcript_file(&file_path, &body, kind.log_label())
}

fn format_order_transcript_block(message: &UserOrderChatMessage, content_block: &str) -> String {
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
    format!(
        "{} - {} - {}\n{}\n\n",
        sender_label, date_str, time_str, content_block
    )
}

fn append_transcript_block(file_path: &Path, formatted_message: &str, label: &str) -> bool {
    match OpenOptions::new().create(true).append(true).open(file_path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(formatted_message.as_bytes()) {
                log::warn!("Failed to write {label} message to file: {e}");
                false
            } else if let Err(e) = file.sync_all() {
                log::warn!("Failed to sync {label} message to file: {e}");
                false
            } else {
                log::debug!("Saved {label} message to {:?}", file_path);
                true
            }
        }
        Err(e) => {
            log::warn!("Failed to open {label} file {:?}: {e}", file_path);
            false
        }
    }
}

fn write_transcript_file(file_path: &Path, body: &str, label: &str) -> bool {
    let tmp_path = file_path.with_extension("txt.tmp");
    let write_ok = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp_path)
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(body.as_bytes()) {
                log::warn!("Failed to write {label} temp file {:?}: {e}", tmp_path);
                false
            } else if let Err(e) = file.sync_all() {
                log::warn!("Failed to sync {label} temp file {:?}: {e}", tmp_path);
                false
            } else {
                true
            }
        }
        Err(e) => {
            log::warn!("Failed to open {label} temp file {:?}: {e}", tmp_path);
            return false;
        }
    };
    if !write_ok {
        let _ = fs::remove_file(&tmp_path);
        return false;
    }
    if let Err(e) = fs::rename(&tmp_path, file_path) {
        log::warn!("Failed to replace {label} file {:?}: {e}", file_path);
        let _ = fs::remove_file(&tmp_path);
        false
    } else {
        log::debug!("Rewrote {label} transcript {:?}", file_path);
        true
    }
}

/// Load cached user order chat from `~/.mostrix/orders_chat/<order_id>.txt`.
pub fn load_order_chat_from_file(order_id: &str) -> Option<Vec<UserOrderChatMessage>> {
    load_order_chat_from_file_by_kind(ChatStorageKind::Orders, order_id)
}

/// Persist one user-to-solver message under the parent order id.
///
/// Returns `true` when the message is durably represented on disk.
pub fn save_user_dispute_chat_message(order_id: &str, message: &UserOrderChatMessage) -> bool {
    save_user_chat_message_by_kind(ChatStorageKind::UserDisputes, order_id, message)
}

/// Load cached user-to-solver messages for an order.
pub fn load_user_dispute_chat_from_file(order_id: &str) -> Option<Vec<UserOrderChatMessage>> {
    load_order_chat_from_file_by_kind(ChatStorageKind::UserDisputes, order_id)
}

/// Max accepted timestamp in the cached user-to-solver transcript.
pub fn user_dispute_chat_since_from_file(order_id: &str) -> Option<i64> {
    load_user_dispute_chat_from_file(order_id)
        .and_then(|msgs| msgs.iter().map(|m| m.timestamp).max())
        .map(clamp_chat_since_cursor_now)
}

/// Max message timestamp from the on-disk order chat transcript (cursor for relay hydrate).
///
/// Clamped to local now so a far-future transcript timestamp cannot poison `since`.
pub fn order_chat_since_from_file(order_id: &str) -> Option<i64> {
    load_order_chat_from_file(order_id)
        .and_then(|msgs| msgs.iter().map(|m| m.timestamp).max())
        .map(crate::util::chat_utils::clamp_chat_since_cursor_now)
}

/// Per-party max timestamps from the on-disk dispute transcript (cursor for relay hydrate).
///
/// Returns `(buyer_since, seller_since)`; a side with no messages yields `None`.
/// Each side is clamped to local now (protocol `since` cursor rule).
pub fn dispute_chat_since_from_file(dispute_id: &str) -> (Option<i64>, Option<i64>) {
    match load_chat_from_file(dispute_id) {
        Some(msgs) => {
            let (buyer_max, seller_max) = max_party_timestamps(&msgs);
            (
                (buyer_max > 0).then_some(crate::util::chat_utils::clamp_chat_since_cursor_now(
                    buyer_max,
                )),
                (seller_max > 0).then_some(crate::util::chat_utils::clamp_chat_since_cursor_now(
                    seller_max,
                )),
            )
        }
        None => (None, None),
    }
}

/// Saves a dispute chat message to a text file in `~/.mostrix/disputes_chat/<dispute_id>.txt`.
///
/// Returns `true` when durably represented on disk (written or already last block).
pub fn save_chat_message(dispute_id: &str, message: &DisputeChatMessage) -> bool {
    save_chat_message_by_kind(ChatStorageKind::Disputes, dispute_id, message)
}

/// Rewrite the full dispute-chat transcript (placeholder-in-place upgrades).
///
/// Returns `true` on successful atomic replace; `false` on I/O failure (caller should
/// not treat the in-memory upgrade as durable).
pub fn rewrite_dispute_chat_messages(dispute_id: &str, messages: &[DisputeChatMessage]) -> bool {
    let file_path = match chat_file_path(ChatStorageKind::Disputes, dispute_id) {
        Some(path) => path,
        None => {
            log::warn!(
                "Invalid dispute chat id format, skipping rewrite: {}",
                dispute_id
            );
            return false;
        }
    };
    let Some(chat_dir) = file_path.parent() else {
        log::warn!(
            "Failed to resolve dispute chat folder for id {}",
            dispute_id
        );
        return false;
    };
    if let Err(e) = fs::create_dir_all(chat_dir) {
        log::warn!("Failed to create dispute chat folder {:?}: {}", chat_dir, e);
        return false;
    }
    let mut body = String::new();
    for message in messages {
        body.push_str(&format_dispute_transcript_block(
            ChatStorageKind::Disputes,
            message,
        ));
    }
    write_transcript_file(&file_path, &body, "dispute chat")
}

fn save_chat_message_by_kind(
    kind: ChatStorageKind,
    chat_id: &str,
    message: &DisputeChatMessage,
) -> bool {
    let file_path = match chat_file_path(kind, chat_id) {
        Some(path) => path,
        None => {
            log::warn!(
                "Invalid {} id format, skipping save: {}",
                kind.log_label(),
                chat_id
            );
            return false;
        }
    };
    let Some(chat_dir) = file_path.parent() else {
        log::warn!(
            "Failed to resolve {} folder for id {}",
            kind.log_label(),
            chat_id
        );
        return false;
    };
    if let Err(e) = fs::create_dir_all(chat_dir) {
        log::warn!(
            "Failed to create {} folder {:?}: {}",
            kind.log_label(),
            chat_dir,
            e
        );
        return false;
    }

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
                return true;
            }
        }
    }

    let formatted_message = format_dispute_transcript_block(kind, message);
    append_transcript_block(&file_path, &formatted_message, kind.log_label())
}

fn format_dispute_transcript_block(kind: ChatStorageKind, message: &DisputeChatMessage) -> String {
    let content_block = transcript_body_for_dispute_message(message);
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
        ChatStorageKind::Orders | ChatStorageKind::UserDisputes => match message.sender {
            ChatSender::Admin => "You",
            ChatSender::Buyer | ChatSender::Seller => "Peer",
        },
    };
    format!(
        "{} - {} - {}\n{}\n\n",
        sender_label, date_str, time_str, content_block
    )
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

fn inner_ids_file_path(
    kind: ChatStorageKind,
    chat_id: &str,
    party_suffix: Option<&str>,
) -> Option<PathBuf> {
    if uuid::Uuid::parse_str(chat_id).is_err() {
        return None;
    }
    let home_dir = dirs::home_dir()?;
    let name = match party_suffix {
        Some(sfx) => format!("{chat_id}.{sfx}.inner_ids"),
        None => format!("{chat_id}.inner_ids"),
    };
    Some(
        home_dir
            .join(".mostrix")
            .join(kind.folder_name())
            .join(name),
    )
}

fn load_inner_ids_from_path(path: &Path) -> HashSet<EventId> {
    let Ok(content) = fs::read_to_string(path) else {
        return HashSet::new();
    };
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            EventId::from_str(line).ok()
        })
        .collect()
}

fn inner_id_cache() -> &'static Mutex<HashMap<PathBuf, HashSet<EventId>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, HashSet<EventId>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_inner_id_set<R>(path: &Path, f: impl FnOnce(&mut HashSet<EventId>) -> R) -> R {
    let mut guard = match inner_id_cache().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let set = guard
        .entry(path.to_path_buf())
        .or_insert_with(|| load_inner_ids_from_path(path));
    f(set)
}

fn inner_id_known_at_path(path: &Path, id: &EventId) -> bool {
    with_inner_id_set(path, |set| set.contains(id))
}

/// Append `id` to the durable set. Returns `false` if already known.
///
/// Uses a process-wide cache so each path is fully parsed at most once; only
/// newly accepted ids are appended. On append failure the cache insert is rolled
/// back so a later retry can succeed.
fn remember_inner_id_at_path(path: &Path, id: &EventId) -> bool {
    let newly_inserted = with_inner_id_set(path, |set| set.insert(*id));
    if !newly_inserted {
        return false;
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            log::warn!("Failed to create chat inner-id dir {:?}: {e}", parent);
            with_inner_id_set(path, |set| {
                set.remove(id);
            });
            return false;
        }
    }
    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => {
            if let Err(e) = writeln!(file, "{}", id.to_hex()) {
                log::warn!("Failed to append chat inner id to {:?}: {e}", path);
                with_inner_id_set(path, |set| {
                    set.remove(id);
                });
                return false;
            }
        }
        Err(e) => {
            log::warn!("Failed to open chat inner-id file {:?}: {e}", path);
            with_inner_id_set(path, |set| {
                set.remove(id);
            });
            return false;
        }
    }
    true
}

/// Load durable inner event ids for an order chat (replay protection).
///
/// Reads through a process-wide per-path cache (first touch loads the `.inner_ids` file).
pub fn load_order_chat_inner_ids(order_id: &str) -> HashSet<EventId> {
    match inner_ids_file_path(ChatStorageKind::Orders, order_id, None) {
        Some(path) => with_inner_id_set(&path, |set| set.clone()),
        None => HashSet::new(),
    }
}

/// Returns `true` if this inner id was already accepted for the order chat.
pub fn order_chat_inner_id_known(order_id: &str, id: &EventId) -> bool {
    match inner_ids_file_path(ChatStorageKind::Orders, order_id, None) {
        Some(path) => inner_id_known_at_path(&path, id),
        None => false,
    }
}

/// Persist an accepted order-chat inner id.
///
/// Returns `false` if the id was already known, or if the durable append failed
/// (cache insert is rolled back so a later retry can succeed). Call only after
/// the matching transcript mutation has succeeded.
pub fn remember_order_chat_inner_id(order_id: &str, id: &EventId) -> bool {
    match inner_ids_file_path(ChatStorageKind::Orders, order_id, None) {
        Some(path) => remember_inner_id_at_path(&path, id),
        None => true,
    }
}

/// Load durable inner event ids for a user-to-solver dispute chat.
pub fn load_user_dispute_chat_inner_ids(order_id: &str) -> HashSet<EventId> {
    match inner_ids_file_path(ChatStorageKind::UserDisputes, order_id, None) {
        Some(path) => with_inner_id_set(&path, |set| set.clone()),
        None => HashSet::new(),
    }
}

/// Returns `true` if this inner id was already accepted for the solver chat.
pub fn user_dispute_chat_inner_id_known(order_id: &str, id: &EventId) -> bool {
    match inner_ids_file_path(ChatStorageKind::UserDisputes, order_id, None) {
        Some(path) => inner_id_known_at_path(&path, id),
        None => false,
    }
}

/// Persist an accepted solver-chat inner id after its transcript is durable.
pub fn remember_user_dispute_chat_inner_id(order_id: &str, id: &EventId) -> bool {
    match inner_ids_file_path(ChatStorageKind::UserDisputes, order_id, None) {
        Some(path) => remember_inner_id_at_path(&path, id),
        None => true,
    }
}

/// Load durable inner event ids for one admin↔party dispute channel.
///
/// Reads through a process-wide per-path cache (first touch loads the `.inner_ids` file).
pub fn load_dispute_chat_inner_ids(dispute_id: &str, party: ChatParty) -> HashSet<EventId> {
    let sfx = match party {
        ChatParty::Buyer => "buyer",
        ChatParty::Seller => "seller",
    };
    match inner_ids_file_path(ChatStorageKind::Disputes, dispute_id, Some(sfx)) {
        Some(path) => with_inner_id_set(&path, |set| set.clone()),
        None => HashSet::new(),
    }
}

/// Returns `true` if this inner id was already accepted for the dispute party chat.
pub fn dispute_chat_inner_id_known(dispute_id: &str, party: ChatParty, id: &EventId) -> bool {
    let sfx = match party {
        ChatParty::Buyer => "buyer",
        ChatParty::Seller => "seller",
    };
    match inner_ids_file_path(ChatStorageKind::Disputes, dispute_id, Some(sfx)) {
        Some(path) => inner_id_known_at_path(&path, id),
        None => false,
    }
}

/// Persist an accepted dispute-chat inner id.
///
/// Returns `false` if the id was already known, or if the durable append failed
/// (cache insert is rolled back so a later retry can succeed). Call only after
/// the matching transcript mutation has succeeded.
pub fn remember_dispute_chat_inner_id(dispute_id: &str, party: ChatParty, id: &EventId) -> bool {
    let sfx = match party {
        ChatParty::Buyer => "buyer",
        ChatParty::Seller => "seller",
    };
    match inner_ids_file_path(ChatStorageKind::Disputes, dispute_id, Some(sfx)) {
        Some(path) => remember_inner_id_at_path(&path, id),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        ChatAttachment, ChatAttachmentType, ChatSender, DisputeChatMessage, UserChatSender,
        UserOrderChatMessage,
    };
    use nostr_sdk::prelude::EventId;

    use super::super::attachments::serialize_attachment_for_transcript;

    #[test]
    fn inner_id_replay_rejected_at_path() {
        let dir = std::env::temp_dir().join(format!("mostrix-inner-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("x.inner_ids");
        let id = EventId::from_slice(&[9u8; 32]).expect("event id");
        assert!(remember_inner_id_at_path(&path, &id));
        assert!(!remember_inner_id_at_path(&path, &id));
        let loaded = load_inner_ids_from_path(&path);
        assert!(loaded.contains(&id));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn transcript_body_roundtrip_via_message_fields() {
        let att = ChatAttachment {
            blossom_url: "blossom://host/hash".to_string(),
            filename: "doc.pdf".to_string(),
            mime_type: None,
            file_type: ChatAttachmentType::File,
            decryption_key: None,
            decryption_key_fallbacks: Vec::new(),
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
