pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::models::AdminDispute;
use crate::models::User;
use crate::settings::{init_settings, Settings};
use crate::ui::helpers::{
    apply_admin_chat_updates, expire_attachment_toast, recover_admin_chat_from_files,
};
use crate::ui::key_handler::{
    apply_pending_key_reload, create_app_channels, handle_key_event,
    reload_runtime_session_after_reconnect, spawn_refresh_mostro_info_task, AppChannels,
    RuntimeReconnectContext,
};
use crate::ui::network_status::spawn_network_status_monitor;
use crate::ui::{MostroInfoFetchResult, OperationResult};
use crate::util::{
    any_relay_reachable, connect_client_safely, handle_message_notification,
    handle_operation_result, hydrate_startup_active_order_dm_state, install_background_panic_hook,
    listen_for_order_messages,
    order_utils::{
        spawn_admin_chat_fetch, start_fetch_scheduler, validate_range_amount, FetchSchedulerResult,
    },
    seed_admin_chat_last_seen, set_dm_router_cmd_tx, set_fatal_error_tx, spawn_save_attachment,
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
    event::{Event, KeyEvent},
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

use crate::ui::{AdminMode, AdminTab, AppState, Tab, UiMode, UserRole};

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
    } = create_app_channels();

    // Set fatal error tx for the app channels
    set_fatal_error_tx(fatal_error_tx).map_err(|msg| anyhow::anyhow!(msg))?;
    install_background_panic_hook();

    // Set dm subscription tx for the app channels
    set_dm_router_cmd_tx(dm_subscription_tx.clone()).map_err(|msg| {
        anyhow::anyhow!("{msg}: DM router sender was not registered; restart the application.")
    })?;

    // Configure Nostr client.
    let my_keys = settings
        .nsec_privkey
        .parse::<Keys>()
        .map_err(|e| anyhow::anyhow!("Invalid NSEC privkey: {}", e))?;
    let mut client = Client::new(my_keys);
    // Add relays.
    for relay in &settings.relays {
        let relay = relay.trim();
        if relay.is_empty() {
            continue;
        }
        client.add_relay(relay).await?;
    }
    let relays_reachable = any_relay_reachable(&settings.relays).await;
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
    } = start_fetch_scheduler(client.clone(), Arc::clone(&current_mostro_pubkey), settings);

    // Parse admin key once; reuse for pubkey (message classification), seeding, and chat fetch.
    let admin_keys: Option<Keys> = if settings.admin_privkey.is_empty() {
        None
    } else {
        match Keys::parse(&settings.admin_privkey) {
            Ok(keys) => Some(keys),
            Err(e) => {
                log::warn!("Invalid admin_privkey in settings: {}", e);
                None
            }
        }
    };
    let admin_chat_pubkey: Option<PublicKey> = admin_keys.as_ref().map(Keys::public_key);

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
                app.mode = UiMode::OperationResult(OperationResult::Error(
                    "First-run setup completed, but mnemonic backup could not be loaded from the database. Please use Settings -> Generate New Keys and back up the new 12 words immediately.".to_string(),
                ));
            }
        }
    }
    // Seed currencies filter cache from settings so UI-side filtering does not
    // need to hit the filesystem on every render.
    app.currencies_filter = settings.currencies_filter.clone();

    if !relays_reachable {
        app.offline_overlay_message = Some(
            "No internet / relays unreachable. Mostrix is retrying connection automatically."
                .to_string(),
        );
    }

    // Background offline monitor: emit status transitions every 5 seconds.
    spawn_network_status_monitor(settings.relays.clone(), network_status_tx.clone());

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

    // Load all admin disputes from database if admin mode
    // (The filter toggle will show InProgress or Finalized based on user selection)
    if app.user_role == UserRole::Admin {
        match AdminDispute::get_all(&pool).await {
            Ok(all_disputes) => {
                app.admin_disputes_in_progress = all_disputes;

                // Pre-compute chat last seen timestamps for all disputes/parties so that the
                // background listener can fetch messages incrementally based on
                // last_seen timestamps stored in the database.
                if admin_keys.is_some() {
                    seed_admin_chat_last_seen(&mut app);
                }

                recover_admin_chat_from_files(
                    &app.admin_disputes_in_progress,
                    &mut app.admin_dispute_chats,
                    &mut app.admin_chat_last_seen,
                );
            }
            Err(e) => {
                log::warn!("Failed to load admin disputes: {}", e);
            }
        }
    }

    // Admin chat keys (for trade-key send/fetch); only set when admin mode
    let admin_chat_keys: Option<Keys> = if app.user_role == UserRole::Admin {
        admin_keys
    } else {
        None
    };

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

    let client_for_messages = client.clone();
    let pool_for_messages = pool.clone();
    let active_order_trade_indices_clone = Arc::clone(&app.active_order_trade_indices);
    let order_last_seen_dm_ts_clone = startup_dm_hydration.order_last_seen_dm_ts.clone();
    let messages_clone = Arc::clone(&app.messages);
    let message_notification_tx_clone = message_notification_tx.clone();
    let pending_notifications_clone = Arc::clone(&app.pending_notifications);
    let mut message_listener_handle: JoinHandle<()> = tokio::spawn(async move {
        listen_for_order_messages(
            client_for_messages,
            pool_for_messages,
            active_order_trade_indices_clone,
            order_last_seen_dm_ts_clone,
            messages_clone,
            message_notification_tx_clone,
            pending_notifications_clone,
            dm_subscription_rx,
        )
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
                    app.mode = UiMode::OperationResult(OperationResult::Error(msg));
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
                    // Check if this is a dispute-related result before handling
                    let is_dispute_related = matches!(&result, OperationResult::Info(msg)
                        if (msg.contains("Dispute") && msg.contains("taken successfully"))
                        || (msg.contains("Dispute") && (msg.contains("settled") || msg.contains("canceled"))));

                    handle_operation_result(result, &mut app);

                    // If this is an Info result about taking or finalizing a dispute, refresh the disputes list
                    if is_dispute_related && app.user_role == UserRole::Admin {
                        match AdminDispute::get_all(&pool).await {
                            Ok(all_disputes) => {
                                app.admin_disputes_in_progress = all_disputes;
                                // Reset selected index to ensure it's within bounds after refresh
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
            }
            key_rotation_result = key_rotation_rx.recv() => {
                if let Some(res) = key_rotation_result {
                    match res {
                        Ok(mnemonic) => {
                            app.backup_requires_restart = true;
                            app.mode = UiMode::BackupNewKeys(mnemonic);
                        }
                        Err(error_msg) => {
                            app.mode = UiMode::OperationResult(OperationResult::Error(error_msg));
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
                            app.mode = UiMode::OperationResult(OperationResult::Error(error_msg));
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
                            if let Err(e) = apply_admin_chat_updates(
                                &mut app,
                                updates,
                                admin_chat_pubkey.as_ref(),
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
            mostro_info_result = mostro_info_rx.recv() => {
                if let Some(res) = mostro_info_result {
                    match res {
                        MostroInfoFetchResult::Ok { info, message } => {
                            app.mostro_info = *info;
                            app.mode = crate::ui::UiMode::OperationResult(
                                crate::ui::OperationResult::Info(message),
                            );
                        }
                        MostroInfoFetchResult::Applied { info } => {
                            app.mostro_info = *info;
                        }
                        MostroInfoFetchResult::Err(e) => {
                            app.mostro_info = None;
                            app.mode = crate::ui::UiMode::OperationResult(
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
                    if let UiMode::AdminMode(AdminMode::AddSolver(ref mut key_state))
                    | UiMode::AdminMode(AdminMode::SetupAdminKey(ref mut key_state)) = app.mode
                    {
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
                        let filtered_text: String = pasted_text
                            .chars()
                            .filter(|c| !c.is_control())
                            .collect();
                        app.observer_shared_key_input.push_str(&filtered_text);
                    }
                    continue;
                }


                // Handle key events
                if let Event::Key(key_event @ KeyEvent { kind: crossterm::event::KeyEventKind::Press, .. }) = event {
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
                        &key_rotation_tx,
                        &seed_words_tx,
                        &mostro_info_tx,
                        &validate_range_amount,
                        admin_chat_keys.as_ref(),
                        Some(&save_attachment_tx),
                        &dm_subscription_tx,
                    ) {
                        Some(true) => {
                            if app.pending_key_reload {
                                apply_pending_key_reload(
                                    &mut app,
                                    &mut client,
                                    &mut mostro_pubkey,
                                    &current_mostro_pubkey,
                                    &pool,
                                    &mut message_listener_handle,
                                    &message_notification_tx,
                                    Arc::clone(&orders),
                                    Arc::clone(&disputes),
                                    &mut order_task,
                                    &mut dispute_task,
                                    &mut dm_subscription_tx,
                                )
                                .await;
                            }
                            while let Ok((dispute_id, attachment)) = save_attachment_rx.try_recv() {
                                spawn_save_attachment(
                                    dispute_id,
                                    attachment,
                                    order_result_tx.clone(),
                                );
                            }
                            continue;
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
            _ = admin_chat_interval.tick(), if app.user_role == UserRole::Admin => {
                if admin_chat_keys.is_some() {
                    spawn_admin_chat_fetch(
                        client.clone(),
                        app.admin_disputes_in_progress.clone(),
                        app.admin_chat_last_seen.clone(),
                        admin_chat_updates_tx.clone(),
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
                    app.mode = UiMode::OperationResult(OperationResult::Error(msg));
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
                    app.mode = UiMode::OperationResult(OperationResult::Error(msg));
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
        let status_lines = vec![
            format!("🧌 Mostro Pubkey: {}", &current_settings.mostro_pubkey),
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
