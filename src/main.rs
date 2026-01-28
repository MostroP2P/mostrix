pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::models::AdminDispute;
use crate::settings::{init_settings, Settings};
use crate::ui::key_handler::handle_key_event;
use crate::ui::{
    AdminChatSharedKey, ChatParty, ChatSender, DisputeChatMessage, MessageNotification,
    OrderResult,
};
use crate::util::{
    fetch_chat_messages_for_shared_key, handle_message_notification, handle_order_result,
    listen_for_order_messages,
    order_utils::{start_fetch_scheduler, FetchSchedulerResult},
    derive_shared_chat_keys, SharedChatKeys,
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

/// Internal structure used by the admin chat fetch logic to describe which
/// disputes/parties should be polled for new messages.
struct AdminChatPlanEntry {
    dispute_id: String,
    party: ChatParty,
    shared_keys: SharedChatKeys,
    last_seen_timestamp: Option<u64>,
}

/// Result of polling for admin chat messages for a single dispute/party.
struct AdminChatUpdate {
    dispute_id: String,
    party: ChatParty,
    messages: Vec<(String, u64, PublicKey)>, // (content, timestamp, sender_pubkey)
}

/// Seed `app.admin_chat_shared_keys` from the list of admin disputes stored in
/// `AppState` and the admin chat keys. This prepares per-(dispute, party)
/// shared keys and last_seen timestamps so the background listener can start
/// fetching messages incrementally.
fn seed_admin_chat_shared_keys(app: &mut AppState, admin_chat_keys: &Keys) {
    // Clone disputes list locally to avoid borrowing `app` immutably and
    // mutably at the same time (Rust borrow checker).
    let disputes = app.admin_disputes_in_progress.clone();

    for dispute in &disputes {
        // Seed buyer party if buyer_pubkey exists
        if let Some(ref buyer_pk_str) = dispute.buyer_pubkey {
            match PublicKey::parse(buyer_pk_str) {
                Ok(buyer_pk) => match derive_shared_chat_keys(admin_chat_keys, &buyer_pk) {
                    Ok(shared) => {
                        app.admin_chat_shared_keys.insert(
                            (dispute.dispute_id.clone(), ChatParty::Buyer),
                            AdminChatSharedKey {
                                shared_keys: shared,
                                last_seen_timestamp: dispute
                                    .buyer_chat_last_seen
                                    .map(|ts| ts as u64),
                            },
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to derive buyer shared chat key for dispute {}: {}",
                            dispute.dispute_id,
                            e
                        );
                    }
                },
                Err(e) => {
                    log::warn!(
                        "Invalid buyer_pubkey for dispute {}: {}",
                        dispute.dispute_id,
                        e
                    );
                }
            }
        }

        // Seed seller party if seller_pubkey exists
        if let Some(ref seller_pk_str) = dispute.seller_pubkey {
            match PublicKey::parse(seller_pk_str) {
                Ok(seller_pk) => match derive_shared_chat_keys(admin_chat_keys, &seller_pk) {
                    Ok(shared) => {
                        app.admin_chat_shared_keys.insert(
                            (dispute.dispute_id.clone(), ChatParty::Seller),
                            AdminChatSharedKey {
                                shared_keys: shared,
                                last_seen_timestamp: dispute
                                    .seller_chat_last_seen
                                    .map(|ts| ts as u64),
                            },
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to derive seller shared chat key for dispute {}: {}",
                            dispute.dispute_id,
                            e
                        );
                    }
                },
                Err(e) => {
                    log::warn!(
                        "Invalid seller_pubkey for dispute {}: {}",
                        dispute.dispute_id,
                        e
                    );
                }
            }
        }
    }
}

/// Build a polling plan from the current UI state (cloned, read-only).
fn build_admin_chat_plan(app: &AppState) -> Vec<AdminChatPlanEntry> {
    app.admin_chat_shared_keys
        .iter()
        .map(|((dispute_id, party), shared)| AdminChatPlanEntry {
            dispute_id: dispute_id.clone(),
            party: *party,
            shared_keys: shared.shared_keys.clone(),
            last_seen_timestamp: shared.last_seen_timestamp,
        })
        .collect()
}

/// Fetch admin chat updates for all entries in the polling plan.
async fn fetch_admin_chat_updates(
    client: &Client,
    plan: &[AdminChatPlanEntry],
) -> Result<Vec<AdminChatUpdate>, anyhow::Error> {
    let mut updates = Vec::new();

    // Default window for initial fetches when no last_seen_timestamp is known.
    // This avoids scanning the entire history on relays.
    let now = Timestamp::now().as_u64();
    let seven_days_secs: u64 = 7 * 24 * 60 * 60;
    let default_since = now.saturating_sub(seven_days_secs);

    for entry in plan {
        // If we don't have a last_seen_timestamp yet, fall back to a 7-day window.
        let effective_since = entry.last_seen_timestamp.unwrap_or(default_since);

        let msgs = fetch_chat_messages_for_shared_key(
            client,
            &entry.shared_keys,
            Some(effective_since),
        )
        .await?;
        if !msgs.is_empty() {
            updates.push(AdminChatUpdate {
                dispute_id: entry.dispute_id.clone(),
                party: entry.party,
                messages: msgs,
            });
        }
    }

    Ok(updates)
}

/// Apply fetched admin chat updates back into the UI state and persist
/// last_seen timestamps to the database.
async fn apply_admin_chat_updates(
    app: &mut AppState,
    updates: Vec<AdminChatUpdate>,
    admin_chat_pubkey: Option<&PublicKey>,
    pool: &sqlx::SqlitePool,
) -> Result<(), anyhow::Error> {
    for update in updates {
        let dispute_key = update.dispute_id.clone();
        let party = update.party;

        // Get or create the chat history vector for this dispute
        let messages_vec = app
            .admin_dispute_chats
            .entry(dispute_key.clone())
            .or_default();

        // Track max timestamp to update last_seen
        let mut max_ts = app
            .admin_chat_shared_keys
            .get(&(dispute_key.clone(), party))
            .and_then(|s| s.last_seen_timestamp)
            .unwrap_or(0);

        for (content, ts, sender_pubkey) in update.messages {
            // Skip messages that we sent ourselves (admin identity), since we
            // already add them locally when sending.
            if let Some(admin_pk) = admin_chat_pubkey {
                if &sender_pubkey == admin_pk {
                    if ts > max_ts {
                        max_ts = ts;
                    }
                    continue;
                }
            }

            let sender = match party {
                ChatParty::Buyer => ChatSender::Buyer,
                ChatParty::Seller => ChatSender::Seller,
            };

            // Avoid duplicates: check if a message with same timestamp, sender and
            // content already exists.
            let is_duplicate = messages_vec.iter().any(|m: &DisputeChatMessage| {
                m.timestamp as u64 == ts && m.sender == sender && m.content == content
            });
            if is_duplicate {
                if ts > max_ts {
                    max_ts = ts;
                }
                continue;
            }

            messages_vec.push(DisputeChatMessage {
                sender,
                content: content.clone(),
                timestamp: ts as i64,
                target_party: None,
            });

            if ts > max_ts {
                max_ts = ts;
            }
        }

        // Update last_seen_timestamp for this dispute/party in memory
        if let Some(shared) = app
            .admin_chat_shared_keys
            .get_mut(&(dispute_key.clone(), party))
        {
            if max_ts > shared.last_seen_timestamp.unwrap_or(0) {
                shared.last_seen_timestamp = Some(max_ts);
            }
        }

        // Persist last_seen_timestamp to the database so we can resume incremental
        // fetching after restart without scanning the full history.
        if max_ts > 0 {
            let ts_i64 = max_ts as i64;
            match party {
                ChatParty::Buyer => {
                    AdminDispute::update_buyer_chat_last_seen_by_dispute_id(
                        pool,
                        &dispute_key,
                        ts_i64,
                    )
                    .await?
                }
                ChatParty::Seller => {
                    AdminDispute::update_seller_chat_last_seen_by_dispute_id(
                        pool,
                        &dispute_key,
                        ts_i64,
                    )
                    .await?
                }
            };
        }
    }

    Ok(())
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

    // Admin identity pubkey for classifying admin vs counterparty chat messages
    let admin_chat_pubkey: Option<PublicKey> = if !settings.admin_privkey.is_empty() {
        match Keys::parse(&settings.admin_privkey) {
            Ok(keys) => Some(keys.public_key()),
            Err(e) => {
                log::warn!("Invalid admin_privkey in settings: {}", e);
                None
            }
        }
    } else {
        None
    };

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

                // Pre-compute shared chat keys for all disputes/parties so that the
                // background listener can fetch messages incrementally based on
                // last_seen timestamps stored in the database.
                if !settings.admin_privkey.is_empty() {
                    match Keys::parse(&settings.admin_privkey) {
                        Ok(admin_chat_keys) => {
                            seed_admin_chat_shared_keys(&mut app, &admin_chat_keys);
                        }
                        Err(e) => {
                            log::warn!(
                                "Invalid admin_privkey in settings for admin chat: {}",
                                e
                            );
                        }
                    }
                }
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

                // Handle mouse events (double-click for invoice selection)
                // Terminal's native text selection will handle the actual selection
                // since we've removed borders from the invoice area for easier selection
                // if let Event::Mouse(_mouse_event) = event {
                //     // Mouse events are enabled for terminal-native text selection
                //     // The borderless invoice display makes it easier to select the invoice text
                //     continue;
                // }

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
                    ) {
                        Some(true) => continue, // Key was handled, continue loop
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
                // Periodically fetch admin chat messages for disputes/parties that
                // have established shared chat keys.
                let plan = build_admin_chat_plan(&app);
                if !plan.is_empty() {
                    match fetch_admin_chat_updates(&client, &plan).await {
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
