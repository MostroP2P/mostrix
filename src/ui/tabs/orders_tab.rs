use std::sync::{Arc, Mutex};

use chrono::DateTime;
use mostro_core::prelude::*;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Table, TableState,
};

use crate::ui::helpers::format_premium;
use crate::ui::{apply_kind_color, AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Renders the available orders table, with fewer columns when terminal width is limited.
///
/// Uses a stateful [`Table`] so ↑↓ selection stays in view when the book is taller
/// than the terminal (same idea as the disputes sidebar scroll list).
pub fn render_orders_tab(
    f: &mut ratatui::Frame,
    area: Rect,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    selected_order_idx: usize,
    app: &AppState,
) {
    let orders_lock = match orders.lock() {
        Ok(g) => g,
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned orders lock: {e}). Please restart the app."
            ));
            let paragraph = Paragraph::new(Span::styled(
                "❌ Internal error. Please restart Mostrix.",
                Style::default().fg(Color::Red),
            ))
            .block(
                Block::default()
                    .title("Orders")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(PRIMARY_COLOR))
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            );
            f.render_widget(paragraph, area);
            return;
        }
    };

    if orders_lock.is_empty() {
        let paragraph = Paragraph::new(Span::styled(
            "📭 No offers found with requested parameters…",
            Style::default().fg(Color::Red),
        ))
        .block(
            Block::default()
                .title("Orders")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(PRIMARY_COLOR))
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );
        f.render_widget(paragraph, area);
        return;
    }

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

    let compact = area.width < 100;
    let header_labels = if compact {
        vec!["📈 Kind", "💵 Fiat Amt", "± Premium", "💳 Payment"]
    } else {
        vec![
            "📈 Kind",
            "🆔 Order Id",
            "📊 Status",
            "₿ Amount",
            "💱 Fiat",
            "💵 Fiat Amt",
            "± Premium",
            "💳 Payment Method",
            "📅 Created",
        ]
    };
    let header_cells = header_labels
        .into_iter()
        .map(|label| Cell::from(label).style(Style::default().add_modifier(Modifier::BOLD)))
        .collect::<Vec<_>>();
    let header = Row::new(header_cells);

    // Track original list indices so TableState can highlight the selected
    // row among the *filtered* display rows.
    let mut display_original_indices: Vec<usize> = Vec::new();
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

            let fiat_amount_text = if order.min_amount.is_none() && order.max_amount.is_none() {
                order.fiat_amount.to_string()
            } else {
                match (order.min_amount, order.max_amount) {
                    (Some(min), Some(max)) => format!("{}-{}", min, max),
                    (Some(min), None) => format!("{}-?", min),
                    (None, Some(max)) => format!("?-{}", max),
                    (None, None) => "?".to_string(),
                }
            };
            let fiat_amount_cell = Cell::from(fiat_amount_text.clone());

            let payment_method_cell = Cell::from(order.payment_method.clone());
            let premium_cell = premium_cell(order.premium);

            // Missing created_at must not fall back to epoch (unwrap_or(0)); propagate None.
            let date_cell = Cell::from(
                order
                    .created_at
                    .and_then(|ts| DateTime::from_timestamp(ts, 0))
                    .map(|d| {
                        d.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|| "Invalid date".to_string()),
            );

            let row = if compact {
                Row::new(vec![
                    kind_cell,
                    Cell::from(format!("{} {}", fiat_amount_text, order.fiat_code)),
                    premium_cell,
                    payment_method_cell,
                ])
            } else {
                Row::new(vec![
                    kind_cell,
                    id_cell,
                    status_cell,
                    amount_cell,
                    fiat_code_cell,
                    fiat_amount_cell,
                    premium_cell,
                    payment_method_cell,
                    date_cell,
                ])
            };

            display_original_indices.push(i);
            Some(row)
        })
        .collect();

    if rows.is_empty() {
        let paragraph = Paragraph::new(Span::styled(
            "📭 No offers match the current currency filter…",
            Style::default().fg(Color::Yellow),
        ))
        .block(
            Block::default()
                .title("Orders")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(PRIMARY_COLOR))
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );
        f.render_widget(paragraph, area);
        return;
    }

    let display_selected_idx = display_original_indices
        .iter()
        .position(|&orig| orig == selected_order_idx)
        .unwrap_or(0);

    let widths = if compact {
        vec![
            Constraint::Max(8),
            Constraint::Max(18),
            Constraint::Max(10),
            Constraint::Min(12),
        ]
    } else {
        vec![
            Constraint::Max(8),
            Constraint::Max(15),
            Constraint::Max(10),
            Constraint::Max(12),
            Constraint::Max(10),
            Constraint::Max(12),
            Constraint::Max(10),
            Constraint::Min(15),
            Constraint::Max(18),
        ]
    };

    let row_count = rows.len();
    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
        .block(
            Block::default()
                .title("Orders")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(PRIMARY_COLOR))
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );

    let mut table_state = TableState::default().with_selected(Some(display_selected_idx));
    f.render_stateful_widget(table, area, &mut table_state);

    // Header + borders consume 3 rows; remaining height is the scrollable body.
    let visible_rows = area.height.saturating_sub(3) as usize;
    if row_count > visible_rows && visible_rows > 0 {
        let mut scrollbar_state = ScrollbarState::new(row_count).position(display_selected_idx);
        f.render_stateful_widget(
            Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
            area,
            &mut scrollbar_state,
        );
    }
}

