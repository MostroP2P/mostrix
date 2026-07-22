//! Messages tab: order list sidebar and trade timeline detail panel.

use chrono::{DateTime, Utc};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, LineGauge, List, ListItem, Padding, Paragraph, Wrap,
};

use mostro_core::prelude::{Payload, SmallOrder};

use crate::ui::helpers;
use crate::ui::orders::{
    listing_timeline_labels, message_action_compact_label_for_message, message_action_emoji,
    message_order_kind_label, message_timeline_warning, message_timeline_warning_for_order_status,
    message_trade_timeline_step, waiting_phase_description, FlowStep, StepLabel,
};
use crate::ui::{OrderMessage, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_messages_tab(
    f: &mut ratatui::Frame,
    area: Rect,
    messages: &[OrderMessage],
    selected_idx: usize,
) {
    let block = Block::default()
        .title(sidebar_title(messages))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));

    f.render_widget(block.clone(), area);
    let inner = block.inner(area);

    if messages.is_empty() {
        render_empty_state(f, inner);
        return;
    }

    let selected_idx = selected_idx.min(messages.len().saturating_sub(1));
    let selected_msg = &messages[selected_idx];

    let columns = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(36), Constraint::Percentage(64)],
    )
    .split(inner);

    let left_chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Min(0), Constraint::Length(1)],
    )
    .split(columns[0]);

    let separator_width = left_chunks[0].width as usize;
    let items = build_sidebar_items(messages, selected_idx, separator_width);

    let list = List::new(items)
        .highlight_style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
        .highlight_symbol("▶ ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    f.render_stateful_widget(
        list,
        left_chunks[0],
        &mut ratatui::widgets::ListState::default().with_selected(Some(selected_idx)),
    );

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("↑↓", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" move · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" open · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Ctrl+H", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" help", Style::default().fg(Color::DarkGray)),
    ]))
    .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(footer, left_chunks[1]);

    render_message_timeline_panel(f, columns[1], selected_msg);
}

