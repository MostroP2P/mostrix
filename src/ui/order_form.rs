use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};

use super::{FormState, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::currencies::{filter_options, name_for, resolve_options};
use crate::ui::orders::FormField;
use crate::util::MostroInstanceInfo;

/// Width of the label column inside the details panel.
const LABEL_W: usize = 10;
/// Column where the value strip starts (arrow/margin + label column + space).
const VALUE_OFFSET: usize = LABEL_W + 2;
/// Subtle fill for unfocused input strips (slightly lighter than the background).
const FIELD_BG: Color = Color::Rgb(38, 44, 58);

/// A single rendered field in the "Order details" panel.
struct Row {
    field: FormField,
    label: &'static str,
    value: Line<'static>,
    prefix_len: usize, // chars before the editable text (for cursor placement)
    text_len: usize,   // length of the editable value (for cursor placement)
    editable: bool,    // whether a text cursor should be shown when focused
}

/// Per-field validation used for inline glyphs and section-header coloring.
/// `Some(true)` = valid, `Some(false)` = invalid/missing, `None` = neutral/optional.
type FieldStatus = Option<bool>;

/// Live-preview validation state.
enum PreviewStatus {
    Ready,
    Missing(String),
    Invalid(String),
}

pub fn render_order_form(
    f: &mut ratatui::Frame,
    area: Rect,
    form: &FormState,
    info: Option<&MostroInstanceInfo>,
) {
    let accepted: &[String] = info
        .map(|i| i.fiat_currencies_accepted.as_slice())
        .unwrap_or(&[]);
    let min_amt = info.and_then(|i| i.min_order_amount);
    let max_amt = info.and_then(|i| i.max_order_amount);

    let block = Block::default()
        .title(Line::from(Span::styled(
            " ✨ Create New Order ",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(area);
    f.render_widget(&block, area);

    let rows = Layout::new(
        Direction::Vertical,
        [
            Constraint::Min(9),
            Constraint::Length(4),
            Constraint::Length(1),
        ],
    )
    .split(inner);

    let top = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(62), Constraint::Percentage(38)],
    )
    .split(rows[0]);

    let currency_row = render_details(f, top[0], form, accepted, min_amt, max_amt);
    render_preview(f, top[1], form);
    render_help(f, rows[1], form);
    render_footer(f, rows[2]);

    if form.currency_picker.open {
        if let Some(anchor) = currency_row {
            render_currency_dropdown(f, anchor, inner, form, accepted);
        }
    }
}

/// One line in the details panel.
enum Vis<'a> {
    Header(&'static str, bool), // title, complete
    Field(&'a Row, FieldStatus),
    Hint(String),
    Spacer,
    Flex,
}

/// Render the "Order details" panel. Returns the Rect of the Currency row so the
/// dropdown overlay can be anchored beneath it.
fn render_details(
    f: &mut ratatui::Frame,
    area: Rect,
    form: &FormState,
    accepted: &[String],
    min_amt: Option<i64>,
    max_amt: Option<i64>,
) -> Option<Rect> {
    let block = Block::default()
        .title(" Order details ")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(area);
    f.render_widget(&block, area);

    let rows = build_rows(form);
    let row_for = |field: FormField| rows.iter().find(|r| r.field == field).unwrap();

    let pricing: Vec<FormField> = if form.use_range {
        vec![
            FormField::AmountSats,
            FormField::FiatAmount,
            FormField::FiatAmountMax,
        ]
    } else {
        vec![FormField::AmountSats, FormField::FiatAmount]
    };
    let sections: [(&str, Vec<FormField>); 3] = [
        ("TRADE", vec![FormField::OrderType, FormField::Currency]),
        ("PRICING", pricing),
        (
            "TERMS",
            vec![
                FormField::PaymentMethod,
                FormField::Premium,
                FormField::Invoice,
                FormField::ExpirationDays,
            ],
        ),
    ];

    // Build the visual item list (centered via equal top/bottom Flex).
    let mut items: Vec<Vis> = vec![Vis::Flex];
    for (idx, (title, fields)) in sections.iter().enumerate() {
        if idx > 0 {
            items.push(Vis::Spacer);
        }
        let complete = fields
            .iter()
            .all(|fld| field_status(form, *fld, accepted) != Some(false));
        items.push(Vis::Header(title, complete));
        for fld in fields {
            items.push(Vis::Field(
                row_for(*fld),
                field_status(form, *fld, accepted),
            ));
            // Instance order-size limits hint under the sats amount field.
            if *fld == FormField::AmountSats {
                if let Some(hint) = limits_hint(min_amt, max_amt) {
                    items.push(Vis::Hint(hint));
                }
            }
        }
    }
    items.push(Vis::Flex);

    let constraints: Vec<Constraint> = items
        .iter()
        .map(|it| match it {
            Vis::Flex => Constraint::Min(0),
            _ => Constraint::Length(1),
        })
        .collect();
    let chunks = Layout::new(Direction::Vertical, constraints).split(inner);

    let strip_width = (inner.width as usize).saturating_sub(VALUE_OFFSET + 1);

    let mut currency_row: Option<Rect> = None;
    let mut cursor: Option<(Rect, usize, usize)> = None;

    for (item, chunk) in items.iter().zip(chunks.iter()) {
        match item {
            Vis::Flex | Vis::Spacer => {}
            Vis::Header(title, complete) => {
                let head_color = if *complete {
                    PRIMARY_COLOR
                } else {
                    Color::Gray
                };
                let dashes = "─".repeat((chunk.width as usize).saturating_sub(title.len() + 3));
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" {title} "),
                            Style::default().fg(head_color).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(dashes, Style::default().fg(Color::DarkGray)),
                    ]))
                    .style(Style::default().bg(BACKGROUND_COLOR)),
                    *chunk,
                );
            }
            Vis::Hint(text) => {
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::raw(" ".repeat(VALUE_OFFSET)),
                        Span::styled(
                            text.clone(),
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]))
                    .style(Style::default().bg(BACKGROUND_COLOR)),
                    *chunk,
                );
            }
            Vis::Field(row, status) => {
                let focused = row.field == form.focused;
                if row.field == FormField::Currency {
                    currency_row = Some(*chunk);
                }
                f.render_widget(
                    Paragraph::new(field_line(row, focused, strip_width, *status))
                        .style(Style::default().bg(BACKGROUND_COLOR)),
                    *chunk,
                );
                if focused && row.editable && !form.currency_picker.open {
                    cursor = Some((*chunk, row.prefix_len, row.text_len));
                }
            }
        }
    }

    if let Some((chunk, prefix, len)) = cursor {
        let x = chunk.x + VALUE_OFFSET as u16 + prefix as u16 + len as u16;
        f.set_cursor_position((x, chunk.y));
    }

    currency_row
}

