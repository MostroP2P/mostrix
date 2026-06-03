use std::time::{Duration, Instant};

use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use nostr_sdk::serde_json::{from_str as json_from_str, to_string as json_to_string, Value};

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

/// Placeholder text for in-memory display when attachment metadata exists without JSON body.
pub(crate) fn attachment_placeholder(att: &ChatAttachment) -> String {
    let kind = match att.file_type {
        ChatAttachmentType::Image => "Image",
        ChatAttachmentType::File => "File",
    };
    format!("[{}: {} - Ctrl+S to save]", kind, att.filename)
}

/// True when the attachment has a Blossom URL and can be downloaded with Ctrl+S.
pub(crate) fn attachment_is_saveable(att: &ChatAttachment) -> bool {
    !att.blossom_url.trim().is_empty()
}

/// Serializes attachment metadata for transcript persistence (round-trips via `try_parse_attachment_message`).
pub(crate) fn serialize_attachment_for_transcript(att: &ChatAttachment) -> String {
    if !attachment_is_saveable(att) {
        return attachment_placeholder(att);
    }
    let msg_type = match att.file_type {
        ChatAttachmentType::Image => "image_encrypted",
        ChatAttachmentType::File => "file_encrypted",
    };
    let mut obj = serde_json::Map::new();
    obj.insert("type".into(), Value::String(msg_type.into()));
    obj.insert("blossom_url".into(), Value::String(att.blossom_url.clone()));
    obj.insert("filename".into(), Value::String(att.filename.clone()));
    if let Some(mime) = &att.mime_type {
        obj.insert("mime_type".into(), Value::String(mime.clone()));
    }
    if let Some(key) = &att.decryption_key {
        if key.len() == 32 {
            obj.insert("key".into(), Value::String(BASE64.encode(key)));
        }
    }
    json_to_string(&Value::Object(obj)).unwrap_or_else(|_| attachment_placeholder(att))
}

/// Parses legacy transcript lines written before JSON persistence.
pub(crate) fn try_parse_attachment_placeholder(
    content: &str,
) -> Option<(ChatAttachmentType, String)> {
    let content = content.trim();
    let inner = content.strip_prefix('[')?.strip_suffix(']')?;
    let (kind, filename) = inner.split_once(": ")?;
    let filename = filename.strip_suffix(" - Ctrl+S to save")?.to_string();
    let file_type = match kind {
        "Image" => ChatAttachmentType::Image,
        "File" => ChatAttachmentType::File,
        _ => return None,
    };
    Some((file_type, filename))
}

/// True when `content` is a legacy placeholder for the given attachment filename.
pub(crate) fn legacy_placeholder_matches_filename(content: &str, filename: &str) -> bool {
    try_parse_attachment_placeholder(content).is_some_and(|(_, f)| f == filename)
}

