pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::settings::{init_settings, Settings};
use crate::util::{
    handle_message_notification, handle_order_result, listen_for_order_messages,
    order_utils::{start_fetch_scheduler, FetchSchedulerResult},
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

use crate::ui::{AppState, TakeOrderState, UiMode, UserRole};

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

    // Event handling: keyboard input and periodic UI refresh.
    let mut events = EventStream::new();
    let mut refresh_interval = interval(Duration::from_millis(500));
    let user_role = &settings.user_mode;
    let mut app = AppState::new(UserRole::from_str(user_role)?);

    // Channel to receive order results from async tasks
    let (order_result_tx, mut order_result_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::ui::OrderResult>();

    // Channel to receive message notifications
    let (message_notification_tx, mut message_notification_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::ui::MessageNotification>();

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
                    handle_order_result(result, &mut app);
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
                    match crate::ui::key_handler::handle_key_event(
                        key_event,
                        &mut app,
                        &orders,
                        &disputes,
                        &pool,
                        &client,
                        settings,
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
        }

        // Ensure the selected index is valid when orders list changes.
        {
            let orders_len = orders.lock().unwrap().len();
            if orders_len > 0 && app.selected_order_idx >= orders_len {
                app.selected_order_idx = orders_len - 1;
            }
        }

        // Ensure the selected dispute index is valid when disputes list changes.
        {
            let disputes_len = disputes.lock().unwrap().len();
            if disputes_len > 0 && app.selected_dispute_idx >= disputes_len {
                app.selected_dispute_idx = disputes_len - 1;
            }
        }

        // Status bar text
        let relays_str = settings.relays.join(" - ");
        // let mostro_short = if settings.mostro_pubkey.len { format!("{}…", &settings.mostro_pubkey[..12]) } else { settings.mostro_pubkey.clone() };
        let status_line = format!(
            "🧌 pubkey - {}   🔗 {}",
            &settings.mostro_pubkey, relays_str
        );
        terminal.draw(|f| ui_draw(f, &app, &orders, &disputes, Some(&status_line)))?;
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
