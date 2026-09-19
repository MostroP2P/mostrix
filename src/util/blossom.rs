//! Blossom URL resolution, blob download/upload, and ChaCha20-Poly1305 encrypt/decrypt.
//! Matches Mostro Mobile encrypted file messaging: blob layout [nonce:12][ciphertext][tag:16].
//! Shared key for decryption: ECDH(admin_sk, sender_pubkey), same as mostro-cli with roles swapped.
//! Uploads try each configured server in order; one failed host does not abort the send.

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chacha20poly1305::aead::{Aead, Generate, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use mostro_core::prelude::SharedKey;
use nostr_sdk::prelude::{EventBuilder, FinalizeEvent, Keys, Kind, PublicKey, Tag, Timestamp};
use reqwest::{header::CONTENT_LENGTH, Client};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

use crate::ui::{ChatAttachment, OperationResult};

/// NIP-24242 Blossom upload authorization event kind.
const BLOSSOM_AUTH_KIND: Kind = Kind::Custom(24242);

/// Default Blossom servers (Mostro Mobile `BlossomConfig.defaultServers`).
///
/// Chat attachments are encrypted before upload (`application/octet-stream`).
/// Only include servers that accept opaque blobs and retain them; media-only
/// hosts sniff content and reject every attachment.
pub const DEFAULT_BLOSSOM_SERVERS: &[&str] = &[
    "https://cdn.hzrd149.com",
    "https://nostr.download",
    "https://blossom-01.uid.ovh",
    "https://files.sovbit.host",
    "https://blssm.us",
];

/// Upload timeout (seconds) for the PUT body (large files).
const BLOSSOM_UPLOAD_TIMEOUT_SECS: u64 = 300;

/// TCP/TLS connect timeout so a hung or blackholed host fails over quickly.
const BLOSSOM_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Built-in default Blossom bases: owned copy of [`DEFAULT_BLOSSOM_SERVERS`]
/// (Mostro Mobile `BlossomConfig.defaultServers`).
pub fn default_blossom_servers() -> Vec<String> {
    DEFAULT_BLOSSOM_SERVERS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// HTTP client for Blossom upload (short connect timeout, long transfer timeout).
pub fn blossom_http_client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(BLOSSOM_CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(BLOSSOM_UPLOAD_TIMEOUT_SECS))
        .build()
        .map_err(|e| anyhow!("blossom HTTP client: {e}"))
}

/// Derives the 32-byte shared decryption key from our (admin) private key and the sender's public key.
/// Mirror of mostro-cli's derive_shared_key: they use (trade_sk, admin_pubkey); we use (admin_sk, sender_pubkey).
pub fn derive_shared_key(admin_keys: &Keys, sender_pubkey: &PublicKey) -> Result<[u8; 32]> {
    let shared = SharedKey::derive(admin_keys.secret_key(), sender_pubkey)
        .map_err(|e| anyhow!("shared key derivation failed: {e}"))?;
    let mut key = [0u8; 32];
    key.copy_from_slice(shared.secret_key().as_secret_bytes());
    Ok(key)
}

/// Max size for a single Blossom blob download (25 MB, same as Mostro Mobile).
pub const BLOSSOM_MAX_BLOB_SIZE: usize = 25 * 1024 * 1024;

/// Timeout for Blossom HTTP GET (seconds).
const BLOSSOM_FETCH_TIMEOUT_SECS: u64 = 30;

/// Converts `blossom://host/path` to `https://host/path`. Other schemes (e.g. `https://`) are returned as-is.
pub fn blossom_url_to_https(url: &str) -> Result<String> {
    let url = url.trim();
    if let Some(stripped) = url.strip_prefix("blossom://") {
        return Ok(format!("https://{}", stripped));
    }
    if url.starts_with("https://") {
        return Ok(url.to_string());
    }
    Err(anyhow!(
        "Blossom URL must start with blossom:// or https://, got: {}",
        url
    ))
}

/// Downloads a blob from the given URL (HTTPS). Enforces timeout and max size.
/// If `timeout_secs` is 0, uses `BLOSSOM_FETCH_TIMEOUT_SECS`.
pub async fn fetch_blob(
    client: &Client,
    url: &str,
    timeout_secs: u64,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let timeout = if timeout_secs == 0 {
        BLOSSOM_FETCH_TIMEOUT_SECS
    } else {
        timeout_secs
    };
    let res = client
        .get(url)
        .timeout(std::time::Duration::from_secs(timeout))
        .send()
        .await
        .map_err(|e| anyhow!("Blossom fetch failed: {}", e))?;
    if !res.status().is_success() {
        return Err(anyhow!("Blossom fetch returned status: {}", res.status()));
    }
    if let Some(len_header) = res.headers().get(CONTENT_LENGTH) {
        if let Ok(len_str) = len_header.to_str() {
            if let Ok(len) = len_str.parse::<usize>() {
                if len > max_bytes {
                    return Err(anyhow!(
                        "Blossom blob too large: {} bytes (max {})",
                        len,
                        max_bytes
                    ));
                }
            }
        }
    }

    let mut body = Vec::new();
    let mut downloaded: usize = 0;
    let mut res = res;
    while let Some(chunk) = res
        .chunk()
        .await
        .map_err(|e| anyhow!("Blossom read body failed: {}", e))?
    {
        downloaded += chunk.len();
        if downloaded > max_bytes {
            return Err(anyhow!(
                "Blossom blob too large while streaming: {} bytes (max {})",
                downloaded,
                max_bytes
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Decrypts a blob with ChaCha20-Poly1305. Blob layout: [nonce:12][ciphertext][auth_tag:16].
/// `key` must be 32 bytes; `nonce` must be 12 bytes (can be the first 12 bytes of the blob or provided separately).
pub fn decrypt_blob(key: &[u8], blob: &[u8]) -> Result<Vec<u8>> {
    if key.len() != 32 {
        return Err(anyhow!("decrypt key must be 32 bytes, got {}", key.len()));
    }
    if blob.len() < 12 + 16 {
        return Err(anyhow!(
            "blob too short for nonce+tag (need at least 28 bytes, got {})",
            blob.len()
        ));
    }
    let (nonce_slice, ciphertext_and_tag) = blob.split_at(12);
    let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|e| anyhow!("key init: {}", e))?;
    let nonce: &Nonce = nonce_slice
        .try_into()
        .map_err(|_| anyhow!("invalid nonce length"))?;
    let plaintext = cipher
        .decrypt(nonce, ciphertext_and_tag)
        .map_err(|e| anyhow!("decrypt failed: {}", e))?;
    Ok(plaintext)
}

/// Encrypts plaintext with ChaCha20-Poly1305. Returns `[nonce:12][ciphertext][tag:16]`.
pub fn encrypt_blob(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    if key.len() != 32 {
        return Err(anyhow!("encrypt key must be 32 bytes, got {}", key.len()));
    }
    let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|e| anyhow!("key init: {}", e))?;
    let nonce = Nonce::generate();
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|e| anyhow!("encrypt failed: {}", e))?;
    let mut blob = nonce.as_slice().to_vec();
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// SHA-256 hex digest of `data` (Blossom `x` tag).
pub fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hex::encode(hash)
}

fn normalize_blossom_server_base(server: &str) -> String {
    server.trim().trim_end_matches('/').to_string()
}

/// Builds a signed NIP-24242 authorization event for Blossom upload.
/// Must use the same identity that publishes the corresponding chat message (order trade key).
pub(crate) fn blossom_upload_auth_header(blob_hash_hex: &str, keys: &Keys) -> Result<String> {
    let now = Timestamp::now().as_secs();
    let expiration = (now + 3600).to_string();
    let tags = vec![
        Tag::parse(["t", "upload"]).map_err(|e| anyhow!("auth tag t: {}", e))?,
        Tag::parse(["x", blob_hash_hex]).map_err(|e| anyhow!("auth tag x: {}", e))?,
        Tag::parse(["expiration", expiration.as_str()])
            .map_err(|e| anyhow!("auth tag expiration: {}", e))?,
    ];
    let signed = FinalizeEvent::finalize(EventBuilder::new(BLOSSOM_AUTH_KIND, "").tags(tags), keys)
        .map_err(|e| anyhow!("sign Blossom auth event: {}", e))?;
    let json = signed.as_json();
    Ok(format!("Nostr {}", BASE64.encode(json.as_bytes())))
}

/// Uploads an encrypted blob to one Blossom server. Returns HTTPS URL `{server}/{hash}`.
pub async fn upload_blob(
    http: &Client,
    server_base: &str,
    blob: &[u8],
    auth_keys: &Keys,
) -> Result<String> {
    let base = normalize_blossom_server_base(server_base);
    if base.is_empty() {
        return Err(anyhow!("empty Blossom server URL"));
    }
    let hash_hex = sha256_hex(blob);
    let auth = blossom_upload_auth_header(&hash_hex, auth_keys)?;
    let url = format!("{base}/upload");
    let res = http
        .put(&url)
        .header("Authorization", auth)
        .header("Content-Type", "application/octet-stream")
        .header("User-Agent", "Mostrix/0.2")
        .timeout(Duration::from_secs(BLOSSOM_UPLOAD_TIMEOUT_SECS))
        .body(blob.to_vec())
        .send()
        .await
        .map_err(|e| anyhow!("Blossom upload failed: {}", e))?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("Blossom upload returned {status}: {body}"));
    }
    Ok(format!("{base}/{hash_hex}"))
}

/// Tries each server in order until one accepts the upload.
/// A single failed host does not abort the send; only exhausting the list fails.
///
/// # Errors
/// Returns `all N Blossom servers failed. Last error: …` when every host fails,
/// or `no Blossom servers configured` when `servers` is empty.
pub async fn upload_blob_with_retry(
    http: &Client,
    servers: &[String],
    blob: &[u8],
    auth_keys: &Keys,
) -> Result<String> {
    if servers.is_empty() {
        return Err(anyhow!("no Blossom servers configured"));
    }
    let mut last_err = anyhow!("no upload attempt");
    for server in servers {
        match upload_blob(http, server, blob, auth_keys).await {
            Ok(url) => return Ok(url),
            Err(e) => {
                log::warn!("Blossom upload failed for {}: {}", server, e);
                last_err = e;
            }
        }
    }
    Err(anyhow!(
        "all {} Blossom servers failed. Last error: {}",
        servers.len(),
        last_err
    ))
}

/// Sanitizes a filename to avoid path traversal: only [a-zA-Z0-9_.-] allowed.
fn sanitize_filename(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "attachment".to_string()
    } else {
        s
    }
}

