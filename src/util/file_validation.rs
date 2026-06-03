//! Validation for user order chat file attachments (Mostro Mobile parity).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::util::blossom::BLOSSOM_MAX_BLOB_SIZE;

/// Classifies an attachment for `file_encrypted` JSON `file_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentFileClass {
    Image,
    Video,
    Document,
}

/// Validated file ready for encrypt + Blossom upload.
#[derive(Clone, Debug)]
pub struct ValidatedAttachment {
    pub data: Vec<u8>,
    pub filename: String,
    pub mime_type: String,
    pub file_class: AttachmentFileClass,
}

const ALLOWED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "pdf", "mp4", "mov", "avi", "doc", "docx",
];

/// Reads and validates a local file for order-chat attachment upload.
pub fn validate_attachment_file(path: &Path) -> Result<ValidatedAttachment> {
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Invalid file path"))?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .ok_or_else(|| anyhow!("File has no extension"))?;

    if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
        return Err(anyhow!(
            "Unsupported file type (.{ext}). Allowed: {}",
            ALLOWED_EXTENSIONS.join(", ")
        ));
    }

    let data =
        std::fs::read(path).map_err(|e| anyhow!("Failed to read {}: {}", path.display(), e))?;
    if data.is_empty() {
        return Err(anyhow!("File is empty"));
    }
    if data.len() > BLOSSOM_MAX_BLOB_SIZE {
        return Err(anyhow!(
            "File too large: {} bytes (max {} MB)",
            data.len(),
            BLOSSOM_MAX_BLOB_SIZE / (1024 * 1024)
        ));
    }

    let file_class = classify_extension(&ext);
    let mime_type = mime_type_for_extension(&ext).to_string();

    if file_class == AttachmentFileClass::Document && ext == "pdf" {
        validate_pdf_header(&data)?;
    }

    Ok(ValidatedAttachment {
        data,
        filename,
        mime_type,
        file_class,
    })
}

fn classify_extension(ext: &str) -> AttachmentFileClass {
    match ext {
        "jpg" | "jpeg" | "png" => AttachmentFileClass::Image,
        "mp4" | "mov" | "avi" => AttachmentFileClass::Video,
        _ => AttachmentFileClass::Document,
    }
}

fn mime_type_for_extension(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    }
}

fn validate_pdf_header(data: &[u8]) -> Result<()> {
    if data.starts_with(b"%PDF-") {
        return Ok(());
    }
    Err(anyhow!("File does not look like a valid PDF"))
}

/// Convenience for callers that already have a [`PathBuf`].
pub fn validate_attachment_path(path: PathBuf) -> Result<ValidatedAttachment> {
    validate_attachment_file(path.as_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_extension() {
        let dir = std::env::temp_dir().join(format!("mostrix_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("evil.exe");
        std::fs::write(&path, b"x").unwrap();
        assert!(validate_attachment_file(&path).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn accepts_small_png() {
        let dir = std::env::temp_dir().join(format!("mostrix_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.png");
        std::fs::write(&path, [0x89, 0x50, 0x4E, 0x47, 0, 0, 0, 1]).unwrap();
        let v = validate_attachment_file(&path).unwrap();
        assert_eq!(v.file_class, AttachmentFileClass::Image);
        assert_eq!(v.mime_type, "image/png");
        let _ = std::fs::remove_dir_all(dir);
    }
}
