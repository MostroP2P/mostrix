//! Post-terminal boot: Nostr connect, schedulers, chat restore, DM listener.

use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use sqlx::SqlitePool;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::interval;
use zeroize::Zeroizing;

use crate::models::User;
use crate::settings::Settings;
use crate::ui::helpers::{
    hydrate_app_admin_keys_from_privkey, load_admin_disputes_at_startup,
    load_user_order_chats_at_startup,
};
use crate::ui::key_handler::spawn_refresh_mostro_info_task;
use crate::ui::network_status::spawn_network_status_monitor;
use crate::ui::startup_splash::{
    dot_count_from_elapsed, render_startup_splash, SPLASH_MIN_DISPLAY_MS, SPLASH_TICK_MS,
};
use crate::ui::{AppState, OperationResult, UiMode, UserRole};
use crate::util::{
    any_relay_reachable, catch_unwind_request_fatal_restart, connect_client_safely,
    hydrate_startup_active_order_dm_state, listen_for_order_messages,
    order_utils::{
        run_relay_order_db_reconcile_once, run_targeted_relay_order_db_reconcile_tick,
        start_fetch_scheduler, FetchSchedulerResult,
    },
    StartupDmHydration,
};

pub struct PostTerminalStartupInput<'a> {
    pub pool: &'a SqlitePool,
    pub settings: &'a Settings,
    pub did_generate_new_settings_file: bool,
    pub user_role: UserRole,
    pub configured_relays: Vec<String>,
    pub dm_subscription_rx:
        tokio::sync::mpsc::UnboundedReceiver<crate::util::OrderDmSubscriptionCmd>,
    pub mostro_info_tx: tokio::sync::mpsc::UnboundedSender<crate::ui::MostroInfoFetchResult>,
    pub network_status_tx: tokio::sync::mpsc::UnboundedSender<crate::ui::NetworkStatus>,
    pub message_notification_tx: tokio::sync::mpsc::UnboundedSender<crate::ui::MessageNotification>,
}

pub struct StartupBootstrap {
    pub client: Client,
    pub mostro_pubkey: PublicKey,
    pub current_mostro_pubkey: Arc<Mutex<PublicKey>>,
    pub orders: Arc<Mutex<Vec<SmallOrder>>>,
    pub disputes: Arc<Mutex<Vec<Dispute>>>,
    pub order_task: JoinHandle<()>,
    pub dispute_task: JoinHandle<()>,
    pub app: AppState,
    pub message_listener_handle: JoinHandle<()>,
    pub relays_reachable: bool,
}

fn set_startup_phase(phase_tx: &watch::Sender<String>, message: &str) {
    let _ = phase_tx.send(message.to_string());
}