/// Downloads an attachment from a Blossom URL, optionally decrypts it, and writes to
/// `~/.mostrix/downloads/<dispute_id>_<sanitized_filename>` (or with `.enc` suffix if no key).
pub async fn save_attachment_to_disk(
    dispute_id: String,
    blossom_url: String,
    filename: String,
    decryption_key: Option<Vec<u8>>,
) -> Result<PathBuf> {
    let url = blossom_url_to_https(blossom_url.trim())?;
    let client = Client::new();
    let blob = fetch_blob(&client, &url, 0, BLOSSOM_MAX_BLOB_SIZE).await?;
    let bytes = match &decryption_key {
        Some(key) => decrypt_blob(key, &blob)?,
        None => blob,
    };
    let sanitized = sanitize_filename(&filename);
    let final_name = if decryption_key.is_some() {
        sanitized
    } else {
        format!("{}.enc", sanitized)
    };
    let home = dirs::home_dir().ok_or_else(|| anyhow!("No home directory"))?;
    let dir = home.join(".mostrix").join("downloads");
    std::fs::create_dir_all(&dir).map_err(|e| anyhow!("Create downloads dir: {}", e))?;
    let path = dir.join(format!("{}_{}", dispute_id, final_name));
    std::fs::write(&path, &bytes).map_err(|e| anyhow!("Write file: {}", e))?;
    Ok(path)
}

