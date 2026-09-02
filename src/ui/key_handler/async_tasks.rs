use crate::models::{Order, User};
use crate::settings::load_settings_from_disk;
use crate::settings::Settings;
use crate::ui::helpers::{
    clear_session_chat_projection, hydrate_app_admin_keys_from_privkey, track_startup_chats,
};
use crate::ui::key_handler::EnterKeyContext;
use crate::ui::pending_trade_index_retry::PendingTradeIndexRetry;
use crate::ui::FormState;
use crate::ui::{
    AdminChatUpdate, AppState, ChatAttachment, LnAddressVerifyResult, MessageNotification,
    MostroInfoFetchResult, NetworkStatus, OperationResult, OrderChatUpdate, TakeOrderState, UiMode,
};
use crate::util::fatal::request_fatal_restart;
use crate::util::fetch_mostro_instance_info;
use crate::util::order_utils::spawn_fetch_scheduler_loops;
use crate::util::{
    any_relay_reachable, connect_client_safely, hydrate_startup_active_order_dm_state,
    is_invalid_trade_index_error, set_chat_router_cmd_tx, set_dm_router_cmd_tx,
    spawn_supervised_chat_listener, spawn_supervised_trade_dm_listener,
    sync_trade_index_from_mostro_and_persist, unsubscribe_dm_listener_subscriptions, ChatRouterCmd,
    FatalNotify, OrderDmSubscriptionCmd, StartupDmHydration,
};
use mostro_core::prelude::{Dispute, SmallOrder, Transport};
use nostr_sdk::prelude::{Client, Keys, Output, PublicKey, SignerAuthenticator};
use sqlx::SqlitePool;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::{
    env, fs,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use zeroize::Zeroizing;

pub struct RuntimeReconnectContext<'a> {
    pub app: &'a mut AppState,
    pub client: &'a mut Client,
    /// In-memory pubkey passed to key handlers; kept in sync with [`Self::current_mostro_pubkey`].
    pub mostro_pubkey: &'a mut PublicKey,
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

const POISONED_UI_FATAL: &str = "Internal error. Please restart Mostrix.";

fn operation_result_for_mostro_command_error(
    err: anyhow::Error,
    retry: PendingTradeIndexRetry,
) -> OperationResult {
    if is_invalid_trade_index_error(&err) {
        OperationResult::TradeIndexOutOfSync { retry }
    } else {
        OperationResult::Error(err.to_string())
    }
}

/// Sync trade index from Mostro, then re-run the command that failed with `InvalidTradeIndex`.
pub fn spawn_trade_index_sync_and_retry(ctx: &EnterKeyContext<'_>, retry: PendingTradeIndexRetry) {
    let pool = ctx.pool.clone();
    let client = ctx.client.clone();
    let order_result_tx = ctx.order_result_tx.clone();
    let dm_subscription_tx = ctx.dm_subscription_tx.clone();
    let fallback_mostro_pubkey = ctx.mostro_pubkey;
    let current_mostro_pubkey = Arc::clone(ctx.current_mostro_pubkey);
    let mostro_info = ctx.mostro_info.clone();

    tokio::spawn(async move {
        let mostro_pubkey = match current_mostro_pubkey.lock() {
            Ok(guard) => *guard,
            Err(_) => fallback_mostro_pubkey,
        };

        if let Err(e) = sync_trade_index_from_mostro_and_persist(
            &pool,
            &client,
            mostro_pubkey,
            mostro_info.as_ref(),
        )
        .await
        {
            let _ = order_result_tx.send(OperationResult::Error(format!(
                "Trade index sync failed: {e}"
            )));
            return;
        }

        let result = match retry {
            PendingTradeIndexRetry::NewOrder { form } => {
                crate::util::send_new_order(
                    &pool,
                    &client,
                    mostro_pubkey,
                    form,
                    Some(&dm_subscription_tx),
                    mostro_info.as_ref(),
                )
                .await
            }
            PendingTradeIndexRetry::TakeOrder {
                take_state,
                amount,
                invoice,
            } => {
                crate::util::take_order(
                    &pool,
                    &client,
                    mostro_pubkey,
                    &take_state.order,
                    amount,
                    invoice,
                    Some(&dm_subscription_tx),
                    mostro_info.as_ref(),
                )
                .await
            }
        };

        match result {
            Ok(op) => {
                let _ = order_result_tx.send(op);
            }
            Err(e) => {
                log::error!("Command failed after trade index sync: {e}");
                let _ = order_result_tx.send(OperationResult::Error(e.to_string()));
            }
        }
    });
}

/// Shared by runtime reset paths: log fatal, set exit-on-close, show error mode.
fn apply_poisoned_mutex_ui_fatal(app: &mut AppState, user_message: String) {
    request_fatal_restart(user_message);
    app.fatal_exit_on_close = true;
    app.mode = UiMode::operation_result(OperationResult::Error(POISONED_UI_FATAL.to_string()));
}

fn clear_messages_or_fatal(app: &mut AppState) -> Result<(), ()> {
    let poison_message = {
        let lock_result = app.messages.lock();
        match lock_result {
            Ok(mut messages) => {
                messages.clear();
                return Ok(());
            }
            Err(e) => {
                format!(
                    "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
                )
            }
        }
    };
    apply_poisoned_mutex_ui_fatal(app, poison_message);
    Err(())
}

/// Clears `active_order_trade_indices` or applies fatal UI state and returns `Err(())` on poison.
fn clear_active_order_indices_or_fatal(app: &mut AppState) -> Result<(), ()> {
    let poison_message = {
        let lock_result = app.active_order_trade_indices.lock();
        match lock_result {
            Ok(mut active) => {
                active.clear();
                return Ok(());
            }
            Err(e) => {
                format!(
                    "Mostrix encountered an internal error (poisoned active order indices lock: {e}). Please restart the app."
                )
            }
        }
    };
    apply_poisoned_mutex_ui_fatal(app, poison_message);
    Err(())
}

/// Resets `pending_notifications` to 0 or applies fatal UI state and returns `Err(())` on poison.
fn reset_pending_notifications_or_fatal(app: &mut AppState) -> Result<(), ()> {
    let poison_message = {
        let lock_result = app.pending_notifications.lock();
        match lock_result {
            Ok(mut pending) => {
                *pending = 0;
                return Ok(());
            }
            Err(e) => {
                format!(
                    "Mostrix encountered an internal error (poisoned pending notifications lock: {e}). Please restart the app."
                )
            }
        }
    };
    apply_poisoned_mutex_ui_fatal(app, poison_message);
    Err(())
}

/// Reset in-memory trade/message state after identity key reload; also clears peer/solver
/// chat transcripts and cursors via [`clear_session_chat_projection`].
fn clear_runtime_session_state(app: &mut AppState) {
    if clear_messages_or_fatal(app).is_err() {
        return;
    }
    if clear_active_order_indices_or_fatal(app).is_err() {
        return;
    }
    if reset_pending_notifications_or_fatal(app).is_err() {
        return;
    }
    clear_session_chat_projection(app);
    app.selected_message_idx = 0;
    app.pending_post_take_operation_result = None;
}

fn clear_runtime_tracking_state_preserve_messages(app: &mut AppState) {
    if clear_active_order_indices_or_fatal(app).is_err() {
        return;
    }
    if reset_pending_notifications_or_fatal(app).is_err() {
        return;
    }
    app.selected_message_idx = 0;
    app.pending_post_take_operation_result = None;
    if let Ok(mut dropped) = app.dropped_user_history_order_ids.lock() {
        dropped.clear();
    }
}

/// Fetch instance info for `mostro_pubkey`, refresh [`AppState`] transport, and return it for the DM listener.
///
/// Uses [`fetch_mostro_instance_info`] (client-side author / `d` / signature checks). Apply goes
/// through [`AppState::set_mostro_info`], which ignores older `created_at` than the cache.
async fn dm_transport_for_mostro(
    client: &Client,
    mostro_pubkey: PublicKey,
    app: &mut AppState,
    log_context: &str,
) -> Transport {
    match fetch_mostro_instance_info(client, mostro_pubkey).await {
        Ok(fetch) => {
            if let Some(info) = fetch.to_apply() {
                app.set_mostro_info(Some(info));
            }
            app.transport
        }
        Err(e) => {
            log::warn!(
                "{log_context}: failed to fetch Mostro instance info: {e}; keeping cached transport"
            );
            app.transport
        }
    }
}

/// Abort and respawn the trade DM listener (e.g. after `protocol_version` / transport changes).
#[allow(clippy::too_many_arguments)]
pub async fn respawn_trade_dm_listener(
    app: &mut AppState,
    client: &Client,
    mostro_pubkey: PublicKey,
    pool: &SqlitePool,
    message_listener_handle: &mut JoinHandle<()>,
    message_notification_tx: &UnboundedSender<MessageNotification>,
    dm_subscription_tx: &mut UnboundedSender<OrderDmSubscriptionCmd>,
    log_context: &str,
) -> Result<(), String> {
    message_listener_handle.abort();
    // Await the old task before draining subs so it cannot register new ids after take().
    let old_listener = std::mem::replace(message_listener_handle, tokio::spawn(async {}));
    let _ = old_listener.await;
    // DM listener subs only — `Client` is shared with order/dispute fetch schedulers.
    unsubscribe_dm_listener_subscriptions(client).await;

    let startup_dm_hydration = match hydrate_startup_active_order_dm_state(pool).await {
        Ok(h) => h,
        Err(e) => {
            log::warn!("{log_context}: failed to hydrate startup active order DM state: {e}");
            StartupDmHydration::empty()
        }
    };
    if let Ok(mut indices) = app.active_order_trade_indices.lock() {
        *indices = startup_dm_hydration.active_order_trade_indices.clone();
    }
    app.startup_popup_floor_ts = startup_dm_hydration.order_last_seen_dm_ts.clone();

    let client_for_messages = client.clone();
    let pool_for_messages = pool.clone();
    let active_order_trade_indices_clone = Arc::clone(&app.active_order_trade_indices);
    let order_last_seen_dm_ts_clone = startup_dm_hydration.order_last_seen_dm_ts.clone();
    let messages_clone = Arc::clone(&app.messages);
    let message_notification_tx_clone = message_notification_tx.clone();
    let pending_notifications_clone = Arc::clone(&app.pending_notifications);
    let dropped_user_history_clone = Arc::clone(&app.dropped_user_history_order_ids);
    let (new_dm_tx, new_dm_rx) = tokio::sync::mpsc::unbounded_channel::<OrderDmSubscriptionCmd>();
    *dm_subscription_tx = new_dm_tx;
    set_dm_router_cmd_tx(dm_subscription_tx.clone()).map_err(|msg| msg.to_string())?;

    let dm_transport = app.transport;
    *message_listener_handle = spawn_supervised_trade_dm_listener(
        client_for_messages,
        mostro_pubkey,
        dm_transport,
        pool_for_messages,
        active_order_trade_indices_clone,
        order_last_seen_dm_ts_clone,
        messages_clone,
        message_notification_tx_clone,
        pending_notifications_clone,
        dropped_user_history_clone,
        new_dm_rx,
    );
    Ok(())
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
                let new_client = Client::builder()
                    .authenticator(SignerAuthenticator::new(new_identity_keys.clone()))
                    .build();
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
                    app.mode = UiMode::operation_result(OperationResult::Error(err));
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
                            request_fatal_restart(format!(
                                "Mostrix encountered an internal error (poisoned Mostro pubkey lock: {e}). Please restart the app."
                            ));
                            app.pending_key_reload = false;
                            app.fatal_exit_on_close = true;
                            app.mode = UiMode::operation_result(OperationResult::Error(
                                "Internal error. Please restart Mostrix.".to_string(),
                            ));
                            return;
                        }
                    }
                    app.currencies_filter = latest_settings.currencies_filter.clone();
                    hydrate_app_admin_keys_from_privkey(app, &latest_settings.admin_privkey);
                    clear_runtime_session_state(app);

                    order_fetch_task.abort();
                    dispute_fetch_task.abort();
                    let (o, d) = spawn_fetch_scheduler_loops(
                        client.clone(),
                        Arc::clone(current_mostro_pubkey),
                        Arc::clone(&orders),
                        Arc::clone(&disputes),
                        &latest_settings,
                        pool.clone(),
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
                    app.startup_popup_floor_ts = startup_dm_hydration.order_last_seen_dm_ts.clone();
                    let active_order_trade_indices_clone =
                        Arc::clone(&app.active_order_trade_indices);
                    let order_last_seen_dm_ts_clone =
                        startup_dm_hydration.order_last_seen_dm_ts.clone();
                    let messages_clone = Arc::clone(&app.messages);
                    let message_notification_tx_clone = message_notification_tx.clone();
                    let pending_notifications_clone = Arc::clone(&app.pending_notifications);
                    let dropped_user_history_clone =
                        Arc::clone(&app.dropped_user_history_order_ids);
                    let (new_dm_tx, new_dm_rx) =
                        tokio::sync::mpsc::unbounded_channel::<OrderDmSubscriptionCmd>();
                    *dm_subscription_tx = new_dm_tx;
                    let router_reg = set_dm_router_cmd_tx(dm_subscription_tx.clone());
                    if let Err(msg) = &router_reg {
                        log::error!("[dm_listener] {}", msg);
                    }
                    let dm_mostro_pubkey = new_mostro_pubkey;
                    let dm_transport =
                        dm_transport_for_mostro(client, new_mostro_pubkey, app, "Key reload").await;
                    *message_listener_handle = spawn_supervised_trade_dm_listener(
                        client_for_messages,
                        dm_mostro_pubkey,
                        dm_transport,
                        pool_for_messages,
                        active_order_trade_indices_clone,
                        order_last_seen_dm_ts_clone,
                        messages_clone,
                        message_notification_tx_clone,
                        pending_notifications_clone,
                        dropped_user_history_clone,
                        new_dm_rx,
                    );

                    app.backup_requires_restart = false;
                    app.pending_key_reload = false;
                    app.mode = match router_reg {
                        Ok(()) => UiMode::operation_result(OperationResult::Info(
                            "Keys reloaded. Active session state has been reset.".to_string(),
                        )),
                        Err(msg) => UiMode::operation_result(OperationResult::Error(format!(
                            "Keys reloaded but DM router registration failed ({msg}). Background trade messages still run; one-shot DM waits may fail until you restart the app."
                        ))),
                    };
                } else {
                    app.pending_key_reload = false;
                    app.mode = UiMode::operation_result(OperationResult::Error(format!(
                        "Invalid Mostro pubkey after key reload: {}",
                        latest_settings.mostro_pubkey
                    )));
                }
            }
            Err(e) => {
                app.pending_key_reload = false;
                app.mode = UiMode::operation_result(OperationResult::Error(format!(
                    "Invalid identity key after reload: {}",
                    e
                )));
            }
        },
        Err(e) => {
            app.pending_key_reload = false;
            app.mode = UiMode::operation_result(OperationResult::Error(format!(
                "Failed to load settings for key reload: {}",
                e
            )));
        }
    }
}

