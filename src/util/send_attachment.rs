//! Send encrypted order-chat attachments (encrypt → Blossom → shared-key GiftWrap).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use nostr_sdk::prelude::PublicKey;
use nostr_sdk::prelude::{Client, Keys, SecretKey};
use sqlx::SqlitePool;
use std::str::FromStr;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{sleep, Duration};

use crate::models::Order;
use crate::settings::Settings;
use crate::ui::helpers::{
    build_file_encrypted_json, build_image_encrypted_json, OutboundAttachmentPayload,
    PreparedOrderChatAttachment,
};
use crate::ui::{OperationResult, UserChatSender, UserOrderChatMessage};
use crate::util::blossom::{
    encrypt_blob, upload_blob_with_retry, BLOSSOM_MAX_BLOB_SIZE, DEFAULT_BLOSSOM_SERVERS,
};
use crate::util::chat_utils::{
    keys_from_shared_hex, order_chat_decryption_key_bytes,
    send_user_order_chat_message_via_shared_key,
};
use crate::util::file_validation::validate_attachment_file;
use crate::util::file_validation::{AttachmentFileClass, ValidatedAttachment};
use crate::util::MostroInstanceInfo;

/// Retries chat DM send after a successful Blossom upload (no re-upload).
const CHAT_SEND_RETRY_ATTEMPTS: u32 = 3;
const CHAT_SEND_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Work queued on `send_order_attachment_tx`.
#[derive(Clone, Debug)]
pub enum SendOrderAttachmentJob {
    /// Read file, encrypt, upload, then send DM.
    FromPath { order_id: String, path: PathBuf },
    /// Re-send DM for a blob already uploaded (after upload-success / send-failure).
    RetryPrepared(PreparedOrderChatAttachment),
}

/// Result of a full upload+send attempt.
enum SendAttachmentAttempt {
    Sent(UserOrderChatMessage, String),
    UploadOkSendFailed(PreparedOrderChatAttachment, String),
}

/// Resolves Blossom server list from settings or defaults.
pub fn blossom_servers_from_settings(settings: &Settings) -> Vec<String> {
    if settings.blossom_servers.is_empty() {
        DEFAULT_BLOSSOM_SERVERS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        settings.blossom_servers.clone()
    }
}

fn file_type_label(class: AttachmentFileClass) -> &'static str {
    match class {
        AttachmentFileClass::Image => "image",
        AttachmentFileClass::Video => "video",
        AttachmentFileClass::Document => "document",
    }
}

fn build_outbound_payload(
    validated: &ValidatedAttachment,
    blossom_url: String,
    encrypted_blob: &[u8],
) -> Result<OutboundAttachmentPayload> {
    let nonce = encrypted_blob
        .get(..12)
        .ok_or_else(|| anyhow!("encrypted blob missing nonce"))?;
    let original_size = validated.data.len();
    let encrypted_size = encrypted_blob.len();
    match validated.file_class {
        AttachmentFileClass::Image => build_image_encrypted_json(
            &blossom_url,
            &validated.filename,
            &validated.mime_type,
            nonce,
            (validated.image_width, validated.image_height),
            original_size,
            encrypted_size,
        ),
        AttachmentFileClass::Video | AttachmentFileClass::Document => build_file_encrypted_json(
            &blossom_url,
            &validated.filename,
            &validated.mime_type,
            file_type_label(validated.file_class),
            nonce,
            original_size,
            encrypted_size,
        ),
    }
}

struct OrderChatKeys {
    trade_keys: Keys,
    shared_keys: Keys,
}

async fn load_order_chat_keys(pool: &SqlitePool, order_id: &str) -> Result<(Order, OrderChatKeys)> {
    let order = Order::get_by_id(pool, order_id)
        .await
        .map_err(|e| anyhow!("order not found: {}", e))?;
    let trade_sk = order
        .trade_keys
        .as_deref()
        .and_then(|h| SecretKey::from_str(h).ok())
        .ok_or_else(|| anyhow!("missing trade keys for order chat send"))?;
    let trade_keys = Keys::new(trade_sk);
    let shared_keys = order
        .order_chat_shared_key_hex
        .as_deref()
        .and_then(keys_from_shared_hex)
        .or_else(|| {
            let cp = order.counterparty_pubkey.as_deref()?;
            let pk = PublicKey::parse(cp).ok()?;
            crate::util::chat_utils::derive_shared_keys(Some(&trade_keys), Some(&pk))
        })
        .ok_or_else(|| anyhow!("could not derive shared keys for order chat"))?;
    Ok((
        order,
        OrderChatKeys {
            trade_keys,
            shared_keys,
        },
    ))
}

async fn send_prepared_with_retries(
    client: &Client,
    keys: &OrderChatKeys,
    json_body: &str,
    mostro_info: Option<&MostroInstanceInfo>,
) -> Result<()> {
    let mut last_err = anyhow!("chat send not attempted");
    for attempt in 0..CHAT_SEND_RETRY_ATTEMPTS {
        match send_user_order_chat_message_via_shared_key(
            client,
            &keys.trade_keys,
            &keys.shared_keys,
            json_body,
            mostro_info,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = e;
                if attempt + 1 < CHAT_SEND_RETRY_ATTEMPTS {
                    log::warn!(
                        "Order chat attachment send attempt {} failed: {}; retrying",
                        attempt + 1,
                        last_err
                    );
                    sleep(CHAT_SEND_RETRY_DELAY).await;
                }
            }
        }
    }
    Err(last_err)
}

