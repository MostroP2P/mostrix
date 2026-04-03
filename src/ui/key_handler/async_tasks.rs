use crate::models::User;
use crate::settings::load_settings_from_disk;
use crate::settings::Settings;
use crate::ui::key_handler::EnterKeyContext;
use crate::ui::FormState;
use crate::ui::{
    AdminChatUpdate, AppState, ChatAttachment, MessageNotification, MostroInfoFetchResult,
    NetworkStatus, OperationResult, TakeOrderState, UiMode,
};
use crate::util::fetch_mostro_instance_info;
use crate::util::listen_for_order_messages;
use crate::util::order_utils::spawn_fetch_scheduler_loops;
use crate::util::{
    any_relay_reachable, connect_client_safely, hydrate_startup_active_order_dm_state,
    set_dm_router_cmd_tx, OrderDmSubscriptionCmd, StartupDmHydration,
};
use mostro_core::prelude::{Dispute, SmallOrder};
use nostr_sdk::prelude::{Client, Keys, PublicKey};
use sqlx::SqlitePool;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::{
    env, fs,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use zeroize::Zeroizing;

pub struct RuntimeReconnectContext<'a> {
    pub app: &'a mut AppState,
    pub client: &'a mut Client,
    pub current_mostro_pubkey: &'a Arc<Mutex<PublicKey>>,
    pub pool: &'a SqlitePool,
    pub message_listener_handle: &'a mut JoinHandle<()>,
    pub message_notification_tx: &'a UnboundedSender<MessageNotification>,
    pub orders: Arc<Mutex<Vec<SmallOrder>>>,
    pub disputes: Arc<Mutex<Vec<Dispute>>>,
    pub order_fetch_task: &'a mut JoinHandle<()>,
    pub dispute_fetch_task: &'a mut JoinHandle<()>,
    pub dm_subscription_tx: &'a mut UnboundedSender<OrderDmSubscriptionCmd>,
    pub settings: &'a Settings,
}

fn clear_runtime_session_state(app: &mut AppState) {
    match app.messages.lock() {
        Ok(mut messages) => messages.clear(),
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
            app.fatal_exit_on_close = true;
            app.mode = UiMode::OperationResult(OperationResult::Error(
                "Internal error. Please restart Mostrix.".to_string(),
            ));
            return;
        }
    }
    match app.active_order_trade_indices.lock() {
        Ok(mut active) => active.clear(),
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
            ));
            app.fatal_exit_on_close = true;
            app.mode = UiMode::OperationResult(OperationResult::Error(
                "Internal error. Please restart Mostrix.".to_string(),
            ));
            return;
        }
    }
    match app.pending_notifications.lock() {
        Ok(mut pending) => *pending = 0,
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned pending notifications lock: {e}). Please restart the app."
            ));
            app.fatal_exit_on_close = true;
            app.mode = UiMode::OperationResult(OperationResult::Error(
                "Internal error. Please restart Mostrix.".to_string(),
            ));
        }
    }
    app.selected_message_idx = 0;
    app.pending_post_take_operation_result = None;
}