/// Join `{relay}: {error}` pairs from `unsubscribe_all` per-relay failures.
fn format_unsubscribe_failures<U, E>(failed: impl IntoIterator<Item = (U, E)>) -> String
where
    U: std::fmt::Display,
    E: std::fmt::Display,
{
    failed
        .into_iter()
        .map(|(url, err)| format!("{url}: {err}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Optional warn detail for `unsubscribe_all`. `Some` is logged; callers must still respawn.
fn unsubscribe_all_warn_detail<T, E: std::fmt::Display>(
    result: Result<&Output<T>, E>,
) -> Option<String> {
    match result {
        Ok(output) if output.failed.is_empty() => None,
        Ok(output) => Some(format!(
            "failed on relays: {}",
            format_unsubscribe_failures(output.failed.iter())
        )),
        Err(e) => Some(format!("failed: {e}")),
    }
}

/// Best-effort `unsubscribe_all` after fetch/DM tasks have been aborted.
///
/// Partial relay CLOSE failures are routine on connectivity flaps. Treating them as
/// fatal returned Err after abort and never respawned the tasks, leaving a
/// permanent blind window until process restart.
async fn unsubscribe_all_best_effort(client: &Client, log_context: &str) {
    let warn = match client.unsubscribe_all().await {
        Ok(output) => unsubscribe_all_warn_detail::<_, std::convert::Infallible>(Ok(&output)),
        Err(e) => unsubscribe_all_warn_detail::<(), _>(Err(e)),
    };
    if let Some(detail) = warn {
        log::warn!("{log_context}: unsubscribe_all {detail} (continuing)");
    }
}

/// Refresh order/dispute relay subscriptions and the trade DM listener from disk settings.
///
/// Lighter than [`apply_pending_key_reload`]: does not rotate identity keys or clear the Messages tab.
/// Used after Mostro pubkey or currency filter changes so live subscriptions match settings.
#[allow(clippy::too_many_arguments)]
pub async fn apply_pending_fetch_scheduler_reload(
    app: &mut AppState,
    client: &mut Client,
    mostro_pubkey: &mut PublicKey,
    current_mostro_pubkey: &Arc<Mutex<PublicKey>>,
    pool: &SqlitePool,
    orders: Arc<Mutex<Vec<SmallOrder>>>,
    disputes: Arc<Mutex<Vec<Dispute>>>,
    order_fetch_task: &mut JoinHandle<()>,
    dispute_fetch_task: &mut JoinHandle<()>,
    message_listener_handle: &mut JoinHandle<()>,
    message_notification_tx: &UnboundedSender<MessageNotification>,
    dm_subscription_tx: &mut UnboundedSender<OrderDmSubscriptionCmd>,
    settings_fallback: &Settings,
) -> Result<(), String> {
    let latest = match load_settings_from_disk() {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "Fetch scheduler reload: could not read settings from disk ({e}); using startup snapshot"
            );
            settings_fallback.clone()
        }
    };

    let new_mostro_pubkey = PublicKey::from_str(&latest.mostro_pubkey).map_err(|e| {
        format!(
            "Invalid Mostro pubkey in settings ({}): {e}",
            latest.mostro_pubkey
        )
    })?;

    message_listener_handle.abort();
    order_fetch_task.abort();
    dispute_fetch_task.abort();
    unsubscribe_all_best_effort(client, "Fetch scheduler reload").await;

    connect_client_safely(client)
        .await
        .map_err(|e| format!("Fetch scheduler reload: failed to reconnect Nostr client: {e}"))?;

    *mostro_pubkey = new_mostro_pubkey;
    match current_mostro_pubkey.lock() {
        Ok(mut active_pubkey) => {
            *active_pubkey = new_mostro_pubkey;
        }
        Err(e) => {
            request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned Mostro pubkey lock: {e}). Please restart the app."
            ));
            app.fatal_exit_on_close = true;
            app.mode = UiMode::operation_result(OperationResult::Error(
                "Internal error. Please restart Mostrix.".to_string(),
            ));
            return Err("Internal error. Please restart Mostrix.".to_string());
        }
    }

    app.currencies_filter = latest.currencies_filter.clone();
    hydrate_app_admin_keys_from_privkey(app, &latest.admin_privkey);

    let (o, d) = spawn_fetch_scheduler_loops(
        client.clone(),
        Arc::clone(current_mostro_pubkey),
        Arc::clone(&orders),
        Arc::clone(&disputes),
        &latest,
        pool.clone(),
    );
    *order_fetch_task = o;
    *dispute_fetch_task = d;

    let client_for_messages = client.clone();
    let pool_for_messages = pool.clone();
    let startup_dm_hydration = match hydrate_startup_active_order_dm_state(pool).await {
        Ok(h) => h,
        Err(e) => {
            log::warn!(
                "Fetch scheduler reload: failed to hydrate startup active order DM state: {e}"
            );
            StartupDmHydration::empty()
        }
    };
    if let Ok(mut indices) = app.active_order_trade_indices.lock() {
        *indices = startup_dm_hydration.active_order_trade_indices.clone();
    }
    app.startup_popup_floor_ts = startup_dm_hydration.order_last_seen_dm_ts.clone();
    let active_order_trade_indices_clone = Arc::clone(&app.active_order_trade_indices);
    let order_last_seen_dm_ts_clone = startup_dm_hydration.order_last_seen_dm_ts.clone();
    let messages_clone = Arc::clone(&app.messages);
    let message_notification_tx_clone = message_notification_tx.clone();
    let pending_notifications_clone = Arc::clone(&app.pending_notifications);
    let dropped_user_history_clone = Arc::clone(&app.dropped_user_history_order_ids);
    let (new_dm_tx, new_dm_rx) = tokio::sync::mpsc::unbounded_channel::<OrderDmSubscriptionCmd>();
    *dm_subscription_tx = new_dm_tx;
    let router_reg = set_dm_router_cmd_tx(dm_subscription_tx.clone());
    if let Err(msg) = &router_reg {
        log::error!("[dm_listener] {msg}");
    }
    let dm_mostro_pubkey = new_mostro_pubkey;
    let dm_transport =
        dm_transport_for_mostro(client, new_mostro_pubkey, app, "Fetch scheduler reload").await;
    *message_listener_handle = spawn_supervised_trade_dm_listener(
        client_for_messages,
        dm_mostro_pubkey,
        dm_transport,
        pool_for_messages,
        active_order_trade_indices_clone,
        order_last_seen_dm_ts_clone,
        messages_clone,
        message_notification_tx_clone,
        pending_notifications_clone,
        dropped_user_history_clone,
        new_dm_rx,
    );

    match router_reg {
        Ok(()) => Ok(()),
        Err(msg) => Err(format!(
            "Subscriptions restarted, but DM router registration failed ({msg}). Consider restarting Mostrix."
        )),
    }
}

