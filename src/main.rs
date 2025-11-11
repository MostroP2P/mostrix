pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::models::User;
use crate::util::{fetch_events_list, Event as UtilEvent, ListKind};
use crate::settings::{init_settings, Settings};
use crossterm::event::EventStream;
use mostro_core::prelude::{NOSTR_REPLACEABLE_EVENT_KIND, Status};


use std::str::FromStr;
use std::sync::{Arc, Mutex};

use chrono::Local;
use chrono::{Duration as ChronoDuration, Utc};
use crossterm::{self, event::{Event, KeyEvent, KeyCode}};
use fern::Dispatch;
use futures::StreamExt;
use nostr_sdk::prelude::*;
use nostr_sdk::EventBuilder;
use std::sync::OnceLock;
use tokio::time::{interval, Duration};
use crossterm::execute;
use crossterm::terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::stdout;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nip44::v2::{ConversationKey, encrypt_to_bytes};
use nostr_sdk::Tag;

/// Constructs (or copies) the configuration file and loads it.
static SETTINGS: OnceLock<Settings> = OnceLock::new();

use crate::ui::{AppState, UiMode, FormState};

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
    let orders: Arc<Mutex<Vec<mostro_core::prelude::SmallOrder>>> = Arc::new(Mutex::new(Vec::new()));

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

    loop {
        tokio::select! {
            maybe_event = events.next() => {
                if let Some(Ok(event)) = maybe_event {
                    if let Event::Key(KeyEvent { code, kind: crossterm::event::KeyEventKind::Press, .. }) = event {
                        match code {
                            KeyCode::Left => {
                                if matches!(app.mode, UiMode::Normal) {
                                    if app.active_tab > 0 {
                                        app.active_tab -= 1;
                                    }
                                    // Exit form mode when leaving Create New Order tab
                                    if app.active_tab != 4 {
                                        app.mode = UiMode::Normal;
                                    }
                                }
                            }
                            KeyCode::Right => {
                                if matches!(app.mode, UiMode::Normal) {
                                    if app.active_tab < 4 {
                                        app.active_tab += 1;
                                    }
                                    // Auto-initialize form when switching to Create New Order tab
                                    if app.active_tab == 4 {
                                        let mut form = FormState::default();
                                        form.kind = "buy".to_string();
                                        form.fiat_code = "USD".to_string();
                                        form.focused = 1;
                                        app.mode = UiMode::CreatingOrder(form);
                                    }
                                }
                            }
                            KeyCode::Up => {
                                match &mut app.mode {
                                    UiMode::Normal => {
                                        if app.active_tab == 0 {
                                            let orders_len = orders.lock().unwrap().len();
                                            if orders_len > 0 && app.selected_order_idx > 0 {
                                                app.selected_order_idx -= 1;
                                            }
                                        }
                                    }
                                    UiMode::CreatingOrder(form) => {
                                        if form.focused > 0 { form.focused -= 1; }
                                    }
                                }
                            }
                            KeyCode::Down => {
                                match &mut app.mode {
                                    UiMode::Normal => {
                                        if app.active_tab == 0 {
                                            let orders_len = orders.lock().unwrap().len();
                                            if orders_len > 0 && app.selected_order_idx < orders_len.saturating_sub(1) {
                                                app.selected_order_idx += 1;
                                            }
                                        }
                                    }
                                    UiMode::CreatingOrder(form) => {
                                        if form.focused < 3 { form.focused += 1; }
                                    }
                                }
                            }
                            KeyCode::Char('n') => {
                                // 'n' key no longer opens form - use Create New Order tab instead
                            }
                            KeyCode::Tab => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    form.focused = (form.focused + 1) % 4;
                                }
                            }
                            KeyCode::BackTab => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    form.focused = form.focused.saturating_sub(1);
                                }
                            }
                            KeyCode::Enter => {
                                match &mut app.mode {
                                    UiMode::Normal => {
                                        // Enter key will be used for taking orders later
                                        // No action for now
                                    }
                                    UiMode::CreatingOrder(form) => {
                                        // Only submit if on Create New Order tab
                                        if app.active_tab == 4 {
                                            // Build and send order via DM using trade key
                                            let kind_str = if form.kind.trim().is_empty() { "buy".to_string() } else { form.kind.trim().to_lowercase() };
                                            let fiat = if form.fiat_code.trim().is_empty() { "USD".to_string() } else { form.fiat_code.trim().to_uppercase() };
                                            let fiat_amount: i64 = form.fiat_amount.trim().parse().unwrap_or(0);
                                            let pm_clean = form.payment_method.trim().to_string();

                                            if let Ok(user) = User::get(&pool).await {
                                                let next_idx = user.last_trade_index.unwrap_or(0) + 1;
                                                if let Ok(trade_keys) = user.derive_trade_keys(next_idx) {
                                                    let _ = User::update_last_trade_index(&pool, next_idx).await;

                                                    let kind_checked = mostro_core::order::Kind::from_str(&kind_str).unwrap_or(mostro_core::order::Kind::Buy);
                                                    let small_order = mostro_core::prelude::SmallOrder::new(
                                                        None,
                                                        Some(kind_checked),
                                                        Some(mostro_core::prelude::Status::Pending),
                                                        0,
                                                        fiat.clone(),
                                                        None,
                                                        None,
                                                        fiat_amount,
                                                        pm_clean.clone(),
                                                        0,
                                                        None,
                                                        None,
                                                        None,
                                                        Some(0),
                                                        None,
                                                    );
                                                    let payload = mostro_core::prelude::Payload::Order(small_order);
                                                    let message = mostro_core::prelude::Message::new_order(
                                                        None,
                                                        None,
                                                        Some(next_idx),
                                                        mostro_core::prelude::Action::NewOrder,
                                                        Some(payload),
                                                    );
                                                    if let Ok(json) = message.as_json() {
                                                        let trade_client = Client::new(trade_keys.clone());
                                                        for relay in &settings.relays { let _ = trade_client.add_relay(relay).await; }
                                                        trade_client.connect().await;
                                                        if let Ok(mostro_pk) = PublicKey::from_str(&settings.mostro_pubkey) {
                                                            // Create encrypted PDM event
                                                            let ck = ConversationKey::derive(trade_keys.secret_key(), &mostro_pk).map_err(|e| anyhow::anyhow!(e.to_string()))?;
                                                            let encrypted = encrypt_to_bytes(&ck, json.as_bytes()).map_err(|e| anyhow::anyhow!(e.to_string()))?;
                                                            let b64 = B64.encode(encrypted);
                                                            let event = EventBuilder::new(nostr_sdk::Kind::PrivateDirectMessage, b64)
                                                                .tag(Tag::public_key(mostro_pk))
                                                                .sign_with_keys(&trade_keys)?;
                                                            if let Err(e) = trade_client.send_event(&event).await {
                                                                log::error!("Failed to send DM: {}", e);
                                                            } else {
                                                                log::info!("New order sent via DM with trade index {}", next_idx);
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            // Reset form after submission
                                            let mut new_form = FormState::default();
                                            new_form.kind = "buy".to_string();
                                            new_form.fiat_code = "USD".to_string();
                                            new_form.focused = 1;
                                            app.mode = UiMode::CreatingOrder(new_form);
                                        }
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                if matches!(app.mode, UiMode::CreatingOrder(_)) {
                                    app.mode = UiMode::Normal;
                                } else {
                                    break
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
                                    }
                                }
                            }
                            KeyCode::Char(c) => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    if form.focused == 0 {
                                        // ignore typing on toggle field
                                    } else {
                                        let target = match form.focused { 1 => &mut form.fiat_code, 2 => &mut form.fiat_amount, _ => &mut form.payment_method };
                                        target.push(c);
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    if form.focused == 0 {
                                        // ignore
                                    } else {
                                        let target = match form.focused { 1 => &mut form.fiat_code, 2 => &mut form.fiat_amount, _ => &mut form.payment_method };
                                        target.pop();
                                    }
                                }
                            }
                            _ => {}
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
        let status_line = format!("🧌 pubkey - {}   🔗 {}", &settings.mostro_pubkey, relays_str);
        terminal.draw(|f| ui_draw(f, &app, &orders, Some(&status_line)))?;
    }

    // Restore terminal to its original state.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
