use crate::models::User;
use crate::settings::load_settings_from_disk;
use crate::settings::Settings;
use crate::ui::{
    AdminChatUpdate, ChatAttachment, MessageNotification, MostroInfoFetchResult, OperationResult,
    TakeOrderState,
};
use crate::util::fetch_mostro_instance_info;
use nostr_sdk::prelude::{Client, PublicKey};
use sqlx::SqlitePool;
use std::str::FromStr;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use zeroize::Zeroizing;

pub struct AppChannels {
    pub order_result_tx: UnboundedSender<OperationResult>,
    pub order_result_rx: UnboundedReceiver<OperationResult>,
    pub key_rotation_tx: UnboundedSender<Result<Zeroizing<String>, String>>,
    pub key_rotation_rx: UnboundedReceiver<Result<Zeroizing<String>, String>>,
    pub seed_words_tx: UnboundedSender<Result<Zeroizing<String>, String>>,
    pub seed_words_rx: UnboundedReceiver<Result<Zeroizing<String>, String>>,
    pub message_notification_tx: UnboundedSender<MessageNotification>,
    pub message_notification_rx: UnboundedReceiver<MessageNotification>,
    pub admin_chat_updates_tx: UnboundedSender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    pub admin_chat_updates_rx: UnboundedReceiver<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    pub save_attachment_tx: UnboundedSender<(String, ChatAttachment)>,
    pub save_attachment_rx: UnboundedReceiver<(String, ChatAttachment)>,
    pub mostro_info_tx: UnboundedSender<MostroInfoFetchResult>,
    pub mostro_info_rx: UnboundedReceiver<MostroInfoFetchResult>,
}

pub fn create_app_channels() -> AppChannels {
    let (order_result_tx, order_result_rx) =
        tokio::sync::mpsc::unbounded_channel::<OperationResult>();
    let (key_rotation_tx, key_rotation_rx) =
        tokio::sync::mpsc::unbounded_channel::<Result<Zeroizing<String>, String>>();
    let (seed_words_tx, seed_words_rx) =
        tokio::sync::mpsc::unbounded_channel::<Result<Zeroizing<String>, String>>();
    let (message_notification_tx, message_notification_rx) =
        tokio::sync::mpsc::unbounded_channel::<MessageNotification>();
    let (admin_chat_updates_tx, admin_chat_updates_rx) =
        tokio::sync::mpsc::unbounded_channel::<Result<Vec<AdminChatUpdate>, anyhow::Error>>();
    let (save_attachment_tx, save_attachment_rx) =
        tokio::sync::mpsc::unbounded_channel::<(String, ChatAttachment)>();
    let (mostro_info_tx, mostro_info_rx) =
        tokio::sync::mpsc::unbounded_channel::<MostroInfoFetchResult>();

    AppChannels {
        order_result_tx,
        order_result_rx,
        key_rotation_tx,
        key_rotation_rx,
        seed_words_tx,
        seed_words_rx,
        message_notification_tx,
        message_notification_rx,
        admin_chat_updates_tx,
        admin_chat_updates_rx,
        save_attachment_tx,
        save_attachment_rx,
        mostro_info_tx,
        mostro_info_rx,
    }
}

