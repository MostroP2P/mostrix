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
    /// PNG/JPEG pixel width (Mostro Mobile `image_encrypted` JSON requires this).
    pub image_width: u32,
    /// PNG/JPEG pixel height (Mostro Mobile `image_encrypted` JSON requires this).
    pub image_height: u32,
}

/// Extensions allowed for My Trades attachment upload (Mostro Mobile parity).
pub const ATTACHMENT_ALLOWED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "pdf", "mp4", "mov", "avi", "doc", "docx",
];

/// True when `path` is a directory or has an allowed attachment extension.
pub fn attachment_extension_allowed(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let ext = e.to_ascii_lowercase();
            ATTACHMENT_ALLOWED_EXTENSIONS.contains(&ext.as_str())
        })
        .unwrap_or(false)
}

fn extension_allowed(ext: &str) -> bool {
    ATTACHMENT_ALLOWED_EXTENSIONS.contains(&ext)
}

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

    if !extension_allowed(&ext) {
        return Err(anyhow!(
            "Unsupported file type (.{ext}). Allowed: {}",
            ATTACHMENT_ALLOWED_EXTENSIONS.join(", ")
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

    let (image_width, image_height) = if file_class == AttachmentFileClass::Image {
        read_image_dimensions(&data)?
    } else {
        (0, 0)
    };

    Ok(ValidatedAttachment {
        data,
        filename,
        mime_type,
        file_class,
        image_width,
        image_height,
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

/// Reads PNG IHDR or JPEG SOF dimensions for Mostro Mobile wire JSON.
fn read_image_dimensions(data: &[u8]) -> Result<(u32, u32)> {
    if data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) && data.len() >= 24 {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        if w > 0 && h > 0 {
            return Ok((w, h));
        }
    }
    if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 {
        let mut i = 2usize;
        while i + 9 < data.len() {
            if data[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            if marker == 0xD9 {
                break;
            }
            if i + 3 >= data.len() {
                break;
            }
            let seg_len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            if seg_len < 2 || i + 2 + seg_len > data.len() {
                break;
            }
            if matches!(
                marker,
                0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB
            ) && seg_len >= 7
            {
                let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                if w > 0 && h > 0 {
                    return Ok((w, h));
                }
            }
            i += 2 + seg_len;
        }
    }
    Err(anyhow!(
        "Could not read image dimensions (PNG/JPEG required for mobile compatibility)"
    ))
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
    fn attachment_extension_allowed_accepts_png() {
        let dir = std::env::temp_dir().join(format!("mostrix_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(attachment_extension_allowed(&dir));
        let path = dir.join("photo.png");
        std::fs::write(&path, b"x").unwrap();
        assert!(attachment_extension_allowed(&path));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn attachment_extension_allowed_rejects_exe() {
        let dir = std::env::temp_dir().join(format!("mostrix_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("evil.exe");
        std::fs::write(&path, b"x").unwrap();
        assert!(!attachment_extension_allowed(&path));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn accepts_small_png() {
        let dir = std::env::temp_dir().join(format!("mostrix_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.png");
        std::fs::write(
            &path,
            [
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG sig
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
                0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
            ],
        )
        .unwrap();
        let v = validate_attachment_file(&path).unwrap();
        assert_eq!(v.file_class, AttachmentFileClass::Image);
        assert_eq!(v.mime_type, "image/png");
        assert!(v.image_width > 0);
        assert!(v.image_height > 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
