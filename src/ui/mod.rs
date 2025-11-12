use std::sync::{Arc, Mutex};

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs};
use chrono::DateTime;
use mostro_core::prelude::*;

pub const PRIMARY_COLOR: Color = Color::Rgb(177, 204, 51); // #b1cc33
pub const BACKGROUND_COLOR: Color = Color::Rgb(29, 33, 44); // #1D212C

pub mod status;

#[derive(Clone, Debug)]
pub enum UiMode {
    Normal,
    CreatingOrder(FormState),
    ConfirmingOrder(FormState), // Confirmation popup
    WaitingForMostro(FormState), // Waiting for Mostro response
    OrderResult(OrderResult), // Show order result (success or error)
}

#[derive(Clone, Debug)]
pub enum OrderResult {
    Success {
        order_id: Option<uuid::Uuid>,
        kind: Option<mostro_core::order::Kind>,
        amount: i64,
        fiat_code: String,
        fiat_amount: i64,
        min_amount: Option<i64>,
        max_amount: Option<i64>,
        payment_method: String,
        premium: i64,
        status: Option<mostro_core::prelude::Status>,
    },
    Error(String),
}

#[derive(Clone, Debug, Default)]
pub struct FormState {
    pub kind: String,          // buy | sell
    pub fiat_code: String,     // e.g. USD, EUR, ARS
    pub fiat_amount: String,   // numeric (single amount or min for range)
    pub fiat_amount_max: String, // max amount for range (optional)
    pub amount: String,        // amount in sats (0 for market)
    pub payment_method: String, // comma separated
    pub premium: String,       // premium percentage
    pub invoice: String,       // optional invoice
    pub expiration_days: String, // expiration days (0 for no expiration)
    pub focused: usize,        // field index
    pub use_range: bool,       // whether to use fiat range
}

pub struct AppState {
    pub active_tab: usize,
    pub selected_order_idx: usize,
    pub mode: UiMode,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            active_tab: 0,
            selected_order_idx: 0,
            mode: UiMode::Normal,
        }
    }
}

/// Apply color coding to status cells based on status type (adapted for ratatui)
fn apply_status_color(status: &str) -> Style {
    let status_lower = status.to_lowercase();
    if status_lower.contains("init")
        || status_lower.contains("pending")
        || status_lower.contains("waiting")
    {
        Style::default().fg(Color::Yellow)
    } else if status_lower.contains("active")
        || status_lower.contains("released")
        || status_lower.contains("settled")
        || status_lower.contains("taken")
        || status_lower.contains("success")
    {
        Style::default().fg(Color::Green)
    } else if status_lower.contains("fiat") {
        Style::default().fg(Color::Cyan)
    } else if status_lower.contains("dispute")
        || status_lower.contains("cancel")
        || status_lower.contains("canceled")
    {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    }
}

/// Apply color coding to order kind cells (adapted for ratatui)
fn apply_kind_color(kind: &mostro_core::order::Kind) -> Style {
    match kind {
        mostro_core::order::Kind::Buy => Style::default().fg(Color::Green),
        mostro_core::order::Kind::Sell => Style::default().fg(Color::Red),
    }
}

