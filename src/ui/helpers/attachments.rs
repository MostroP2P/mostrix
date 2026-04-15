use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use nostr_sdk::serde_json::{from_str as json_from_str, Value};

use crate::ui::{AppState, ChatAttachment, ChatAttachmentType};

/// Toast expiry duration for attachment notification.
const ATTACHMENT_TOAST_DURATION: Duration = Duration::from_secs(8);

/// Clears the transient attachment toast when it has expired.
/// Intended to be called from the main update/tick path before rendering.
pub fn expire_attachment_toast(app: &mut AppState) {
    if app
        .attachment_toast
        .as_ref()
        .is_some_and(|(_, t)| t.elapsed() > ATTACHMENT_TOAST_DURATION)
    {
        app.attachment_toast = None;
    }
}

/// Placeholder text written to transcript file for attachment messages (no blob persisted).
pub(crate) fn attachment_placeholder(att: &ChatAttachment) -> String {
    let kind = match att.file_type {
        ChatAttachmentType::Image => "Image",
        ChatAttachmentType::File => "File",
    };
    format!("[{}: {} - Ctrl+S to save]", kind, att.filename)
}

/// Parses Mostro Mobile image_encrypted / file_encrypted JSON.
/// Returns (ChatAttachment, display_content) or None.
pub(crate) fn try_parse_attachment_message(content: &str) -> Option<(ChatAttachment, String)> {
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

pub(crate) fn build_attachment_toast(filename: &str) -> (String, Instant) {
    (
        format!("📎 File received: {} — Ctrl+S to save", filename),
        Instant::now(),
    )
}
