use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use ratatui::layout::{Alignment, Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, Wrap};

use crate::ui::helpers::{
    format_local_timestamp, format_premium, get_filtered_book_orders, render_table_list_scrollbar,
    selected_book_display_idx,
};
use crate::ui::orders::{OrderBookFilterField, OrderBookFilterState};
use crate::ui::{apply_kind_color, AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Renders the available orders table, with fewer columns when terminal width is limited.
///
/// Uses a persistent [`TableState`] (`app.orders_table_state`) so ↑↓ selection stays
/// in view when the book is taller than the terminal (viewport offset survives
/// frames). Selection is resolved by order id against the currency-filtered
/// projection (`helpers/order_selection.rs`) so highlight and Enter stay aligned.
/// Vertical scrollbar uses [`render_table_list_scrollbar`] (offset + data-row track).
/// On short terminals (`height < 4`) the header is dropped so a data row remains.
pub fn render_orders_tab(
    f: &mut ratatui::Frame,
    area: Rect,
    orders: &Arc<Mutex<Vec<SmallOrder>>>,
    app: &mut AppState,
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

    let filtered =
        get_filtered_book_orders(&orders_lock, &app.currencies_filter, &app.order_filters);
    if filtered.is_empty() {
        let paragraph = Paragraph::new(Span::styled(
            "📭 No offers match the current filters…",
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

    let (filter_area, table_area) =
        split_filter_and_table(area, app.order_filters.has_active_filters());
    if let Some(filter_area) = filter_area {
        render_order_filter_bar(f, filter_area, app);
    }

    let display_selected_idx =
        selected_book_display_idx(app.selected_order_id, &filtered).unwrap_or(0);

    let compact = table_area.width < 100;
    // Drop the header when height < 4 so at least one data row stays visible
    // (same short-terminal rule as Disputes Pending).
    let show_header = table_area.height >= 4;
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

    let rows: Vec<Row> = filtered
        .iter()
        .map(|(_orig, order)| {
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
                    .and_then(|ts| format_local_timestamp(ts, "%Y-%m-%d %H:%M"))
                    .unwrap_or_else(|| "Invalid date".to_string()),
            );

            if compact {
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
            }
        })
        .collect();

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
    let mut table = Table::new(rows, widths)
        .row_highlight_style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
        .block(
            Block::default()
                .title("Orders")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(PRIMARY_COLOR))
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );

    if show_header {
        let header_cells = header_labels
            .into_iter()
            .map(|label| Cell::from(label).style(Style::default().add_modifier(Modifier::BOLD)))
            .collect::<Vec<_>>();
        table = table.header(Row::new(header_cells));
    }

    app.orders_table_state.select(Some(display_selected_idx));
    f.render_stateful_widget(table, table_area, &mut app.orders_table_state);

    let header_rows = u16::from(show_header);
    let visible_rows = table_area.height.saturating_sub(2 + header_rows) as usize;
    render_table_list_scrollbar(
        f,
        table_area,
        row_count,
        visible_rows,
        header_rows,
        app.orders_table_state.offset(),
    );
}

fn split_filter_and_table(area: Rect, has_filters: bool) -> (Option<Rect>, Rect) {
    if !has_filters || area.height < 7 {
        return (None, area);
    }
    let chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Min(3)],
    )
    .split(area);
    (Some(chunks[0]), chunks[1])
}

fn render_order_filter_bar(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let summary = app.order_filters.summary();
    let hint = if area.width < 80 {
        "Shift+F filters | Shift+X clear"
    } else {
        "Shift+F: edit filters | Shift+X: clear filters | Enter: take/cancel selected order"
    };
    let text = vec![
        Line::from(vec![
            Span::styled("Filters: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(summary),
        ]),
        Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray))),
    ];
    f.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::default()
                .title("Order Filters")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(PRIMARY_COLOR))
                .style(Style::default().bg(BACKGROUND_COLOR)),
        ),
        area,
    );
}

fn premium_cell(premium: i64) -> Cell<'static> {
    let (text, color) = format_premium(premium);
    Cell::from(text).style(Style::default().fg(color))
}

