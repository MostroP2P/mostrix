use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use ratatui::layout::{Alignment, Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Table,
};

use crate::ui::currencies::{filter_options, resolve_options};
use crate::ui::helpers::{
    format_local_timestamp, format_premium, get_filtered_book_orders, render_table_list_scrollbar,
    selected_book_display_idx,
};
use crate::ui::orders::{orders_filter_inline_layout, OrderBookFilterField, OrderBookFilterState};
use crate::ui::{apply_kind_color, AppState, UiMode, BACKGROUND_COLOR, PRIMARY_COLOR};

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
    let use_inline = orders_filter_inline_layout(area.width, area.height);
    if let UiMode::OrderFilters(ref mut state) = app.mode {
        state.inline = use_inline;
    }
    let editing_filters = matches!(app.mode, UiMode::OrderFilters(_));
    let show_bar = if use_inline {
        true
    } else {
        app.order_filters.has_active_filters() && !editing_filters
    };
    let (filter_area, table_area) = split_filter_and_table(area, show_bar, use_inline);
    if let Some(filter_area) = filter_area {
        if let UiMode::OrderFilters(ref state) = app.mode {
            if state.inline {
                render_order_filter_editable_bar(f, filter_area, state);
            } else {
                render_order_filter_bar(f, filter_area, app);
            }
        } else {
            render_order_filter_bar(f, filter_area, app);
        }
    }

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
        f.render_widget(paragraph, table_area);
        if let (Some(filter_area), UiMode::OrderFilters(state)) = (filter_area, &app.mode) {
            if state.currency_picker.open {
                render_order_filter_currency_dropdown(f, filter_area, area, state);
            }
        }
        return;
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

    if let (Some(filter_area), UiMode::OrderFilters(state)) = (filter_area, &app.mode) {
        if state.currency_picker.open {
            render_order_filter_currency_dropdown(f, filter_area, area, state);
        }
    }
}

fn split_filter_and_table(area: Rect, show_bar: bool, use_inline: bool) -> (Option<Rect>, Rect) {
    let min_height = if use_inline { 8 } else { 7 };
    if !show_bar || area.height < min_height {
        return (None, area);
    }
    let bar_h = 4;
    let chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(bar_h), Constraint::Min(3)],
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
            Span::styled(
                "Filters: ",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(summary, Style::default().fg(Color::White)),
        ]),
        Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray))),
    ];
    f.render_widget(
        Paragraph::new(text).block(
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

fn render_order_filter_editable_bar(
    f: &mut ratatui::Frame,
    area: Rect,
    state: &OrderBookFilterState,
) {
    let block = Block::default()
        .title(" Order Filters (editing) ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::new(
        Direction::Vertical,
        [Constraint::Length(1), Constraint::Length(1)],
    )
    .split(inner);

    let chip = |field: OrderBookFilterField| -> Span<'static> {
        let selected = field == state.focused;
        let (value, value_color) = filter_field_display_colored(state, field);
        let label = match field {
            OrderBookFilterField::Kind => "Kind",
            OrderBookFilterField::FiatCurrency => "Fiat",
            OrderBookFilterField::Premium => "Premium",
        };
        let text = format!("{label}:{value}");
        if selected {
            Span::styled(
                format!("[{text}]"),
                Style::default()
                    .fg(Color::Black)
                    .bg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!(" {text} "), Style::default().fg(value_color))
        }
    };

    f.render_widget(
        Paragraph::new(Line::from(vec![
            chip(OrderBookFilterField::Kind),
            Span::raw(" "),
            chip(OrderBookFilterField::FiatCurrency),
            Span::raw(" "),
            chip(OrderBookFilterField::Premium),
        ])),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            "Tab field • ↑↓ rotate/step • Space/type Fiat • Backspace clear • Enter apply • Esc done",
            Style::default().fg(Color::DarkGray),
        )),
        rows[1],
    );
}

fn filter_field_display_colored(
    state: &OrderBookFilterState,
    field: OrderBookFilterField,
) -> (String, Color) {
    match field {
        OrderBookFilterField::Kind => (state.filters.kind.label().to_string(), Color::Gray),
        OrderBookFilterField::FiatCurrency => {
            let code = state.filters.fiat_code.trim();
            if code.is_empty() {
                ("Any▾".to_string(), Color::Gray)
            } else {
                (format!("{}▾", code.to_ascii_uppercase()), Color::White)
            }
        }
        OrderBookFilterField::Premium => (
            crate::ui::orders::order_book_premium_label(state.filters.premium),
            crate::ui::orders::order_book_premium_color(state.filters.premium),
        ),
    }
}

