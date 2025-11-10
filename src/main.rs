pub mod adapter;
pub mod db;
pub mod models;
pub mod settings;
pub mod ui;
pub mod util;

use crate::models::{Order, User};
use crate::settings::{init_settings, Settings};
use crossterm::event::EventStream;
use mostro_core::prelude::{NOSTR_REPLACEABLE_EVENT_KIND, Status};


use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::vec;

use chrono::Local;
use chrono::{Duration as ChronoDuration, Utc};
use crossterm::{self, event::{Event, KeyEvent, KeyCode}};
use fern::Dispatch;
use futures::StreamExt;
// Removed dependency on NOSTR_REPLACEABLE_EVENT_KIND to avoid unresolved import
use nostr_sdk::prelude::RelayPoolNotification;
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

/// Parses a nostr_sdk::Event (expected to be a NIP-69 order event) into an Order.
/// It extracts tags:
/// - "d": order identifier (used as Code)
/// - "k": order kind ("buy" or "sell")
/// - "s": status (e.g. "pending")
/// - "amt": bitcoin amount (in satoshis)
/// - "f": fiat currency code
/// - "fa": fiat amount
/// - "pm": payment method
fn parse_order_event(event: nostr_sdk::Event) -> Option<Order> {
    let mut id = None;
    let mut kind = None;
    let mut status = None;
    let mut amount = None;
    let mut fiat_code = None;
    let mut fiat_amount = None;
    let mut payment_method = None;

    // Iterate over the tags using iter(), avoiding any errors with &event.tags.
    for tag in event.tags.iter() {
        let tag = tag.as_slice();
        match tag[0].as_str() {
            "d" => {
                if tag.len() > 1 {
                    id = Some(tag[1].clone());
                }
            }
            "k" => {
                if tag.len() > 1 {
                    kind = Some(tag[1].clone());
                }
            }
            "s" => {
                if tag.len() > 1 {
                    status = Some(tag[1].clone());
                }
            }
            "amt" => {
                if tag.len() > 1 {
                    amount = tag[1].parse::<i64>().ok();
                }
            }
            "f" => {
                if tag.len() > 1 {
                    fiat_code = Some(tag[1].clone());
                }
            }
            "fa" => {
                if tag.len() > 1 {
                    fiat_amount = tag[1].parse::<i64>().ok();
                }
            }
            "pm" => {
                if tag.len() > 1 {
                    let mut pm = vec![];
                    for tag in tag.iter().skip(1) {
                        pm.push(tag.clone());
                    }
                    payment_method = Some(pm.join(", "));
                }
            }
            _ => {}
        }
    }

    // Check that all required fields are present.
    if let (
        Some(kind),
        Some(id),
        Some(status),
        Some(amount),
        Some(fiat_code),
        Some(fiat_amount),
        Some(payment_method),
    ) = (
        kind,
        id,
        status,
        amount,
        fiat_code,
        fiat_amount,
        payment_method,
    ) {
        Some(Order {
            id: Some(id),
            kind: Some(kind),
            status: Some(status),
            amount,
            fiat_code,
            min_amount: None,
            max_amount: None,
            fiat_amount,
            payment_method,
            is_mine: false,
            premium: 0,
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            created_at: None,
            expires_at: None,
        })
    } else {
        None
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
    let orders: Arc<Mutex<Vec<Order>>> = Arc::new(Mutex::new(Vec::new()));

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

    // Fetch initial orders list using reused logic from mostro-cli
    // Filter for pending orders only, matching the original behavior
    if let Ok(fetched_orders) = adapter::fetch_orders(
        &client,
        mostro_pubkey,
        Some(Status::Pending),
        None, // No currency filter
        None, // No kind filter
    )
    .await
    {
        let mut lock = orders.lock().unwrap();
        lock.clear();
        for order in fetched_orders {
            if let Some(existing) = lock.iter_mut().find(|o| o.id == order.id) {
                *existing = order;
            } else {
                lock.push(order);
            }
        }
    }

    // Asynchronous task to handle incoming notifications.
    let orders_clone = Arc::clone(&orders);
    let mut notifications = client.notifications();
    tokio::spawn(async move {
        while let Ok(notification) = notifications.recv().await {
            if let RelayPoolNotification::Event { event, .. } = notification {
                if event.kind == Kind::Custom(NOSTR_REPLACEABLE_EVENT_KIND) {
                    if let Some(order) = parse_order_event((*event).clone()) {
                        let mut orders_lock = orders_clone.lock().unwrap();
                        // If status still pending we add it or update it
                        if order.status == Some("pending".to_string()) {
                            if let Some(existing) =
                                orders_lock.iter_mut().find(|o| o.id == order.id)
                            {
                                *existing = order;
                            } else {
                                orders_lock.push(order);
                            }
                        } else {
                            // If status is not pending we remove it from the list
                            orders_lock.retain(|o| o.id != order.id);
                        }
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
                                }
                            }
                            KeyCode::Right => {
                                if matches!(app.mode, UiMode::Normal) {
                                    if app.active_tab < 3 {
                                        app.active_tab += 1;
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
                                if matches!(app.mode, UiMode::Normal) {
                                    let mut form = FormState::default();
                                    form.kind = "buy".to_string();
                                    form.fiat_code = "USD".to_string();
                                    form.focused = 1; // start editing on Valuta
                                    app.mode = UiMode::CreatingOrder(form);
                                }
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
                                        if app.active_tab == 0 {
                                            // Apri form crea ordine da Orders
                                            let mut form = FormState::default();
                                            form.kind = "buy".to_string();
                                            form.fiat_code = "USD".to_string();
                                            form.focused = 1; // start editing on Valuta
                                            app.mode = UiMode::CreatingOrder(form);
                                        }
                                    }
                                    UiMode::CreatingOrder(form) => {
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

                                        app.mode = UiMode::Normal;
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
                            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                                if let UiMode::CreatingOrder(ref mut form) = app.mode {
                                    match code {
                                        KeyCode::Left => {
                                            if form.focused == 0 {
                                                form.kind = if form.kind.to_lowercase() == "buy" { "sell".into() } else { "buy".into() };
                                            } else if form.focused > 0 {
                                                form.focused -= 1;
                                            }
                                        }
                                        KeyCode::Right => {
                                            if form.focused == 0 {
                                                form.kind = if form.kind.to_lowercase() == "buy" { "sell".into() } else { "buy".into() };
                                            } else if form.focused < 3 {
                                                form.focused += 1;
                                            }
                                        }
                                        KeyCode::Char(' ') => {
                                            if form.focused == 0 {
                                                form.kind = if form.kind.to_lowercase() == "buy" { "sell".into() } else { "buy".into() };
                                            }
                                        }
                                        _ => {}
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