/// Build a single field line: focus arrow + `label` on the panel background,
/// then the value on a tinted "input strip" (green when focused) padded to
/// `strip_width`, with a trailing ✓/✗ status glyph.
fn field_line(row: &Row, focused: bool, strip_width: usize, status: FieldStatus) -> Line<'static> {
    let arrow = if focused { "▸" } else { " " };
    let label_style = if focused {
        Style::default()
            .fg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let strip_bg = if focused { PRIMARY_COLOR } else { FIELD_BG };

    let mut spans = vec![Span::styled(
        format!("{arrow}{:<width$} ", row.label, width = LABEL_W),
        label_style,
    )];

    let mut used = 0usize;
    for s in &row.value.spans {
        used += s.content.chars().count();
        let mut st = s.style.bg(strip_bg);
        if focused {
            st = st.fg(Color::Black);
        }
        spans.push(Span::styled(s.content.clone(), st));
    }

    let glyph = status.map(|ok| if ok { "✓ " } else { "✗ " });
    let reserved = if glyph.is_some() { 2 } else { 0 };
    let pad = strip_width.saturating_sub(used + reserved);
    if pad > 0 {
        spans.push(Span::styled(" ".repeat(pad), Style::default().bg(strip_bg)));
    }
    if let Some(g) = glyph {
        let ok = status == Some(true);
        let fg = if focused {
            Color::Black
        } else if ok {
            Color::Green
        } else {
            Color::Red
        };
        spans.push(Span::styled(g, Style::default().fg(fg).bg(strip_bg)));
    }

    Line::from(spans)
}

