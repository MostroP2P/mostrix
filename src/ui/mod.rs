use std::sync::{Arc, Mutex};

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs};

use crate::models::Order;

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

pub fn ui_draw(
    f: &mut ratatui::Frame,
    app: &AppState,
    orders: &Arc<Mutex<Vec<Order>>>,
    status_line: Option<&str>,
) {
    // Create layout: one row for tabs and the rest for content.
    let chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)],
    )
    .split(f.area());

    // Define tab titles.
    let tab_titles = ["Orders", "My Trades", "Messages", "Settings"]
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
        // "Orders" tab: show table with pending orders.
        let header_cells = [
            "Kind",
            "Sats Amount",
            "Fiat",
            "Fiat Amount",
            "Payment Method",
        ]
        .iter()
        .map(|h| Cell::from(*h))
        .collect::<Vec<Cell>>();
        let header = Row::new(header_cells).style(Style::default().add_modifier(Modifier::BOLD));

        let orders_lock = orders.lock().unwrap();
        let rows: Vec<Row> = orders_lock
            .iter()
            .enumerate()
            .map(|(i, order)| {
                let kind = order.kind.clone().unwrap_or_default();
                let fiat_code = order.fiat_code.clone();
                let amount = if order.amount == 0 {
                    "Market Price".to_string()
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
                Constraint::Max(5),
                Constraint::Max(11),
                Constraint::Max(5),
                Constraint::Max(12),
                Constraint::Min(10),
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
    }

    // Bottom status bar
    if let Some(line) = status_line {
        status::render_status_bar(f, chunks[2], line);
    }

    // Overlay: Create Order form
    if let UiMode::CreatingOrder(form) = &app.mode {
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
            Span::styled("[ buy ] ", Style::default().add_modifier(buy_sel)),
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