/// Reload Nostr client, Mostro pubkey, and message listener after the user persisted new keys
/// (`pending_key_reload`). Updates `app` and shared runtime state on success; sets an error
/// [`OperationResult`] on failure.
#[allow(clippy::too_many_arguments)]
pub async fn apply_pending_key_reload(
    app: &mut AppState,
    client: &mut Client,
    mostro_pubkey: &mut PublicKey,
    current_mostro_pubkey: &Arc<Mutex<PublicKey>>,
    pool: &SqlitePool,
    message_listener_handle: &mut JoinHandle<()>,
    message_notification_tx: &UnboundedSender<MessageNotification>,
    orders: Arc<Mutex<Vec<SmallOrder>>>,
    disputes: Arc<Mutex<Vec<Dispute>>>,
    order_fetch_task: &mut JoinHandle<()>,
    dispute_fetch_task: &mut JoinHandle<()>,
    dm_subscription_tx: &mut UnboundedSender<OrderDmSubscriptionCmd>,
) {
    match load_settings_from_disk() {
        Ok(latest_settings) => match latest_settings.nsec_privkey.parse::<Keys>() {
            Ok(new_identity_keys) => {
                let new_client = Client::new(new_identity_keys);
                let mut reload_error: Option<String> = None;
                for relay in &latest_settings.relays {
                    let relay = relay.trim();
                    if relay.is_empty() {
                        continue;
                    }
                    if let Err(e) = new_client.add_relay(relay).await {
                        reload_error =
                            Some(format!("Failed to add relay during key reload: {}", e));
                        break;
                    }
                }
                if let Some(err) = reload_error {
                    app.pending_key_reload = false;
                    app.mode = UiMode::OperationResult(OperationResult::Error(err));
                } else if let Ok(new_mostro_pubkey) =
                    PublicKey::from_str(&latest_settings.mostro_pubkey)
                {
                    message_listener_handle.abort();
                    if let Err(e) = connect_client_safely(&new_client).await {
                        log::warn!("Key reload: failed to connect Nostr client: {e}");
                    }

                    *client = new_client;
                    *mostro_pubkey = new_mostro_pubkey;
                    match current_mostro_pubkey.lock() {
                        Ok(mut active_pubkey) => {
                            *active_pubkey = new_mostro_pubkey;
                        }
                        Err(e) => {
                            crate::util::request_fatal_restart(format!(
                                "Mostrix encountered an internal error (poisoned Mostro pubkey lock: {e}). Please restart the app."
                            ));
                            app.pending_key_reload = false;
                            app.fatal_exit_on_close = true;
                            app.mode = UiMode::OperationResult(OperationResult::Error(
                                "Internal error. Please restart Mostrix.".to_string(),
                            ));
                            return;
                        }
                    }
                    app.currencies_filter = latest_settings.currencies_filter.clone();
                    clear_runtime_session_state(app);

                    order_fetch_task.abort();
                    dispute_fetch_task.abort();
                    let (o, d) = spawn_fetch_scheduler_loops(
                        client.clone(),
                        Arc::clone(current_mostro_pubkey),
                        Arc::clone(&orders),
                        Arc::clone(&disputes),
                        &latest_settings,
                    );
                    *order_fetch_task = o;
                    *dispute_fetch_task = d;

                    let client_for_messages = client.clone();
                    let pool_for_messages = pool.clone();
                    let startup_dm_hydration =
                        match hydrate_startup_active_order_dm_state(pool).await {
                            Ok(h) => h,
                            Err(e) => {
                                log::warn!(
                                "Key reload: failed to hydrate startup active order DM state: {}",
                                e
                            );
                                StartupDmHydration::empty()
                            }
                        };
                    if let Ok(mut indices) = app.active_order_trade_indices.lock() {
                        *indices = startup_dm_hydration.active_order_trade_indices.clone();
                    }
                    let active_order_trade_indices_clone =
                        Arc::clone(&app.active_order_trade_indices);
                    let order_last_seen_dm_ts_clone =
                        startup_dm_hydration.order_last_seen_dm_ts.clone();
                    let messages_clone = Arc::clone(&app.messages);
                    let message_notification_tx_clone = message_notification_tx.clone();
                    let pending_notifications_clone = Arc::clone(&app.pending_notifications);
                    let (new_dm_tx, new_dm_rx) =
                        tokio::sync::mpsc::unbounded_channel::<OrderDmSubscriptionCmd>();
                    *dm_subscription_tx = new_dm_tx;
                    let router_reg = set_dm_router_cmd_tx(dm_subscription_tx.clone());
                    if let Err(msg) = &router_reg {
                        log::error!("[dm_listener] {}", msg);
                    }
                    *message_listener_handle = tokio::spawn(async move {
                        listen_for_order_messages(
                            client_for_messages,
                            pool_for_messages,
                            active_order_trade_indices_clone,
                            order_last_seen_dm_ts_clone,
                            messages_clone,
                            message_notification_tx_clone,
                            pending_notifications_clone,
                            new_dm_rx,
                        )
                        .await;
                    });

                    app.backup_requires_restart = false;
                    app.pending_key_reload = false;
                    app.mode = match router_reg {
                        Ok(()) => UiMode::OperationResult(OperationResult::Info(
                            "Keys reloaded. Active session state has been reset.".to_string(),
                        )),
                        Err(msg) => UiMode::OperationResult(OperationResult::Error(format!(
                            "Keys reloaded but DM router registration failed ({msg}). Background trade messages still run; one-shot DM waits may fail until you restart the app."
                        ))),
                    };
                } else {
                    app.pending_key_reload = false;
                    app.mode = UiMode::OperationResult(OperationResult::Error(format!(
                        "Invalid Mostro pubkey after key reload: {}",
                        latest_settings.mostro_pubkey
                    )));
                }
            }
            Err(e) => {
                app.pending_key_reload = false;
                app.mode = UiMode::OperationResult(OperationResult::Error(format!(
                    "Invalid identity key after reload: {}",
                    e
                )));
            }
        },
        Err(e) => {
            app.pending_key_reload = false;
            app.mode = UiMode::OperationResult(OperationResult::Error(format!(
                "Failed to load settings for key reload: {}",
                e
            )));
        }
    }
}

