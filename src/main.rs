pub mod db;
pub mod models;
pub mod settings;
pub mod shared;
pub mod ui;
pub mod util;

use crate::models::AdminDispute;
use crate::models::User;
use crate::settings::{init_settings, Settings};
use crate::ui::helpers::{
    admin_chat_keys_clone_for_role, apply_admin_chat_updates, apply_user_order_chat_updates,
    expire_attachment_toast, hydrate_app_admin_keys_from_privkey, load_admin_disputes_at_startup,
    load_user_order_chats_at_startup, refresh_my_trades_maker_book_cache,
    sync_user_order_history_messages_from_db,
};
use crate::ui::key_handler::{
    apply_pending_runtime_reloads, create_app_channels, handle_key_event,
    handle_mouse_invoice_paste_fallback, reload_runtime_session_after_reconnect,
    spawn_refresh_mostro_info_task, AppChannels, RuntimeReconnectContext,
};
use crate::ui::network_status::spawn_network_status_monitor;
use crate::ui::{LnAddressVerifyResult, MostroInfoFetchResult, OperationResult};
use crate::util::{
    any_relay_reachable, catch_unwind_request_fatal_restart, connect_client_safely,
    handle_message_notification, handle_operation_result, hydrate_startup_active_order_dm_state,
    install_background_panic_hook, listen_for_order_messages,
    order_utils::{
        run_relay_order_db_reconcile_once, run_targeted_relay_order_db_reconcile_tick,
        spawn_admin_chat_fetch, spawn_user_order_chat_fetch, start_fetch_scheduler,
        validate_range_amount, FetchSchedulerResult,
    },
    set_dm_router_cmd_tx, set_fatal_error_tx, set_order_result_tx, spawn_save_attachment,
    StartupDmHydration,
};
use crossterm::event::EventStream;
use mostro_core::prelude::*;