/// Sidebar title with total trade count and, when present, an unread badge.
fn sidebar_title(messages: &[OrderMessage]) -> Line<'static> {
    let unread = messages.iter().filter(|m| !m.read).count();
    let mut spans = vec![Span::styled(
        format!(" 📨 My Trades ({}) ", messages.len()),
        Style::default()
            .fg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD),
    )];
    if unread > 0 {
        spans.push(Span::styled("· ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(
            format!("● {unread} new "),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

/// Emoji + color for the order-kind dot shown on each sidebar row.
fn kind_dot(kind_label: &str) -> (&'static str, Color) {
    match kind_label {
        "BUY" => ("🟢", Color::Green),
        "SELL" => ("🔴", Color::Red),
        _ => ("⚪", Color::DarkGray),
    }
}

/// Builds the three-line-plus-separator sidebar rows: kind/id, action, relative time/unread.
fn build_sidebar_items(
    messages: &[OrderMessage],
    selected_idx: usize,
    separator_width: usize,
) -> Vec<ListItem<'static>> {
    let last_idx = messages.len().saturating_sub(1);
    messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            let is_selected = idx == selected_idx;
            let base_style = if is_selected {
                Style::default()
                    .bg(PRIMARY_COLOR)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else if !msg.read {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let kind_label = message_order_kind_label(msg);
            let (dot, kind_color) = kind_dot(kind_label);
            let kind_style = if is_selected {
                base_style
            } else {
                Style::default().fg(kind_color).add_modifier(Modifier::BOLD)
            };

            let short_id = helpers::short_order_id(msg.order_id);
            let line1 = Line::from(vec![
                Span::styled(format!("{dot} "), kind_style),
                Span::styled(format!("{kind_label:<4} "), kind_style),
                Span::styled(short_id, base_style),
            ]);

            let action = msg.message.get_inner_message_kind().action.clone();
            let emoji = message_action_emoji(&action);
            let action_label = message_action_compact_label_for_message(msg);
            let line2 = Line::from(vec![
                Span::styled(format!("  {emoji} "), base_style),
                Span::styled(action_label.to_string(), base_style),
            ]);

            let time = helpers::relative_time_compact(msg.timestamp);
            let mut line3_spans = vec![
                Span::styled("  🕐 ", base_style),
                Span::styled(time, base_style),
            ];
            if !msg.read {
                line3_spans.push(Span::styled(
                    " · unread ",
                    Style::default().fg(Color::DarkGray),
                ));
                line3_spans.push(Span::styled("●", Style::default().fg(Color::Yellow)));
            }
            let line3 = Line::from(line3_spans);

            let mut lines = vec![line1, line2, line3];
            if idx != last_idx {
                lines.push(Line::from(Span::styled(
                    "─".repeat(separator_width.max(1)),
                    Style::default().fg(Color::DarkGray),
                )));
            }

            ListItem::new(lines)
        })
        .collect()
}

/// Empty state: a centered mailbox glyph over a short "no trades yet" message
/// pointing at the Orders tab. On narrow/short panels the art is dropped so the
/// text stays readable (readability over decoration).
fn render_empty_state(f: &mut ratatui::Frame, area: Rect) {
    let art = helpers::MAILBOX_EMPTY_ART;
    let art_w = art.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;

    // Text lines wrap (unlike the art), so the message stays readable when narrow.
    let text_lines = vec![
        Line::from(Span::styled(
            "Your mailbox is empty",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "No trades yet — messages from your orders will appear here.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Go to the Orders tab to create or take an order",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    let show_art = area.width >= art_w && area.height >= art.len() as u16 + 4;
    // Reserve a few rows for text (it may wrap to 2 lines on narrow panels).
    let text_h = 4u16;
    let block_h = if show_art {
        art.len() as u16 + 1 + text_h
    } else {
        text_h
    };
    let mut y = area.y + area.height.saturating_sub(block_h) / 2;

    if show_art {
        let art_area = Rect {
            x: area.x,
            y,
            width: area.width,
            height: art.len() as u16,
        };
        helpers::render_centered_lines(f, art_area, art, style_mailbox_art);
        y = y.saturating_add(art.len() as u16 + 1);
    }

    let text_area = Rect {
        x: area.x,
        y,
        width: area.width,
        height: area.height.saturating_sub(y.saturating_sub(area.y)),
    };
    f.render_widget(
        Paragraph::new(text_lines)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true }),
        text_area,
    );
}

fn style_mailbox_art(line: &str) -> Vec<Span<'static>> {
    vec![Span::styled(
        line.to_string(),
        Style::default().fg(Color::DarkGray),
    )]
}

fn render_message_timeline_panel(f: &mut ratatui::Frame, area: Rect, selected_msg: &OrderMessage) {
    // Panel order mirrors the target mockup: header, progress stepper, TRADE
    // snapshot (fills the freed space), then the STATUS banner at the bottom.
    // On narrow panels the STATUS text wraps onto more lines, so give it extra
    // height there so the "what's next" copy stays fully readable.
    let narrow = !use_two_column_trade(area.width.saturating_sub(2));
    let status_height = if narrow { 7 } else { 4 };
    let right_chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Min(0),
            Constraint::Length(status_height),
        ],
    )
    .split(area);

    render_header_card(f, right_chunks[0], selected_msg);

    let step_labels = listing_timeline_labels(selected_msg);
    render_trade_stepper(
        f,
        right_chunks[1],
        message_trade_timeline_step(selected_msg),
        &step_labels,
    );

    render_trade_snapshot_card(f, right_chunks[2], selected_msg);
    render_status_card(f, right_chunks[3], selected_msg);
}

