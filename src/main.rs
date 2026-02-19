pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::models::AdminDispute;
use crate::settings::{init_settings, Settings};
use crate::ui::helpers::{
    apply_admin_chat_updates, expire_attachment_toast, recover_admin_chat_from_files,
};
use crate::ui::key_handler::handle_key_event;
use crate::ui::{
    AdminChatLastSeen, AdminChatUpdate, ChatAttachment, ChatParty, MessageNotification, OrderResult,
};
use crate::util::{
    handle_message_notification, handle_order_result, listen_for_order_messages,
    order_utils::{spawn_admin_chat_fetch, start_fetch_scheduler, FetchSchedulerResult},
    spawn_save_attachment,
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

/// Constructs (or copies) the configuration file and loads it.
pub static SETTINGS: OnceLock<Settings> = OnceLock::new();

use crate::ui::{AdminMode, AppState, TakeOrderState, UiMode, UserRole};

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

/// Seed `app.admin_chat_last_seen` with last_seen timestamps per (dispute, party)
/// from the list of admin disputes (DB fields buyer_chat_last_seen / seller_chat_last_seen).
fn seed_admin_chat_last_seen(app: &mut AppState, _admin_chat_keys: &Keys) {
    let disputes = app.admin_disputes_in_progress.clone();

    for dispute in &disputes {
        if dispute.buyer_pubkey.is_some() {
            app.admin_chat_last_seen.insert(
                (dispute.dispute_id.clone(), ChatParty::Buyer),
                AdminChatLastSeen {
                    last_seen_timestamp: dispute.buyer_chat_last_seen,
                },
            );
        }
        if dispute.seller_pubkey.is_some() {
            app.admin_chat_last_seen.insert(
                (dispute.dispute_id.clone(), ChatParty::Seller),
                AdminChatLastSeen {
                    last_seen_timestamp: dispute.seller_chat_last_seen,
                },
            );
        }
    }
}

/// Validates the range amount input against min/max limits
fn validate_range_amount(take_state: &mut TakeOrderState) {
    if take_state.amount_input.is_empty() {
        take_state.validation_error = None;
        return;
    }

    let amount = match take_state.amount_input.parse::<f64>() {
        Ok(val) => val,
        Err(_) => {
            take_state.validation_error = Some("Invalid number format".to_string());
            return;
        }
    };

    let min = take_state.order.min_amount.unwrap_or(0) as f64;
    let max = take_state.order.max_amount.unwrap_or(0) as f64;

    if amount < min || amount > max {
        take_state.validation_error = Some(format!(
            "Amount must be between {} and {} {}",
            min, max, take_state.order.fiat_code
        ));
    } else {
        take_state.validation_error = None;
    }
}

/// Draws the TUI interface with tabs and active content.
/// The "Orders" tab shows a table of pending orders and highlights the selected row.
use crate::ui::ui_draw;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    log::info!("MostriX started");
    let settings = init_settings();
    let pool = db::init_db().await?;
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

    // Configure Nostr client.
    let my_keys = settings
        .nsec_privkey
        .parse::<Keys>()
        .map_err(|e| anyhow::anyhow!("Invalid NSEC privkey: {}", e))?;
    let client = Client::new(my_keys);
    // Add relays.
    for relay in &settings.relays {
        client.add_relay(relay).await?;
    }
    client.connect().await;

    let mostro_pubkey = PublicKey::from_str(&settings.mostro_pubkey)
        .map_err(|e| anyhow::anyhow!("Invalid Mostro pubkey: {}", e))?;

    // Start background tasks to fetch orders and disputes
    let FetchSchedulerResult { orders, disputes } =
        start_fetch_scheduler(client.clone(), mostro_pubkey);

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
    let mut admin_chat_interval = interval(Duration::from_secs(5));
    let user_role = &settings.user_mode;
    let mut app = AppState::new(UserRole::from_str(user_role)?);

    // Load all admin disputes from database if admin mode
    // (The filter toggle will show InProgress or Finalized based on user selection)
    if app.user_role == UserRole::Admin {
        match AdminDispute::get_all(&pool).await {
            Ok(all_disputes) => {
                app.admin_disputes_in_progress = all_disputes;

                // Pre-compute chat last seen timestamps for all disputes/parties so that the
                // background listener can fetch messages incrementally based on
                // last_seen timestamps stored in the database.
                if let Some(ref keys) = admin_keys {
                    seed_admin_chat_last_seen(&mut app, keys);
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

    // Channel to receive order results from async tasks
    let (order_result_tx, mut order_result_rx) =
        tokio::sync::mpsc::unbounded_channel::<OrderResult>();

    // Channel to receive message notifications
    let (message_notification_tx, mut message_notification_rx) =
        tokio::sync::mpsc::unbounded_channel::<MessageNotification>();

    // Channel to receive admin chat fetch results (fetch runs in spawned task)
    let (admin_chat_updates_tx, mut admin_chat_updates_rx) =
        tokio::sync::mpsc::unbounded_channel::<Result<Vec<AdminChatUpdate>, anyhow::Error>>();

    // Channel to trigger save of selected attachment (Ctrl+S in dispute chat)
    let (save_attachment_tx, mut save_attachment_rx) =
        tokio::sync::mpsc::unbounded_channel::<(String, ChatAttachment)>();

    // Admin chat keys (for trade-key send/fetch); only set when admin mode
    let admin_chat_keys: Option<Keys> = if app.user_role == UserRole::Admin {
        admin_keys
    } else {
        None
    };

    // Spawn background task to listen for messages on active orders
    let client_for_messages = client.clone();
    let pool_for_messages = pool.clone();
    let active_order_trade_indices_clone = Arc::clone(&app.active_order_trade_indices);
    let messages_clone = Arc::clone(&app.messages);
    let message_notification_tx_clone = message_notification_tx.clone();
    let pending_notifications_clone = Arc::clone(&app.pending_notifications);
    tokio::spawn(async move {
        listen_for_order_messages(
            client_for_messages,
            pool_for_messages,
            active_order_trade_indices_clone,
            messages_clone,
            message_notification_tx_clone,
            pending_notifications_clone,
        )
        .await;
    });

    loop {
        tokio::select! {
            result = order_result_rx.recv() => {
                if let Some(result) = result {
                    // Check if this is a dispute-related result before handling
                    let is_dispute_related = matches!(&result, OrderResult::Info(msg)
                        if (msg.contains("Dispute") && msg.contains("taken successfully"))
                        || (msg.contains("Dispute") && (msg.contains("settled") || msg.contains("canceled"))));

                    handle_order_result(result, &mut app);

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
                            // Filter out control characters (especially newlines) that could trigger unwanted actions
                            let filtered_text: String = pasted_text
                                .chars()
                                .filter(|c| !c.is_control() || *c == '\t')
                                .collect();
                            invoice_state.invoice_input.push_str(&filtered_text);
                            // Set flag to ignore Enter key immediately after paste
                            invoice_state.just_pasted = true;
                        }
                    }
                    // Handle paste for admin key input popups
                    if let UiMode::AdminMode(AdminMode::AddSolver(ref mut key_state))
                    | UiMode::AdminMode(AdminMode::SetupAdminKey(ref mut key_state)) = app.mode
                    {
                        if key_state.focused {
                            // Filter out control characters (especially newlines) that could trigger unwanted actions
                            let filtered_text: String = pasted_text
                                .chars()
                                .filter(|c| !c.is_control() || *c == '\t')
                                .collect();
                            key_state.key_input.push_str(&filtered_text);
                            // Set flag to ignore Enter key immediately after paste
                            key_state.just_pasted = true;
                        }
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
                        &order_result_tx,
                        &validate_range_amount,
                        admin_chat_keys.as_ref(),
                        Some(&save_attachment_tx),
                    ) {
                        Some(true) => {
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
            let orders_len = orders.lock().unwrap().len();
            if orders_len > 0 && app.selected_order_idx >= orders_len {
                app.selected_order_idx = orders_len - 1;
            }
        }

        // Ensure the selected dispute index is valid when disputes list changes.
        // Only count "initiated" disputes since that's what we display
        {
            use mostro_core::prelude::*;
            use std::str::FromStr;
            let disputes_lock = disputes.lock().unwrap();
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
        // Reload settings from disk so newly added relays and currencies are reflected immediately.
        let current_settings =
            crate::settings::load_settings_from_disk().unwrap_or_else(|_| settings.clone());
        let relays_str = current_settings.relays.join(" - ");
        let currencies_str = if current_settings.currencies.is_empty() {
            "All".to_string()
        } else {
            current_settings.currencies.join(", ")
        };
        let status_lines = vec![
            format!("🧌 Mostro Pubkey: {}", &current_settings.mostro_pubkey),
            format!("🔗 Relays: {}", relays_str),
            format!("💱 Currencies: {}", currencies_str),
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