use std::str::FromStr;
use std::sync::Arc;
use tokio::task::JoinHandle;

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
use zeroize::Zeroizing;

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
        mostro_info_tx,
        mut mostro_info_rx,
        mut dm_subscription_tx,
        dm_subscription_rx,
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
    set_order_result_tx(order_result_tx.clone()).map_err(|msg| anyhow::anyhow!(msg))?;

    // Configure Nostr client.
    let my_keys = settings
        .nsec_privkey
        .parse::<Keys>()
        .map_err(|e| anyhow::anyhow!("Invalid NSEC privkey: {}", e))?;
    let mut client = Client::new(my_keys);

    let configured_relays: Vec<String> = settings
        .relays
        .iter()
        .map(|relay| relay.trim())
        .filter(|relay| !relay.is_empty())
        .map(|relay| relay.to_owned())
        .collect();

    // Add relays.
    for relay in &configured_relays {
        client.add_relay(relay).await?;
    }
    let relays_reachable = any_relay_reachable(&configured_relays).await;
    if !relays_reachable {
        log::warn!("No configured relays reachable; nostr connect may fail");
    }
    if let Err(e) = connect_client_safely(&client).await {
        log::warn!("Failed to connect Nostr client at startup: {e}");
    }

    let mut mostro_pubkey = PublicKey::from_str(&settings.mostro_pubkey)
        .map_err(|e| anyhow::anyhow!("Invalid Mostro pubkey: {}", e))?;
    let current_mostro_pubkey = Arc::new(std::sync::Mutex::new(mostro_pubkey));

    // Start background tasks to fetch orders and disputes
    let FetchSchedulerResult {
        orders,
        disputes,
        mut order_task,
        mut dispute_task,
    } = start_fetch_scheduler(
        client.clone(),
        Arc::clone(&current_mostro_pubkey),
        settings,
        pool.clone(),
    );

    // Event handling: keyboard input and periodic UI refresh.
    let mut events = EventStream::new();
    let mut refresh_interval = interval(Duration::from_millis(150));
    let mut admin_chat_interval = interval(Duration::from_secs(2));
    let user_role = &settings.user_mode;
    let mut app = AppState::new(UserRole::from_str(user_role)?);
    // On first launch, ensure the new user can immediately back up the 12 words.
    // Do NOT force switching into Settings tab; show the overlay on the normal initial tab.
    if init.did_generate_new_settings_file {
        match User::get(&pool).await {
            Ok(user) => {
                app.backup_requires_restart = false;
                app.mode = UiMode::BackupNewKeys(Zeroizing::new(user.mnemonic));
            }
            Err(e) => {
                log::error!(
                    "First-run backup flow: failed to load generated user mnemonic: {}",
                    e
                );
                app.mode = UiMode::operation_result(OperationResult::Error(
                    "First-run setup completed, but mnemonic backup could not be loaded from the database. Please use Settings -> Generate New Keys and back up the new 12 words immediately.".to_string(),
                ));
            }
        }
    }
    // Seed currencies filter cache from settings so UI-side filtering does not
    // need to hit the filesystem on every render.
    app.currencies_filter = settings.currencies_filter.clone();
    hydrate_app_admin_keys_from_privkey(&mut app, &settings.admin_privkey);

    if !relays_reachable {
        app.offline_overlay_message = Some(
            "No internet / relays unreachable. Mostrix is retrying connection automatically."
                .to_string(),
        );
    }

    // Background offline monitor: emit status transitions every 5 seconds.
    spawn_network_status_monitor(configured_relays.clone(), network_status_tx.clone());

    // Initial Mostro instance info (same path as manual refresh; no startup toast).
    // Only attempt this when at least one relay is reachable, otherwise some
    // nostr client paths may panic on machines without network.
    if relays_reachable {
        spawn_refresh_mostro_info_task(
            client.clone(),
            mostro_pubkey,
            mostro_info_tx.clone(),
            false,
        );
    }

    // Load admin disputes at startup (only when role is admin)
    load_admin_disputes_at_startup(&pool, &mut app).await;
    if relays_reachable {
        if let Err(e) = run_relay_order_db_reconcile_once(&client, &pool, mostro_pubkey).await {
            log::warn!("Startup relay order DB reconcile failed: {}", e);
        }
        let startup_targeted_cursor = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        if let Err(e) = run_targeted_relay_order_db_reconcile_tick(
            &client,
            &pool,
            mostro_pubkey,
            &startup_targeted_cursor,
        )
        .await
        {
            log::warn!("Startup targeted relay order DB reconcile failed: {}", e);
        }
    }
    load_user_order_chats_at_startup(&client, &pool, &mut app).await;

    // Spawn background task to listen for messages on active orders
    let startup_dm_hydration = match hydrate_startup_active_order_dm_state(&pool).await {
        Ok(h) => h,
        Err(e) => {
            log::warn!(
                "Failed to hydrate startup active order DM state from DB: {}",
                e
            );
            StartupDmHydration::empty()
        }
    };
    if let Ok(mut indices) = app.active_order_trade_indices.lock() {
        *indices = startup_dm_hydration.active_order_trade_indices.clone();
    } else {
        log::warn!("Failed to seed startup active order map (poisoned lock)");
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
    let mut message_listener_handle: JoinHandle<()> = tokio::spawn(async move {
        catch_unwind_request_fatal_restart("trade DM listener", async move {
            listen_for_order_messages(
                client_for_messages,
                pool_for_messages,
                active_order_trade_indices_clone,
                order_last_seen_dm_ts_clone,
                messages_clone,
                message_notification_tx_clone,
                pending_notifications_clone,
                dropped_user_history_clone,
                dm_subscription_rx,
            )
            .await;
        })
        .await;
    });

    loop {
        tokio::select! {
            fatal = fatal_error_rx.recv() => {
                if let Some(msg) = fatal {
                    // Stop background work and prompt the user to restart.
                    order_task.abort();
                    dispute_task.abort();
                    message_listener_handle.abort();
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
                            app.mostro_info = *info;
                            app.mode = crate::ui::UiMode::operation_result(
                                crate::ui::OperationResult::Info(message),
                            );
                        }
                        MostroInfoFetchResult::Applied { info } => {
                            app.mostro_info = *info;
                        }
                        MostroInfoFetchResult::Err(e) => {
                            app.mostro_info = None;
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
                        &dm_subscription_tx,
                    ) {
                        Some(true) => {
                            if app.pending_key_reload || app.pending_fetch_scheduler_reload {
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
            _ = admin_chat_interval.tick() => {
                if app.user_role == UserRole::Admin {
                    if app.admin_keys.is_some() {
                        spawn_admin_chat_fetch(
                            client.clone(),
                            app.admin_disputes_in_progress.clone(),
                            app.admin_chat_last_seen.clone(),
                            admin_chat_updates_tx.clone(),
                        );
                    }
                } else if app.user_role == UserRole::User {
                    spawn_user_order_chat_fetch(
                        client.clone(),
                        pool.clone(),
                        app.order_chat_last_seen.clone(),
                        user_order_chat_updates_tx.clone(),
                    );
                }
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
        drain_save_attachment_queue(&mut save_attachment_rx, &order_result_tx);
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