pub fn render_order_filter_popup(f: &mut ratatui::Frame, state: &OrderBookFilterState) {
    let area = f.area();
    let popup = center_rect(
        area,
        72.min(area.width.saturating_sub(2)),
        15.min(area.height),
    );
    f.render_widget(Clear, popup);

    let rows = OrderBookFilterField::ALL
        .iter()
        .map(|field| {
            let selected = *field == state.focused;
            let style = if selected {
                Style::default().bg(PRIMARY_COLOR).fg(Color::Black)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(vec![
                Span::styled(if selected { ">" } else { " " }, style),
                Span::styled(format!(" {:<20}", field.label()), style),
                Span::styled(filter_field_value(state, *field), style),
            ])
        })
        .collect::<Vec<_>>();

    let mut lines = vec![
        Line::from(Span::styled(
            "Enter Apply | Esc Cancel | Up/Down Field",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Space Cycle kind | Shift+X Clear",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
    ];
    lines.extend(rows);

    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .title("Order Filters")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(PRIMARY_COLOR))
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            )
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn filter_field_value(state: &OrderBookFilterState, field: OrderBookFilterField) -> String {
    match field {
        OrderBookFilterField::Kind => state.filters.kind.label().to_string(),
        OrderBookFilterField::FiatCurrency => empty_display(&state.filters.fiat_code),
        OrderBookFilterField::FiatAmountMin => empty_display(&state.filters.fiat_amount_min),
        OrderBookFilterField::FiatAmountMax => empty_display(&state.filters.fiat_amount_max),
        OrderBookFilterField::PremiumMin => empty_display(&state.filters.premium_min),
        OrderBookFilterField::PremiumMax => empty_display(&state.filters.premium_max),
        OrderBookFilterField::PaymentMethod => empty_display(&state.filters.payment_method),
        OrderBookFilterField::CreatedWithinDays => {
            empty_display(&state.filters.created_within_days)
        }
    }
}

fn empty_display(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "Any".to_string()
    } else {
        trimmed.to_string()
    }
}

fn center_rect(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    area
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use uuid::Uuid;

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
            id: Some(Uuid::new_v4()),
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
        let mut app = AppState::new(UserRole::User);
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn orders_table_renders_premium_column() {
        let buf = render_at_width(130, -3);
        assert!(buffer_contains(&buf, "Premium"));
        assert!(buffer_contains(&buf, "-3%"));
        assert!(buffer_contains(&buf, "SEPA"));
        assert!(!buffer_contains(&buf, "Order Filters"));
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

    #[test]
    fn orders_table_shows_active_filter_summary() {
        let backend = TestBackend::new(130, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let orders = Arc::new(Mutex::new(vec![sample_order("SEPA", 2)]));
        let mut app = AppState::new(UserRole::User);
        app.order_filters.fiat_code = "USD".to_string();
        app.order_filters.payment_method = "sep".to_string();

        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Filters:"));
        assert!(buffer_contains(buf, "fiat=USD"));
        assert!(buffer_contains(buf, "payment~sep"));
    }

    #[test]
    fn order_filter_popup_lists_all_filter_fields() {
        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = OrderBookFilterState::default();
        state.filters.payment_method = "cash".to_string();

        terminal
            .draw(|f| render_order_filter_popup(f, &state))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Buy/Sell"));
        assert!(buffer_contains(buf, "Fiat currency"));
        assert!(buffer_contains(buf, "Fiat amount min"));
        assert!(buffer_contains(buf, "Premium max %"));
        assert!(buffer_contains(buf, "Payment method"));
        assert!(buffer_contains(buf, "Created within days"));
        assert!(buffer_contains(buf, "Shift+X Clear"));
        assert!(buffer_contains(buf, "cash"));
    }

    /// When more orders exist than table body rows, selecting a late row must
    /// scroll the stateful table so that marker is visible.
    #[test]
    fn orders_table_scrolls_to_keep_selected_row_visible() {
        let mut book = Vec::new();
        let mut last_id = Uuid::nil();
        for i in 0..40 {
            let o = sample_order(&format!("PAY-{i:02}"), 0);
            last_id = o.id.unwrap();
            book.push(o);
        }
        let orders = Arc::new(Mutex::new(book));
        let mut app = AppState::new(UserRole::User);
        app.selected_order_id = Some(last_id);
        // Height 6 keeps the Orders table compact enough to skip the filter bar while still
        // leaving body rows to scroll.
        let backend = TestBackend::new(130, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
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
        let mut book = Vec::new();
        let mut first_id = Uuid::nil();
        for i in 0..40 {
            let o = sample_order(&format!("PAY-{i:02}"), 0);
            if i == 0 {
                first_id = o.id.unwrap();
            }
            book.push(o);
        }
        let orders = Arc::new(Mutex::new(book));
        let mut app = AppState::new(UserRole::User);
        app.selected_order_id = Some(first_id);
        let backend = TestBackend::new(130, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
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

    /// Highlighted row after a currency filter must match what Enter would take:
    /// a hidden previous selection falls back to the first visible fiat.
    #[test]
    fn orders_table_highlights_visible_fallback_when_selection_filtered_out() {
        let usd_id = Uuid::new_v4();
        let eur_id = Uuid::new_v4();
        let orders = Arc::new(Mutex::new(vec![
            SmallOrder {
                id: Some(usd_id),
                kind: Some(mostro_core::order::Kind::Buy),
                fiat_code: "USD".to_string(),
                fiat_amount: 100,
                amount: 50_000,
                payment_method: "PAY-USD".to_string(),
                ..Default::default()
            },
            SmallOrder {
                id: Some(eur_id),
                kind: Some(mostro_core::order::Kind::Sell),
                fiat_code: "EUR".to_string(),
                fiat_amount: 200,
                amount: 60_000,
                payment_method: "PAY-EUR".to_string(),
                ..Default::default()
            },
        ]));
        let mut app = AppState::new(UserRole::User);
        app.selected_order_id = Some(usd_id);
        app.currencies_filter = vec!["EUR".to_string()];

        let backend = TestBackend::new(130, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "PAY-EUR"),
            "visible filtered order must appear"
        );
        assert!(
            !buffer_contains(buf, "PAY-USD"),
            "filtered-out USD order must not appear"
        );

        let selected =
            crate::ui::helpers::selected_filtered_book_order(&app, &orders.lock().unwrap())
                .expect("Enter resolves a visible order");
        assert_eq!(selected.id, Some(eur_id));
        assert_eq!(selected.payment_method, "PAY-EUR");
    }

    #[test]
    fn scrollbar_preserves_borders_and_header_when_scrolled() {
        let mut book = Vec::new();
        let mut last_id = Uuid::nil();
        for i in 0..40 {
            let o = sample_order(&format!("PAY-{i:02}"), 0);
            last_id = o.id.unwrap();
            book.push(o);
        }
        let orders = Arc::new(Mutex::new(book));
        let mut app = AppState::new(UserRole::User);
        app.selected_order_id = Some(last_id);

        let backend = TestBackend::new(130, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        let right = buf.area.width - 1;
        assert_eq!(buf[(right, 0)].symbol(), "╮", "top-right corner intact");
        assert_eq!(
            buf[(right, buf.area.height - 1)].symbol(),
            "╯",
            "bottom-right corner intact"
        );
        assert_eq!(
            buf[(right, 1)].symbol(),
            "│",
            "header row border must not be overwritten by the scrollbar"
        );
        assert!(
            buffer_contains(buf, "Premium"),
            "header must still render while scrolled"
        );
    }

    /// Selecting the last order must park the scrollbar thumb against the end
    /// cap (`▼`), with no empty track (`║`) between thumb and bottom.
    #[test]
    fn scrollbar_thumb_reaches_track_bottom_on_last_row() {
        let mut book = Vec::new();
        let mut last_id = Uuid::nil();
        for i in 0..40 {
            let o = sample_order(&format!("PAY-{i:02}"), 0);
            last_id = o.id.unwrap();
            book.push(o);
        }
        let orders = Arc::new(Mutex::new(book));
        let mut app = AppState::new(UserRole::User);
        app.selected_order_id = Some(last_id);

        // height 10 → borders+header leave 7 data rows; track y=2..8 with ▲…▼
        let backend = TestBackend::new(130, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        let right = buf.area.width - 1;
        let end_cap_y = buf.area.height - 2; // ▼ just above bottom border
        let above_end = end_cap_y - 1;
        assert_eq!(
            buf[(right, end_cap_y)].symbol(),
            "▼",
            "scrollbar end cap must sit on the last track row"
        );
        assert_eq!(
            buf[(right, above_end)].symbol(),
            "█",
            "thumb must reach the cell above ▼ when the last order is selected"
        );
    }

    #[test]
    fn short_area_drops_header_but_shows_selected_row() {
        let o = sample_order("PAY-SHORT", 0);
        let id = o.id.unwrap();
        let orders = Arc::new(Mutex::new(vec![o]));
        let mut app = AppState::new(UserRole::User);
        app.selected_order_id = Some(id);

        let backend = TestBackend::new(130, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(
            !buffer_contains(buf, "Premium"),
            "header should be dropped when the area is too short"
        );
        assert!(
            buffer_contains(buf, "PAY-SHORT"),
            "selected order row must be visible without the header"
        );
    }
}
