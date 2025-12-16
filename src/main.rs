pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::settings::{init_settings, Settings};
use crate::util::{fetch_events_list, listen_for_order_messages, Event as UtilEvent, ListKind};
use crossterm::event::EventStream;
use mostro_core::prelude::*;

use std::str::FromStr;
use std::sync::{Arc, Mutex};

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
use tokio::time::{interval, interval_at, Duration, Instant};

/// Constructs (or copies) the configuration file and loads it.
pub static SETTINGS: OnceLock<Settings> = OnceLock::new();

use crate::ui::{AppState, TakeOrderState, UiMode};

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

    // Shared state: orders are stored in memory.
    let orders: Arc<Mutex<Vec<SmallOrder>>> = Arc::new(Mutex::new(Vec::new()));

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

    // Asynchronous task to handle incoming notifications.
    let orders_clone = Arc::clone(&orders);
    let client_clone = client.clone();
    let mostro_pubkey_clone = mostro_pubkey;
    tokio::spawn(async move {
        // Periodically refresh orders list (immediate first fetch, then every 30 seconds)
        let mut refresh_interval = interval_at(Instant::now(), Duration::from_secs(10));
        loop {
            refresh_interval.tick().await;
            if let Ok(fetched_events) = fetch_events_list(
                ListKind::Orders,
                Some(Status::Pending),
                None,
                None,
                &client_clone,
                mostro_pubkey_clone,
                None,
            )
            .await
            {
                let mut orders_lock = orders_clone.lock().unwrap();
                orders_lock.clear();
                for event in fetched_events {
                    if let UtilEvent::SmallOrder(order) = event {
                        orders_lock.push(order);
                    }
                }
            }
        }
    });

    // Event handling: keyboard input and periodic UI refresh.
    let mut events = EventStream::new();
    let mut refresh_interval = interval(Duration::from_millis(500));
    let mut app = AppState::new();

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
    tokio::spawn(async move {
        listen_for_order_messages(
            client_for_messages,
            pool_for_messages,
            active_order_trade_indices_clone,
            messages_clone,
            message_notification_tx_clone,
        )
        .await;
    });

    loop {
        tokio::select! {
            result = order_result_rx.recv() => {
                if let Some(result) = result {
                    // Handle PaymentRequestRequired - show invoice popup for buy orders
                    if let crate::ui::OrderResult::PaymentRequestRequired { order, invoice, sat_amount, trade_index } = &result {
                        // Track trade_index
                        if let Some(order_id) = order.id {
                            let mut indices = app.active_order_trade_indices.lock().unwrap();
                            indices.insert(order_id, *trade_index);
                            log::info!("Tracking order {} with trade_index {}", order_id, trade_index);
                        }

                        // Create MessageNotification to show PayInvoice popup
                        let notification = crate::ui::MessageNotification {
                            order_id: order.id,
                            message_preview: "Payment Request".to_string(),
                            timestamp: chrono::Utc::now().timestamp() as u64,
                            action: mostro_core::prelude::Action::PayInvoice,
                            sat_amount: *sat_amount,
                            invoice: Some(invoice.clone()),
                        };

                        // Create invoice state (not focused since this is display-only)
                        let invoice_state = crate::ui::InvoiceInputState {
                            invoice_input: String::new(),
                            focused: false,
                            just_pasted: false,
                            copied_to_clipboard: false,
                        };
                        // Reuse pay invoice popup for buy orders when taking an order
                        app.mode = UiMode::NewMessageNotification(notification, mostro_core::prelude::Action::PayInvoice, invoice_state);
                        continue;
                    }

                    // Track trade_index for taken orders
                    if let crate::ui::OrderResult::Success { order_id, trade_index, .. } = &result {
                        if let (Some(order_id), Some(trade_index)) = (order_id, trade_index) {
                            let mut indices = app.active_order_trade_indices.lock().unwrap();
                            indices.insert(*order_id, *trade_index);
                            log::info!("Tracking order {} with trade_index {}", order_id, trade_index);
                        }
                    }

                    // Set appropriate result mode based on current state
                    match app.mode {
                        UiMode::WaitingTakeOrder(_) => {
                            app.mode = UiMode::OrderResult(result);
                        }
                        UiMode::WaitingAddInvoice => {
                            app.mode = UiMode::OrderResult(result);
                        }
                        UiMode::NewMessageNotification(_, _, _) => {
                            // If we have a notification, replace it with the result
                            app.mode = UiMode::OrderResult(result);
                        }
                        _ => {
                            app.mode = UiMode::OrderResult(result);
                        }
                    }
                }
            }
            notification = message_notification_rx.recv() => {
                if let Some(notification) = notification {
                    // Only show popup automatically for PayInvoice and AddInvoice,
                    // and only if we haven't already shown it for this message.
                    match notification.action {
                        Action::PayInvoice | Action::AddInvoice => {
                            let mut should_show_popup = false;

                            if let Some(order_id) = notification.order_id {
                                // Try to find the corresponding OrderMessage and check its popup flag.
                                let mut messages = app.messages.lock().unwrap();
                                if let Some(order_msg) = messages
                                    .iter_mut()
                                    .find(|m| m.order_id == Some(order_id))
                                {
                                    if !order_msg.auto_popup_shown {
                                        order_msg.auto_popup_shown = true;
                                        should_show_popup = true;
                                    }
                                } else {
                                    // No matching message found (e.g. race condition) - fall back to showing once.
                                    should_show_popup = true;
                                }
                            } else {
                                // No order_id associated, show once.
                                should_show_popup = true;
                            }

                            if should_show_popup {
                                let invoice_state = crate::ui::InvoiceInputState {
                                    invoice_input: String::new(),
                                    // Only focus input for AddInvoice, PayInvoice is display-only.
                                    focused: matches!(notification.action, Action::AddInvoice),
                                    just_pasted: false,
                                    copied_to_clipboard: false,
                                };
                                let action = notification.action.clone();
                                app.mode =
                                    UiMode::NewMessageNotification(notification, action, invoice_state);
                            } else {
                                // Popup already shown once; just bump pending counter.
                                let mut pending = app.pending_notifications.lock().unwrap();
                                *pending += 1;
                            }
                        }
                        _ => {
                            // For other actions, just increment pending notifications counter
                            let mut pending = app.pending_notifications.lock().unwrap();
                            *pending += 1;
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

        // Status bar text
        let relays_str = settings.relays.join(" - ");
        // let mostro_short = if settings.mostro_pubkey.len { format!("{}…", &settings.mostro_pubkey[..12]) } else { settings.mostro_pubkey.clone() };
        let status_line = format!(
            "🧌 pubkey - {}   🔗 {}",
            &settings.mostro_pubkey, relays_str
        );
        terminal.draw(|f| ui_draw(f, &app, &orders, Some(&status_line)))?;
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
