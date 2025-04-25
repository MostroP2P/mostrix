pub mod db;
pub mod models;
pub mod settings;

use crate::models::Order;
use crate::settings::{Settings, init_settings};

use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::io::stdout;

use chrono::{Utc, Duration as ChronoDuration};
use futures::StreamExt;
use crossterm::event::{Event as CEvent, EventStream, KeyCode, KeyEvent};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Style, Modifier, Color};
use ratatui::widgets::{Tabs, Block, Borders, Table, Row, Cell, Paragraph};
use ratatui::text::{Line, Span};
use ratatui::Terminal;
use tokio::time::{interval, Duration};
use fern::Dispatch;
use chrono::Local;
use nostr_sdk::prelude::*;
use nostr_sdk::prelude::RelayPoolNotification;
use mostro_core::NOSTR_REPLACEABLE_EVENT_KIND;
use std::sync::OnceLock;

/// Constructs (or copies) the configuration file and loads it.
static SETTINGS: OnceLock<Settings> = OnceLock::new();

// Official Mostro colors.
const PRIMARY_COLOR: Color = Color::Rgb(177, 204, 51);    // #b1cc33
const BACKGROUND_COLOR: Color = Color::Rgb(29, 33, 44);     // #1D212C

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
                    payment_method = Some(tag[1].clone());
                }
            }
            _ => {}
        }
    }

    // Check that all required fields are present.
    if kind.is_some()
        && id.is_some()
        && status.is_some()
        && amount.is_some()
        && fiat_code.is_some()
        && fiat_amount.is_some()
        && payment_method.is_some()
    {
        Some(Order {
            id,
            kind,
            status,
            amount: amount.unwrap(),
            fiat_code: fiat_code.unwrap(),
            min_amount: None,
            max_amount: None,
            fiat_amount: fiat_amount.unwrap(),
            payment_method: payment_method.unwrap(),
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
fn ui_draw(
    f: &mut ratatui::Frame,
    active_tab: usize,
    orders: &Arc<Mutex<Vec<Order>>>,
    selected_order_idx: usize,
) {
    // Create layout: one row for tabs and the rest for content.
    let chunks = Layout::new(
        Direction::Vertical,
        &[Constraint::Length(3), Constraint::Min(0)]
    )
    .split(f.area());

    // Define tab titles.
    let tab_titles = ["Orders", "My Trades", "Messages", "Settings"]
        .iter()
        .map(|t| Line::from(*t))
        .collect::<Vec<Line>>();
    let tabs = Tabs::new(tab_titles)
        .select(active_tab)
        .block(Block::default().borders(Borders::ALL).style(Style::default().bg(BACKGROUND_COLOR)))
        .highlight_style(Style::default().fg(PRIMARY_COLOR).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[0]);

    let content_area = chunks[1];
    if active_tab == 0 {
        // "Orders" tab: show table with pending orders.
        let header_cells = ["Kind", "Sats Amount", "Fiat", "Fiat Amount", "Payment Method"]
            .iter()
            .map(|h| Cell::from(*h))
            .collect::<Vec<Cell>>();
        let header = Row::new(header_cells)
            .style(Style::default().add_modifier(Modifier::BOLD));

        let orders_lock = orders.lock().unwrap();
        let rows: Vec<Row> = orders_lock.iter().enumerate().map(|(i, order)| {
            let kind = order.kind.clone().unwrap_or_default();
            let fiat_code = order.fiat_code.clone();
            let amount = if order.amount == 0 {
                "M/P".to_string()
            } else {
                order.amount.to_string()
            };
            let fiat_amount = order.fiat_amount.to_string();
            let payment_method = order.payment_method.clone();
            let row = Row::new(vec![
                Cell::from(kind),
                Cell::from(amount),
                Cell::from(fiat_code),
                Cell::from(fiat_amount),
                Cell::from(payment_method),
            ]);
            if i == selected_order_idx {
                // Highlight the selected row.
                row.style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
            } else {
                row
            }
        }).collect();

        let table = Table::new(
            rows,
            &[
                Constraint::Max(5),
                Constraint::Max(11),
                Constraint::Max(5),
                Constraint::Max(12),
                Constraint::Min(10),
            ]
        )
        .header(header)
        .block(Block::default().title("Orders").borders(Borders::ALL).style(Style::default().bg(BACKGROUND_COLOR)));
        f.render_widget(table, content_area);
    } else if active_tab == 1 {
        let paragraph = Paragraph::new(Span::raw("Coming soon"))
            .block(Block::default().title("My Trades").borders(Borders::ALL).style(Style::default().bg(BACKGROUND_COLOR)));
        f.render_widget(paragraph, content_area);
    } else if active_tab == 2 {
        let paragraph = Paragraph::new(Span::raw("Coming soon"))
            .block(Block::default().title("Messages").borders(Borders::ALL).style(Style::default().bg(BACKGROUND_COLOR)));
        f.render_widget(paragraph, content_area);
    } else if active_tab == 3 {
        let paragraph = Paragraph::new(Span::raw("Coming soon"))
            .block(Block::default().title("Settings").borders(Borders::ALL).style(Style::default().bg(BACKGROUND_COLOR)));
        f.render_widget(paragraph, content_area);
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    log::info!("MostriX started");
    let settings = init_settings();
    db::init_db().await?;
    // Initialize logger
    setup_logger(&settings.log_level).expect("Can't initialize logger");
    // Set the terminal in raw mode and switch to the alternate screen.
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Shared state: orders are stored in memory.
    let orders: Arc<Mutex<Vec<Order>>> = Arc::new(Mutex::new(Vec::new()));

    // Configure Nostr client.
    let my_keys = Keys::generate();
    let client = Client::new(my_keys);
    // Add the Mostro relay.
    client.add_relay("wss://relay.mostro.network").await?;
    client.connect().await;

    // Hardcoded Mostro Pubkey.
    let mostro_pubkey_str = "82fa8cb978b43c79b2156585bac2c011176a21d2aead6d9f7c575c005be88390";
    let mostro_pubkey = PublicKey::from_str(mostro_pubkey_str)
        .map_err(|e| anyhow::anyhow!("Invalid Mostro pubkey: {}", e))?;

    // Calculate timestamp for events in the last 7 days.
    let since_time = Utc::now()
        .checked_sub_signed(ChronoDuration::days(7))
        .ok_or_else(|| anyhow::anyhow!("Failed to compute time"))?
        .timestamp() as u64;
    let timestamp = Timestamp::from(since_time);

    // Build the filter for NIP-69 (orders) events from Mostro.
    let filter = Filter::new()
        .author(mostro_pubkey)
        .limit(20)
        .since(timestamp)
        .custom_tag(SingleLetterTag::lowercase(Alphabet::Y), "mostro")
        .custom_tag(SingleLetterTag::lowercase(Alphabet::Z), "order")
        .kind(Kind::Custom(NOSTR_REPLACEABLE_EVENT_KIND));

    // Subscribe to the filter.
    client.subscribe(filter, None).await?;

    // Asynchronous task to handle incoming notifications.
    let orders_clone = Arc::clone(&orders);
    let mut notifications = client.notifications();
    tokio::spawn(async move {
        while let Ok(notification) = notifications.recv().await {
            if let RelayPoolNotification::Event { event, .. } = notification {
                if event.kind == Kind::Custom(NOSTR_REPLACEABLE_EVENT_KIND) {
                    if let Some(order) = parse_order_event((*event).clone()) {
                        let mut orders_lock = orders_clone.lock().unwrap();
                        // Update the existing order (if the id matches) or add a new one.
                        if let Some(existing) = orders_lock.iter_mut().find(|o| o.id == order.id) {
                            *existing = order;
                        } else {
                            orders_lock.push(order);
                        }
                    }
                }
            }
        }
    });

    // Event handling: keyboard input and periodic UI refresh.
    let mut events = EventStream::new();
    let mut refresh_interval = interval(Duration::from_millis(500));
    let mut active_tab: usize = 0;
    // Selected order index for the "Orders" table.
    let mut selected_order_idx: usize = 0;

    loop {
        tokio::select! {
            maybe_event = events.next() => {
                if let Some(Ok(event)) = maybe_event {
                    if let CEvent::Key(KeyEvent { code, .. }) = event {
                        match code {
                            KeyCode::Left => {
                                if active_tab > 0 {
                                    active_tab -= 1;
                                }
                            }
                            KeyCode::Right => {
                                if active_tab < 3 {
                                    active_tab += 1;
                                }
                            }
                            KeyCode::Up => {
                                if active_tab == 0 {
                                    let orders_len = orders.lock().unwrap().len();
                                    if orders_len > 0 && selected_order_idx > 0 {
                                        selected_order_idx -= 1;
                                    }
                                }
                            }
                            KeyCode::Down => {
                                if active_tab == 0 {
                                    let orders_len = orders.lock().unwrap().len();
                                    if orders_len > 0 && selected_order_idx < orders_len.saturating_sub(1) {
                                        selected_order_idx += 1;
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                if active_tab == 0 {
                                    let orders_lock = orders.lock().unwrap();
                                    if let Some(order) = orders_lock.get(selected_order_idx) {
                                        log::info!("selected order {:#?}", order);
                                    }
                                }
                            }
                            KeyCode::Char('q') | KeyCode::Esc => break,
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
            if orders_len > 0 && selected_order_idx >= orders_len {
                selected_order_idx = orders_len - 1;
            }
        }

        terminal.draw(|f| ui_draw(f, active_tab, &orders, selected_order_idx))?;
    }

    // Restore terminal to its original state.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