/// Reconnect runtime background tasks after connectivity returns.
///
/// Mirrors the `apply_pending_key_reload` flow (abort/respawn fetch loops and DM listener),
/// but does not change keys or Mostro pubkey.
#[allow(clippy::too_many_arguments)]
pub async fn reload_runtime_session_after_reconnect(
    ctx: RuntimeReconnectContext<'_>,
) -> Result<(), String> {
    if !any_relay_reachable(&ctx.settings.relays).await {
        return Err("No internet / relays unreachable".to_string());
    }

    ctx.message_listener_handle.abort();
    ctx.order_fetch_task.abort();
    ctx.dispute_fetch_task.abort();
    ctx.client.unsubscribe_all().await;

    connect_client_safely(ctx.client)
        .await
        .map_err(|e| format!("Reconnect: failed to connect Nostr client: {e}"))?;

    ctx.app.currencies_filter = ctx.settings.currencies_filter.clone();
    clear_runtime_session_state(ctx.app);

    let (o, d) = spawn_fetch_scheduler_loops(
        ctx.client.clone(),
        Arc::clone(ctx.current_mostro_pubkey),
        Arc::clone(&ctx.orders),
        Arc::clone(&ctx.disputes),
        ctx.settings,
    );
    *ctx.order_fetch_task = o;
    *ctx.dispute_fetch_task = d;

    let client_for_messages = ctx.client.clone();
    let pool_for_messages = ctx.pool.clone();
    let startup_dm_hydration = match hydrate_startup_active_order_dm_state(ctx.pool).await {
        Ok(h) => h,
        Err(e) => {
            log::warn!(
                "Reconnect: failed to hydrate startup active order DM state: {}",
                e
            );
            StartupDmHydration::empty()
        }
    };
    if let Ok(mut indices) = ctx.app.active_order_trade_indices.lock() {
        *indices = startup_dm_hydration.active_order_trade_indices.clone();
    }
    let active_order_trade_indices_clone = Arc::clone(&ctx.app.active_order_trade_indices);
    let order_last_seen_dm_ts_clone = startup_dm_hydration.order_last_seen_dm_ts.clone();
    let messages_clone = Arc::clone(&ctx.app.messages);
    let message_notification_tx_clone = ctx.message_notification_tx.clone();
    let pending_notifications_clone = Arc::clone(&ctx.app.pending_notifications);
    let (new_dm_tx, new_dm_rx) = tokio::sync::mpsc::unbounded_channel::<OrderDmSubscriptionCmd>();
    *ctx.dm_subscription_tx = new_dm_tx;
    let router_reg = set_dm_router_cmd_tx(ctx.dm_subscription_tx.clone());
    if let Err(msg) = &router_reg {
        log::error!("[dm_listener] {}", msg);
    }
    *ctx.message_listener_handle = tokio::spawn(async move {
        listen_for_order_messages(
            client_for_messages,
            pool_for_messages,
            active_order_trade_indices_clone,
            order_last_seen_dm_ts_clone,
            messages_clone,
            message_notification_tx_clone,
            pending_notifications_clone,
            new_dm_rx,
        )
        .await;
    });

    let mostro_pubkey = match ctx.current_mostro_pubkey.lock() {
        Ok(pk) => *pk,
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned Mostro pubkey lock: {e}). Please restart the app."
            ));
            return Err("Internal error. Please restart Mostrix.".to_string());
        }
    };
    match fetch_mostro_instance_info(ctx.client, mostro_pubkey).await {
        Ok(Some(info)) => {
            ctx.app.mostro_info = Some(info);
        }
        Ok(None) => {
            // Keep prior info if any; not an error.
        }
        Err(e) => {
            log::warn!("Reconnect: failed to refresh Mostro instance info: {}", e);
        }
    }

    match router_reg {
        Ok(()) => Ok(()),
        Err(msg) => Err(format!(
            "Reconnected, but DM router registration failed ({msg}). Consider restarting Mostrix."
        )),
    }
}

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
    pub dm_subscription_tx: UnboundedSender<OrderDmSubscriptionCmd>,
    pub dm_subscription_rx: UnboundedReceiver<OrderDmSubscriptionCmd>,
    pub network_status_tx: UnboundedSender<NetworkStatus>,
    pub network_status_rx: UnboundedReceiver<NetworkStatus>,
    pub fatal_error_tx: UnboundedSender<String>,
    pub fatal_error_rx: UnboundedReceiver<String>,
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
    let (dm_subscription_tx, dm_subscription_rx) =
        tokio::sync::mpsc::unbounded_channel::<OrderDmSubscriptionCmd>();
    let (network_status_tx, network_status_rx) =
        tokio::sync::mpsc::unbounded_channel::<NetworkStatus>();
    let (fatal_error_tx, fatal_error_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

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
        dm_subscription_tx,
        dm_subscription_rx,
        network_status_tx,
        network_status_rx,
        fatal_error_tx,
        fatal_error_rx,
    }
}