fn render_order_filter_currency_dropdown(
    f: &mut ratatui::Frame,
    anchor: Rect,
    bounds: Rect,
    state: &OrderBookFilterState,
) {
    let options = resolve_options(&[]);
    let filtered = filter_options(&options, &state.currency_picker.filter);
    let selected = state
        .currency_picker
        .selected
        .min(filtered.len().saturating_sub(1));
    render_filter_list_dropdown(
        f,
        anchor,
        bounds,
        &format!(" Currency ({} common) ", options.len()),
        filtered
            .iter()
            .map(|o| {
                if o.name.is_empty() {
                    o.code.clone()
                } else {
                    format!("{:<5}{}", o.code, o.name)
                }
            })
            .collect(),
        selected,
        "type filter • ↑↓ • Enter select • Esc",
    );
}

fn render_filter_list_dropdown(
    f: &mut ratatui::Frame,
    anchor: Rect,
    bounds: Rect,
    title: &str,
    rows: Vec<String>,
    selected: usize,
    hint: &str,
) {
    let x = anchor.x.saturating_add(2);
    let max_width = (bounds.x + bounds.width).saturating_sub(x);
    let width = 42u16.clamp(24, max_width.max(24)).min(max_width);
    let content_rows = rows.len().clamp(1, 8) as u16;
    let height = content_rows + 3;
    let mut y = anchor.y + anchor.height;
    if y + height > bounds.y + bounds.height {
        y = anchor.y.saturating_sub(height);
    }
    let popup = Rect {
        x,
        y,
        width,
        height: height.min(bounds.y + bounds.height - y),
    };
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);
    let split = Layout::new(
        Direction::Vertical,
        [Constraint::Min(1), Constraint::Length(1)],
    )
    .split(inner);

    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  no match",
                Style::default().fg(Color::DarkGray),
            )),
            split[0],
        );
    } else {
        let row_count = rows.len();
        let items: Vec<ListItem> = rows.into_iter().map(ListItem::new).collect();
        let list = List::new(items)
            .style(Style::default().fg(Color::White).bg(BACKGROUND_COLOR))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› ");
        let mut list_state = ListState::default().with_selected(Some(selected));
        f.render_stateful_widget(list, split[0], &mut list_state);
        if row_count > split[0].height as usize {
            let mut sb_state = ScrollbarState::new(row_count).position(selected);
            f.render_stateful_widget(
                Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
                split[0],
                &mut sb_state,
            );
        }
    }
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
        split[1],
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
        76.min(area.width.saturating_sub(2)),
        16.min(area.height),
    );
    f.render_widget(Clear, popup);

    let inner_width = popup.width.saturating_sub(2) as usize;
    let inner_height = popup.height.saturating_sub(2);
    let compact = popup.height < 14 || popup.width < 56;
    let lines = if compact {
        compact_order_filter_popup_lines(state, inner_width, inner_height)
    } else {
        full_order_filter_popup_lines(state, inner_width)
    };

    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Left).block(
            Block::default()
                .title("Order Filters")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(PRIMARY_COLOR))
                .style(Style::default().bg(BACKGROUND_COLOR)),
        ),
        popup,
    );

    if state.currency_picker.open {
        render_order_filter_currency_dropdown(f, popup, area, state);
    }
}

fn full_order_filter_popup_lines(
    state: &OrderBookFilterState,
    inner_width: usize,
) -> Vec<Line<'static>> {
    let label_width = 20;
    let value_width = inner_width.saturating_sub(label_width + 3);
    let mut lines = vec![
        shortcut_line(&[("Enter", "Apply"), ("Esc", "Cancel"), ("Tab", "Field")]),
        shortcut_line(&[
            ("↑↓", "Rotate"),
            ("Space", "Fiat picker"),
            ("Shift+X", "Clear"),
        ]),
        active_filters_line(state, inner_width),
        Line::from(""),
    ];
    lines.extend(
        OrderBookFilterField::ALL
            .iter()
            .map(|field| filter_field_line(state, *field, label_width, value_width)),
    );
    lines.push(Line::from(""));
    lines.push(focused_field_hint_line(state.focused, inner_width));
    lines
}