/// Runs [`apply_pending_key_reload`] or [`apply_pending_fetch_scheduler_reload`] when the
/// corresponding [`AppState`] flag is set (key reload takes precedence).
#[allow(clippy::too_many_arguments)]
pub async fn apply_pending_runtime_reloads(
    app: &mut AppState,
    client: &mut Client,
    mostro_pubkey: &mut PublicKey,
    current_mostro_pubkey: &Arc<Mutex<PublicKey>>,
    pool: &SqlitePool,
    message_listener_handle: &mut JoinHandle<()>,
    message_notification_tx: &UnboundedSender<MessageNotification>,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    disputes: &Arc<Mutex<Vec<Dispute>>>,
    order_fetch_task: &mut JoinHandle<()>,
    dispute_fetch_task: &mut JoinHandle<()>,
    dm_subscription_tx: &mut UnboundedSender<OrderDmSubscriptionCmd>,
    settings_fallback: &Settings,
) {
    if app.pending_key_reload {
        app.pending_fetch_scheduler_reload = false;
        apply_pending_key_reload(
            app,
            client,
            mostro_pubkey,
            current_mostro_pubkey,
            pool,
            message_listener_handle,
            message_notification_tx,
            Arc::clone(orders),
            Arc::clone(disputes),
            order_fetch_task,
            dispute_fetch_task,
            dm_subscription_tx,
        )
        .await;
    } else if app.pending_fetch_scheduler_reload {
        match apply_pending_fetch_scheduler_reload(
            app,
            client,
            mostro_pubkey,
            current_mostro_pubkey,
            pool,
            Arc::clone(orders),
            Arc::clone(disputes),
            order_fetch_task,
            dispute_fetch_task,
            message_listener_handle,
            message_notification_tx,
            dm_subscription_tx,
            settings_fallback,
        )
        .await
        {
            Ok(()) => {
                app.pending_fetch_scheduler_reload = false;
            }
            Err(e) => {
                log::warn!("{e}");
                // Keep `pending_fetch_scheduler_reload` set so a later tick can retry after relays/connectivity recover.
            }
        }
    }
}

