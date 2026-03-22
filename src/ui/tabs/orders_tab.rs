use std::sync::{Arc, Mutex};

use chrono::DateTime;
use mostro_core::prelude::*;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::ui::{apply_kind_color, AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_orders_tab(
    f: &mut ratatui::Frame,
    area: Rect,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    selected_order_idx: usize,
    app: &AppState,
) {
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
        f.render_widget(paragraph, area);
    } else {
        // Build a case-insensitive set of currencies from the cached filter.
        let currency_filter: Option<std::collections::HashSet<String>> =
            if app.currencies_filter.is_empty() {
                None
            } else {
                Some(
                    app.currencies_filter
                        .iter()
                        .map(|c| c.to_uppercase())
                        .collect::<std::collections::HashSet<String>>(),
                )
            };

        let order_passes_filter = |order: &SmallOrder| -> bool {
            if let Some(ref filter_set) = currency_filter {
                let fiat_upper = order.fiat_code.to_uppercase();
                filter_set.contains(&fiat_upper)
            } else {
                true
            }
        };

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
            .filter_map(|(i, order)| {
                if !order_passes_filter(order) {
                    return None;
                }

                let kind_cell = if let Some(k) = &order.kind {
                    Cell::from(k.to_string()).style(apply_kind_color(k))
                } else {
                    Cell::from("BUY/SELL")
                };

                let id_cell = Cell::from(
                    order
                        .id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "N/A".to_string()),
                );

                let status_str = order
                    .status
                    .unwrap_or(mostro_core::order::Status::Active)
                    .to_string();
                let status_cell = Cell::from(status_str);

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
                // Convert UTC timestamp to local time for display
                let date_cell = Cell::from(
                    date.map(|d| {
                        d.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
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

                Some(if i == selected_order_idx {
                    row.style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
                } else {
                    row
                })
            })
            .collect();

        let table = Table::new(
            rows,
            &[
                Constraint::Max(8),
                Constraint::Max(15),
                Constraint::Max(10),
                Constraint::Max(12),
                Constraint::Max(10),
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
        f.render_widget(table, area);
    }
}