fn compact_order_filter_popup_lines(
    state: &OrderBookFilterState,
    inner_width: usize,
    inner_height: u16,
) -> Vec<Line<'static>> {
    let label = state.focused.label();
    let value = compact_field_value(state, inner_width);
    let mut lines = vec![shortcut_line(&[("Enter", "Apply"), ("Esc", "Cancel")])];

    if inner_height > 4 {
        lines.push(shortcut_line(&[
            ("Tab", "Field"),
            ("↑↓", "Rotate"),
            ("Shift+X", "Clear"),
        ]));
    } else {
        lines.push(shortcut_line(&[("Shift+X", "Clear")]));
    }

    lines.push(Line::from(Span::styled(
        label,
        Style::default()
            .fg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD),
    )));
    let (_, value_color) = filter_field_display_colored(state, state.focused);
    lines.push(Line::from(vec![
        Span::styled("Value: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            value,
            Style::default()
                .fg(value_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    if inner_height > 5 {
        lines.push(focused_field_hint_line(state.focused, inner_width));
    }

    lines
}

fn active_filters_line(state: &OrderBookFilterState, inner_width: usize) -> Line<'static> {
    if state.filters.has_active_filters() {
        let summary_width = inner_width.saturating_sub("Active: ".len());
        Line::from(vec![
            Span::styled("Active: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_for_cell(&state.filters.summary(), summary_width)
                    .trim_end()
                    .to_string(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(Span::styled(
            truncate_for_cell(
                "Active: no filters. Set fields below, then press Enter.",
                inner_width,
            )
            .trim_end()
            .to_string(),
            Style::default().fg(Color::DarkGray),
        ))
    }
}

fn shortcut_line(items: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (idx, (key, label)) in items.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
        }
        spans.push(Span::styled(
            *key,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

fn filter_field_line(
    state: &OrderBookFilterState,
    field: OrderBookFilterField,
    label_width: usize,
    value_width: usize,
) -> Line<'static> {
    Line::from(filter_field_spans(state, field, label_width, value_width))
}

fn filter_field_spans(
    state: &OrderBookFilterState,
    field: OrderBookFilterField,
    label_width: usize,
    value_width: usize,
) -> Vec<Span<'static>> {
    let selected = field == state.focused;
    let row_style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let label_style = if selected {
        row_style
    } else {
        Style::default().fg(Color::Gray)
    };
    let value = filter_field_value(state, field);
    let premium_color = if field == OrderBookFilterField::Premium {
        crate::ui::orders::order_book_premium_color(state.filters.premium)
    } else {
        Color::White
    };
    let value_style = if value == "Any" {
        if selected {
            row_style
        } else {
            Style::default().fg(Color::DarkGray)
        }
    } else if selected {
        row_style
    } else {
        Style::default()
            .fg(premium_color)
            .add_modifier(Modifier::BOLD)
    };
    let value = if value_width == 0 {
        value
    } else {
        truncate_for_cell(&value, value_width)
    };

    vec![
        Span::styled(if selected { "> " } else { "  " }, row_style),
        Span::styled(format!("{:<label_width$}", field.label()), label_style),
        Span::styled(" ", row_style),
        Span::styled(value, value_style),
    ]
}

fn focused_field_hint_line(field: OrderBookFilterField, inner_width: usize) -> Line<'static> {
    let hint = match field {
        OrderBookFilterField::Kind => "↑↓ or Space cycles Any / Buy / Sell.",
        OrderBookFilterField::FiatCurrency => {
            "↑↓ rotates currencies (opens picker). Space or type to search. Backspace clears."
        }
        OrderBookFilterField::Premium => {
            "↑↓ step premium by 1% (+ green / 0 white / − red). Backspace clears to Any."
        }
    };
    Line::from(vec![
        Span::styled("Hint: ", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(
            truncate_for_cell(hint, inner_width.saturating_sub("Hint: ".len()))
                .trim_end()
                .to_string(),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

fn truncate_for_cell(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut chars = value.chars();
    let mut out: String = chars.by_ref().take(width).collect();
    if chars.next().is_some() && width > 1 {
        out.pop();
        out.push('.');
    }
    format!("{out:<width$}")
}

fn compact_field_value(state: &OrderBookFilterState, inner_width: usize) -> String {
    let value = filter_field_value(state, state.focused);
    truncate_for_cell(&value, inner_width.saturating_sub("Value: ".len()))
        .trim_end()
        .to_string()
}

fn filter_field_value(state: &OrderBookFilterState, field: OrderBookFilterField) -> String {
    match field {
        OrderBookFilterField::Kind => state.filters.kind.label().to_string(),
        OrderBookFilterField::FiatCurrency => {
            let code = state.filters.fiat_code.trim();
            if code.is_empty() {
                "Any ▾".to_string()
            } else {
                format!("{} ▾", code.to_ascii_uppercase())
            }
        }
        OrderBookFilterField::Premium => {
            crate::ui::orders::order_book_premium_label(state.filters.premium)
        }
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

    use crate::ui::orders::OrderBookFilters;
    use crate::ui::{UiMode, UserRole};

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
        app.order_filters.premium = Some(3);

        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Filters:"));
        assert!(buffer_contains(buf, "fiat=USD"));
        assert!(buffer_contains(buf, "premium=+3%"));
        assert!(buffer_contains(buf, "Shift+F: edit filters"));
    }

    #[test]
    fn orders_table_keeps_filter_summary_when_filters_match_no_orders() {
        let backend = TestBackend::new(130, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let orders = Arc::new(Mutex::new(vec![sample_order("SEPA", 2)]));
        let mut app = AppState::new(UserRole::User);
        app.order_filters.fiat_code = "EUR".to_string();

        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Filters:"));
        assert!(buffer_contains(buf, "fiat=EUR"));
        assert!(buffer_contains(buf, "No offers match"));
    }

    #[test]
    fn order_filter_popup_lists_kind_fiat_and_premium() {
        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = OrderBookFilterState::default();
        state.filters.premium = Some(-2);

        terminal
            .draw(|f| render_order_filter_popup(f, &state))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Buy/Sell"));
        assert!(buffer_contains(buf, "Fiat currency"));
        assert!(buffer_contains(buf, "Premium"));
        assert!(buffer_contains(buf, "-2%"));
        assert!(buffer_contains(buf, "Shift+X Clear"));
        assert!(!buffer_contains(buf, "Payment method"));
        assert!(!buffer_contains(buf, "Created within"));
        assert!(!buffer_contains(buf, "Fiat amount"));
    }

    #[test]
    fn short_order_filter_popup_keeps_focused_premium_visible() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = OrderBookFilterState {
            focused: OrderBookFilterField::Premium,
            filters: OrderBookFilters {
                premium: Some(0),
                ..Default::default()
            },
            ..Default::default()
        };

        terminal
            .draw(|f| render_order_filter_popup(f, &state))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Premium"));
        assert!(buffer_contains(buf, "0%"));
        assert!(buffer_contains(buf, "Shift+X Clear"));
    }

    #[test]
    fn narrow_order_filter_popup_keeps_focused_fiat_visible() {
        let backend = TestBackend::new(40, 18);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = OrderBookFilterState {
            focused: OrderBookFilterField::FiatCurrency,
            filters: OrderBookFilters {
                fiat_code: "EUR".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        terminal
            .draw(|f| render_order_filter_popup(f, &state))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Fiat currency"));
        assert!(buffer_contains(buf, "EUR"));
        assert!(buffer_contains(buf, "Shift+X"));
    }

    #[test]
    fn tiny_order_filter_popup_keeps_clear_shortcut_and_value_visible() {
        let backend = TestBackend::new(36, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = OrderBookFilterState {
            focused: OrderBookFilterField::Premium,
            filters: OrderBookFilters {
                premium: Some(3),
                ..Default::default()
            },
            ..Default::default()
        };

        terminal
            .draw(|f| render_order_filter_popup(f, &state))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Shift+X Clear"));
        assert!(buffer_contains(buf, "Premium"));
        assert!(buffer_contains(buf, "+3%"));
    }

    #[test]
    fn order_filter_popup_shows_active_fiat_in_summary() {
        let backend = TestBackend::new(78, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = OrderBookFilterState {
            focused: OrderBookFilterField::FiatCurrency,
            filters: OrderBookFilters {
                fiat_code: "USD".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        terminal
            .draw(|f| render_order_filter_popup(f, &state))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "fiat=USD"));
        assert!(buffer_contains(buf, "USD ▾"));
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

    #[test]
    fn large_terminal_shows_editable_inline_filter_bar_when_editing() {
        let backend = TestBackend::new(130, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let orders = Arc::new(Mutex::new(vec![sample_order("SEPA", 2)]));
        let mut app = AppState::new(UserRole::User);
        app.mode = UiMode::OrderFilters(OrderBookFilterState::from_filters(
            OrderBookFilters {
                fiat_code: "USD".to_string(),
                ..Default::default()
            },
            true,
        ));

        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Order Filters (editing)"));
        assert!(buffer_contains(buf, "Kind:"));
        assert!(buffer_contains(buf, "Fiat:USD▾"));
        assert!(buffer_contains(buf, "Premium:"));
        assert!(buffer_contains(buf, "Tab field"));
        assert!(!buffer_contains(buf, "Shift+F: edit filters"));
    }

    #[test]
    fn small_terminal_hides_inline_bar_while_filter_popup_is_open() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let orders = Arc::new(Mutex::new(vec![sample_order("SEPA", 2)]));
        let mut app = AppState::new(UserRole::User);
        app.order_filters.fiat_code = "USD".to_string();
        app.mode = UiMode::OrderFilters(OrderBookFilterState::from_filters(
            app.order_filters.clone(),
            false,
        ));

        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(!buffer_contains(buf, "Order Filters (editing)"));
        assert!(!buffer_contains(buf, "Filters:"));
        assert!(buffer_contains(buf, "SEPA"));
    }

    #[test]
    fn empty_filter_results_keep_inline_bar_visible() {
        let backend = TestBackend::new(130, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let orders = Arc::new(Mutex::new(vec![sample_order("SEPA", 2)]));
        let mut app = AppState::new(UserRole::User);
        app.order_filters.fiat_code = "EUR".to_string();
        app.mode = UiMode::OrderFilters(OrderBookFilterState::from_filters(
            app.order_filters.clone(),
            true,
        ));

        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Order Filters (editing)"));
        assert!(buffer_contains(buf, "Fiat:EUR▾"));
        assert!(buffer_contains(buf, "No offers match"));
    }

    #[test]
    fn currency_dropdown_overlay_renders_when_picker_open() {
        let backend = TestBackend::new(130, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let orders = Arc::new(Mutex::new(vec![sample_order("SEPA", 2)]));
        let mut app = AppState::new(UserRole::User);
        let mut state = OrderBookFilterState::from_filters(OrderBookFilters::default(), true);
        state.focused = OrderBookFilterField::FiatCurrency;
        state.currency_picker.open = true;
        state.currency_picker.filter = "EUR".to_string();
        app.mode = UiMode::OrderFilters(state);

        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Currency"));
        assert!(buffer_contains(buf, "EUR"));
        assert!(buffer_contains(buf, "type filter"));
    }

    #[test]
    fn inline_bar_shows_colored_premium_band_label() {
        let backend = TestBackend::new(130, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let orders = Arc::new(Mutex::new(vec![sample_order("SEPA", 2)]));
        let mut app = AppState::new(UserRole::User);
        app.mode = UiMode::OrderFilters(OrderBookFilterState::from_filters(
            OrderBookFilters {
                premium: Some(-2),
                ..Default::default()
            },
            true,
        ));

        terminal
            .draw(|f| render_orders_tab(f, f.area(), &orders, &mut app))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Premium:-2%"));
        assert!(!buffer_contains(buf, "Pay:"));
        assert!(!buffer_contains(buf, "Days:"));
    }

    #[test]
    fn order_filter_popup_shows_dropdown_hint_on_fiat() {
        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = OrderBookFilterState {
            focused: OrderBookFilterField::FiatCurrency,
            ..Default::default()
        };

        terminal
            .draw(|f| render_order_filter_popup(f, &state))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Fiat currency"));
        assert!(buffer_contains(buf, "▾") || buffer_contains(buf, "Space"));
    }
}