/// Reconnect runtime background tasks after connectivity returns.
///
/// Mirrors the `apply_pending_key_reload` flow (abort/respawn fetch loops and DM listener).
/// Identity keys are unchanged; [`Self::mostro_pubkey`] / [`Self::current_mostro_pubkey`] are
/// refreshed from `settings` so they match disk (e.g. if the Mostro instance pubkey changed).
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
    unsubscribe_all_best_effort(ctx.client, "Reconnect").await;

    connect_client_safely(ctx.client)
        .await
        .map_err(|e| format!("Reconnect: failed to connect Nostr client: {e}"))?;

    let new_mostro_pubkey = PublicKey::from_str(&ctx.settings.mostro_pubkey).map_err(|e| {
        format!(
            "Reconnect: invalid Mostro pubkey in settings ({}): {e}",
            ctx.settings.mostro_pubkey
        )
    })?;
    *ctx.mostro_pubkey = new_mostro_pubkey;
    match ctx.current_mostro_pubkey.lock() {
        Ok(mut active_pubkey) => {
            *active_pubkey = new_mostro_pubkey;
        }
        Err(e) => {
            request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned Mostro pubkey lock: {e}). Please restart the app."
            ));
            return Err("Internal error. Please restart Mostrix.".to_string());
        }
    }

    ctx.app.currencies_filter = ctx.settings.currencies_filter.clone();
    hydrate_app_admin_keys_from_privkey(ctx.app, &ctx.settings.admin_privkey);
    clear_runtime_tracking_state_preserve_messages(ctx.app);

    let (o, d) = spawn_fetch_scheduler_loops(
        ctx.client.clone(),
        Arc::clone(ctx.current_mostro_pubkey),
        Arc::clone(&ctx.orders),
        Arc::clone(&ctx.disputes),
        ctx.settings,
        ctx.pool.clone(),
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
    ctx.app.startup_popup_floor_ts = startup_dm_hydration.order_last_seen_dm_ts.clone();
    let active_order_trade_indices_clone = Arc::clone(&ctx.app.active_order_trade_indices);
    let order_last_seen_dm_ts_clone = startup_dm_hydration.order_last_seen_dm_ts.clone();
    let messages_clone = Arc::clone(&ctx.app.messages);
    let message_notification_tx_clone = ctx.message_notification_tx.clone();
    let pending_notifications_clone = Arc::clone(&ctx.app.pending_notifications);
    let dropped_user_history_clone = Arc::clone(&ctx.app.dropped_user_history_order_ids);
    let (new_dm_tx, new_dm_rx) = tokio::sync::mpsc::unbounded_channel::<OrderDmSubscriptionCmd>();
    *ctx.dm_subscription_tx = new_dm_tx;
    let router_reg = set_dm_router_cmd_tx(ctx.dm_subscription_tx.clone());
    if let Err(msg) = &router_reg {
        log::error!("[dm_listener] {}", msg);
    }
    let dm_mostro_pubkey = new_mostro_pubkey;
    let dm_transport =
        dm_transport_for_mostro(ctx.client, new_mostro_pubkey, ctx.app, "Reconnect").await;
    *ctx.message_listener_handle = spawn_supervised_trade_dm_listener(
        client_for_messages,
        dm_mostro_pubkey,
        dm_transport,
        pool_for_messages,
        active_order_trade_indices_clone,
        order_last_seen_dm_ts_clone,
        messages_clone,
        message_notification_tx_clone,
        pending_notifications_clone,
        dropped_user_history_clone,
        new_dm_rx,
    );

    match router_reg {
        Ok(()) => Ok(()),
        Err(msg) => Err(format!(
            "Reconnected, but DM router registration failed ({msg}). Consider restarting Mostrix."
        )),
    }
}