/// Restores display text and optional attachment from a transcript message body.
pub(crate) fn message_fields_from_transcript_content(
    content_block: &str,
) -> (String, Option<ChatAttachment>) {
    if let Some((attachment, display)) = try_parse_attachment_message(content_block) {
        if attachment_is_saveable(&attachment) {
            return (display, Some(attachment));
        }
        return (display, None);
    }
    (content_block.to_string(), None)
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
    let blossom_url = obj.get("blossom_url")?.as_str()?.trim().to_string();
    if blossom_url.is_empty() {
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

/// Outbound attachment wire JSON + in-memory metadata after a successful send.
#[derive(Clone, Debug)]
pub struct OutboundAttachmentPayload {
    pub json_body: String,
    pub attachment: ChatAttachment,
    pub display_content: String,
}

/// Blossom upload done; order-chat DM still pending or failed (retry without re-upload).
#[derive(Clone, Debug)]
pub struct PreparedOrderChatAttachment {
    pub order_id: String,
    pub blossom_url: String,
    pub filename: String,
    pub outbound: OutboundAttachmentPayload,
}

/// Builds `image_encrypted` JSON for order chat (shared-key decrypt; no embedded `key`).
pub fn build_image_encrypted_json(
    blossom_url: &str,
    filename: &str,
    mime_type: &str,
    nonce: &[u8],
    original_size: usize,
    encrypted_size: usize,
) -> Result<OutboundAttachmentPayload> {
    if nonce.len() != 12 {
        return Err(anyhow::anyhow!(
            "attachment nonce must be 12 bytes, got {}",
            nonce.len()
        ));
    }
    let mut obj = serde_json::Map::new();
    obj.insert("type".into(), Value::String("image_encrypted".into()));
    obj.insert("blossom_url".into(), Value::String(blossom_url.to_string()));
    obj.insert("nonce".into(), Value::String(BASE64.encode(nonce)));
    obj.insert("filename".into(), Value::String(filename.to_string()));
    obj.insert("mime_type".into(), Value::String(mime_type.to_string()));
    obj.insert("original_size".into(), Value::Number(original_size.into()));
    obj.insert(
        "encrypted_size".into(),
        Value::Number(encrypted_size.into()),
    );
    let json_body = json_to_string(&Value::Object(obj))?;
    let (attachment, display_content) = try_parse_attachment_message(&json_body)
        .ok_or_else(|| anyhow::anyhow!("failed to build attachment display from JSON"))?;
    Ok(OutboundAttachmentPayload {
        json_body,
        attachment,
        display_content,
    })
}

/// Builds `file_encrypted` JSON for order chat.
pub fn build_file_encrypted_json(
    blossom_url: &str,
    filename: &str,
    mime_type: &str,
    file_type: &str,
    nonce: &[u8],
    original_size: usize,
    encrypted_size: usize,
) -> Result<OutboundAttachmentPayload> {
    if nonce.len() != 12 {
        return Err(anyhow::anyhow!(
            "attachment nonce must be 12 bytes, got {}",
            nonce.len()
        ));
    }
    let mut obj = serde_json::Map::new();
    obj.insert("type".into(), Value::String("file_encrypted".into()));
    obj.insert("blossom_url".into(), Value::String(blossom_url.to_string()));
    obj.insert("nonce".into(), Value::String(BASE64.encode(nonce)));
    obj.insert("filename".into(), Value::String(filename.to_string()));
    obj.insert("mime_type".into(), Value::String(mime_type.to_string()));
    obj.insert("file_type".into(), Value::String(file_type.to_string()));
    obj.insert("original_size".into(), Value::Number(original_size.into()));
    obj.insert(
        "encrypted_size".into(),
        Value::Number(encrypted_size.into()),
    );
    let json_body = json_to_string(&Value::Object(obj))?;
    let (attachment, display_content) = try_parse_attachment_message(&json_body)
        .ok_or_else(|| anyhow::anyhow!("failed to build attachment display from JSON"))?;
    Ok(OutboundAttachmentPayload {
        json_body,
        attachment,
        display_content,
    })
}

pub fn build_attachment_toast(filename: &str) -> (String, Instant) {
    (
        format!("📎 File received: {} — Ctrl+S to save", filename),
        Instant::now(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_attachment() -> ChatAttachment {
        ChatAttachment {
            blossom_url: "blossom://example.com/abc123".to_string(),
            filename: "photo.png".to_string(),
            mime_type: Some("image/png".to_string()),
            file_type: ChatAttachmentType::Image,
            decryption_key: None,
        }
    }

    #[test]
    fn serialize_roundtrip_restores_attachment() {
        let att = sample_attachment();
        let json = serialize_attachment_for_transcript(&att);
        assert!(json.starts_with('{'));
        let (parsed, display) = try_parse_attachment_message(&json).expect("parse");
        assert_eq!(parsed.blossom_url, att.blossom_url);
        assert_eq!(parsed.filename, att.filename);
        assert!(display.contains("photo.png"));
    }

    #[test]
    fn legacy_placeholder_parser_extracts_filename() {
        let (ft, name) =
            try_parse_attachment_placeholder("[File: report.pdf - Ctrl+S to save]").expect("parse");
        assert_eq!(ft, ChatAttachmentType::File);
        assert_eq!(name, "report.pdf");
    }

    #[test]
    fn legacy_placeholder_matches_filename_by_name() {
        assert!(legacy_placeholder_matches_filename(
            "[Image: photo.png - Ctrl+S to save]",
            "photo.png"
        ));
        assert!(!legacy_placeholder_matches_filename(
            "[Image: other.png - Ctrl+S to save]",
            "photo.png"
        ));
    }

    #[test]
    fn empty_blossom_url_is_not_saveable_or_parsed() {
        let json = r#"{"type":"file_encrypted","blossom_url":"","filename":"ciao.txt"}"#;
        assert!(try_parse_attachment_message(json).is_none());
        let (content, att) = message_fields_from_transcript_content(json);
        assert!(att.is_none());
        assert!(content.contains("ciao.txt"));
    }

    #[test]
    fn message_fields_from_json_transcript() {
        let att = sample_attachment();
        let json = serialize_attachment_for_transcript(&att);
        let (content, restored) = message_fields_from_transcript_content(&json);
        assert!(restored.is_some());
        assert_eq!(restored.unwrap().blossom_url, att.blossom_url);
        assert!(content.contains("photo.png"));
    }

    #[test]
    fn build_image_encrypted_json_roundtrip() {
        let nonce = [7u8; 12];
        let out = build_image_encrypted_json(
            "https://blossom.example/abc",
            "pic.png",
            "image/png",
            &nonce,
            100,
            120,
        )
        .expect("build");
        assert!(out.json_body.contains("image_encrypted"));
        assert!(!out.json_body.contains("\"key\""));
        let parsed = try_parse_attachment_message(&out.json_body).expect("parse");
        assert_eq!(parsed.0.filename, "pic.png");
    }

    #[test]
    fn build_file_encrypted_json_includes_file_type() {
        let nonce = [1u8; 12];
        let out = build_file_encrypted_json(
            "https://blossom.example/def",
            "doc.pdf",
            "application/pdf",
            "document",
            &nonce,
            50,
            70,
        )
        .expect("build");
        assert!(out.json_body.contains("file_encrypted"));
        assert!(out.json_body.contains("document"));
    }
}