pub fn ui_draw(
    f: &mut ratatui::Frame,
    app: &AppState,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    status_line: Option<&str>,
) {
    // Create layout: one row for tabs and the rest for content.
    let chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)],
    )
    .split(f.area());

    // Define tab titles.
    let tab_titles = ["Orders", "My Trades", "Messages", "Settings", "Create New Order"]
        .iter()
        .map(|t| Line::from(*t))
        .collect::<Vec<Line>>();
    let tabs = Tabs::new(tab_titles)
        .select(app.active_tab)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        )
        .highlight_style(
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, chunks[0]);

    let content_area = chunks[1];
    if app.active_tab == 0 {
        // "Orders" tab: show table with pending orders (beautified like mostro-cli).
        let orders_lock = orders.lock().unwrap();
        
        if orders_lock.is_empty() {
            let paragraph = Paragraph::new(Span::styled(
                "📭 No offers found with requested parameters…",
                Style::default().fg(Color::Red),
            ))
            .block(
                Block::default()
                    .title("Orders")
                    .borders(Borders::ALL)
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            );
            f.render_widget(paragraph, content_area);
        } else {
            let header_cells = vec![
                Cell::from("📈 Kind").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("🆔 Order Id").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("📊 Status").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("₿ Amount").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("💱 Fiat").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("💵 Fiat Amt").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("💳 Payment Method").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("📅 Created").style(Style::default().add_modifier(Modifier::BOLD)),
            ];
            let header = Row::new(header_cells);

            let rows: Vec<Row> = orders_lock
                .iter()
                .enumerate()
                .map(|(i, order)| {
                    let kind_cell = if let Some(k) = &order.kind {
                        Cell::from(k.to_string()).style(apply_kind_color(k))
                    } else {
                        Cell::from("BUY/SELL")
                    };
                    
                    let id_cell = Cell::from(
                        order.id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "N/A".to_string()),
                    );
                    
                    let status_str = order
                        .status
                        .unwrap_or(mostro_core::order::Status::Active)
                        .to_string();
                    let status_cell = Cell::from(status_str.clone()).style(apply_status_color(&status_str));
                    
                    let amount_cell = Cell::from(if order.amount == 0 {
                        "market".to_string()
                    } else {
                        order.amount.to_string()
                    });
                    
                    let fiat_code_cell = Cell::from(order.fiat_code.clone());
                    
                    let fiat_amount_cell = if order.min_amount.is_none() && order.max_amount.is_none() {
                        Cell::from(order.fiat_amount.to_string())
                    } else {
                        let range_str = match (order.min_amount, order.max_amount) {
                            (Some(min), Some(max)) => format!("{}-{}", min, max),
                            (Some(min), None) => format!("{}-?", min),
                            (None, Some(max)) => format!("?-{}", max),
                            (None, None) => "?".to_string(),
                        };
                        Cell::from(range_str)
                    };
                    
                    let payment_method_cell = Cell::from(order.payment_method.clone());
                    
                    let date = DateTime::from_timestamp(order.created_at.unwrap_or(0), 0);
                    let date_cell = Cell::from(
                        date.map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| "Invalid date".to_string()),
                    );
                    
                    let row = Row::new(vec![
                        kind_cell,
                        id_cell,
                        status_cell,
                        amount_cell,
                        fiat_code_cell,
                        fiat_amount_cell,
                        payment_method_cell,
                        date_cell,
                    ]);
                    
                    if i == app.selected_order_idx {
                        // Highlight the selected row.
                        row.style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
                    } else {
                        row
                    }
                })
                .collect();

            let table = Table::new(
                rows,
                &[
                    Constraint::Max(8),
                    Constraint::Max(15),
                    Constraint::Max(10),
                    Constraint::Max(12),
                    Constraint::Max(6),
                    Constraint::Max(12),
                    Constraint::Min(15),
                    Constraint::Max(18),
                ],
            )
            .header(header)
            .block(
                Block::default()
                    .title("Orders")
                    .borders(Borders::ALL)
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            );
            f.render_widget(table, content_area);
        }
    } else if app.active_tab == 1 {
        let paragraph = Paragraph::new(Span::raw("Coming soon")).block(
            Block::default()
                .title("My Trades")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );
        f.render_widget(paragraph, content_area);
    } else if app.active_tab == 2 {
        let paragraph = Paragraph::new(Span::raw("Coming soon")).block(
            Block::default()
                .title("Messages")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );
        f.render_widget(paragraph, content_area);
    } else if app.active_tab == 3 {
        let paragraph = Paragraph::new(Span::raw("Coming soon")).block(
            Block::default()
                .title("Settings")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );
        f.render_widget(paragraph, content_area);
    } else if app.active_tab == 4 {
        // Create New Order tab - show form in content area
        if let UiMode::CreatingOrder(form) = &app.mode {
            // Calculate number of fields dynamically
            let field_count = if form.use_range { 10 } else { 9 };
            let mut constraints = vec![Constraint::Length(1)]; // spacer
            for _ in 0..field_count {
                constraints.push(Constraint::Length(3));
            }
            constraints.push(Constraint::Length(1)); // hint
            
            let inner_chunks = Layout::new(Direction::Vertical, constraints).split(content_area);

            let block = Block::default()
                .title("✨ Create New Order")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
            f.render_widget(block, content_area);

            let mut field_idx = 1;

            // Field 0: Tipo (toggle buy/sell)
            let tipo_title = Block::default()
                .title(Line::from(vec![
                    Span::styled("📈 ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled("Order Type", Style::default().add_modifier(Modifier::BOLD)),
                ]))
                .borders(Borders::ALL)
                .style(
                    if form.focused == 0 {
                        Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                    } else {
                        Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                    },
                );
            let is_buy = form.kind.to_lowercase() == "buy";
            let tipo_line = if is_buy {
                Line::from(vec![
                    Span::styled("🟢 [ buy ]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                ])
            } else {
                Line::from(vec![
                    Span::styled("🔴 [ sell ]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                ])
            };
            f.render_widget(Paragraph::new(tipo_line).block(tipo_title), inner_chunks[field_idx]);
            field_idx += 1;

            // Field 1: Currency
            let valuta = Paragraph::new(Line::from(form.fiat_code.clone())).block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("💱 ", Style::default().fg(Color::Cyan)),
                        Span::styled("Currency", Style::default().add_modifier(Modifier::BOLD)),
                    ]))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 1 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                        },
                    ),
            );
            f.render_widget(valuta, inner_chunks[field_idx]);
            field_idx += 1;

            // Field 2: Amount (sats)
            let amount = Paragraph::new(Line::from(form.amount.clone())).block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("₿ ", Style::default().fg(Color::Yellow)),
                        Span::styled("Amount (sats)", Style::default().add_modifier(Modifier::BOLD)),
                    ]))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 2 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                        },
                    ),
            );
            f.render_widget(amount, inner_chunks[field_idx]);
            field_idx += 1;

            // Field 3: Fiat Amount (min or single)
            let fiat_title = if form.use_range {
                "Fiat Amount (Min)"
            } else {
                "Fiat Amount"
            };
            let qty = Paragraph::new(Line::from(form.fiat_amount.clone())).block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("💰 ", Style::default().fg(Color::Yellow)),
                        Span::styled(fiat_title, Style::default().add_modifier(Modifier::BOLD)),
                    ]))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 3 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                        },
                    ),
            );
            f.render_widget(qty, inner_chunks[field_idx]);
            field_idx += 1;

            // Field 4: Fiat Amount Max (if range)
            if form.use_range {
                let qty_max = Paragraph::new(Line::from(form.fiat_amount_max.clone())).block(
                    Block::default()
                        .title(Line::from(vec![
                            Span::styled("💰 ", Style::default().fg(Color::Yellow)),
                            Span::styled("Fiat Amount (Max)", Style::default().add_modifier(Modifier::BOLD)),
                        ]))
                        .borders(Borders::ALL)
                        .style(
                            if form.focused == 4 {
                                Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                            } else {
                                Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                            },
                        ),
                );
                f.render_widget(qty_max, inner_chunks[field_idx]);
                field_idx += 1;
            }

            // Field 5: Payment Method
            let pm = Paragraph::new(Line::from(form.payment_method.clone())).block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("💳 ", Style::default().fg(Color::Magenta)),
                        Span::styled("Payment Method", Style::default().add_modifier(Modifier::BOLD)),
                    ]))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 5 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                        },
                    ),
            );
            f.render_widget(pm, inner_chunks[field_idx]);
            field_idx += 1;

            // Field 6: Premium
            let premium = Paragraph::new(Line::from(form.premium.clone())).block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("📈 ", Style::default().fg(Color::Green)),
                        Span::styled("Premium (%)", Style::default().add_modifier(Modifier::BOLD)),
                    ]))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 6 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                        },
                    ),
            );
            f.render_widget(premium, inner_chunks[field_idx]);
            field_idx += 1;

            // Field 7: Invoice (optional)
            let invoice = Paragraph::new(Line::from(form.invoice.clone())).block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("🧾 ", Style::default().fg(Color::Blue)),
                        Span::styled("Invoice (optional)", Style::default().add_modifier(Modifier::BOLD)),
                    ]))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 7 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                        },
                    ),
            );
            f.render_widget(invoice, inner_chunks[field_idx]);
            field_idx += 1;

            // Field 8: Expiration Days
            let exp = Paragraph::new(Line::from(form.expiration_days.clone())).block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("⏰ ", Style::default().fg(Color::Red)),
                        Span::styled("Expiration (days, 0=none)", Style::default().add_modifier(Modifier::BOLD)),
                    ]))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 8 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR).fg(Color::White)
                        },
                    ),
            );
            f.render_widget(exp, inner_chunks[field_idx]);
            field_idx += 1;

            // Footer hint
            let hint = Paragraph::new(Line::from(vec![
                Span::styled("💡 ", Style::default().fg(Color::Cyan)),
                Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" submit • "),
                Span::styled("Tab", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" focus • "),
                Span::styled("Space", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::raw(" toggle type/range • "),
                Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" cancel"),
            ]))
            .block(Block::default());
            f.render_widget(hint, inner_chunks[field_idx]);

            // Show cursor in active text field
            let cursor_field = match form.focused {
                1 => Some((inner_chunks[2], &form.fiat_code)),
                2 => Some((inner_chunks[3], &form.amount)),
                3 => Some((inner_chunks[4], &form.fiat_amount)),
                4 if form.use_range => Some((inner_chunks[5], &form.fiat_amount_max)),
                5 => Some((inner_chunks[if form.use_range { 6 } else { 5 }], &form.payment_method)),
                6 => Some((inner_chunks[if form.use_range { 7 } else { 6 }], &form.premium)),
                7 => Some((inner_chunks[if form.use_range { 8 } else { 7 }], &form.invoice)),
                8 => Some((inner_chunks[if form.use_range { 9 } else { 8 }], &form.expiration_days)),
                _ => None,
            };
            if let Some((chunk, text)) = cursor_field {
                let x = chunk.x + 1 + text.len() as u16;
                let y = chunk.y + 1;
                f.set_cursor_position((x, y));
            }
        } else {
            // Initialize form if not already in CreatingOrder mode
            let paragraph = Paragraph::new(Span::raw("Initializing form...")).block(
                Block::default()
                    .title("Create New Order")
                    .borders(Borders::ALL)
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            );
            f.render_widget(paragraph, content_area);
        }
    }

    // Bottom status bar
    if let Some(line) = status_line {
        status::render_status_bar(f, chunks[2], line);
    }

    // Confirmation popup overlay
    if let UiMode::ConfirmingOrder(form) = &app.mode {
        use ratatui::layout::Rect;
        let area = f.area();
        let popup_width = area.width.saturating_sub(area.width / 4);
        let popup_height = 20;
        let popup_x = area.x + (area.width - popup_width) / 2;
        let popup_y = area.y + (area.height - popup_height) / 2;
        let popup = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        let inner_chunks = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(1), // spacer
                Constraint::Length(2), // title
                Constraint::Length(1), // separator
                Constraint::Length(1), // kind
                Constraint::Length(1), // currency
                Constraint::Length(1), // amount
                Constraint::Length(1), // fiat amount
                Constraint::Length(1), // payment method
                Constraint::Length(1), // premium
                Constraint::Length(1), // invoice (if present)
                Constraint::Length(1), // expiration
                Constraint::Length(1), // separator
                Constraint::Length(1), // confirmation prompt
            ],
        )
        .split(popup);

        let block = Block::default()
            .title("📋 Order Confirmation")
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
        f.render_widget(block, popup);

        // Title
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Please review your order:", Style::default().add_modifier(Modifier::BOLD)),
            ])),
            inner_chunks[1],
        );

        // Order details
        let kind_str = if form.kind.to_lowercase() == "buy" {
            "🟢 Buy"
        } else {
            "🔴 Sell"
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("Order Type: "),
                Span::styled(kind_str, Style::default().fg(PRIMARY_COLOR)),
            ])),
            inner_chunks[3],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("Currency: "),
                Span::styled(&form.fiat_code, Style::default().fg(PRIMARY_COLOR)),
            ])),
            inner_chunks[4],
        );

        let amount_str = if form.amount.is_empty() || form.amount == "0" {
            "market".to_string()
        } else {
            format!("{} sats", form.amount)
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("Amount: "),
                Span::styled(amount_str, Style::default().fg(PRIMARY_COLOR)),
            ])),
            inner_chunks[5],
        );

        let fiat_str = if form.use_range && !form.fiat_amount_max.is_empty() {
            format!("{}-{} {}", form.fiat_amount, form.fiat_amount_max, form.fiat_code)
        } else {
            format!("{} {}", form.fiat_amount, form.fiat_code)
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("Fiat Amount: "),
                Span::styled(fiat_str, Style::default().fg(PRIMARY_COLOR)),
            ])),
            inner_chunks[6],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("Payment Method: "),
                Span::styled(&form.payment_method, Style::default().fg(PRIMARY_COLOR)),
            ])),
            inner_chunks[7],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("Premium: "),
                Span::styled(format!("{}%", form.premium), Style::default().fg(PRIMARY_COLOR)),
            ])),
            inner_chunks[8],
        );

        if !form.invoice.is_empty() {
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("Invoice: "),
                    Span::styled(&form.invoice, Style::default().fg(PRIMARY_COLOR)),
                ])),
                inner_chunks[9],
            );
        }

        let exp_str = if form.expiration_days.is_empty() || form.expiration_days == "0" {
            "No expiration".to_string()
        } else {
            format!("{} days", form.expiration_days)
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("Expiration: "),
                Span::styled(exp_str, Style::default().fg(PRIMARY_COLOR)),
            ])),
            inner_chunks[10],
        );

        // Confirmation prompt
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Press ", Style::default()),
                Span::styled("Y", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" to confirm or "),
                Span::styled("N", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" to cancel"),
            ])),
            inner_chunks[12],
        );
    }

    // Waiting for Mostro popup overlay
    if let UiMode::WaitingForMostro(_) = &app.mode {
        use ratatui::layout::Rect;
        let area = f.area();
        let popup_width = 50;
        let popup_height = 7;
        let popup_x = area.x + (area.width - popup_width) / 2;
        let popup_y = area.y + (area.height - popup_height) / 2;
        let popup = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        let block = Block::default()
            .title("⏳ Waiting for Mostro")
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
        f.render_widget(block, popup);

        let inner_chunks = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(1), // spacer
                Constraint::Length(1), // message
                Constraint::Length(1), // spinner
            ],
        )
        .split(popup);

        f.render_widget(
            Paragraph::new(Line::from("Sending order and waiting for confirmation..."))
                .alignment(ratatui::layout::Alignment::Center),
            inner_chunks[1],
        );

        // Simple spinner animation (could be enhanced)
        let spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
        let spinner_idx = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            / 100) as usize
            % spinner.chars().count();
        let spinner_char = spinner.chars().nth(spinner_idx).unwrap_or('⠋');
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{}", spinner_char), Style::default().fg(PRIMARY_COLOR)),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            inner_chunks[2],
        );
    }

    // Order result popup overlay
    if let UiMode::OrderResult(result) = &app.mode {
        use ratatui::layout::Rect;
        let area = f.area();
        let popup_width = 70;
        let popup_height = match result {
            OrderResult::Success { .. } => 18,
            OrderResult::Error(_) => 8,
        };
        let popup_x = area.x + (area.width - popup_width) / 2;
        let popup_y = area.y + (area.height - popup_height) / 2;
        let popup = Rect {
            x: popup_x,
            y: popup_y,
            width: popup_width,
            height: popup_height,
        };

        match result {
            OrderResult::Success {
                order_id,
                kind,
                amount,
                fiat_code,
                fiat_amount,
                min_amount,
                max_amount,
                payment_method,
                premium,
                status,
            } => {
                let block = Block::default()
                    .title("✅ Order Created Successfully")
                    .borders(Borders::ALL)
                    .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Green));
                f.render_widget(block, popup);

                let mut lines = vec![];
                
                if let Some(id) = order_id {
                    lines.push(Line::from(vec![
                        Span::styled("📋 Order ID: ", Style::default().fg(PRIMARY_COLOR)),
                        Span::styled(id.to_string(), Style::default()),
                    ]));
                }
                
                if let Some(k) = kind {
                    lines.push(Line::from(vec![
                        Span::styled("📈 Type: ", Style::default().fg(PRIMARY_COLOR)),
                        Span::styled(format!("{:?}", k), Style::default()),
                    ]));
                }
                
                if *amount > 0 {
                    lines.push(Line::from(vec![
                        Span::styled("💰 Amount: ", Style::default().fg(PRIMARY_COLOR)),
                        Span::styled(format!("{} sats", amount), Style::default()),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("💰 Amount: ", Style::default().fg(PRIMARY_COLOR)),
                        Span::styled("Market rate", Style::default()),
                    ]));
                }
                
                if let (Some(min), Some(max)) = (min_amount, max_amount) {
                    lines.push(Line::from(vec![
                        Span::styled("💵 Fiat Range: ", Style::default().fg(PRIMARY_COLOR)),
                        Span::styled(format!("{}-{} {}", min, max, fiat_code), Style::default()),
                    ]));
                } else if *fiat_amount > 0 {
                    lines.push(Line::from(vec![
                        Span::styled("💵 Fiat Amount: ", Style::default().fg(PRIMARY_COLOR)),
                        Span::styled(format!("{} {}", fiat_amount, fiat_code), Style::default()),
                    ]));
                }
                
                if !payment_method.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("💳 Payment Method: ", Style::default().fg(PRIMARY_COLOR)),
                        Span::styled(payment_method.clone(), Style::default()),
                    ]));
                }
                
                if *premium != 0 {
                    lines.push(Line::from(vec![
                        Span::styled("📈 Premium: ", Style::default().fg(PRIMARY_COLOR)),
                        Span::styled(format!("{}%", premium), Style::default()),
                    ]));
                }
                
                if let Some(s) = status {
                    lines.push(Line::from(vec![
                        Span::styled("📊 Status: ", Style::default().fg(PRIMARY_COLOR)),
                        Span::styled(format!("{:?}", s), Style::default()),
                    ]));
                }
                
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Press ESC to close", Style::default().fg(Color::DarkGray)),
                ]));

                let paragraph = Paragraph::new(lines)
                    .alignment(ratatui::layout::Alignment::Left);
                f.render_widget(paragraph, popup);
            }
            OrderResult::Error(error_msg) => {
                let block = Block::default()
                    .title("❌ Order Failed")
                    .borders(Borders::ALL)
                    .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Red));
                f.render_widget(block, popup);

                // Wrap error message if too long
                let error_lines: Vec<Line> = error_msg
                    .chars()
                    .collect::<Vec<_>>()
                    .chunks(popup_width as usize - 4)
                    .map(|chunk| Line::from(chunk.iter().collect::<String>()))
                    .collect();

                let mut lines = vec![];
                for line in error_lines {
                    lines.push(line);
                }
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("Press ESC to close", Style::default().fg(Color::DarkGray)),
                ]));

                let paragraph = Paragraph::new(lines)
                    .alignment(ratatui::layout::Alignment::Left);
                f.render_widget(paragraph, popup);
            }
        }
    }
}