/// Abort and respawn the shared-key chat subscription router on a new/renewed client.
///
/// Needed after key reload (client replaced) or reconnect / fetch-scheduler reload
/// (`client.unsubscribe_all()` drops the chat subscription). Rebuilds the router with a fresh
/// command channel, re-registers the global sender, and re-emits the active track set
/// ([`track_startup_chats`], option B) so every chat resubscribes on the new client/session.
pub async fn respawn_chat_listener(
    app: &AppState,
    client: &Client,
    pool: &SqlitePool,
    chat_listener_handle: &mut JoinHandle<()>,
    chat_router_cmd_tx: &mut UnboundedSender<ChatRouterCmd>,
    admin_chat_updates_tx: &Sender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    user_order_chat_updates_tx: &Sender<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
) -> Result<(), String> {
    chat_listener_handle.abort();
    // Await the old task so it releases its subscription/state before the new one starts.
    let old = std::mem::replace(chat_listener_handle, tokio::spawn(async {}));
    let _ = old.await;

    let (new_tx, new_rx) = tokio::sync::mpsc::unbounded_channel::<ChatRouterCmd>();
    *chat_router_cmd_tx = new_tx;
    set_chat_router_cmd_tx(chat_router_cmd_tx.clone()).map_err(|msg| msg.to_string())?;

    let client_for_chat = client.clone();
    let admin_tx = admin_chat_updates_tx.clone();
    let user_tx = user_order_chat_updates_tx.clone();
    *chat_listener_handle =
        spawn_supervised_chat_listener(client_for_chat, admin_tx, user_tx, new_rx);

    // Re-emit the active track set so all chats resubscribe on the new client/session.
    track_startup_chats(pool, app).await;
    Ok(())
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
    pub admin_chat_updates_tx: Sender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    pub admin_chat_updates_rx: Receiver<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    pub user_order_chat_updates_tx: Sender<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
    pub user_order_chat_updates_rx: Receiver<Result<Vec<OrderChatUpdate>, anyhow::Error>>,
    pub save_attachment_tx: UnboundedSender<(String, ChatAttachment)>,
    pub save_attachment_rx: UnboundedReceiver<(String, ChatAttachment)>,
    pub send_order_attachment_tx: UnboundedSender<crate::util::SendOrderAttachmentJob>,
    pub send_order_attachment_rx: UnboundedReceiver<crate::util::SendOrderAttachmentJob>,
    pub mostro_info_tx: UnboundedSender<MostroInfoFetchResult>,
    pub mostro_info_rx: UnboundedReceiver<MostroInfoFetchResult>,
    pub dm_subscription_tx: UnboundedSender<OrderDmSubscriptionCmd>,
    pub dm_subscription_rx: UnboundedReceiver<OrderDmSubscriptionCmd>,
    pub chat_router_cmd_tx: UnboundedSender<crate::util::ChatRouterCmd>,
    pub chat_router_cmd_rx: UnboundedReceiver<crate::util::ChatRouterCmd>,
    pub network_status_tx: UnboundedSender<NetworkStatus>,
    pub network_status_rx: UnboundedReceiver<NetworkStatus>,
    pub fatal_error_tx: UnboundedSender<FatalNotify>,
    pub fatal_error_rx: UnboundedReceiver<FatalNotify>,
    pub ln_address_result_tx: UnboundedSender<LnAddressVerifyResult>,
    pub ln_address_result_rx: UnboundedReceiver<LnAddressVerifyResult>,
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
        tokio::sync::mpsc::channel::<Result<Vec<AdminChatUpdate>, anyhow::Error>>(
            crate::util::chat_security::CHAT_UPDATE_CAPACITY,
        );
    let (user_order_chat_updates_tx, user_order_chat_updates_rx) =
        tokio::sync::mpsc::channel::<Result<Vec<OrderChatUpdate>, anyhow::Error>>(
            crate::util::chat_security::CHAT_UPDATE_CAPACITY,
        );
    let (save_attachment_tx, save_attachment_rx) =
        tokio::sync::mpsc::unbounded_channel::<(String, ChatAttachment)>();
    let (send_order_attachment_tx, send_order_attachment_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::util::SendOrderAttachmentJob>();
    let (mostro_info_tx, mostro_info_rx) =
        tokio::sync::mpsc::unbounded_channel::<MostroInfoFetchResult>();
    let (dm_subscription_tx, dm_subscription_rx) =
        tokio::sync::mpsc::unbounded_channel::<OrderDmSubscriptionCmd>();
    let (chat_router_cmd_tx, chat_router_cmd_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::util::ChatRouterCmd>();
    let (network_status_tx, network_status_rx) =
        tokio::sync::mpsc::unbounded_channel::<NetworkStatus>();
    let (fatal_error_tx, fatal_error_rx) = tokio::sync::mpsc::unbounded_channel::<FatalNotify>();
    let (ln_address_result_tx, ln_address_result_rx) =
        tokio::sync::mpsc::unbounded_channel::<LnAddressVerifyResult>();

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
        user_order_chat_updates_tx,
        user_order_chat_updates_rx,
        save_attachment_tx,
        save_attachment_rx,
        send_order_attachment_tx,
        send_order_attachment_rx,
        mostro_info_tx,
        mostro_info_rx,
        dm_subscription_tx,
        dm_subscription_rx,
        chat_router_cmd_tx,
        chat_router_cmd_rx,
        network_status_tx,
        network_status_rx,
        fatal_error_tx,
        fatal_error_rx,
        ln_address_result_tx,
        ln_address_result_rx,
    }
}

pub fn spawn_send_new_order_task(ctx: &EnterKeyContext<'_>, form: FormState) {
    let pool = ctx.pool.clone();
    let client = ctx.client.clone();
    let order_result_tx = ctx.order_result_tx.clone();
    let dm_subscription_tx = ctx.dm_subscription_tx.clone();
    let fallback_mostro_pubkey = ctx.mostro_pubkey;
    let current_mostro_pubkey = Arc::clone(ctx.current_mostro_pubkey);
    let mostro_info = ctx.mostro_info.clone();
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
        let form_for_retry = form.clone();
        match crate::util::send_new_order(
            &pool,
            &client,
            mostro_pubkey,
            form,
            Some(&dm_subscription_tx),
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(result) => {
                let _ = order_result_tx.send(result);
            }
            Err(e) => {
                log::error!("Failed to send order: {}", e);
                let _ = order_result_tx.send(operation_result_for_mostro_command_error(
                    e,
                    PendingTradeIndexRetry::NewOrder {
                        form: form_for_retry,
                    },
                ));
            }
        }
    });
}

