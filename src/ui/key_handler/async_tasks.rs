use crate::settings::load_settings_from_disk;
use crate::ui::{MostroInfoFetchResult, OperationResult, TakeOrderState};
use crate::util::fetch_mostro_instance_info;
use crate::SETTINGS;
use nostr_sdk::prelude::{Client, PublicKey};
use sqlx::SqlitePool;
use std::str::FromStr;
use tokio::sync::mpsc::UnboundedSender;
use zeroize::Zeroizing;

pub fn spawn_send_new_order_task(
    pool: SqlitePool,
    client: Client,
    mostro_pubkey: PublicKey,
    form: crate::ui::FormState,
    result_tx: UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        let settings = match SETTINGS.get() {
            Some(s) => s,
            None => {
                let error_msg =
                    "Settings not initialized. Please restart the application.".to_string();
                log::error!("{}", error_msg);
                let _ = result_tx.send(OperationResult::Error(error_msg));
                return;
            }
        };

        match crate::util::send_new_order(&pool, &client, settings, mostro_pubkey, &form).await {
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
    mostro_pubkey: PublicKey,
    take_state: TakeOrderState,
    amount: Option<i64>,
    invoice: Option<String>,
    result_tx: UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        let settings = match SETTINGS.get() {
            Some(s) => s,
            None => {
                let error_msg =
                    "Settings not initialized. Please restart the application.".to_string();
                log::error!("{}", error_msg);
                let _ = result_tx.send(OperationResult::Error(error_msg));
                return;
            }
        };
        match crate::util::take_order(
            &pool,
            &client,
            settings,
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