/// Spawns a task to download the attachment, optionally decrypt it, and write to
/// `~/.mostrix/downloads/`. Sends `OperationResult::Info(path)` or `OperationResult::Error` on completion.
pub fn spawn_save_attachment(
    dispute_id: String,
    attachment: ChatAttachment,
    order_result_tx: UnboundedSender<OperationResult>,
) {
    let blossom_url = attachment.blossom_url;
    let filename = attachment.filename;
    let decryption_key = attachment.decryption_key;
    tokio::spawn(async move {
        match save_attachment_to_disk(dispute_id, blossom_url, filename, decryption_key).await {
            Ok(path) => {
                let _ = order_result_tx.send(OperationResult::Info(format!(
                    "Saved to {}",
                    path.display()
                )));
            }
            Err(e) => {
                let _ = order_result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blossom_url_to_https_ok() {
        assert_eq!(
            blossom_url_to_https("blossom://blossom.primal.net/abc").unwrap(),
            "https://blossom.primal.net/abc"
        );
        assert_eq!(
            blossom_url_to_https("https://blossom.primal.net/abc").unwrap(),
            "https://blossom.primal.net/abc"
        );
    }

    #[test]
    fn blossom_url_to_https_rejects_other() {
        assert!(blossom_url_to_https("http://evil.com/x").is_err());
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = [7u8; 32];
        let plain = b"hello encrypted attachment";
        let blob = encrypt_blob(&key, plain).unwrap();
        let out = decrypt_blob(&key, &blob).unwrap();
        assert_eq!(out, plain);
    }

    #[test]
    fn sha256_hex_known_empty() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn upload_auth_event_uses_signer_pubkey() {
        use nostr_sdk::prelude::Event;

        let keys = Keys::generate();
        let header = blossom_upload_auth_header("deadbeef", &keys).expect("auth header");
        let b64 = header.strip_prefix("Nostr ").expect("Nostr prefix");
        let json = BASE64.decode(b64).expect("base64");
        let json_str = std::str::from_utf8(&json).expect("utf8");
        let event = Event::from_json(json_str).expect("event json");
        assert_eq!(event.pubkey, keys.public_key());
    }

    #[test]
    fn default_blossom_servers_are_https_bases() {
        assert!(!DEFAULT_BLOSSOM_SERVERS.is_empty());
        for server in DEFAULT_BLOSSOM_SERVERS {
            assert!(
                server.starts_with("https://"),
                "expected https base, got {server}"
            );
            assert!(
                !server.ends_with('/'),
                "base must not have trailing slash: {server}"
            );
        }
        assert!(
            !DEFAULT_BLOSSOM_SERVERS
                .iter()
                .any(|s| s.contains("poster.place")),
            "dead host blossom.poster.place must not be in the default list"
        );
    }

    fn content_length_from_headers(headers: &[u8]) -> Option<usize> {
        let text = std::str::from_utf8(headers).ok()?;
        for line in text.split("\r\n") {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                return value.trim().parse().ok();
            }
        }
        None
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> Vec<u8> {
        use std::io::Read;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let mut data = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => data.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
            if let Some(idx) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let header_end = idx + 4;
                if let Some(len) = content_length_from_headers(&data[..header_end]) {
                    if data.len() >= header_end + len {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        data
    }

    fn install_rustls_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn spawn_put_server(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock blossom");
        let addr = listener.local_addr().expect("local addr");
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let req = read_http_request(&mut stream);
            let header_text = String::from_utf8_lossy(&req).to_ascii_lowercase();
            if header_text.contains("expect: 100-continue") {
                let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
                let _ = read_http_request(&mut stream);
            }
            let reason = if (200..300).contains(&status) {
                "OK"
            } else {
                "ERR"
            };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn upload_retries_next_server_after_unreachable_host() {
        install_rustls_provider();
        let (ok_base, handle) = spawn_put_server(200, "ok");
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let keys = Keys::generate();
        let servers = vec!["http://127.0.0.1:1".to_string(), ok_base.clone()];
        let url = upload_blob_with_retry(&http, &servers, b"blob", &keys)
            .await
            .expect("second server must succeed after the first is down");
        assert!(
            url.starts_with(&ok_base),
            "expected upload URL on the healthy server, got {url}"
        );
        handle.join().ok();
    }

    #[tokio::test]
    async fn upload_retries_next_server_after_http_error() {
        install_rustls_provider();
        let (bad_base, bad_handle) = spawn_put_server(500, "nope");
        let (ok_base, ok_handle) = spawn_put_server(200, "ok");
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let keys = Keys::generate();
        let servers = vec![bad_base, ok_base.clone()];
        let url = upload_blob_with_retry(&http, &servers, b"blob", &keys)
            .await
            .expect("HTTP 500 on the first host must not block failover");
        assert!(
            url.starts_with(&ok_base),
            "expected upload URL on the healthy server, got {url}"
        );
        bad_handle.join().ok();
        ok_handle.join().ok();
    }

    #[tokio::test]
    async fn upload_all_servers_fail_mentions_count_and_last_error() {
        install_rustls_provider();
        let (bad_a, handle_a) = spawn_put_server(401, "auth");
        let (bad_b, handle_b) = spawn_put_server(502, "gateway");
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let keys = Keys::generate();
        let servers = vec![bad_a, bad_b.clone()];
        let err = upload_blob_with_retry(&http, &servers, b"blob", &keys)
            .await
            .expect_err("every server failed");
        let msg = err.to_string();
        assert!(
            msg.contains("all 2 Blossom servers failed"),
            "expected aggregate failure, got {msg}"
        );
        assert!(
            msg.contains("502") || msg.contains(&bad_b),
            "expected last-server detail in {msg}"
        );
        handle_a.join().ok();
        handle_b.join().ok();
    }
}