fn build_rows(form: &FormState) -> Vec<Row> {
    let is_buy = form.kind.eq_ignore_ascii_case("buy");

    let type_val = {
        let (label, color) = if is_buy {
            ("buy", Color::Green)
        } else {
            ("sell", Color::Red)
        };
        Line::from(vec![
            Span::styled("● ", Style::default().fg(color)),
            Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled("   ⇄ Space", Style::default().fg(Color::DarkGray)),
        ])
    };

    let currency_val = if form.currency_picker.open {
        Line::from(vec![
            Span::raw(form.currency_picker.filter.clone()),
            Span::styled("▏", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        let trimmed = form.fiat_code.trim();
        if trimmed.is_empty() {
            Line::from(vec![
                Span::styled("— none —", Style::default().fg(Color::DarkGray)),
                Span::styled("   ▾ pick", Style::default().fg(Color::DarkGray)),
            ])
        } else {
            let code = trimmed.to_ascii_uppercase();
            let name = name_for(&code);
            let mut spans = vec![Span::styled(
                code,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )];
            if !name.is_empty() {
                spans.push(Span::styled(
                    format!("  {name}"),
                    Style::default().fg(Color::Gray),
                ));
            }
            spans.push(Span::styled(
                "   ▾ pick",
                Style::default().fg(Color::DarkGray),
            ));
            Line::from(spans)
        }
    };

    let amount_val = if form.focused == FormField::AmountSats {
        Line::from(form.amount.clone())
    } else if form.amount.trim() == "0" || form.amount.trim().is_empty() {
        Line::from(Span::styled(
            "market",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ))
    } else {
        Line::from(format!("{} sats", group_thousands(&form.amount)))
    };

    let mut rows = vec![
        Row {
            field: FormField::OrderType,
            label: "Type",
            value: type_val,
            prefix_len: 0,
            text_len: 0,
            editable: false,
        },
        Row {
            field: FormField::Currency,
            label: "Currency",
            value: currency_val,
            prefix_len: 0,
            text_len: 0,
            editable: false,
        },
        Row {
            field: FormField::AmountSats,
            label: "Amount",
            value: amount_val,
            prefix_len: 0,
            text_len: form.amount.len(),
            editable: true,
        },
    ];

    let (tag, tag_color) = if form.use_range {
        ("[Range] ", Color::Magenta)
    } else {
        ("[Single] ", Color::Cyan)
    };
    rows.push(Row {
        field: FormField::FiatAmount,
        label: if form.use_range { "Fiat min" } else { "Fiat" },
        value: Line::from(vec![
            Span::styled(
                tag,
                Style::default().fg(tag_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(form.fiat_amount.clone()),
        ]),
        prefix_len: tag.chars().count(),
        text_len: form.fiat_amount.len(),
        editable: true,
    });

    if form.use_range {
        rows.push(Row {
            field: FormField::FiatAmountMax,
            label: "Fiat max",
            value: Line::from(form.fiat_amount_max.clone()),
            prefix_len: 0,
            text_len: form.fiat_amount_max.len(),
            editable: true,
        });
    }

    rows.push(Row {
        field: FormField::PaymentMethod,
        label: "Method",
        value: dim_if_empty(&form.payment_method, "(any)"),
        prefix_len: 0,
        text_len: form.payment_method.len(),
        editable: true,
    });
    rows.push(Row {
        field: FormField::Premium,
        label: "Premium",
        value: if form.focused == FormField::Premium {
            Line::from(form.premium.clone())
        } else {
            premium_line(&form.premium)
        },
        prefix_len: 0,
        text_len: form.premium.len(),
        editable: true,
    });
    rows.push(Row {
        field: FormField::Invoice,
        label: "Invoice",
        value: dim_if_empty(&form.invoice, "(optional)"),
        prefix_len: 0,
        text_len: form.invoice.len(),
        editable: true,
    });
    rows.push(Row {
        field: FormField::ExpirationDays,
        label: "Expiry",
        value: if form.focused == FormField::ExpirationDays {
            Line::from(form.expiration_days.clone())
        } else {
            expiry_line(&form.expiration_days)
        },
        prefix_len: 0,
        text_len: form.expiration_days.len(),
        editable: true,
    });

    rows
}

fn render_preview(f: &mut ratatui::Frame, area: Rect, form: &FormState) {
    let block = Block::default()
        .title(" Live preview ")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(area);
    f.render_widget(&block, area);

    let lines = build_preview_lines(form);
    let card_h = (lines.len() as u16 + 2).min(inner.height.saturating_sub(2));

    let split = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(card_h),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ],
    )
    .split(inner);

    // Receipt-style card.
    let card = Block::default()
        .title(" Order ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let card_inner = card.inner(split[0]);
    f.render_widget(card, split[0]);
    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: true }),
        card_inner,
    );

    // Status dot beneath the card.
    let status = match validate(form) {
        PreviewStatus::Ready => Line::from(vec![
            Span::styled("● ", Style::default().fg(Color::Green)),
            Span::styled("ready to submit", Style::default().fg(Color::Green)),
        ]),
        PreviewStatus::Missing(what) => Line::from(vec![
            Span::styled("● ", Style::default().fg(Color::Yellow)),
            Span::styled(format!("fill: {what}"), Style::default().fg(Color::Yellow)),
        ]),
        PreviewStatus::Invalid(why) => Line::from(vec![
            Span::styled("● ", Style::default().fg(Color::Red)),
            Span::styled(format!("invalid: {why}"), Style::default().fg(Color::Red)),
        ]),
    };
    f.render_widget(
        Paragraph::new(status).style(Style::default().bg(BACKGROUND_COLOR)),
        split[2],
    );
}

fn build_preview_lines(form: &FormState) -> Vec<Line<'static>> {
    let is_buy = form.kind.eq_ignore_ascii_case("buy");
    let (side, side_color) = if is_buy {
        ("BUY", Color::Green)
    } else {
        ("SELL", Color::Red)
    };
    let code = if form.fiat_code.trim().is_empty() {
        "—".to_string()
    } else {
        form.fiat_code.trim().to_ascii_uppercase()
    };

    let sats = if form.amount.trim() == "0" || form.amount.trim().is_empty() {
        "market price".to_string()
    } else {
        format!("{} sats", group_thousands(&form.amount))
    };

    let fiat = if form.use_range {
        let min = non_empty(&form.fiat_amount, "?");
        let max = non_empty(&form.fiat_amount_max, "?");
        format!("{min} – {max} {code}")
    } else {
        format!("{} {code}", non_empty(&form.fiat_amount, "?"))
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{side} "),
                Style::default().fg(side_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(sats),
        ]),
        Line::from(format!("for  {fiat}")),
        premium_preview_line(&form.premium),
    ];

    let method = form.payment_method.trim();
    lines.push(Line::from(format!(
        "via  {}",
        if method.is_empty() { "—" } else { method }
    )));
    lines.push(Line::from(expiry_preview(&form.expiration_days)));

    if let Some(price) = implied_price_per_btc(form) {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("≈ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/BTC", group_thousands(&price.to_string())),
                Style::default().fg(Color::Cyan),
            ),
        ]));
    }
    lines
}