pub fn spawn_send_new_order_task(ctx: &EnterKeyContext<'_>, form: FormState) {
    let pool = ctx.pool.clone();
    let client = ctx.client.clone();
    let order_result_tx = ctx.order_result_tx.clone();
    let dm_subscription_tx = ctx.dm_subscription_tx.clone();
    let fallback_mostro_pubkey = ctx.mostro_pubkey;
    let current_mostro_pubkey = Arc::clone(ctx.current_mostro_pubkey);
    tokio::spawn(async move {
        let mostro_pubkey = match current_mostro_pubkey.lock() {
            Ok(guard) => *guard,
            Err(_) => {
                log::warn!(
                    "Failed to lock runtime Mostro pubkey; using settings snapshot (fallback)"
                );
                fallback_mostro_pubkey
            }
        };
        match crate::util::send_new_order(
            &pool,
            &client,
            mostro_pubkey,
            form,
            Some(&dm_subscription_tx),
        )
        .await
        {
            Ok(result) => {
                let _ = order_result_tx.send(result);
            }
            Err(e) => {
                log::error!("Failed to send order: {}", e);
                let _ = order_result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_take_order_task(
    pool: SqlitePool,
    client: Client,
    settings: Settings,
    mostro_pubkey: PublicKey,
    take_state: TakeOrderState,
    amount: Option<i64>,
    invoice: Option<String>,
    result_tx: UnboundedSender<OperationResult>,
    dm_subscription_tx: UnboundedSender<OrderDmSubscriptionCmd>,
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
            Some(&dm_subscription_tx),
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

/// `show_result_toast`: when false (e.g. startup), only [`MostroInfoFetchResult::Applied`] is sent on
/// success and errors are logged without UI.
pub fn spawn_refresh_mostro_info_task(
    client: Client,
    mostro_pubkey: PublicKey,
    tx: UnboundedSender<MostroInfoFetchResult>,
    show_result_toast: bool,
) {
    tokio::spawn(async move {
        let result = fetch_mostro_instance_info(&client, mostro_pubkey).await;
        if !show_result_toast {
            match &result {
                Ok(Some(_)) => {}
                Ok(None) => {
                    log::info!("No Mostro instance info event found for current Mostro pubkey");
                }
                Err(e) => {
                    log::warn!("Failed to fetch Mostro instance info: {}", e);
                }
            }
            if let Ok(info) = result {
                let _ = tx.send(MostroInfoFetchResult::Applied {
                    info: Box::new(info),
                });
            }
            return;
        }
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
                let new_user = User::from_mnemonic(mnemonic.clone())?;
                let mut tx = pool.begin().await?;
                User::replace_all_in_tx(&new_user, &mut tx).await?;

                let mut s = crate::settings::load_settings_from_disk()?;
                s.nsec_privkey = derived_nsec.clone();
                let toml_string = toml::to_string_pretty(&s)
                    .map_err(|e| anyhow::anyhow!("Failed to serialize settings: {}", e))?;

                let home_dir = dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
                let package_name = env!("CARGO_PKG_NAME");
                let hidden_file_path = home_dir
                    .join(format!(".{package_name}"))
                    .join("settings.toml");
                let executable_file_path = env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|dir| dir.join("settings.toml")));
                let target_settings_file = executable_file_path
                    .filter(|p| p.exists())
                    .unwrap_or(hidden_file_path);

                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let tmp_path = target_settings_file.with_extension(format!("tmp-{}", nanos));
                fs::write(&tmp_path, toml_string).map_err(|e| {
                    anyhow::anyhow!("Failed to write temporary settings file: {}", e)
                })?;

                if let Err(e) = tx.commit().await {
                    let _ = fs::remove_file(&tmp_path);
                    return Err(anyhow::anyhow!("Failed to commit user update: {}", e));
                }
                if let Err(e) = fs::rename(&tmp_path, &target_settings_file) {
                    let _ = fs::remove_file(&tmp_path);
                    return Err(anyhow::anyhow!(
                        "Failed to atomically replace settings: {}",
                        e
                    ));
                }
            } else {
                let mut s = crate::settings::load_settings_from_disk()?;
                s.admin_privkey = derived_nsec.clone();
                crate::settings::save_settings(&s)?;
            }
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
