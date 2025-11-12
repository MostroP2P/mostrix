pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::models::User;
use crate::settings::{init_settings, Settings};
use crate::util::{
    fetch_events_list, parse_dm_events, send_dm, wait_for_dm, Event as UtilEvent, ListKind,
    FETCH_EVENTS_TIMEOUT,
};
use crossterm::event::EventStream;
use mostro_core::prelude::{Status, NOSTR_REPLACEABLE_EVENT_KIND};

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use chrono::Local;
use chrono::{Duration as ChronoDuration, Utc};
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
use tokio::time::{interval, Duration};

/// Constructs (or copies) the configuration file and loads it.
static SETTINGS: OnceLock<Settings> = OnceLock::new();

use crate::ui::{AppState, FormState, Tab, UiMode};

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

/// Send a new order to Mostro (similar to execute_new_order in mostro-cli)
async fn send_new_order(
    pool: &sqlx::sqlite::SqlitePool,
    client: &Client,
    _settings: &Settings,
    mostro_pubkey: PublicKey,
    form: &crate::ui::FormState,
) -> Result<crate::ui::OrderResult, anyhow::Error> {
    use std::collections::HashMap;

    // Parse form data
    let kind_str = if form.kind.trim().is_empty() {
        "buy".to_string()
    } else {
        form.kind.trim().to_lowercase()
    };
    let fiat_code = if form.fiat_code.trim().is_empty() {
        "USD".to_string()
    } else {
        form.fiat_code.trim().to_uppercase()
    };

    let amount: i64 = form.amount.trim().parse().unwrap_or(0);

    // Check if fiat currency is available on Yadio if amount is 0
    if amount == 0 {
        let api_req_string = "https://api.yadio.io/currencies".to_string();
        let fiat_list_check = reqwest::get(api_req_string)
            .await?
            .json::<HashMap<String, String>>()
            .await?
            .contains_key(&fiat_code);
        if !fiat_list_check {
            return Err(anyhow::anyhow!("{} is not present in the fiat market, please specify an amount with -a flag to fix the rate", fiat_code));
        }
    }

    let kind_checked = mostro_core::order::Kind::from_str(&kind_str)
        .map_err(|_| anyhow::anyhow!("Invalid order kind"))?;

    let expiration_days: i64 = form.expiration_days.trim().parse().unwrap_or(0);
    let expires_at = match expiration_days {
        0 => None,
        _ => {
            let now = chrono::Utc::now();
            let expires_at = now + chrono::Duration::days(expiration_days);
            Some(expires_at.timestamp())
        }
    };

    // Handle fiat amount (single or range)
    let (fiat_amount, min_amount, max_amount) =
        if form.use_range && !form.fiat_amount_max.trim().is_empty() {
            let min: i64 = form.fiat_amount.trim().parse().unwrap_or(0);
            let max: i64 = form.fiat_amount_max.trim().parse().unwrap_or(0);
            (0, Some(min), Some(max))
        } else {
            let fiat: i64 = form.fiat_amount.trim().parse().unwrap_or(0);
            (fiat, None, None)
        };

    let payment_method = form.payment_method.trim().to_string();
    let premium: i64 = form.premium.trim().parse().unwrap_or(0);
    let invoice = if form.invoice.trim().is_empty() {
        None
    } else {
        Some(form.invoice.trim().to_string())
    };

    // Get user and trade keys
    let user = User::get(pool).await?;
    let next_idx = user.last_trade_index.unwrap_or(0) + 1;
    let trade_keys = user.derive_trade_keys(next_idx)?;
    let _ = User::update_last_trade_index(pool, next_idx).await;

    // Create SmallOrder
    let small_order = mostro_core::prelude::SmallOrder::new(
        None,
        Some(kind_checked),
        Some(mostro_core::prelude::Status::Pending),
        amount,
        fiat_code.clone(),
        min_amount,
        max_amount,
        fiat_amount,
        payment_method.clone(),
        premium,
        None,
        None,
        invoice.clone(),
        Some(0),
        expires_at,
    );

    // Create message
    let request_id = uuid::Uuid::new_v4().as_u128() as u64;
    let order_content = mostro_core::prelude::Payload::Order(small_order);
    let message = mostro_core::prelude::Message::new_order(
        None,
        Some(request_id),
        Some(next_idx),
        mostro_core::prelude::Action::NewOrder,
        Some(order_content),
    );

    // Serialize message
    let message_json = message
        .as_json()
        .map_err(|_| anyhow::anyhow!("Failed to serialize message"))?;

    log::info!(
        "Sending new order via DM with trade index {} and request_id {}",
        next_idx,
        request_id
    );

    // Wait for Mostro response (subscribes first, then sends message to avoid missing messages)
    let recv_event = wait_for_dm(client, &trade_keys, FETCH_EVENTS_TIMEOUT, async {
        // Send DM inside the future passed to wait_for_dm
        send_dm(client, &trade_keys, &mostro_pubkey, message_json).await
    })
    .await?;

    // Parse DM events
    let messages = parse_dm_events(recv_event, &trade_keys, None).await;

    if let Some((response_message, _, _)) = messages.first() {
        let inner_message = response_message.get_inner_message_kind();
        match inner_message.request_id {
            Some(id) => {
                if request_id == id {
                    // Request ID matches, process the response
                    match inner_message.action {
                        mostro_core::prelude::Action::NewOrder => {
                            if let Some(mostro_core::prelude::Payload::Order(order)) =
                                &inner_message.payload
                            {
                                log::info!(
                                    "✅ Order created successfully! Order ID: {:?}",
                                    order.id
                                );

                                // Return success with order details
                                Ok(crate::ui::OrderResult::Success {
                                    order_id: order.id,
                                    kind: order.kind,
                                    amount: order.amount,
                                    fiat_code: order.fiat_code.clone(),
                                    fiat_amount: order.fiat_amount,
                                    min_amount: order.min_amount,
                                    max_amount: order.max_amount,
                                    payment_method: order.payment_method.clone(),
                                    premium: order.premium,
                                    status: order.status,
                                })
                            } else {
                                // Response without order details - return what we sent
                                Ok(crate::ui::OrderResult::Success {
                                    order_id: None,
                                    kind: Some(kind_checked),
                                    amount,
                                    fiat_code: fiat_code.clone(),
                                    fiat_amount,
                                    min_amount,
                                    max_amount,
                                    payment_method: payment_method.clone(),
                                    premium,
                                    status: Some(mostro_core::prelude::Status::Pending),
                                })
                            }
                        }
                        _ => {
                            log::warn!("Received unexpected action: {:?}", inner_message.action);
                            Err(anyhow::anyhow!(
                                "Unexpected action: {:?}",
                                inner_message.action
                            ))
                        }
                    }
                } else {
                    log::warn!(
                        "Received response with mismatched request_id. Expected: {}, Got: {}",
                        request_id,
                        id
                    );
                    Err(anyhow::anyhow!("Mismatched request_id"))
                }
            }
            None if inner_message.action == mostro_core::prelude::Action::RateReceived
                || inner_message.action == mostro_core::prelude::Action::NewOrder =>
            {
                // Some actions don't require request_id matching
                if let Some(mostro_core::prelude::Payload::Order(order)) = &inner_message.payload {
                    Ok(crate::ui::OrderResult::Success {
                        order_id: order.id,
                        kind: order.kind,
                        amount: order.amount,
                        fiat_code: order.fiat_code.clone(),
                        fiat_amount: order.fiat_amount,
                        min_amount: order.min_amount,
                        max_amount: order.max_amount,
                        payment_method: order.payment_method.clone(),
                        premium: order.premium,
                        status: order.status,
                    })
                } else {
                    Ok(crate::ui::OrderResult::Success {
                        order_id: None,
                        kind: Some(kind_checked),
                        amount,
                        fiat_code: fiat_code.clone(),
                        fiat_amount,
                        min_amount,
                        max_amount,
                        payment_method: payment_method.clone(),
                        premium,
                        status: Some(mostro_core::prelude::Status::Pending),
                    })
                }
            }
            None => {
                log::warn!(
                    "Received response with null request_id. Expected: {}",
                    request_id
                );
                Err(anyhow::anyhow!("Response with null request_id"))
            }
        }
    } else {
        log::error!("No response received from Mostro");
        Err(anyhow::anyhow!("No response received from Mostro"))
    }
}

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
    let my_keys = Keys::generate();
    let client = Client::new(my_keys);
    // Add q.
    for relay in &settings.relays {
        client.add_relay(relay).await?;
    }
    client.connect().await;

    let mostro_pubkey = PublicKey::from_str(&settings.mostro_pubkey)
        .map_err(|e| anyhow::anyhow!("Invalid Mostro pubkey: {}", e))?;

    // Calculate timestamp for events in the last 7 days.
    let since_time = Utc::now()
        .checked_sub_signed(ChronoDuration::days(7))
        .ok_or_else(|| anyhow::anyhow!("Failed to compute time"))?
        .timestamp() as u64;
    let timestamp = Timestamp::from(since_time);

    // Build the filter for NIP-69 (orders) events from Mostro.
    let mut filter = Filter::new()
        .author(mostro_pubkey)
        .limit(20)
        .since(timestamp)
        .custom_tag(SingleLetterTag::lowercase(Alphabet::Y), "mostro")
        .custom_tag(SingleLetterTag::lowercase(Alphabet::Z), "order")
        .kind(Kind::Custom(NOSTR_REPLACEABLE_EVENT_KIND));

    for c in &settings.currencies {
        filter = filter.custom_tag(SingleLetterTag::lowercase(Alphabet::F), c);
    }
    // Subscribe to the filter.
    client.subscribe(filter, None).await?;

    // Fetch initial orders list using fetch_events_list with ListKind::Orders
    // Filter for pending orders only, matching the original behavior
    if let Ok(fetched_events) = fetch_events_list(
        ListKind::Orders,
        Some(Status::Pending),
        None, // No currency filter
        None, // No kind filter
        &client,
        mostro_pubkey,
        None,
    )
    .await
    {
        let mut lock = orders.lock().unwrap();
        lock.clear();
        for event in fetched_events {
            if let UtilEvent::SmallOrder(order) = event {
                if let Some(existing) = lock.iter_mut().find(|o| o.id == order.id) {
                    *existing = order;
                } else {
                    lock.push(order);
                }
            }
        }
    }

    // Asynchronous task to handle incoming notifications.
    let orders_clone = Arc::clone(&orders);
    let client_clone = client.clone();
    let mostro_pubkey_clone = mostro_pubkey;
    tokio::spawn(async move {
        // Periodically refresh orders list
        let mut refresh_interval = tokio::time::interval(Duration::from_secs(30));
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
                                if matches!(app.mode, UiMode::Normal) {
                                    app.active_tab = app.active_tab.prev();
                                    // Exit form mode when leaving Create New Order tab
                                    if app.active_tab != Tab::CreateNewOrder {
                                        app.mode = UiMode::Normal;
                                    }
                                }
                            }
                            KeyCode::Right => {
                                if matches!(app.mode, UiMode::Normal) {
                                    app.active_tab = app.active_tab.next();
                                    // Auto-initialize form when switching to Create New Order tab
                                    if app.active_tab == Tab::CreateNewOrder {
                                        let form = FormState {
                                            kind: "buy".to_string(),
                                            fiat_code: "USD".to_string(),
                                            amount: "0".to_string(),
                                            premium: "0".to_string(),
                                            expiration_days: "0".to_string(),
                                            focused: 1,
                                            ..Default::default()
                                        };
                                        app.mode = UiMode::CreatingOrder(form);
                                    }
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
                                        if form.focused > 0 { form.focused -= 1; }
                                    }
                                    UiMode::ConfirmingOrder(_) => {
                                        // No navigation in confirmation mode
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
                                        if form.focused < 8 { form.focused += 1; }
                                    }
                                    UiMode::ConfirmingOrder(_) => {
                                        // No navigation in confirmation mode
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
                                }
                            }
                            KeyCode::BackTab => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    form.focused = if form.focused == 0 { 8 } else { form.focused - 1 };
                                }
                            }
                            KeyCode::Enter => {
                                match &mut app.mode {
                                    UiMode::Normal => {
                                        // Enter key will be used for taking orders later
                                        // No action for now
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
                                    UiMode::WaitingForMostro(_) => {
                                        // No action while waiting
                                    }
                                    UiMode::OrderResult(_) => {
                                        // No action in result mode
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
                                    UiMode::WaitingForMostro(_) => {
                                        // Can't cancel while waiting
                                    }
                                    UiMode::OrderResult(_) => {
                                        // Close result popup, return to normal mode
                                        app.mode = UiMode::Normal;
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
                                }
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                if let UiMode::ConfirmingOrder(form) = &app.mode {
                                    // User cancelled, go back to form
                                    app.mode = UiMode::CreatingOrder(form.clone());
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
