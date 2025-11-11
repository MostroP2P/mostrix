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
}

#[derive(Clone, Debug, Default)]
pub struct FormState {
    pub kind: String,          // buy | sell
    pub fiat_code: String,     // e.g. USD, EUR, ARS
    pub fiat_amount: String,   // numeric (quantità)
    pub payment_method: String, // comma separated
    pub focused: usize,        // 0..=3
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
            let inner_chunks = Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(1), // spacer
                    Constraint::Length(3), // tipo
                    Constraint::Length(3), // valuta
                    Constraint::Length(3), // quantità
                    Constraint::Length(3), // metodo
                    Constraint::Length(1), // hint
                ],
            )
            .split(content_area);

            let block = Block::default()
                .title("✨ Create New Order")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
            f.render_widget(block, content_area);

            // Header spacer
            f.render_widget(Paragraph::new(Line::from("")).block(Block::default()), inner_chunks[0]);

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
            f.render_widget(Paragraph::new(tipo_line).block(tipo_title), inner_chunks[1]);

            // Field 1: Valuta
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
            f.render_widget(valuta, inner_chunks[2]);

            // Field 2: Quantità (fiat amount)
            let qty = Paragraph::new(Line::from(form.fiat_amount.clone())).block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("💰 ", Style::default().fg(Color::Yellow)),
                        Span::styled("Amount", Style::default().add_modifier(Modifier::BOLD)),
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
            f.render_widget(qty, inner_chunks[3]);

            // Field 3: Metodo Pagamento
            let pm = Paragraph::new(Line::from(form.payment_method.clone())).block(
                Block::default()
                    .title(Line::from(vec![
                        Span::styled("💳 ", Style::default().fg(Color::Magenta)),
                        Span::styled("Payment Method", Style::default().add_modifier(Modifier::BOLD)),
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
            f.render_widget(pm, inner_chunks[4]);

            // Footer hint
            let hint = Paragraph::new(Line::from(vec![
                Span::styled("💡 ", Style::default().fg(Color::Cyan)),
                Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" submit • "),
                Span::styled("Tab", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" focus • "),
                Span::styled("←/→/Space", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::raw(" toggle type • "),
                Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" cancel"),
            ]))
            .block(Block::default());
            f.render_widget(hint, inner_chunks[5]);

            // Show cursor in active text field
            match form.focused {
                1 => {
                    let x = inner_chunks[2].x + 1 + form.fiat_code.len() as u16;
                    let y = inner_chunks[2].y + 1;
                    f.set_cursor_position((x, y));
                }
                2 => {
                    let x = inner_chunks[3].x + 1 + form.fiat_amount.len() as u16;
                    let y = inner_chunks[3].y + 1;
                    f.set_cursor_position((x, y));
                }
                3 => {
                    let x = inner_chunks[4].x + 1 + form.payment_method.len() as u16;
                    let y = inner_chunks[4].y + 1;
                    f.set_cursor_position((x, y));
                }
                _ => {}
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

    // Overlay: Create Order form (only show when NOT on Create New Order tab)
    if let UiMode::CreatingOrder(form) = &app.mode {
        if app.active_tab != 4 {
            // Only show overlay if not on Create New Order tab
            use ratatui::layout::Rect;
            let area = f.area();
            let popup_width = area.width.saturating_sub(area.width / 6);
            let popup_height = 14;
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
                    Constraint::Length(3), // tipo
                    Constraint::Length(3), // valuta
                    Constraint::Length(3), // quantità
                    Constraint::Length(3), // metodo
                    Constraint::Length(1), // hint
                ],
            )
            .split(popup);

            let block = Block::default()
                .title("Crea Ordine")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR));
            f.render_widget(block, popup);

            // Header spacer
            f.render_widget(Paragraph::new(Line::from("")).block(Block::default()), inner_chunks[0]);

            // Field 0: Tipo (toggle buy/sell)
            let tipo_title = Block::default()
                .title(Line::from("Tipo (buy/sell)"))
                .borders(Borders::ALL)
                .style(
                    if form.focused == 0 {
                        Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                    } else {
                        Style::default().bg(BACKGROUND_COLOR)
                    },
                );
            let buy_sel = if form.kind.to_lowercase() == "buy" { Modifier::BOLD } else { Modifier::empty() };
            let sell_sel = if form.kind.to_lowercase() == "sell" { Modifier::BOLD } else { Modifier::empty() };
            let tipo_line = Line::from(vec![
                Span::styled("[ buy ]", Style::default().add_modifier(buy_sel)),
                Span::raw(" "),
                Span::styled("[ sell ]", Style::default().add_modifier(sell_sel)),
            ]);
            f.render_widget(Paragraph::new(tipo_line).block(tipo_title), inner_chunks[1]);

            // Field 1: Valuta
            let valuta = Paragraph::new(Line::from(form.fiat_code.clone())).block(
                Block::default()
                    .title(Line::from("Valuta"))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 1 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR)
                        },
                    ),
            );
            f.render_widget(valuta, inner_chunks[2]);

            // Field 2: Quantità (fiat amount)
            let qty = Paragraph::new(Line::from(form.fiat_amount.clone())).block(
                Block::default()
                    .title(Line::from("Quantità"))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 2 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR)
                        },
                    ),
            );
            f.render_widget(qty, inner_chunks[3]);

            // Field 3: Metodo Pagamento
            let pm = Paragraph::new(Line::from(form.payment_method.clone())).block(
                Block::default()
                    .title(Line::from("Metodo pagamento"))
                    .borders(Borders::ALL)
                    .style(
                        if form.focused == 3 {
                            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
                        } else {
                            Style::default().bg(BACKGROUND_COLOR)
                        },
                    ),
            );
            f.render_widget(pm, inner_chunks[4]);

            // Footer hint
            let hint = Paragraph::new(Line::from(
                "Enter apre/submit • Tab focus • ←/→/Spazio cambia tipo • Esc annulla",
            ))
            .block(Block::default());
            let mut hint_area = inner_chunks.last().copied().unwrap_or(popup);
            hint_area.y = popup.y + popup.height - 1;
            f.render_widget(hint, hint_area);

            // Show cursor in active text field
            match form.focused {
                1 => {
                    let x = inner_chunks[2].x + 1 + form.fiat_code.len() as u16;
                    let y = inner_chunks[2].y + 1;
                    f.set_cursor_position((x, y));
                }
                2 => {
                    let x = inner_chunks[3].x + 1 + form.fiat_amount.len() as u16;
                    let y = inner_chunks[3].y + 1;
                    f.set_cursor_position((x, y));
                }
                3 => {
                    let x = inner_chunks[4].x + 1 + form.payment_method.len() as u16;
                    let y = inner_chunks[4].y + 1;
                    f.set_cursor_position((x, y));
                }
                _ => {}
            }
        }
    }
}