/// STATUS banner: a one-line status (normal path / canceled / dispute) and, on
/// the happy path, a `👉 Next:` callout describing the pending action. Replaces
/// the old "State" box and the redundant numbered timeline. Text wraps, so it
/// stays readable on narrow panels (which also get extra height from the caller).
fn render_status_card(f: &mut ratatui::Frame, area: Rect, msg: &OrderMessage) {
    let block = Block::default()
        .title(Span::styled(
            " STATUS ",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(area);
    f.render_widget(&block, area);

    let (emoji, text, color, is_normal) = status_banner(msg);
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{emoji} "), Style::default().fg(color)),
        Span::styled(
            text,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])];
    if is_normal {
        lines.push(Line::from(vec![
            Span::styled(
                "👉 Next: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                waiting_phase_description(msg).to_string(),
                Style::default().fg(Color::Gray),
            ),
        ]));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// Status banner content: `(emoji, text, color, is_normal)`. Prefers the
/// persisted order status, then the last action, to classify canceled/dispute
/// vs the happy path. `is_normal` gates the "what's next" callout.
fn status_banner(msg: &OrderMessage) -> (&'static str, String, Color, bool) {
    let action = msg.message.get_inner_message_kind().action.clone();
    let warning = message_timeline_warning_for_order_status(msg.order_status)
        .or_else(|| message_timeline_warning(&action));
    match warning {
        Some(text) if text.contains("dispute") => ("⚖️", text.to_string(), Color::Magenta, false),
        Some(text) => ("❌", text.to_string(), Color::Red, false),
        None => (
            "✅",
            "Trade is on the normal path".to_string(),
            Color::Green,
            true,
        ),
    }
}

/// Header card: order id + kind badge + maker/taker role chip, plus the absolute
/// and relative last-update time.
fn render_header_card(f: &mut ratatui::Frame, area: Rect, msg: &OrderMessage) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(area);
    f.render_widget(&block, area);

    let kind_label = message_order_kind_label(msg);
    let (dot, kind_color) = kind_dot(kind_label);
    let (role_emoji, role_label) = role_chip(msg.is_mine);

    let line1 = Line::from(vec![
        Span::styled("🧾 Order ", Style::default().fg(Color::Gray)),
        Span::styled(
            helpers::short_order_id(msg.order_id),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            format!("{dot} {kind_label}"),
            Style::default().fg(kind_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("   "),
        Span::styled(
            format!("{role_emoji} {role_label}"),
            Style::default().fg(Color::Cyan),
        ),
    ]);

    let absolute = DateTime::<Utc>::from_timestamp(msg.timestamp, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let relative = helpers::relative_time_compact(msg.timestamp);
    let line2 = Line::from(Span::styled(
        format!("Last update: {absolute} ({relative})"),
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(Paragraph::new(vec![line1, line2]), inner);
}

/// TRADE snapshot card: a receipt of the order payload (fiat, sats, premium,
/// method, trade index, role). Values gracefully fall back to `—`. Renders two
/// columns when there is room, and stacks into a single column on narrow panels
/// so nothing is squeezed off-screen.
fn render_trade_snapshot_card(f: &mut ratatui::Frame, area: Rect, msg: &OrderMessage) {
    let block = Block::default()
        .title(Span::styled(
            " TRADE ",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(area);
    f.render_widget(&block, area);

    let inner_kind = msg.message.get_inner_message_kind();
    let order = match &inner_kind.payload {
        Some(Payload::Order(order)) => Some(order),
        _ => None,
    };

    let (premium, premium_color) = premium_display(order);
    let (role_emoji, role_label) = role_chip(msg.is_mine);
    let white = Style::default().fg(Color::White);

    let fiat = snapshot_field("💰", "Fiat", fiat_display(order), white);
    let sats = snapshot_field("⚡", "Sats", sats_display(order, msg.sat_amount), white);
    let trade_idx = snapshot_field("🔑", "Trade idx", msg.trade_index.to_string(), white);
    let premium = snapshot_field("📈", "Premium", premium, Style::default().fg(premium_color));
    let method = snapshot_field("🧾", "Method", method_display(order), white);
    let role = snapshot_field(
        role_emoji,
        "Role",
        role_label.to_string(),
        Style::default().fg(Color::Cyan),
    );

    if use_two_column_trade(inner.width) {
        let cols = Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        )
        .split(inner);
        f.render_widget(Paragraph::new(vec![fiat, sats, trade_idx]), cols[0]);
        f.render_widget(Paragraph::new(vec![premium, method, role]), cols[1]);
    } else {
        // Narrow panel: stack every field in one readable column.
        f.render_widget(
            Paragraph::new(vec![fiat, sats, premium, method, trade_idx, role]),
            inner,
        );
    }
}

/// One `emoji label value` row inside the TRADE snapshot card. The leading emoji
/// stays at the start of the row (kept out of the aligned label/value columns) so
/// double-width glyphs never break alignment.
fn snapshot_field(emoji: &str, label: &str, value: String, value_style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {emoji} "), Style::default().fg(Color::Gray)),
        Span::styled(format!("{label:<10}"), Style::default().fg(Color::DarkGray)),
        Span::styled(value, value_style),
    ])
}

/// Maker/taker chip from [`OrderMessage::is_mine`] (`is_mine` == "I am the maker").
fn role_chip(is_mine: Option<bool>) -> (&'static str, &'static str) {
    match is_mine {
        Some(true) => ("👤", "Maker"),
        Some(false) => ("🤝", "Taker"),
        None => ("❔", "—"),
    }
}

/// Fiat amount (or min–max range) with currency code; `—` when no order payload.
fn fiat_display(order: Option<&SmallOrder>) -> String {
    let Some(order) = order else {
        return "—".to_string();
    };
    let code = order.fiat_code.trim().to_ascii_uppercase();
    let value = match (order.min_amount, order.max_amount) {
        (Some(min), Some(max)) if min > 0 && max > 0 => format!("{min}–{max}"),
        _ if order.fiat_amount > 0 => order.fiat_amount.to_string(),
        _ => String::new(),
    };
    match (value.is_empty(), code.is_empty()) {
        (true, true) => "—".to_string(),
        (true, false) => code,
        (false, true) => value,
        (false, false) => format!("{value} {code}"),
    }
}

/// Sats for the trade: prefer the message `sat_amount`, then the order amount,
/// showing `market` for a 0-amount (market-price) order and `—` when unknown.
fn sats_display(order: Option<&SmallOrder>, sat_amount: Option<i64>) -> String {
    if let Some(sats) = sat_amount {
        if sats > 0 {
            return group_thousands(&sats.to_string());
        }
    }
    match order {
        Some(order) if order.amount > 0 => group_thousands(&order.amount.to_string()),
        Some(_) => "market".to_string(),
        None => "—".to_string(),
    }
}

/// Premium string + color: `+p%` green, `p%` red, `0%` gray, `—` when absent.
fn premium_display(order: Option<&SmallOrder>) -> (String, Color) {
    match order {
        None => ("—".to_string(), Color::DarkGray),
        Some(order) => match order.premium {
            0 => ("0%".to_string(), Color::Gray),
            p if p > 0 => (format!("+{p}%"), Color::Green),
            p => (format!("{p}%"), Color::Red),
        },
    }
}

/// Payment method, or `—` when absent/empty.
fn method_display(order: Option<&SmallOrder>) -> String {
    match order {
        Some(order) if !order.payment_method.trim().is_empty() => {
            order.payment_method.trim().to_string()
        }
        _ => "—".to_string(),
    }
}

/// Group an integer string into thousands (e.g. `142857` → `142,857`).
fn group_thousands(raw: &str) -> String {
    let trimmed = raw.trim();
    let (sign, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", trimmed),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return raw.to_string();
    }
    let mut out = String::new();
    let n = digits.len();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}{out}")
}

/// Compact progress stepper: a single-line colored glyph track
/// (`✔──✔──◉──○──○──○`) with the step labels underneath, plus a `LineGauge`
/// showing `Step N of 6`. Render-only; `FlowStep`/label logic is unchanged.
fn render_trade_stepper(
    f: &mut ratatui::Frame,
    area: Rect,
    current_step: FlowStep,
    steps: &[StepLabel; 6],
) {
    let current = current_step.step_number();

    let block = Block::default()
        .title(Span::styled(
            " PROGRESS ",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(area);
    f.render_widget(&block, area);

    if use_full_progress(inner.width) {
        render_progress_full(f, inner, current, steps);
    } else {
        render_progress_compact(f, inner, current, steps);
    }
}

/// Progress gauge widget (`Step N of 6` + `▰▱` bar) shared by both layouts.
fn progress_gauge(current: usize, total: usize) -> LineGauge<'static> {
    let ratio = (current as f64 / total as f64).clamp(0.0, 1.0);
    LineGauge::default()
        .filled_symbol("▰")
        .unfilled_symbol("▱")
        .filled_style(Style::default().fg(PRIMARY_COLOR))
        .unfilled_style(Style::default().fg(Color::DarkGray))
        .ratio(ratio)
        .label(Span::styled(
            format!("Step {current} of {total} "),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))
}

/// Render the glyph track across `area` as six columns, optionally with the
/// top/bottom `StepLabel` words centered beneath each glyph.
fn render_glyph_columns(
    f: &mut ratatui::Frame,
    area: Rect,
    current: usize,
    steps: &[StepLabel; 6],
    with_labels: bool,
) {
    let step_columns = Layout::new(Direction::Horizontal, [Constraint::Ratio(1, 6); 6]).split(area);
    for (idx, step_label) in steps.iter().enumerate() {
        let (glyph, style) = step_glyph(idx + 1, current);
        let width = step_columns[idx].width as usize;
        let mut lines = vec![glyph_cell_line(
            width,
            glyph,
            style,
            idx == 0,
            idx == steps.len() - 1,
        )];
        if with_labels {
            lines.push(Line::from(Span::styled(
                center_in(step_label.top, width),
                style,
            )));
            lines.push(Line::from(Span::styled(
                center_in(step_label.bottom, width),
                style,
            )));
        }
        f.render_widget(Paragraph::new(lines), step_columns[idx]);
    }
}

/// Wide layout: glyph track + per-step labels on the left, gauge on the right.
fn render_progress_full(
    f: &mut ratatui::Frame,
    inner: Rect,
    current: usize,
    steps: &[StepLabel; 6],
) {
    let halves = Layout::new(
        Direction::Horizontal,
        [Constraint::Min(0), Constraint::Length(20)],
    )
    .split(inner);
    render_glyph_columns(f, halves[0], current, steps, true);
    // Gauge on the top row of its column, aligned with the glyph track.
    let gauge_area = Rect {
        height: 1,
        ..halves[1]
    };
    f.render_widget(progress_gauge(current, steps.len()), gauge_area);
}

/// Narrow layout: drop the six per-step labels (unreadable when cramped) in
/// favor of a full-width glyph track, a full-width gauge, and a single clear
/// "current step" line — readability over beauty on small screens.
fn render_progress_compact(
    f: &mut ratatui::Frame,
    inner: Rect,
    current: usize,
    steps: &[StepLabel; 6],
) {
    let rows = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ],
    )
    .split(inner);

    render_glyph_columns(f, rows[0], current, steps, false);
    f.render_widget(progress_gauge(current, steps.len()), rows[1]);

    let current_label = steps
        .get(current.saturating_sub(1))
        .map(|s| s.as_single_line())
        .unwrap_or_default();
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("▸ {current_label}"),
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(ratatui::layout::Alignment::Center),
        rows[2],
    );
}

/// Whether the TRADE card has room for its two-column receipt layout.
fn use_two_column_trade(inner_width: u16) -> bool {
    inner_width >= 48
}

/// Whether the PROGRESS block has room for the full glyph-track + per-step
/// labels + side gauge layout (else fall back to the compact stacked layout).
fn use_full_progress(inner_width: u16) -> bool {
    inner_width >= 68
}

/// Glyph + style for one step relative to the current step: done (`✔`, green),
/// current (`◉`, primary), or upcoming (`○`, dim).
fn step_glyph(step_number: usize, current: usize) -> (&'static str, Style) {
    if step_number < current {
        (
            "✔",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else if step_number == current {
        (
            "◉",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        ("○", Style::default().fg(Color::DarkGray))
    }
}

/// One cell of the glyph track: the step glyph centered in `width`, flanked by
/// dim `─` connectors (blanked at the outer edges of the first/last steps) so
/// adjacent cells join into a continuous line.
fn glyph_cell_line(
    width: usize,
    glyph: &str,
    glyph_style: Style,
    is_first: bool,
    is_last: bool,
) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }
    let mid = width / 2;
    let left_n = mid;
    let right_n = width - mid - 1;
    let dash = Style::default().fg(Color::DarkGray);
    let left = if is_first {
        " ".repeat(left_n)
    } else {
        "─".repeat(left_n)
    };
    let right = if is_last {
        " ".repeat(right_n)
    } else {
        "─".repeat(right_n)
    };
    Line::from(vec![
        Span::styled(left, dash),
        Span::styled(glyph.to_string(), glyph_style),
        Span::styled(right, dash),
    ])
}

/// Center `text` within `width`, truncating (by char) when it does not fit.
fn center_in(text: &str, width: usize) -> String {
    let truncated: String = text.chars().take(width).collect();
    let len = truncated.chars().count();
    if len >= width {
        return truncated;
    }
    let total = width - len;
    let left = total / 2;
    let right = total - left;
    format!("{}{}{}", " ".repeat(left), truncated, " ".repeat(right))
}

#[cfg(test)]
mod sidebar_tests {
    use super::*;
    use mostro_core::prelude::{Action, Message};
    use nostr_sdk::Keys;

    fn sample_message(read: bool) -> OrderMessage {
        let keys = Keys::generate();
        OrderMessage {
            message: Message::new_order(None, None, None, Action::PayInvoice, None),
            timestamp: 0,
            sender: keys.public_key(),
            order_id: None,
            trade_index: 0,
            sat_amount: None,
            buyer_invoice: None,
            order_kind: None,
            is_mine: None,
            order_status: None,
            read,
            auto_popup_shown: false,
        }
    }

    fn spans_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn kind_dot_maps_buy_sell_and_unknown_to_distinct_glyphs() {
        assert_eq!(kind_dot("BUY"), ("🟢", Color::Green));
        assert_eq!(kind_dot("SELL"), ("🔴", Color::Red));
        assert_eq!(kind_dot("N/A"), ("⚪", Color::DarkGray));
    }

    #[test]
    fn sidebar_title_omits_unread_badge_when_all_read() {
        let messages = vec![sample_message(true), sample_message(true)];
        let text = spans_text(&sidebar_title(&messages));
        assert!(text.contains("My Trades (2)"));
        assert!(!text.contains("new"));
    }

    #[test]
    fn sidebar_title_shows_unread_count_badge() {
        let messages = vec![
            sample_message(false),
            sample_message(true),
            sample_message(false),
        ];
        let text = spans_text(&sidebar_title(&messages));
        assert!(text.contains("My Trades (3)"));
        assert!(text.contains("● 2 new"));
    }

    #[test]
    fn build_sidebar_items_adds_separator_between_rows_but_not_after_last() {
        let messages = vec![sample_message(false), sample_message(true)];
        let items = build_sidebar_items(&messages, 0, 20);
        assert_eq!(items.len(), 2);
        // Non-last row: kind/id + action + time + separator = 4 lines.
        assert_eq!(items[0].height(), 4);
        // Last row: no trailing separator = 3 lines.
        assert_eq!(items[1].height(), 3);
    }
}

#[cfg(test)]
mod trade_snapshot_tests {
    use super::*;
    use mostro_core::prelude::{SmallOrder, Status};

    fn order(fiat_amount: i64, min: Option<i64>, max: Option<i64>) -> SmallOrder {
        SmallOrder {
            fiat_code: "USD".to_string(),
            fiat_amount,
            min_amount: min,
            max_amount: max,
            amount: 0,
            premium: 0,
            payment_method: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn fiat_display_none_is_placeholder() {
        assert_eq!(fiat_display(None), "—");
    }

    #[test]
    fn fiat_display_single_amount_with_code() {
        assert_eq!(fiat_display(Some(&order(100, None, None))), "100 USD");
    }

    #[test]
    fn fiat_display_prefers_min_max_range() {
        assert_eq!(
            fiat_display(Some(&order(0, Some(50), Some(200)))),
            "50–200 USD"
        );
    }

    #[test]
    fn sats_display_prefers_message_sat_amount_grouped() {
        let mut o = order(100, None, None);
        o.amount = 5;
        assert_eq!(sats_display(Some(&o), Some(142_857)), "142,857");
    }

    #[test]
    fn sats_display_falls_back_to_order_amount_then_market() {
        let mut o = order(100, None, None);
        o.amount = 1_000;
        assert_eq!(sats_display(Some(&o), None), "1,000");
        o.amount = 0;
        assert_eq!(sats_display(Some(&o), None), "market");
        assert_eq!(sats_display(None, None), "—");
    }

    #[test]
    fn premium_display_signs_and_colors() {
        let mut o = order(100, None, None);
        o.premium = 2;
        assert_eq!(premium_display(Some(&o)), ("+2%".to_string(), Color::Green));
        o.premium = -3;
        assert_eq!(premium_display(Some(&o)), ("-3%".to_string(), Color::Red));
        o.premium = 0;
        assert_eq!(premium_display(Some(&o)), ("0%".to_string(), Color::Gray));
        assert_eq!(premium_display(None), ("—".to_string(), Color::DarkGray));
    }

    #[test]
    fn method_display_trims_and_falls_back() {
        let mut o = order(100, None, None);
        o.payment_method = "  SEPA  ".to_string();
        assert_eq!(method_display(Some(&o)), "SEPA");
        o.payment_method = "   ".to_string();
        assert_eq!(method_display(Some(&o)), "—");
        assert_eq!(method_display(None), "—");
    }

    #[test]
    fn role_chip_maps_maker_taker_unknown() {
        assert_eq!(role_chip(Some(true)), ("👤", "Maker"));
        assert_eq!(role_chip(Some(false)), ("🤝", "Taker"));
        assert_eq!(role_chip(None), ("❔", "—"));
    }

    fn message_with(action: mostro_core::prelude::Action, status: Option<Status>) -> OrderMessage {
        use mostro_core::prelude::Message;
        use nostr_sdk::Keys;
        OrderMessage {
            message: Message::new_order(None, None, None, action, None),
            timestamp: 0,
            sender: Keys::generate().public_key(),
            order_id: None,
            trade_index: 0,
            sat_amount: None,
            buyer_invoice: None,
            order_kind: None,
            is_mine: None,
            order_status: status,
            read: true,
            auto_popup_shown: true,
        }
    }

    #[test]
    fn status_banner_happy_path_is_normal_with_next_callout() {
        use mostro_core::prelude::Action;
        let (emoji, _text, color, is_normal) =
            status_banner(&message_with(Action::PayInvoice, None));
        assert_eq!(emoji, "✅");
        assert_eq!(color, Color::Green);
        assert!(is_normal);
    }

    #[test]
    fn status_banner_flags_canceled_and_dispute() {
        use mostro_core::prelude::Action;
        let (emoji, _t, color, is_normal) =
            status_banner(&message_with(Action::Canceled, Some(Status::Canceled)));
        assert_eq!(emoji, "❌");
        assert_eq!(color, Color::Red);
        assert!(!is_normal);

        let (emoji, _t, color, is_normal) =
            status_banner(&message_with(Action::Dispute, Some(Status::Dispute)));
        assert_eq!(emoji, "⚖️");
        assert_eq!(color, Color::Magenta);
        assert!(!is_normal);
    }

    #[test]
    fn group_thousands_formats_and_passes_through_non_digits() {
        assert_eq!(group_thousands("142857"), "142,857");
        assert_eq!(group_thousands("999"), "999");
        assert_eq!(group_thousands("market"), "market");
    }
}

#[cfg(test)]
mod stepper_tests {
    use super::*;

    #[test]
    fn step_glyph_marks_done_current_and_upcoming() {
        // current step = 3
        assert_eq!(step_glyph(1, 3).0, "✔");
        assert_eq!(step_glyph(2, 3).0, "✔");
        assert_eq!(step_glyph(3, 3).0, "◉");
        assert_eq!(step_glyph(4, 3).0, "○");
    }

    #[test]
    fn center_in_centers_and_truncates() {
        assert_eq!(center_in("Rate", 8), "  Rate  ");
        assert_eq!(center_in("odd", 6), " odd  ");
        // Longer than width: truncate by char, no panic.
        assert_eq!(center_in("Counterparty", 5), "Count");
    }

    #[test]
    fn responsive_thresholds_switch_layouts_by_width() {
        // TRADE: two columns only when wide enough, else single stacked column.
        assert!(use_two_column_trade(60));
        assert!(use_two_column_trade(48));
        assert!(!use_two_column_trade(47));
        assert!(!use_two_column_trade(20));
        // PROGRESS: full labels+gauge only when wide, else compact stacked.
        assert!(use_full_progress(80));
        assert!(use_full_progress(68));
        assert!(!use_full_progress(67));
        assert!(!use_full_progress(30));
    }

    #[test]
    fn glyph_cell_line_blanks_outer_edges_of_first_and_last() {
        let (glyph, style) = step_glyph(1, 1);
        let first = glyph_cell_line(5, glyph, style, true, false);
        let first_text: String = first.spans.iter().map(|s| s.content.as_ref()).collect();
        // First cell: no connector to the left of the glyph, dashes to the right.
        assert_eq!(first_text, "  ◉──");

        let last = glyph_cell_line(5, glyph, style, false, true);
        let last_text: String = last.spans.iter().map(|s| s.content.as_ref()).collect();
        // Last cell: dashes to the left, blank to the right.
        assert_eq!(last_text, "──◉  ");
    }
}