pub fn spawn_send_new_order_task(
    pool: SqlitePool,
    client: Client,
    settings: Settings,
    mostro_pubkey: PublicKey,
    form: crate::ui::FormState,
    result_tx: UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        match crate::util::send_new_order(&pool, &client, &settings, mostro_pubkey, &form).await {
            Ok(result) => {
                let _ = result_tx.send(result);
            }
            Err(e) => {
                log::error!("Failed to send order: {}", e);
                let _ = result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}

pub fn spawn_take_order_task(
    pool: SqlitePool,
    client: Client,
    settings: Settings,
    mostro_pubkey: PublicKey,
    take_state: TakeOrderState,
    amount: Option<i64>,
    invoice: Option<String>,
    result_tx: UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        match crate::util::take_order(
            &pool,
            &client,
            &settings,
            mostro_pubkey,
            &take_state.order,
            amount,
            invoice,
        )
        .await
        {
            Ok(result) => {
                let _ = result_tx.send(result);
            }
            Err(e) => {
                log::error!("Failed to take order: {}", e);
                let _ = result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}

pub fn spawn_refresh_mostro_info_from_settings_task(
    client: Client,
    tx: UnboundedSender<MostroInfoFetchResult>,
) {
    tokio::spawn(async move {
        let settings = match load_settings_from_disk() {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(MostroInfoFetchResult::Err(format!(
                    "Failed to load settings: {}",
                    e
                )));
                return;
            }
        };
        let mostro_pubkey = match PublicKey::from_str(&settings.mostro_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                let _ = tx.send(MostroInfoFetchResult::Err(format!(
                    "Invalid Mostro pubkey in settings: {}",
                    e
                )));
                return;
            }
        };
        let result = fetch_mostro_instance_info(&client, mostro_pubkey).await;
        let res = match result {
            Ok(Some(info)) => MostroInfoFetchResult::Ok {
                info: Box::new(Some(info)),
                message: "Mostro instance info refreshed from relays.".to_string(),
            },
            Ok(None) => MostroInfoFetchResult::Ok {
                info: Box::new(None),
                message: "No Mostro instance info event found for the current pubkey.".to_string(),
            },
            Err(e) => {
                MostroInfoFetchResult::Err(format!("Failed to refresh Mostro instance info: {}", e))
            }
        };
        let _ = tx.send(res);
    });
}

pub fn spawn_refresh_mostro_info_task(
    client: Client,
    mostro_pubkey: PublicKey,
    tx: UnboundedSender<MostroInfoFetchResult>,
) {
    tokio::spawn(async move {
        let result = fetch_mostro_instance_info(&client, mostro_pubkey).await;
        let res = match result {
            Ok(info) => MostroInfoFetchResult::Ok {
                info: Box::new(info),
                message: "Mostro instance info updated.".to_string(),
            },
            Err(e) => {
                log::warn!(
                    "Failed to refresh Mostro instance info after pubkey change: {}",
                    e
                );
                MostroInfoFetchResult::Err(e.to_string())
            }
        };
        let _ = tx.send(res);
    });
}

pub fn spawn_add_relay_task(client: Client, relay: String) {
    tokio::spawn(async move {
        if let Err(e) = client.add_relay(relay.trim()).await {
            log::error!("Failed to add relay at runtime: {}", e);
        }
    });
}

pub fn spawn_key_rotation_task(
    pool: SqlitePool,
    is_user_mode: bool,
    mnemonic: String,
    derived_nsec: String,
    rotation_tx: UnboundedSender<Result<Zeroizing<String>, String>>,
) {
    tokio::spawn(async move {
        let rotation_result: Result<(), anyhow::Error> = async {
            if is_user_mode {
                crate::models::User::replace_all_atomic(mnemonic.clone(), &pool).await?;
            }

            let mut s = crate::settings::load_settings_from_disk()?;
            if is_user_mode {
                s.nsec_privkey = derived_nsec.clone();
            } else {
                s.admin_privkey = derived_nsec.clone();
            }
            crate::settings::save_settings(&s)?;
            Ok(())
        }
        .await;

        match rotation_result {
            Ok(()) => {
                let _ = rotation_tx.send(Ok(Zeroizing::new(mnemonic)));
            }
            Err(e) => {
                log::error!("Failed to persist key rotation before backup popup: {}", e);
                let _ = rotation_tx.send(Err(format!("Failed to save new keys: {}", e)));
            }
        }
    });
}

pub fn spawn_load_seed_words_task(
    pool: SqlitePool,
    tx: UnboundedSender<Result<Zeroizing<String>, String>>,
) {
    tokio::spawn(async move {
        match User::get(&pool).await {
            Ok(user) => {
                let _ = tx.send(Ok(Zeroizing::new(user.mnemonic)));
            }
            Err(e) => {
                let _ = tx.send(Err(format!(
                    "Failed to load seed words from database: {}",
                    e
                )));
            }
        }
    });
}