fn implied_price_per_btc(form: &FormState) -> Option<i64> {
    if form.use_range {
        return None;
    }
    let sats = form.amount.trim().parse::<i64>().ok()?;
    let fiat = form.fiat_amount.trim().parse::<f64>().ok()?;
    if sats <= 0 || fiat <= 0.0 {
        return None;
    }
    let btc = sats as f64 / 100_000_000.0;
    Some((fiat / btc).round() as i64)
}

fn render_help(f: &mut ratatui::Frame, area: Rect, form: &FormState) {
    let help_paragraph = Paragraph::new(build_field_help(form))
        .block(
            Block::default()
                .title(" Field help ")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR)),
        )
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: true });
    f.render_widget(help_paragraph, area);
}

fn render_footer(f: &mut ratatui::Frame, area: Rect) {
    let hint = Paragraph::new(Line::from(vec![
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" submit • "),
        Span::styled(
            "Tab",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" focus • "),
        Span::styled(
            "Space",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" toggle • "),
        Span::styled(
            "Esc",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" cancel"),
    ]))
    .alignment(Alignment::Center)
    .style(Style::default().bg(BACKGROUND_COLOR));
    f.render_widget(hint, area);
}

fn render_currency_dropdown(
    f: &mut ratatui::Frame,
    anchor: Rect,
    bounds: Rect,
    form: &FormState,
    currencies: &[String],
) {
    let options = resolve_options(currencies);
    let filtered = filter_options(&options, &form.currency_picker.filter);
    let selected = form
        .currency_picker
        .selected
        .min(filtered.len().saturating_sub(1));

    let x = anchor.x + VALUE_OFFSET as u16;
    let max_width = (bounds.x + bounds.width).saturating_sub(x);
    let width = 40u16.clamp(24, max_width.max(24)).min(max_width);
    let content_rows = filtered.len().clamp(1, 8) as u16;
    let height = content_rows + 3; // border (2) + hint (1)
    let mut y = anchor.y + 1;
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

    let title = if currencies.is_empty() {
        format!(" Select currency ({} common) ", options.len())
    } else {
        format!(" Select currency ({} accepted) ", options.len())
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let split = Layout::new(
        Direction::Vertical,
        [Constraint::Min(1), Constraint::Length(1)],
    )
    .split(inner);

    if filtered.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  no match",
                Style::default().fg(Color::DarkGray),
            ))
            .style(Style::default().bg(BACKGROUND_COLOR)),
            split[0],
        );
    } else {
        let items: Vec<ListItem> = filtered
            .iter()
            .map(|o| {
                let mut spans = vec![Span::styled(
                    format!("{:<5}", o.code),
                    Style::default().add_modifier(Modifier::BOLD),
                )];
                if !o.name.is_empty() {
                    spans.push(Span::styled(
                        o.name.clone(),
                        Style::default().add_modifier(Modifier::DIM),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .style(Style::default().fg(Color::White).bg(BACKGROUND_COLOR))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› ");
        let mut state = ListState::default().with_selected(Some(selected));
        f.render_stateful_widget(list, split[0], &mut state);

        if filtered.len() > split[0].height as usize {
            let mut sb_state = ScrollbarState::new(filtered.len()).position(selected);
            f.render_stateful_widget(
                Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
                split[0],
                &mut sb_state,
            );
        }
    }

    f.render_widget(
        Paragraph::new(Span::styled(
            "type filter • ↑↓ move • Enter select • Esc close",
            Style::default().fg(Color::DarkGray),
        ))
        .style(Style::default().bg(BACKGROUND_COLOR)),
        split[1],
    );

    let caret_x =
        anchor.x + VALUE_OFFSET as u16 + form.currency_picker.filter.chars().count() as u16;
    f.set_cursor_position((caret_x, anchor.y));
}

// ── validation & formatting helpers ──────────────────────────────────────────

fn field_status(form: &FormState, field: FormField, accepted: &[String]) -> FieldStatus {
    match field {
        FormField::OrderType => None,
        FormField::Currency => {
            let code = form.fiat_code.trim().to_ascii_uppercase();
            if code.is_empty() {
                Some(false)
            } else if accepted.is_empty() || accepted.iter().any(|a| a.eq_ignore_ascii_case(&code))
            {
                Some(true)
            } else {
                Some(false)
            }
        }
        FormField::AmountSats => {
            let t = form.amount.trim();
            if t.is_empty() || t == "0" {
                Some(true) // market order
            } else {
                Some(t.parse::<i64>().map(|n| n > 0).unwrap_or(false))
            }
        }
        FormField::FiatAmount => {
            let t = form.fiat_amount.trim();
            if t.is_empty() {
                Some(false)
            } else {
                Some(t.parse::<i64>().map(|n| n > 0).unwrap_or(false))
            }
        }
        FormField::FiatAmountMax => {
            if !form.use_range {
                None
            } else {
                match (
                    form.fiat_amount.trim().parse::<i64>(),
                    form.fiat_amount_max.trim().parse::<i64>(),
                ) {
                    (Ok(min), Ok(max)) if max > min => Some(true),
                    _ => Some(false),
                }
            }
        }
        FormField::PaymentMethod => Some(!form.payment_method.trim().is_empty()),
        FormField::Premium => {
            let t = form.premium.trim();
            Some(t.is_empty() || t.parse::<i64>().is_ok())
        }
        FormField::Invoice => {
            if form.invoice.trim().is_empty() {
                None
            } else {
                Some(true)
            }
        }
        FormField::ExpirationDays => {
            let t = form.expiration_days.trim();
            if t.is_empty() {
                Some(true)
            } else {
                Some(t.parse::<i64>().map(|n| n >= 0).unwrap_or(false))
            }
        }
    }
}

fn limits_hint(min_amt: Option<i64>, max_amt: Option<i64>) -> Option<String> {
    match (min_amt, max_amt) {
        (Some(min), Some(max)) => Some(format!(
            "limits {} – {} sats",
            group_thousands(&min.to_string()),
            group_thousands(&max.to_string())
        )),
        (Some(min), None) => Some(format!("min {} sats", group_thousands(&min.to_string()))),
        (None, Some(max)) => Some(format!("max {} sats", group_thousands(&max.to_string()))),
        (None, None) => None,
    }
}

fn non_empty(s: &str, fallback: &'static str) -> String {
    let t = s.trim();
    if t.is_empty() {
        fallback.to_string()
    } else {
        t.to_string()
    }
}

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

fn dim_if_empty(value: &str, placeholder: &'static str) -> Line<'static> {
    if value.trim().is_empty() {
        Line::from(Span::styled(
            placeholder,
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ))
    } else {
        Line::from(value.to_string())
    }
}

fn premium_line(premium: &str) -> Line<'static> {
    match premium.trim().parse::<i64>() {
        Ok(0) => Line::from("0 %"),
        Ok(p) if p > 0 => Line::from(Span::styled(
            format!("+{p} %"),
            Style::default().fg(Color::Green),
        )),
        Ok(p) => Line::from(Span::styled(
            format!("{p} %"),
            Style::default().fg(Color::Red),
        )),
        Err(_) => dim_if_empty(premium, "0 %"),
    }
}

fn expiry_line(days: &str) -> Line<'static> {
    match days.trim().parse::<i64>() {
        Ok(0) => Line::from(Span::styled(
            "no expiry",
            Style::default().fg(Color::DarkGray),
        )),
        Ok(1) => Line::from("1 day"),
        Ok(d) => Line::from(format!("{d} days")),
        Err(_) => dim_if_empty(days, "1 day"),
    }
}

fn premium_preview_line(premium: &str) -> Line<'static> {
    match premium.trim().parse::<i64>() {
        Ok(0) => Line::from("@ no premium"),
        Ok(p) if p > 0 => Line::from(vec![
            Span::raw("@ "),
            Span::styled(format!("+{p}%"), Style::default().fg(Color::Green)),
            Span::raw(" premium"),
        ]),
        Ok(p) => Line::from(vec![
            Span::raw("@ "),
            Span::styled(format!("{p}%"), Style::default().fg(Color::Red)),
            Span::raw(" discount"),
        ]),
        Err(_) => Line::from("@ ? premium"),
    }
}

fn expiry_preview(days: &str) -> String {
    match days.trim().parse::<i64>() {
        Ok(0) => "no expiry".to_string(),
        Ok(d) => format!("expires in {d}d"),
        Err(_) => "expiry ?".to_string(),
    }
}

/// Short description of what makes the order not ready, or `None` when it is
/// ready to submit. Used by the leave-confirmation popup.
pub fn missing_hint(form: &FormState) -> Option<String> {
    match validate(form) {
        PreviewStatus::Ready => None,
        PreviewStatus::Missing(what) => Some(format!("missing {what}")),
        PreviewStatus::Invalid(why) => Some(format!("invalid {why}")),
    }
}

fn validate(form: &FormState) -> PreviewStatus {
    if form.fiat_code.trim().is_empty() {
        return PreviewStatus::Missing("currency".into());
    }
    if !(form.amount.trim().is_empty() || form.amount.trim() == "0")
        && form.amount.trim().parse::<i64>().is_err()
    {
        return PreviewStatus::Invalid("amount".into());
    }
    if form.fiat_amount.trim().is_empty() {
        return PreviewStatus::Missing("fiat amount".into());
    }
    if form.fiat_amount.trim().parse::<i64>().is_err() {
        return PreviewStatus::Invalid("fiat amount".into());
    }
    if form.use_range {
        match form.fiat_amount_max.trim().parse::<i64>() {
            Ok(max) => {
                let min = form.fiat_amount.trim().parse::<i64>().unwrap_or(0);
                if max <= min {
                    return PreviewStatus::Invalid("max must exceed min".into());
                }
            }
            Err(_) => return PreviewStatus::Missing("fiat max".into()),
        }
    }
    if form.payment_method.trim().is_empty() {
        return PreviewStatus::Missing("payment method".into());
    }
    if !form.premium.trim().is_empty() && form.premium.trim().parse::<i64>().is_err() {
        return PreviewStatus::Invalid("premium".into());
    }
    if !form.expiration_days.trim().is_empty()
        && form.expiration_days.trim().parse::<i64>().is_err()
    {
        return PreviewStatus::Invalid("expiration".into());
    }
    PreviewStatus::Ready
}

fn build_field_help(form: &FormState) -> Vec<Line<'static>> {
    if form.currency_picker.open {
        return vec![
            Line::from("Currency"),
            Line::from("Type to filter, ↑↓ to move, Enter to pick, Esc to close."),
        ];
    }
    match form.focused {
        FormField::OrderType => vec![
            Line::from("Order Type"),
            Line::from("Choose whether you want to buy or sell bitcoin. Space toggles buy/sell."),
        ],
        FormField::Currency => vec![
            Line::from("Currency"),
            Line::from("Press Enter/Space or start typing to open the currency picker."),
        ],
        FormField::AmountSats => vec![
            Line::from("Amount (sats)"),
            Line::from("Satoshis to trade. Use 0 for a market order at the current price."),
        ],
        FormField::FiatAmount => vec![
            Line::from("Fiat Amount"),
            Line::from("Price in fiat. Space toggles a single amount or a range (e.g. 100-200)."),
        ],
        FormField::FiatAmountMax if form.use_range => vec![
            Line::from("Fiat Amount (Max)"),
            Line::from("Upper bound of the fiat amount range."),
        ],
        FormField::PaymentMethod => vec![
            Line::from("Payment Method"),
            Line::from("How you send/receive fiat. Spaces allowed (e.g. \"Bank transfer\")."),
        ],
        FormField::Premium => vec![
            Line::from("Premium (%)"),
            Line::from("Markup or discount vs. the reference price. + premium, − discount."),
        ],
        FormField::Invoice => vec![
            Line::from("Invoice (optional)"),
            Line::from("Pre-generated Lightning invoice. Leave empty for Mostro to handle it."),
        ],
        FormField::ExpirationDays => vec![
            Line::from("Expiration (days)"),
            Line::from("How long the order stays active. Use 0 for no expiration."),
        ],
        _ => vec![
            Line::from("Create New Order"),
            Line::from("Fill the fields on the left and press Enter to submit."),
        ],
    }
}

pub fn render_form_initializing(f: &mut ratatui::Frame, area: Rect) {
    let block = Block::default()
        .title(Line::from(Span::styled(
            " ✨ Create New Order ",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, area);
}
