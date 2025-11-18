pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::settings::{init_settings, Settings};
use crate::util::{fetch_events_list, send_new_order, Event as UtilEvent, ListKind};
use crossterm::event::EventStream;
use mostro_core::prelude::Status;

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use chrono::Local;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{
    self,
    event::{Event, KeyCode, KeyEvent},
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

use crate::ui::{AppState, FormState, Tab, TakeOrderState, UiMode};

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
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    // Shared state: orders are stored in memory.
    let orders: Arc<Mutex<Vec<mostro_core::prelude::SmallOrder>>> =
        Arc::new(Mutex::new(Vec::new()));

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

    loop {
        tokio::select! {
            result = order_result_rx.recv() => {
                if let Some(result) = result {
                    app.mode = UiMode::OrderResult(result);
                }
            }
            maybe_event = events.next() => {
                if let Some(Ok(Event::Key(KeyEvent { code, kind: crossterm::event::KeyEventKind::Press, .. }))) = maybe_event {
                    match code {
                            KeyCode::Left => {
                                match &mut app.mode {
                                    UiMode::Normal => {
                                        app.active_tab = app.active_tab.prev();
                                        // Exit form mode when leaving Create New Order tab
                                        if app.active_tab != Tab::CreateNewOrder {
                                            app.mode = UiMode::Normal;
                                        }
                                    }
                                    UiMode::TakingOrder(ref mut take_state) => {
                                        // Switch to YES button (left side)
                                        take_state.selected_button = true;
                                    }
                                    _ => {}
                                }
                            }
                            KeyCode::Right => {
                                match &mut app.mode {
                                    UiMode::Normal => {
                                        app.active_tab = app.active_tab.next();
                                        // Auto-initialize form when switching to Create New Order tab
                                        if app.active_tab == Tab::CreateNewOrder {
                                            let form = FormState {
                                                kind: "buy".to_string(),
                                                fiat_code: "USD".to_string(),
                                                amount: "0".to_string(),
                                                premium: "0".to_string(),
                                                expiration_days: "1".to_string(),
                                                focused: 1,
                                                ..Default::default()
                                            };
                                            app.mode = UiMode::CreatingOrder(form);
                                        }
                                    }
                                    UiMode::TakingOrder(ref mut take_state) => {
                                        // Switch to NO button (right side)
                                        take_state.selected_button = false;
                                    }
                                    _ => {}
                                }
                            }
                            KeyCode::Up => {
                                match &mut app.mode {
                                    UiMode::Normal => {
                                        if app.active_tab == Tab::Orders {
                                            let orders_len = orders.lock().unwrap().len();
                                            if orders_len > 0 && app.selected_order_idx > 0 {
                                                app.selected_order_idx -= 1;
                                            }
                                        }
                                    }
                                    UiMode::CreatingOrder(form) => {
                                        if form.focused > 0 {
                                            form.focused -= 1;
                                            // Skip field 4 if not using range (go from 5 to 3)
                                            if form.focused == 4 && !form.use_range {
                                                form.focused = 3;
                                            }
                                        }
                                    }
                                    UiMode::ConfirmingOrder(_) => {
                                        // No navigation in confirmation mode
                                    }
                                    UiMode::TakingOrder(_) => {
                                        // No navigation in take order mode
                                    }
                                    UiMode::WaitingForMostro(_) => {
                                        // No navigation in waiting mode
                                    }
                                    UiMode::OrderResult(_) => {
                                        // No navigation in result mode
                                    }
                                }
                            }
                            KeyCode::Down => {
                                match &mut app.mode {
                                    UiMode::Normal => {
                                        if app.active_tab == Tab::Orders {
                                            let orders_len = orders.lock().unwrap().len();
                                            if orders_len > 0 && app.selected_order_idx < orders_len.saturating_sub(1) {
                                                app.selected_order_idx += 1;
                                            }
                                        }
                                    }
                                    UiMode::CreatingOrder(form) => {
                                        if form.focused < 8 {
                                            form.focused += 1;
                                            // Skip field 4 if not using range (go from 3 to 5)
                                            if form.focused == 4 && !form.use_range {
                                                form.focused = 5;
                                            }
                                        }
                                    }
                                    UiMode::ConfirmingOrder(_) => {
                                        // No navigation in confirmation mode
                                    }
                                    UiMode::TakingOrder(_) => {
                                        // No navigation in take order mode
                                    }
                                    UiMode::WaitingForMostro(_) => {
                                        // No navigation in waiting mode
                                    }
                                    UiMode::OrderResult(_) => {
                                        // No navigation in result mode
                                    }
                                }
                            }
                            KeyCode::Tab => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    form.focused = (form.focused + 1) % 9;
                                    // Skip field 4 if not using range
                                    if form.focused == 4 && !form.use_range {
                                        form.focused = 5;
                                    }
                                }
                            }
                            KeyCode::BackTab => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    form.focused = if form.focused == 0 { 8 } else { form.focused - 1 };
                                    // Skip field 4 if not using range
                                    if form.focused == 4 && !form.use_range {
                                        form.focused = 3;
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                match &mut app.mode {
                                    UiMode::Normal => {
                                        // Show take order popup when Enter is pressed in Orders tab
                                        if app.active_tab == Tab::Orders {
                                            let orders_lock = orders.lock().unwrap();
                                            if let Some(order) = orders_lock.get(app.selected_order_idx) {
                                                let is_range_order = order.min_amount.is_some() || order.max_amount.is_some();
                                                let take_state = TakeOrderState {
                                                    order: order.clone(),
                                                    amount_input: String::new(),
                                                    is_range_order,
                                                    validation_error: None,
                                                    selected_button: true, // Default to YES
                                                };
                                                app.mode = UiMode::TakingOrder(take_state);
                                            }
                                        }
                                    }
                                    UiMode::CreatingOrder(form) => {
                                        // Show confirmation popup when Enter is pressed
                                        if app.active_tab == Tab::CreateNewOrder {
                                            app.mode = UiMode::ConfirmingOrder(form.clone());
                                        }
                                    }
                                    UiMode::ConfirmingOrder(_) => {
                                        // Enter acts as Yes in confirmation
                                        // This will be handled by 'y' key
                                    }
                                    UiMode::TakingOrder(ref take_state) => {
                                        // Enter confirms the selected button
                                        if take_state.selected_button {
                                            // YES selected - check validation and proceed
                                            if take_state.is_range_order
                                            && take_state.amount_input.is_empty() || take_state.validation_error.is_some() {
                                                // Can't proceed
                                                continue;
                                            }
                                            // TODO: Implement actual order taking logic here
                                            app.mode = UiMode::Normal;
                                        } else {
                                            // NO selected - cancel
                                            app.mode = UiMode::Normal;
                                        }
                                    }
                                    UiMode::WaitingForMostro(_) => {
                                        // No action while waiting
                                    }
                                    UiMode::OrderResult(_) => {
                                        // Close result popup, return to Orders tab
                                        app.mode = UiMode::Normal;
                                        app.active_tab = Tab::Orders;
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                match &mut app.mode {
                                    UiMode::CreatingOrder(_) => {
                                        app.mode = UiMode::Normal;
                                    }
                                    UiMode::ConfirmingOrder(form) => {
                                        // Cancel confirmation, go back to form
                                        app.mode = UiMode::CreatingOrder(form.clone());
                                    }
                                    UiMode::TakingOrder(_) => {
                                        // Cancel taking order, return to normal mode
                                        app.mode = UiMode::Normal;
                                    }
                                    UiMode::WaitingForMostro(_) => {
                                        // Can't cancel while waiting
                                    }
                                    UiMode::OrderResult(_) => {
                                        // Close result popup, return to Orders tab
                                        app.mode = UiMode::Normal;
                                        app.active_tab = Tab::Orders;
                                    }
                                    _ => break,
                                }
                            }
                            KeyCode::Char('q') => break,
                            KeyCode::Char(' ') => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    if form.focused == 0 {
                                        // Toggle buy/sell
                                        form.kind = if form.kind.to_lowercase() == "buy" {
                                            "sell".to_string()
                                        } else {
                                            "buy".to_string()
                                        };
                                    } else if form.focused == 3 {
                                        // Toggle range mode
                                        form.use_range = !form.use_range;
                                    }
                                }
                            }
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                if let UiMode::ConfirmingOrder(form) = &app.mode {
                                    // User confirmed, send the order
                                    let form_clone = form.clone();
                                    app.mode = UiMode::WaitingForMostro(form_clone.clone());

                                    // Spawn async task to send order
                                    let pool_clone = pool.clone();
                                    let client_clone = client.clone();
                                    let settings_clone = settings;
                                    let mostro_pubkey_clone = mostro_pubkey;
                                    let result_tx = order_result_tx.clone();

                                    tokio::spawn(async move {
                                        match send_new_order(&pool_clone, &client_clone, settings_clone, mostro_pubkey_clone, &form_clone).await {
                                            Ok(result) => {
                                                let _ = result_tx.send(result);
                                            }
                                            Err(e) => {
                                                log::error!("Failed to send order: {}", e);
                                                let _ = result_tx.send(crate::ui::OrderResult::Error(e.to_string()));
                                            }
                                        }
                                    });
                                } else if let UiMode::TakingOrder(ref take_state) = &app.mode {
                                    // User confirmed taking the order
                                    // Check validation first
                                    if take_state.is_range_order {
                                        if take_state.amount_input.is_empty() {
                                            // Can't proceed without amount
                                            continue;
                                        }
                                        if take_state.validation_error.is_some() {
                                            // Can't proceed with invalid amount
                                            continue;
                                        }
                                    }
                                    // TODO: Implement actual order taking logic here
                                    // For now, just close the popup
                                    app.mode = UiMode::Normal;
                                }
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                if let UiMode::ConfirmingOrder(form) = &app.mode {
                                    // User cancelled, go back to form
                                    app.mode = UiMode::CreatingOrder(form.clone());
                                } else if let UiMode::TakingOrder(_) = &app.mode {
                                    // User cancelled taking the order
                                    app.mode = UiMode::Normal;
                                }
                            }
                            KeyCode::Char(c) => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    if form.focused == 0 {
                                        // ignore typing on toggle field
                                    } else {
                                        let target = match form.focused {
                                            1 => &mut form.fiat_code,
                                            2 => &mut form.amount,
                                            3 => &mut form.fiat_amount,
                                            4 if form.use_range => &mut form.fiat_amount_max,
                                            5 => &mut form.payment_method,
                                            6 => &mut form.premium,
                                            7 => &mut form.invoice,
                                            8 => &mut form.expiration_days,
                                            _ => unreachable!(),
                                        };
                                        target.push(c);
                                    }
                                } else if let UiMode::TakingOrder(ref mut take_state) = app.mode {
                                    // Allow typing in the amount input field for range orders
                                    if take_state.is_range_order {
                                        // Only allow digits and decimal point
                                        if c.is_ascii_digit() || c == '.' {
                                            take_state.amount_input.push(c);
                                            // Validate after typing
                                            validate_range_amount(take_state);
                                        }
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    if form.focused == 0 {
                                        // ignore
                                    } else {
                                        let target = match form.focused {
                                            1 => &mut form.fiat_code,
                                            2 => &mut form.amount,
                                            3 => &mut form.fiat_amount,
                                            4 if form.use_range => &mut form.fiat_amount_max,
                                            5 => &mut form.payment_method,
                                            6 => &mut form.premium,
                                            7 => &mut form.invoice,
                                            8 => &mut form.expiration_days,
                                            _ => unreachable!(),
                                        };
                                        target.pop();
                                    }
                                } else if let UiMode::TakingOrder(ref mut take_state) = app.mode {
                                    // Allow backspace in the amount input field
                                    if take_state.is_range_order {
                                        take_state.amount_input.pop();
                                        // Validate after deletion
                                        validate_range_amount(take_state);
                                    }
                                }
                            }
                            _ => {}
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
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
