pub mod db;
pub mod models;
pub mod settings;
pub mod shared;
pub mod startup;
pub mod ui;
pub mod util;

use crate::models::AdminDispute;
use crate::models::User;
use crate::settings::{init_settings, Settings};
use crate::ui::helpers::{
    admin_chat_keys_clone_for_role, apply_admin_chat_updates, apply_user_order_chat_updates,
    expire_attachment_toast, load_admin_disputes_at_startup, refresh_my_trades_maker_book_cache,
    sync_user_order_history_messages_from_db,
};
use crate::ui::key_handler::{
    apply_pending_runtime_reloads, create_app_channels, handle_key_event,
    handle_mouse_invoice_paste_fallback, reload_runtime_session_after_reconnect,
    respawn_chat_listener, respawn_trade_dm_listener, AppChannels, RuntimeReconnectContext,
};
use crate::ui::{LnAddressVerifyResult, MostroInfoFetchResult, OperationResult};
use crate::util::{
    blossom_servers_from_settings, handle_message_notification, handle_operation_result,
    install_background_panic_hook, order_utils::validate_range_amount, set_chat_router_cmd_tx,
    set_dm_router_cmd_tx, set_fatal_error_tx, set_order_result_tx, spawn_save_attachment,
    spawn_send_order_chat_attachment,
};
use crossterm::event::EventStream;
use mostro_core::prelude::*;

use std::str::FromStr;
use std::sync::Arc;

use chrono::Local;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{
    self,
    event::{Event, KeyEvent, MouseButton, MouseEventKind},
};
use fern::Dispatch;
use futures::StreamExt;
use nostr_sdk::prelude::*;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::stdout;
use std::sync::OnceLock;
use tokio::time::{interval, Duration};

/// Constructs (or copies) the configuration file and loads it.
pub static SETTINGS: OnceLock<Settings> = OnceLock::new();

/// Applies one [`OperationResult`] from the background task channel (save attachment, orders, etc.).
async fn apply_order_result(pool: &SqlitePool, app: &mut AppState, result: OperationResult) {
    let is_dispute_related = matches!(&result, OperationResult::Info(msg)
        if (msg.contains("Dispute") && msg.contains("taken successfully"))
            || msg.contains("Dispute finalized"));
    let resync_my_trades_from_db = matches!(&result, OperationResult::OrderHistoryDeleted { .. });
    let refresh_maker_book_cache = matches!(
        &result,
        OperationResult::MyTradesMakerBookChanged | OperationResult::Success(_)
    );

    if refresh_maker_book_cache && app.user_role == UserRole::User {
        refresh_my_trades_maker_book_cache(pool, app).await;
    }

    if !matches!(result, OperationResult::MyTradesMakerBookChanged) {
        handle_operation_result(result, app);
    }
    if resync_my_trades_from_db && app.user_role == UserRole::User {
        sync_user_order_history_messages_from_db(pool, app).await;
    }

    if is_dispute_related && app.user_role == UserRole::Admin {
        match AdminDispute::get_all(pool).await {
            Ok(all_disputes) => {
                app.admin_disputes_in_progress = all_disputes;
                app.selected_in_progress_idx = 0;
                log::info!(
                    "Refreshed admin disputes list: {} total disputes",
                    app.admin_disputes_in_progress.len()
                );
            }
            Err(e) => {
                log::warn!("Failed to refresh admin disputes: {}", e);
            }
        }
    }
}

/// Drains pending save-attachment jobs and spawns download tasks.
fn drain_save_attachment_queue(
    save_attachment_rx: &mut UnboundedReceiver<(String, ChatAttachment)>,
    order_result_tx: &UnboundedSender<OperationResult>,
) {
    while let Ok((dispute_id, attachment)) = save_attachment_rx.try_recv() {
        spawn_save_attachment(dispute_id, attachment, order_result_tx.clone());
    }
}

/// Drains pending send-attachment jobs (encrypt → Blossom → order chat DM).
fn drain_send_order_attachment_queue(
    send_attachment_rx: &mut UnboundedReceiver<crate::util::SendOrderAttachmentJob>,
    client: &Client,
    pool: &SqlitePool,
    settings: &Settings,
    mostro_info: &Option<crate::util::MostroInstanceInfo>,
    order_result_tx: &UnboundedSender<OperationResult>,
) {
    let servers = blossom_servers_from_settings(settings);
    while let Ok(job) = send_attachment_rx.try_recv() {
        spawn_send_order_chat_attachment(
            job,
            client.clone(),
            pool.clone(),
            servers.clone(),
            mostro_info.clone(),
            order_result_tx.clone(),
        );
    }
}