fn local_message_from_prepared(prepared: &PreparedOrderChatAttachment) -> UserOrderChatMessage {
    UserOrderChatMessage {
        sender: UserChatSender::You,
        content: prepared.outbound.display_content.clone(),
        timestamp: chrono::Utc::now().timestamp(),
        attachment: Some(prepared.outbound.attachment.clone()),
    }
}

/// Sends DM only for an attachment already uploaded to Blossom.
pub async fn send_prepared_order_chat_attachment(
    client: &Client,
    pool: &SqlitePool,
    prepared: &PreparedOrderChatAttachment,
    mostro_info: Option<&MostroInstanceInfo>,
) -> Result<(UserOrderChatMessage, String)> {
    let (_, keys) = load_order_chat_keys(pool, &prepared.order_id).await?;
    send_prepared_with_retries(client, &keys, &prepared.outbound.json_body, mostro_info).await?;
    let info = format!("Attachment sent: {}", prepared.filename);
    Ok((local_message_from_prepared(prepared), info))
}

/// Encrypts, uploads, sends attachment JSON over order chat.
async fn send_order_chat_attachment_from_path(
    client: &Client,
    pool: &SqlitePool,
    order_id: &str,
    path: &Path,
    blossom_servers: &[String],
    mostro_info: Option<&MostroInstanceInfo>,
) -> Result<SendAttachmentAttempt> {
    let validated = validate_attachment_file(path)?;
    let (order, keys) = load_order_chat_keys(pool, order_id).await?;

    let key_vec = order_chat_decryption_key_bytes(&order)
        .ok_or_else(|| anyhow!("missing order chat shared key for attachment encrypt"))?;
    let key: [u8; 32] = key_vec
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("shared key must be 32 bytes"))?;

    let encrypted_blob = encrypt_blob(&key, &validated.data)?;
    if encrypted_blob.len() > BLOSSOM_MAX_BLOB_SIZE {
        return Err(anyhow!(
            "encrypted blob too large ({} bytes)",
            encrypted_blob.len()
        ));
    }

    let http = reqwest::Client::new();
    let blossom_url =
        upload_blob_with_retry(&http, blossom_servers, &encrypted_blob, &keys.trade_keys).await?;
    let outbound = build_outbound_payload(&validated, blossom_url.clone(), &encrypted_blob)?;

    let prepared = PreparedOrderChatAttachment {
        order_id: order_id.to_string(),
        blossom_url,
        filename: validated.filename.clone(),
        outbound,
    };

    match send_prepared_with_retries(client, &keys, &prepared.outbound.json_body, mostro_info).await
    {
        Ok(()) => {
            let info = format!("Attachment sent: {}", validated.filename);
            Ok(SendAttachmentAttempt::Sent(
                local_message_from_prepared(&prepared),
                info,
            ))
        }
        Err(e) => Ok(SendAttachmentAttempt::UploadOkSendFailed(
            prepared,
            format!(
                "chat send failed after {} attempts: {}",
                CHAT_SEND_RETRY_ATTEMPTS, e
            ),
        )),
    }
}

/// Background task: send attachment and notify the UI via `order_result_tx`.
pub fn spawn_send_order_chat_attachment(
    job: SendOrderAttachmentJob,
    client: Client,
    pool: SqlitePool,
    blossom_servers: Vec<String>,
    mostro_info: Option<MostroInstanceInfo>,
    order_result_tx: UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        match job {
            SendOrderAttachmentJob::FromPath { order_id, path } => {
                match send_order_chat_attachment_from_path(
                    &client,
                    &pool,
                    &order_id,
                    &path,
                    &blossom_servers,
                    mostro_info.as_ref(),
                )
                .await
                {
                    Ok(SendAttachmentAttempt::Sent(chat_message, info)) => {
                        let _ = order_result_tx.send(OperationResult::OrderChatAttachmentSent {
                            order_id,
                            chat_message,
                            info_message: info,
                        });
                    }
                    Ok(SendAttachmentAttempt::UploadOkSendFailed(prepared, error)) => {
                        let _ =
                            order_result_tx.send(OperationResult::OrderChatAttachmentSendFailed {
                                prepared,
                                error,
                            });
                    }

                    Err(e) => {
                        let _ = order_result_tx.send(OperationResult::OrderChatAttachmentError {
                            order_id,
                            error: e.to_string(),
                        });
                    }
                }
            }
            SendOrderAttachmentJob::RetryPrepared(prepared) => {
                let order_id = prepared.order_id.clone();
                match send_prepared_order_chat_attachment(
                    &client,
                    &pool,
                    &prepared,
                    mostro_info.as_ref(),
                )
                .await
                {
                    Ok((chat_message, info)) => {
                        let _ = order_result_tx.send(OperationResult::OrderChatAttachmentSent {
                            order_id,
                            chat_message,
                            info_message: info,
                        });
                    }
                    Err(e) => {
                        let _ =
                            order_result_tx.send(OperationResult::OrderChatAttachmentSendFailed {
                                prepared,
                                error: e.to_string(),
                            });
                    }
                }
            }
        }
    });
}