/// Verify LNURL-pay metadata (`tag: payRequest`), then persist trimmed address to `settings.toml`.
pub fn spawn_verify_and_save_ln_address_task(
    address: String,
    result_tx: UnboundedSender<LnAddressVerifyResult>,
) {
    tokio::spawn(async move {
        let trimmed = address.trim().to_string();
        if trimmed.is_empty() {
            let _ = result_tx.send(LnAddressVerifyResult::Err(
                "Lightning address cannot be empty".to_string(),
            ));
            return;
        }

        match crate::util::ln_address::ln_address_pay_request_reachable(&trimmed).await {
            Ok(()) => match load_settings_from_disk() {
                Ok(mut s) => {
                    s.ln_address = trimmed.clone();
                    match crate::settings::save_settings(&s) {
                        Ok(()) => {
                            log::info!("Lightning address saved after LNURL verification");
                            let _ = result_tx.send(LnAddressVerifyResult::Verified {
                                message: "Lightning address saved (LNURL endpoint verified)."
                                    .to_string(),
                            });
                        }
                        Err(e) => {
                            let _ = result_tx.send(LnAddressVerifyResult::Err(format!(
                                "Address verified but failed to save settings: {}",
                                e
                            )));
                        }
                    }
                }
                Err(e) => {
                    let _ = result_tx.send(LnAddressVerifyResult::Err(format!(
                        "Failed to load settings: {}",
                        e
                    )));
                }
            },
            Err(e) => {
                let _ = result_tx.send(LnAddressVerifyResult::Err(format!(
                    "Could not verify Lightning address: {}",
                    e
                )));
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_take_order_task(
    pool: SqlitePool,
    client: Client,
    mostro_pubkey: PublicKey,
    take_state: TakeOrderState,
    amount: Option<i64>,
    invoice: Option<String>,
    result_tx: UnboundedSender<OperationResult>,
    dm_subscription_tx: UnboundedSender<OrderDmSubscriptionCmd>,
    mostro_info: Option<crate::util::MostroInstanceInfo>,
) {
    tokio::spawn(async move {
        let retry = PendingTradeIndexRetry::TakeOrder {
            take_state: take_state.clone(),
            amount,
            invoice: invoice.clone(),
        };
        match crate::util::take_order(
            &pool,
            &client,
            mostro_pubkey,
            &take_state.order,
            amount,
            invoice,
            Some(&dm_subscription_tx),
            mostro_info.as_ref(),
        )
        .await
        {
            Ok(result) => {
                let _ = result_tx.send(result);
            }
            Err(e) => {
                log::error!("Failed to take order: {}", e);
                let _ = result_tx.send(operation_result_for_mostro_command_error(e, retry));
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
            Ok(crate::util::MostroInstanceInfoFetch::Found(info)) => MostroInfoFetchResult::Ok {
                info,
                message: "Mostro instance info refreshed from relays.".to_string(),
            },
            Ok(crate::util::MostroInstanceInfoFetch::NotFound) => MostroInfoFetchResult::NotFound {
                message: "No Mostro instance info event found for the current pubkey.".to_string(),
            },
            Ok(crate::util::MostroInstanceInfoFetch::Rejected { fetched }) => {
                MostroInfoFetchResult::Rejected {
                    message: format!(
                        "Rejected {fetched} unauthentic instance-info event(s); keeping cached settings."
                    ),
                }
            }
            Err(e) => MostroInfoFetchResult::FetchFailed {
                message: format!("Failed to refresh Mostro instance info: {}", e),
            },
        };
        let _ = tx.send(res);
    });
}

/// Refresh instance info after the configured Mostro pubkey changes.
///
/// Sends [`MostroInfoFetchResult`] to the main loop (UI toast + optional DM listener respawn).
pub fn spawn_refresh_mostro_info_task(
    client: Client,
    mostro_pubkey: PublicKey,
    tx: UnboundedSender<MostroInfoFetchResult>,
) {
    tokio::spawn(async move {
        let result = fetch_mostro_instance_info(&client, mostro_pubkey).await;
        let res = match result {
            Ok(crate::util::MostroInstanceInfoFetch::Found(info)) => MostroInfoFetchResult::Ok {
                info,
                message: "Mostro instance info updated.".to_string(),
            },
            Ok(crate::util::MostroInstanceInfoFetch::NotFound) => MostroInfoFetchResult::NotFound {
                message: "No Mostro instance info event found for the current pubkey.".to_string(),
            },
            Ok(crate::util::MostroInstanceInfoFetch::Rejected { fetched }) => {
                MostroInfoFetchResult::Rejected {
                    message: format!(
                        "Rejected {fetched} unauthentic instance-info event(s); keeping cached settings."
                    ),
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to refresh Mostro instance info after pubkey change: {}",
                    e
                );
                MostroInfoFetchResult::FetchFailed {
                    message: format!("Failed to refresh Mostro instance info: {}", e),
                }
            }
        };
        let _ = tx.send(res);
    });
}

/// Rotate the **user** identity (mnemonic + `nsec_privkey`). Admin key changes
/// must use **Change Admin Key** with the Mostro daemon nsec — do not pass
/// `is_user_mode = false` (that path is rejected).
pub fn spawn_key_rotation_task(
    pool: SqlitePool,
    is_user_mode: bool,
    mnemonic: String,
    derived_nsec: String,
    rotation_tx: UnboundedSender<Result<Zeroizing<String>, String>>,
) {
    tokio::spawn(async move {
        let rotation_result: Result<(), anyhow::Error> = async {
            if !is_user_mode {
                return Err(anyhow::anyhow!(
                    "Admin key rotation via Generate New Keys is disabled; use Change Admin Key"
                ));
            }

            let new_user = User::from_mnemonic(mnemonic.clone())?;
            let mut tx = pool.begin().await?;
            User::replace_all_in_tx(&new_user, &mut tx).await?;
            Order::delete_all_in_tx(&mut tx).await?;
            log::info!("User key rotation: cleared orders table (stale trade keys)");

            let mut s = crate::settings::load_settings_from_disk()?;
            s.nsec_privkey = derived_nsec.clone();
            let toml_string = toml::to_string_pretty(&s)
                .map_err(|e| anyhow::anyhow!("Failed to serialize settings: {}", e))?;

            let home_dir =
                dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
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
            fs::write(&tmp_path, toml_string)
                .map_err(|e| anyhow::anyhow!("Failed to write temporary settings file: {}", e))?;

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

/// Import a BIP-39 mnemonic: wipe local session state, insert the new user, and
/// atomically update `nsec_privkey` (clearing `ln_address`) in settings.toml.
///
/// On success the caller should set `pending_key_reload` and
/// `pending_import_restore` so the main loop reloads identity and runs restore.
pub fn spawn_import_seed_task(
    pool: SqlitePool,
    mnemonic: String,
    derived_nsec: String,
    import_tx: UnboundedSender<Result<Zeroizing<String>, String>>,
) {
    tokio::spawn(async move {
        match crate::util::session_wipe::import_seed_and_wipe_session(
            &pool,
            mnemonic.clone(),
            derived_nsec,
        )
        .await
        {
            Ok(()) => {
                let _ = import_tx.send(Ok(Zeroizing::new(mnemonic)));
            }
            Err(e) => {
                log::error!("Failed to import seed words: {}", e);
                let _ = import_tx.send(Err(format!("Failed to import seed: {}", e)));
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

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::prelude::RelayUrl;

    #[test]
    fn format_unsubscribe_failures_joins_url_and_error() {
        let failed = [
            ("wss://relay.one/", "connection closed"),
            ("wss://relay.two/", "timeout"),
        ];
        assert_eq!(
            format_unsubscribe_failures(failed),
            "wss://relay.one/: connection closed; wss://relay.two/: timeout"
        );
    }

    #[test]
    fn format_unsubscribe_failures_empty_is_empty_string() {
        let failed: [(&str, &str); 0] = [];
        assert_eq!(format_unsubscribe_failures(failed), "");
    }

    #[test]
    fn unsubscribe_all_success_has_no_warn_detail() {
        let output = Output::new(());
        assert_eq!(
            unsubscribe_all_warn_detail::<(), std::convert::Infallible>(Ok(&output)),
            None
        );
    }

    #[test]
    fn unsubscribe_all_partial_failure_is_warn_not_fatal() {
        let mut output = Output::new(());
        let url = RelayUrl::parse("wss://relay.example/").expect("valid relay url");
        output.failed.insert(url, "connection closed".to_string());
        let detail =
            unsubscribe_all_warn_detail::<(), std::convert::Infallible>(Ok(&output)).expect("warn");
        assert!(detail.contains("wss://relay.example"));
        assert!(detail.contains("connection closed"));
    }

    #[test]
    fn unsubscribe_all_err_is_warn_not_fatal() {
        let detail = unsubscribe_all_warn_detail::<(), _>(Err("pool closed")).expect("warn");
        assert_eq!(detail, "failed: pool closed");
    }
}