/// Drains completed background tasks so the UI can show popups without waiting for input.
async fn drain_order_result_queue(
    order_result_rx: &mut UnboundedReceiver<OperationResult>,
    pool: &SqlitePool,
    app: &mut AppState,
) {
    while let Ok(result) = order_result_rx.try_recv() {
        apply_order_result(pool, app, result).await;
    }
}

use crate::ui::{AdminMode, AdminTab, AppState, ChatAttachment, Tab, UiMode, UserRole};
use sqlx::SqlitePool;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// Initialize logger function
fn setup_logger(level: &str) -> Result<(), fern::InitError> {
    let log_level = match level.to_lowercase().as_str() {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "info" => log::LevelFilter::Info,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        _ => log::LevelFilter::Info, // Default to Info for invalid values
    };
    Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}] [{}] - {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(log_level)
        .chain(fern::log_file("app.log")?) // Guarda en logs/app.log
        .apply()?;
    Ok(())
}

fn apply_pasted_text_to_active_input(app: &mut AppState, pasted_text: &str) {
    // Handle paste for invoice input
    if let UiMode::NewMessageNotification(_, Action::AddInvoice, ref mut invoice_state) = app.mode {
        if invoice_state.focused {
            let filtered_text: String = pasted_text
                .chars()
                .filter(|c| !c.is_control() || *c == '\t')
                .collect();
            invoice_state.invoice_input.push_str(&filtered_text);
            invoice_state.just_pasted = true;
        }
    }

    // Handle paste for admin key input popups
    if let UiMode::AdminMode(AdminMode::AddSolver(ref mut add_solver_state)) = app.mode {
        if add_solver_state.key_input.focused {
            let filtered_text: String = pasted_text
                .chars()
                .filter(|c| !c.is_control() || *c == '\t')
                .collect();
            add_solver_state
                .key_input
                .key_input
                .push_str(&filtered_text);
            add_solver_state.key_input.just_pasted = true;
        }
    } else if let UiMode::AdminMode(AdminMode::SetupAdminKey(ref mut key_state)) = app.mode {
        if key_state.focused {
            let filtered_text: String = pasted_text
                .chars()
                .filter(|c| !c.is_control() || *c == '\t')
                .collect();
            key_state.key_input.push_str(&filtered_text);
            key_state.just_pasted = true;
        }
    } else if let UiMode::AddLnAddress(ref mut key_state) = app.mode {
        if key_state.focused {
            let filtered_text: String = pasted_text
                .chars()
                .filter(|c| !c.is_control() || *c == '\t')
                .collect();
            key_state.key_input.push_str(&filtered_text);
            key_state.just_pasted = true;
        }
    }

    // Handle paste for observer shared key input
    if matches!(app.active_tab, Tab::Admin(AdminTab::Observer)) {
        let filtered_text: String = pasted_text.chars().filter(|c| !c.is_control()).collect();
        app.observer_shared_key_input.push_str(&filtered_text);
    }
}