pub async fn run_post_terminal_startup(
    input: PostTerminalStartupInput<'_>,
    phase_tx: &watch::Sender<String>,
) -> Result<StartupBootstrap> {
    set_startup_phase(phase_tx, "Starting…");

    let my_keys = input
        .settings
        .nsec_privkey
        .parse::<Keys>()
        .map_err(|e| anyhow::anyhow!("Invalid NSEC privkey: {}", e))?;
    let client = Client::new(my_keys);

    for relay in &input.configured_relays {
        client.add_relay(relay).await?;
    }

    set_startup_phase(phase_tx, "Connecting to relays…");
    let relays_reachable = any_relay_reachable(&input.configured_relays).await;
    if !relays_reachable {
        log::warn!("No configured relays reachable; nostr connect may fail");
    }
    if let Err(e) = connect_client_safely(&client).await {
        log::warn!("Failed to connect Nostr client at startup: {e}");
    }

    let mostro_pubkey = PublicKey::from_str(&input.settings.mostro_pubkey)
        .map_err(|e| anyhow::anyhow!("Invalid Mostro pubkey: {}", e))?;
    let current_mostro_pubkey = Arc::new(Mutex::new(mostro_pubkey));

    set_startup_phase(phase_tx, "Loading market data…");
    let FetchSchedulerResult {
        orders,
        disputes,
        order_task,
        dispute_task,
    } = start_fetch_scheduler(
        client.clone(),
        Arc::clone(&current_mostro_pubkey),
        input.settings,
        input.pool.clone(),
    );

    let mut app = AppState::new(input.user_role);
    if input.did_generate_new_settings_file {
        match User::get(input.pool).await {
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
    app.currencies_filter = input.settings.currencies_filter.clone();
    hydrate_app_admin_keys_from_privkey(&mut app, &input.settings.admin_privkey);

    if !relays_reachable {
        app.offline_overlay_message = Some(
            "No internet / relays unreachable. Mostrix is retrying connection automatically."
                .to_string(),
        );
    }

    spawn_network_status_monitor(
        input.configured_relays.clone(),
        input.network_status_tx.clone(),
    );

    if relays_reachable {
        spawn_refresh_mostro_info_task(
            client.clone(),
            mostro_pubkey,
            input.mostro_info_tx.clone(),
            false,
        );
    }

    set_startup_phase(phase_tx, "Restoring chats…");
    load_admin_disputes_at_startup(input.pool, &mut app).await;
    if relays_reachable {
        if let Err(e) = run_relay_order_db_reconcile_once(&client, input.pool, mostro_pubkey).await
        {
            log::warn!("Startup relay order DB reconcile failed: {}", e);
        }
        let startup_targeted_cursor = Arc::new(Mutex::new(0usize));
        if let Err(e) = run_targeted_relay_order_db_reconcile_tick(
            &client,
            input.pool,
            mostro_pubkey,
            &startup_targeted_cursor,
        )
        .await
        {
            log::warn!("Startup targeted relay order DB reconcile failed: {}", e);
        }
    }
    load_user_order_chats_at_startup(&client, input.pool, &mut app).await;

    set_startup_phase(phase_tx, "Almost ready…");
    let startup_dm_hydration = match hydrate_startup_active_order_dm_state(input.pool).await {
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
    let pool_for_messages = input.pool.clone();
    let active_order_trade_indices_clone = Arc::clone(&app.active_order_trade_indices);
    let order_last_seen_dm_ts_clone = startup_dm_hydration.order_last_seen_dm_ts.clone();
    let messages_clone = Arc::clone(&app.messages);
    let message_notification_tx_clone = input.message_notification_tx.clone();
    let pending_notifications_clone = Arc::clone(&app.pending_notifications);
    let dropped_user_history_clone = Arc::clone(&app.dropped_user_history_order_ids);
    let dm_subscription_rx = input.dm_subscription_rx;

    let message_listener_handle = tokio::spawn(async move {
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

    Ok(StartupBootstrap {
        client,
        mostro_pubkey,
        current_mostro_pubkey,
        orders,
        disputes,
        order_task,
        dispute_task,
        app,
        message_listener_handle,
        relays_reachable,
    })
}

/// Runs post-terminal init behind an animated splash unless `MOSTRIX_NO_SPLASH` is set.
pub async fn run_startup_with_splash(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    input: PostTerminalStartupInput<'_>,
) -> Result<StartupBootstrap> {
    let (phase_tx, phase_rx) = watch::channel(String::from("Starting…"));

    if std::env::var_os("MOSTRIX_NO_SPLASH").is_some() {
        return run_post_terminal_startup(input, &phase_tx).await;
    }

    let splash_started = Instant::now();
    let mut splash_tick = interval(Duration::from_millis(SPLASH_TICK_MS));
    splash_tick.tick().await;

    let mut init = Box::pin(run_post_terminal_startup(input, &phase_tx));

    let bootstrap = loop {
        tokio::select! {
            biased;
            res = &mut init => break res?,
            _ = splash_tick.tick() => {
                let dots = dot_count_from_elapsed(&splash_started);
                let phase = phase_rx.borrow().clone();
                terminal.draw(|f| render_startup_splash(f, dots, &phase))?;
            }
        }
    };

    let min_display = Duration::from_millis(SPLASH_MIN_DISPLAY_MS);
    while splash_started.elapsed() < min_display {
        tokio::select! {
            _ = splash_tick.tick() => {
                let dots = dot_count_from_elapsed(&splash_started);
                let phase = phase_rx.borrow().clone();
                terminal.draw(|f| render_startup_splash(f, dots, &phase))?;
            }
        }
    }

    Ok(bootstrap)
}