fn premium_cell(premium: i64) -> Cell<'static> {
    let (text, color) = format_premium(premium);
    Cell::from(text).style(Style::default().fg(color))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use crate::ui::UserRole;

    fn buffer_contains(buf: &ratatui::buffer::Buffer, needle: &str) -> bool {
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
            flat.push('\n');
        }
        flat.contains(needle)
    }

    fn sample_order(payment_method: &str, premium: i64) -> SmallOrder {
        SmallOrder {
            kind: Some(mostro_core::order::Kind::Buy),
            fiat_code: "USD".to_string(),
            fiat_amount: 100,
            amount: 50_000,
            premium,
            payment_method: payment_method.to_string(),
            ..Default::default()
        }
    }

    fn render_at_width(width: u16, premium: i64) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        let orders = Arc::new(Mutex::new(vec![sample_order("SEPA", premium)]));
        let app = AppState::new(UserRole::User);
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, 0, &app))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn orders_table_renders_premium_column() {
        let buf = render_at_width(130, -3);
        assert!(buffer_contains(&buf, "Premium"));
        assert!(buffer_contains(&buf, "-3%"));
        assert!(buffer_contains(&buf, "SEPA"));
    }

    #[test]
    fn narrow_orders_table_keeps_premium_readable() {
        let buf = render_at_width(60, -3);
        assert!(buffer_contains(&buf, "Premium"));
        assert!(buffer_contains(&buf, "-3%"));
        assert!(buffer_contains(&buf, "100 USD"));
        assert!(buffer_contains(&buf, "SEPA"));
        assert!(!buffer_contains(&buf, "Created"));
    }

    /// When more orders exist than table body rows, selecting a late row must
    /// scroll the stateful table so that marker is visible.
    #[test]
    fn orders_table_scrolls_to_keep_selected_row_visible() {
        let orders = Arc::new(Mutex::new(
            (0..40)
                .map(|i| sample_order(&format!("PAY-{i:02}"), 0))
                .collect::<Vec<_>>(),
        ));
        let app = AppState::new(UserRole::User);
        // Height 10 → ~7 body rows after borders+header; selecting index 39 must scroll.
        let backend = TestBackend::new(130, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, 39, &app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "PAY-39"),
            "selected late order must be visible after table scroll"
        );
        assert!(
            !buffer_contains(buf, "PAY-00"),
            "first order should scroll off-screen when selecting the last"
        );
    }

    #[test]
    fn orders_table_shows_first_rows_when_selection_is_at_top() {
        let orders = Arc::new(Mutex::new(
            (0..40)
                .map(|i| sample_order(&format!("PAY-{i:02}"), 0))
                .collect::<Vec<_>>(),
        ));
        let app = AppState::new(UserRole::User);
        let backend = TestBackend::new(130, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, 0, &app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "PAY-00"),
            "first order must stay visible when selected"
        );
        assert!(
            !buffer_contains(buf, "PAY-39"),
            "last order should not appear while scrolled to the top"
        );
    }
}
