//! Send encrypted order-chat attachments (encrypt → Blossom → shared-key GiftWrap).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use nostr_sdk::prelude::PublicKey;
use nostr_sdk::prelude::{Client, Keys, SecretKey};
use sqlx::SqlitePool;
use std::str::FromStr;
use tokio::sync::mpsc::UnboundedSender;

use crate::models::Order;
use crate::settings::Settings;
use crate::ui::helpers::{build_file_encrypted_json, build_image_encrypted_json};
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
) -> Result<crate::ui::helpers::OutboundAttachmentPayload> {
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

/// Encrypts, uploads, sends attachment JSON over order chat, returns local chat row.
pub async fn send_order_chat_attachment(
    client: &Client,
    pool: &SqlitePool,
    order_id: &str,
    path: &Path,
    blossom_servers: &[String],
    mostro_info: Option<&MostroInstanceInfo>,
) -> Result<(UserOrderChatMessage, String)> {
    let validated = validate_attachment_file(path)?;
    let order = Order::get_by_id(pool, order_id)
        .await
        .map_err(|e| anyhow!("order not found: {}", e))?;

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
    let blossom_url = upload_blob_with_retry(&http, blossom_servers, &encrypted_blob).await?;
    let outbound = build_outbound_payload(&validated, blossom_url, &encrypted_blob)?;

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

    send_user_order_chat_message_via_shared_key(
        client,
        &trade_keys,
        &shared_keys,
        &outbound.json_body,
        mostro_info,
    )
    .await?;

    let ts = chrono::Utc::now().timestamp();
    let local_msg = UserOrderChatMessage {
        sender: UserChatSender::You,
        content: outbound.display_content,
        timestamp: ts,
        attachment: Some(outbound.attachment),
    };
    let info = format!("Attachment sent: {}", validated.filename);
    Ok((local_msg, info))
}

/// Background task: send attachment and notify the UI via `order_result_tx`.
pub fn spawn_send_order_chat_attachment(
    order_id: String,
    path: PathBuf,
    client: Client,
    pool: SqlitePool,
    blossom_servers: Vec<String>,
    mostro_info: Option<MostroInstanceInfo>,
    order_result_tx: UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        let result = async {
            send_order_chat_attachment(
                &client,
                &pool,
                &order_id,
                &path,
                &blossom_servers,
                mostro_info.as_ref(),
            )
            .await
        }
        .await;

        match result {
            Ok((chat_message, info)) => {
                let _ = order_result_tx.send(OperationResult::OrderChatAttachmentSent {
                    order_id,
                    chat_message,
                    info_message: info,
                });
            }
            Err(e) => {
                let _ = order_result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}