/// Draws the TUI interface with tabs and active content.
/// The "Orders" tab shows a table of pending orders and highlights the selected row.
use crate::ui::ui_draw;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Set rustls crypto provider once (required when both ring and aws-lc-rs are in the dependency tree)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("rustls default crypto provider");

    log::info!("MostriX started");
    let pool = db::init_db().await?;
    // Derive the user's `nsec` from the DB identity/index-0 key (mnemonic-backed),
    // so DB keys and settings stay in sync on first launch.
    let identity_keys = User::get_identity_keys(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("Error deriving identity keys: {}", e))?;
    let init = init_settings(Some(identity_keys))
        .map_err(|e| anyhow::anyhow!("Error loading settings: {}", e))?;
    let settings = init.settings;
    // Initialize logger
    setup_logger(&settings.log_level).expect("Can't initialize logger");
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(
        out,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let AppChannels {
        order_result_tx,
        mut order_result_rx,
        key_rotation_tx,
        mut key_rotation_rx,
        seed_words_tx,
        mut seed_words_rx,
        message_notification_tx,
        mut message_notification_rx,
        admin_chat_updates_tx,
        mut admin_chat_updates_rx,
        user_order_chat_updates_tx,
        mut user_order_chat_updates_rx,
        save_attachment_tx,
        mut save_attachment_rx,

        send_order_attachment_tx,
        mut send_order_attachment_rx,
        mostro_info_tx,
        mut mostro_info_rx,
        mut dm_subscription_tx,
        dm_subscription_rx,
        mut chat_router_cmd_tx,
        chat_router_cmd_rx,
        network_status_tx,
        mut network_status_rx,
        fatal_error_tx,
        mut fatal_error_rx,
        ln_address_result_tx,
        mut ln_address_result_rx,
    } = create_app_channels();

    // Set fatal error tx for the app channels
    set_fatal_error_tx(fatal_error_tx).map_err(|msg| anyhow::anyhow!(msg))?;
    install_background_panic_hook();

    // Set dm subscription tx for the app channels
    set_dm_router_cmd_tx(dm_subscription_tx.clone()).map_err(|msg| {
        anyhow::anyhow!("{msg}: DM router sender was not registered; restart the application.")
    })?;
    // Set chat router tx (shared-key order + dispute chat subscriptions).
    set_chat_router_cmd_tx(chat_router_cmd_tx.clone()).map_err(|msg| {
        anyhow::anyhow!("{msg}: chat router sender was not registered; restart the application.")
    })?;
    set_order_result_tx(order_result_tx.clone()).map_err(|msg| anyhow::anyhow!(msg))?;

    let configured_relays: Vec<String> = settings
        .relays
        .iter()
        .map(|relay| relay.trim())
        .filter(|relay| !relay.is_empty())
        .map(|relay| relay.to_owned())
        .collect();

    let user_role = UserRole::from_str(&settings.user_mode)?;

    let startup::StartupBootstrap {
        mut client,
        mut mostro_pubkey,
        current_mostro_pubkey,
        orders,
        disputes,
        mut order_task,
        mut dispute_task,
        mut app,
        mut message_listener_handle,
        mut chat_listener_handle,
        ..
    } = startup::run_startup_with_splash(
        &mut terminal,
        startup::PostTerminalStartupInput {
            pool: &pool,
            settings,
            did_generate_new_settings_file: init.did_generate_new_settings_file,
            user_role,
            configured_relays,
            dm_subscription_rx,
            chat_router_cmd_rx,
            mostro_info_tx: mostro_info_tx.clone(),
            network_status_tx,
            message_notification_tx: message_notification_tx.clone(),
            admin_chat_updates_tx: admin_chat_updates_tx.clone(),
            user_order_chat_updates_tx: user_order_chat_updates_tx.clone(),
        },
    )
    .await?;

    // Event handling: keyboard input and periodic UI refresh.
    let mut events = EventStream::new();
    let mut refresh_interval = interval(Duration::from_millis(150));

    loop {
        tokio::select! {
            fatal = fatal_error_rx.recv() => {
                if let Some(msg) = fatal {
                    // Stop background work and prompt the user to restart.
                    order_task.abort();
                    dispute_task.abort();
                    message_listener_handle.abort();
                    chat_listener_handle.abort();
                    app.fatal_exit_on_close = true;
                    app.mode = UiMode::operation_result(OperationResult::Error(msg));
                }
            }
            net = network_status_rx.recv() => {
                if let Some(status) = net {
                    match status {
                        crate::ui::NetworkStatus::Offline(msg) => {
                            app.offline_overlay_message = Some(format!(
                                "{msg}. Mostrix will retry connection every 5 seconds."
                            ));
                        }
                        crate::ui::NetworkStatus::Online(_msg) => {
                            // Attempt reconnect + full runtime reload (mirrors key reload path).
                            let latest_settings = crate::settings::load_settings_from_disk()
                                .unwrap_or_else(|_| settings.clone());
                            match reload_runtime_session_after_reconnect(
                                RuntimeReconnectContext {
                                    app: &mut app,
                                    client: &mut client,
                                    mostro_pubkey: &mut mostro_pubkey,
                                    current_mostro_pubkey: &current_mostro_pubkey,
                                    pool: &pool,
                                    message_listener_handle: &mut message_listener_handle,
                                    message_notification_tx: &message_notification_tx,
                                    orders: Arc::clone(&orders),
                                    disputes: Arc::clone(&disputes),
                                    order_fetch_task: &mut order_task,
                                    dispute_fetch_task: &mut dispute_task,
                                    dm_subscription_tx: &mut dm_subscription_tx,
                                    settings: &latest_settings,
                                },
                            )
                            .await
                            {
                                Ok(()) => {
                                    app.offline_overlay_message = None;
                                    // Reconnect ran `unsubscribe_all`; rebuild the chat subscription.
                                    if let Err(e) = respawn_chat_listener(
                                        &app,
                                        &client,
                                        &pool,
                                        &mut chat_listener_handle,
                                        &mut chat_router_cmd_tx,
                                        &admin_chat_updates_tx,
                                        &user_order_chat_updates_tx,
                                    )
                                    .await
                                    {
                                        log::error!(
                                            "Failed to respawn chat listener after reconnect: {e}"
                                        );
                                    }
                                }
                                Err(e) => {
                                    app.offline_overlay_message = Some(format!(
                                        "Reconnect failed: {e}. Retrying every 5 seconds."
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            result = order_result_rx.recv() => {
                if let Some(result) = result {
                    apply_order_result(&pool, &mut app, result).await;
                }
            }
            ln_address_verify = ln_address_result_rx.recv() => {
                if let Some(ln_res) = ln_address_verify {
                    let op = match ln_res {
                        LnAddressVerifyResult::Verified { message } => {
                            OperationResult::Info(message)
                        }
                        LnAddressVerifyResult::Err(e) => OperationResult::Error(e),
                    };
                    handle_operation_result(op, &mut app);
                }
            }
            key_rotation_result = key_rotation_rx.recv() => {
                if let Some(res) = key_rotation_result {
                    match res {
                        Ok(mnemonic) => {
                            app.backup_requires_restart = true;
                            app.mode = UiMode::BackupNewKeys(mnemonic);
                        }
                        Err(error_msg) => {
                            app.mode = UiMode::operation_result(OperationResult::Error(error_msg));
                        }
                    }
                }
            }
            seed_words_result = seed_words_rx.recv() => {
                if let Some(res) = seed_words_result {
                    match res {
                        Ok(mnemonic) => {
                            app.backup_requires_restart = false;
                            app.mode = UiMode::BackupNewKeys(mnemonic);
                        }
                        Err(error_msg) => {
                            app.mode = UiMode::operation_result(OperationResult::Error(error_msg));
                        }
                    }
                }
            }
            notification = message_notification_rx.recv() => {
                if let Some(notification) = notification {
                    handle_message_notification(notification, &mut app);
                }
            }
            admin_chat_result = admin_chat_updates_rx.recv() => {
                if let Some(result) = admin_chat_result {
                    match result {
                        Ok(updates) => {
                            let admin_pk = app.admin_keys.as_ref().map(|k| k.public_key());
                            if let Err(e) = apply_admin_chat_updates(
                                &mut app,
                                updates,
                                admin_pk.as_ref(),
                                &pool,
                            )
                            .await
                            {
                                log::warn!("Failed to apply admin chat updates: {}", e);
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to fetch admin chat updates: {}", e);
                        }
                    }
                }
            }
            user_order_chat_result = user_order_chat_updates_rx.recv() => {
                if let Some(result) = user_order_chat_result {
                    match result {
                        Ok(updates) => {
                            apply_user_order_chat_updates(&mut app, updates);
                        }
                        Err(e) => {
                            log::warn!("Failed to fetch user order chat updates: {}", e);
                        }
                    }
                }
            }
            mostro_info_result = mostro_info_rx.recv() => {
                if let Some(res) = mostro_info_result {
                    match res {
                        MostroInfoFetchResult::Ok { info, message } => {
                            let old_transport = app.transport;
                            app.set_mostro_info(*info);
                            if old_transport != app.transport {
                                log::warn!(
                                    "Mostro protocol transport changed {:?} -> {:?}; restarting DM listener",
                                    old_transport,
                                    app.transport
                                );
                                if let Err(e) = respawn_trade_dm_listener(
                                    &mut app,
                                    &client,
                                    mostro_pubkey,
                                    &pool,
                                    &mut message_listener_handle,
                                    &message_notification_tx,
                                    &mut dm_subscription_tx,
                                    "Mostro info refresh",
                                )
                                .await
                                {
                                    log::error!(
                                        "Failed to restart DM listener after transport change: {e}"
                                    );
                                }
                            }
                            app.mode = crate::ui::UiMode::operation_result(
                                crate::ui::OperationResult::Info(message),
                            );
                        }
                        MostroInfoFetchResult::Applied { info } => {
                            app.set_mostro_info(*info);
                        }
                        MostroInfoFetchResult::Err(e) => {
                            app.set_mostro_info(None);
                            app.mode = crate::ui::UiMode::operation_result(
                                crate::ui::OperationResult::Error(e),
                            );
                        }
                    }
                }
            }
            maybe_event = events.next() => {
                // Handle errors in event stream
                let event = match maybe_event {
                    Some(Ok(event)) => event,
                    Some(Err(e)) => {
                        log::error!("Error reading event: {}", e);
                        continue;
                    }
                    None => {
                        // Event stream ended, exit gracefully
                        break;
                    }
                };

                // Handle paste events (bracketed paste mode)
                if let Event::Paste(pasted_text) = event {
                    apply_pasted_text_to_active_input(&mut app, &pasted_text);
                    continue;
                }

                // Handle right-click paste when mouse capture is enabled.
                // Some terminals do not emit Event::Paste for mouse paste.
                if let Event::Mouse(mouse_event) = event {
                    if matches!(mouse_event.kind, MouseEventKind::Down(MouseButton::Right)) {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            if let Ok(text) = clipboard.get_text() {
                                apply_pasted_text_to_active_input(&mut app, &text);
                            }
                        }
                    }
                    continue;
                }

                if handle_mouse_invoice_paste_fallback(&event, &mut app) {
                    continue;
                }


                // Handle key events
                if let Event::Key(key_event @ KeyEvent { kind: crossterm::event::KeyEventKind::Press, .. }) = event {
                    let admin_chat_keys_owned = admin_chat_keys_clone_for_role(&app);
                    let admin_chat_keys = admin_chat_keys_owned.as_ref();
                    match handle_key_event(
                        key_event,
                        &mut app,
                        &orders,
                        &disputes,
                        &pool,
                        &client,
                        mostro_pubkey,
                        &current_mostro_pubkey,
                        &order_result_tx,
                        &ln_address_result_tx,
                        &key_rotation_tx,
                        &seed_words_tx,
                        &mostro_info_tx,
                        &validate_range_amount,
                        admin_chat_keys,
                        Some(&save_attachment_tx),
                        Some(&send_order_attachment_tx),
                        &dm_subscription_tx,
                    ) {
                        Some(true) => {
                            let needs_chat_respawn =
                                app.pending_key_reload || app.pending_fetch_scheduler_reload;
                            if needs_chat_respawn {
                                apply_pending_runtime_reloads(
                                    &mut app,
                                    &mut client,
                                    &mut mostro_pubkey,
                                    &current_mostro_pubkey,
                                    &pool,
                                    &mut message_listener_handle,
                                    &message_notification_tx,
                                    &orders,
                                    &disputes,
                                    &mut order_task,
                                    &mut dispute_task,
                                    &mut dm_subscription_tx,
                                    settings,
                                )
                                .await;
                                // Reloads replace the client / run `unsubscribe_all`, dropping the
                                // chat subscription; respawn once the reload actually completed.
                                if !app.pending_key_reload && !app.pending_fetch_scheduler_reload {
                                    if let Err(e) = respawn_chat_listener(
                                        &app,
                                        &client,
                                        &pool,
                                        &mut chat_listener_handle,
                                        &mut chat_router_cmd_tx,
                                        &admin_chat_updates_tx,
                                        &user_order_chat_updates_tx,
                                    )
                                    .await
                                    {
                                        log::error!(
                                            "Failed to respawn chat listener after reload: {e}"
                                        );
                                    }
                                }
                            }
                            if app.pending_admin_disputes_reload {
                                app.pending_admin_disputes_reload = false;
                                load_admin_disputes_at_startup(&pool, &mut app).await;
                            }
                        }
                        Some(false) => break,   // Exit requested (q key)
                        None => {
                            // Key not handled by handler - this shouldn't happen with current implementation
                            continue;
                        }
                    }
                }
            },
            _ = refresh_interval.tick() => {
                // Refresh the UI even if there is no input.
            }
        }

        // Ensure the selected index is valid when orders list changes.
        {
            let orders_len = match orders.lock() {
                Ok(g) => g.len(),
                Err(e) => {
                    let msg = format!(
                        "Mostrix encountered an internal error (poisoned orders lock: {e}). Please restart the app."
                    );
                    order_task.abort();
                    dispute_task.abort();
                    message_listener_handle.abort();
                    chat_listener_handle.abort();
                    app.fatal_exit_on_close = true;
                    app.mode = UiMode::operation_result(OperationResult::Error(msg));
                    0
                }
            };
            if orders_len > 0 && app.selected_order_idx >= orders_len {
                app.selected_order_idx = orders_len - 1;
            }
        }

        // Ensure the selected dispute index is valid when disputes list changes.
        // Only count "initiated" disputes since that's what we display
        {
            use mostro_core::prelude::*;
            use std::str::FromStr;
            let disputes_lock = match disputes.lock() {
                Ok(g) => g,
                Err(e) => {
                    let msg = format!(
                        "Mostrix encountered an internal error (poisoned disputes lock: {e}). Please restart the app."
                    );
                    order_task.abort();
                    dispute_task.abort();
                    message_listener_handle.abort();
                    chat_listener_handle.abort();
                    app.fatal_exit_on_close = true;
                    app.mode = UiMode::operation_result(OperationResult::Error(msg));
                    continue;
                }
            };
            let initiated_count = disputes_lock
                .iter()
                .filter(|d| {
                    DisputeStatus::from_str(d.status.as_str())
                        .map(|s| s == DisputeStatus::Initiated)
                        .unwrap_or(false)
                })
                .count();
            if initiated_count > 0 && app.selected_dispute_idx >= initiated_count {
                app.selected_dispute_idx = initiated_count.saturating_sub(1);
            } else if initiated_count == 0 {
                app.selected_dispute_idx = 0;
            }
        }

        // Process async completions before draw so popups appear without extra keypresses.
        let current_settings =
            crate::settings::load_settings_from_disk().unwrap_or_else(|_| settings.clone());
        drain_save_attachment_queue(&mut save_attachment_rx, &order_result_tx);
        drain_send_order_attachment_queue(
            &mut send_order_attachment_rx,
            &client,
            &pool,
            &current_settings,
            &app.mostro_info,
            &order_result_tx,
        );
        drain_order_result_queue(&mut order_result_rx, &pool, &mut app).await;

        // Expire transient UI timers/toasts before rendering.
        expire_attachment_toast(&mut app);

        // Status bar text - 3 separate lines
        // Reload settings from disk so newly added relays are reflected immediately.
        let current_settings =
            crate::settings::load_settings_from_disk().unwrap_or_else(|_| settings.clone());
        let relays_str = current_settings.relays.join(" - ");
        // Mostro instance currencies string
        let mostro_instance_currencies = match app.mostro_info.as_ref() {
            Some(info) if !info.fiat_currencies_accepted.is_empty() => {
                info.fiat_currencies_accepted.join(", ")
            }
            _ => "All (from Mostro instance)".to_string(),
        };
        // Currencies filters string
        let currencies_filter_str = match current_settings.currencies_filter.is_empty() {
            true => "All currencies are accepted".to_string(),
            false => current_settings.currencies_filter.join(", "),
        };
        // Mostro name (Lightning node alias) from instance info
        let mostro_alias = match app.mostro_info.as_ref() {
            Some(info) => info
                .lnd_node_alias
                .as_deref()
                .unwrap_or("unknown")
                .to_string(),
            None => "unknown".to_string(),
        };
        let status_lines = vec![
            format!(
                "🧌 Mostro name: {} | Pubkey: {}",
                mostro_alias, &current_settings.mostro_pubkey
            ),
            format!("🔗 Relays: {}", relays_str),
            format!(
                "💱 Currencies: {} - Filters: {}",
                mostro_instance_currencies, currencies_filter_str
            ),
        ];
        terminal.draw(|f| ui_draw(f, &mut app, &orders, &disputes, Some(&status_lines)))?;
    }

    // Restore terminal to its original state.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
