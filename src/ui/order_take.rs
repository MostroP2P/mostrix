use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::helpers::format_premium;

use super::{TakeOrderState, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Renders the Take Order confirmation, using a compact layout on short terminals.
pub fn render_order_take(f: &mut ratatui::Frame, take_state: &TakeOrderState) {
    let area = f.area();
    let popup_width = area.width.saturating_sub(area.width / 4);
    // Adjust height based on whether it's a range order (needs input field and error)
    // Calculate total height needed from the fixed constraints and surrounding popup space.
    // Base constraints: spacer(1) + title(2) + separator(1) + kind(1) + currency(1) + fiat(1) + payment(1) + premium(1) + buttons(3) + help(1) = 13
    // For range: + label(1) + input(3) + error(1) + spacer(1) = +6 (always reserve error space to prevent resizing)
    // Popup border and vertical breathing room: +4
    // Keep these preferred heights stable while space permits; short terminals use a compact view.
    let preferred_popup_height = if take_state.is_range_order {
        23 // Base(13) + range(6) + popup space(4) = 23
    } else {
        17 // Base(13) + popup space(4) = 17
    };
    let popup_height = preferred_popup_height.min(area.height);
    let compact = popup_height < preferred_popup_height;
    // Center the popup using Flex::Center
    let popup = {
        let [popup] = Layout::horizontal([Constraint::Length(popup_width)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(popup_height)])
            .flex(Flex::Center)
            .areas(popup);
        popup
    };

    // Clear the popup area to make it fully opaque
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("📥 Take Order")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let popup_inner = block.inner(popup);
    f.render_widget(block, popup);

    if compact {
        render_compact_order_take(f, popup_inner, take_state);
        return;
    }

    let mut constraints = vec![
        Constraint::Length(1), // spacer
        Constraint::Length(2), // title
        Constraint::Length(1), // separator
        Constraint::Length(1), // kind
        Constraint::Length(1), // currency
        Constraint::Length(1), // fiat amount (or range)
        Constraint::Length(1), // payment method
        Constraint::Length(1), // premium
    ];

    // Add input field and error for range orders
    // Always reserve space for error message to prevent layout changes when typing
    if take_state.is_range_order {
        constraints.push(Constraint::Length(1)); // label
        constraints.push(Constraint::Length(3)); // input box (with borders)
        constraints.push(Constraint::Length(1)); // error message (always reserve space, even if empty)
    }

    constraints.push(Constraint::Length(3)); // YES/NO buttons (need space for borders, margins, and content)
    if take_state.is_range_order {
        constraints.push(Constraint::Length(1)); // spacer for buttons
    }
    constraints.push(Constraint::Length(1)); // help text

    let inner_chunks = Layout::new(Direction::Vertical, constraints).split(popup);

    // Title
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Review order details:",
            Style::default().add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[1],
    );

    // Order details
    let kind_str = if let Some(kind) = &take_state.order.kind {
        match kind {
            mostro_core::order::Kind::Buy => "🟢 Buy",
            mostro_core::order::Kind::Sell => "🔴 Sell",
        }
    } else {
        "❓ Unknown"
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Order Type: "),
            Span::styled(kind_str, Style::default().fg(PRIMARY_COLOR)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[3],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Currency: "),
            Span::styled(
                &take_state.order.fiat_code,
                Style::default().fg(PRIMARY_COLOR),
            ),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[4],
    );

    // Fiat amount - show range if applicable
    let fiat_str = if take_state.is_range_order {
        let min = take_state.order.min_amount.unwrap_or(0);
        let max = take_state.order.max_amount.unwrap_or(0);
        format!("{}-{} {}", min, max, take_state.order.fiat_code)
    } else {
        format!(
            "{} {}",
            take_state.order.fiat_amount, take_state.order.fiat_code
        )
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Fiat Amount: "),
            Span::styled(fiat_str, Style::default().fg(PRIMARY_COLOR)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[5],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Payment Method: "),
            Span::styled(
                &take_state.order.payment_method,
                Style::default().fg(PRIMARY_COLOR),
            ),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[6],
    );

    let (premium_text, premium_color) = format_premium(take_state.order.premium);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Premium: "),
            Span::styled(premium_text, Style::default().fg(premium_color)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[7],
    );

    // Input field for range orders
    // Calculate button index: buttons come after premium and any range fields
    // For range orders: indices 0-6 (base), 7 (premium), 8-10 (range fields), 11 (buttons)
    // For non-range: indices 0-6 (base), 7 (premium), 8 (buttons)
    let button_idx = if take_state.is_range_order {
        11 // range fields at 8-10, buttons at 11
    } else {
        8 // premium at 7, buttons at 8
    };

    if take_state.is_range_order {
        let min = take_state.order.min_amount.unwrap_or(0);
        let max = take_state.order.max_amount.unwrap_or(0);
        let currency = &take_state.order.fiat_code;

        // Label
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("Enter amount ("),
                Span::styled(
                    format!("{}-{} {}", min, max, currency),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("):"),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            inner_chunks[8],
        );

        // Input box with borders
        let input_text = if take_state.amount_input.is_empty() {
            format!("{} {}", min, currency) // Default to min
        } else {
            format!("{} {}", take_state.amount_input, currency)
        };

        // Determine border color based on validation
        let border_color = if take_state.validation_error.is_some() {
            Color::Red
        } else if take_state.amount_input.is_empty() {
            Color::Yellow
        } else {
            Color::Green
        };

        // Create a smaller input box centered in the area
        let input_area = inner_chunks[9];
        let input_width = (input_area.width * 2 / 3).min(30); // Max 30 chars wide, 2/3 of available width
        let input_x = input_area.x + (input_area.width.saturating_sub(input_width)) / 2;
        let input_rect = Rect {
            x: input_x,
            y: input_area.y,
            width: input_width,
            height: input_area.height,
        };

        let input_block = Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(border_color));

        f.render_widget(input_block, input_rect);

        // Input text inside the box
        let inner_input = Layout::new(Direction::Horizontal, [Constraint::Min(0)])
            .margin(1)
            .split(input_rect);

        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                &input_text,
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            inner_input[0],
        );

        // Error message - always render in reserved space (show empty if no error)
        let error_chunk = inner_chunks[10];
        if let Some(error_msg) = &take_state.validation_error {
            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    format!("⚠️  {}", error_msg),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )]))
                .alignment(ratatui::layout::Alignment::Center),
                error_chunk,
            );
        }
        // If no error, leave the space empty (prevents layout shift)
    }

    // YES/NO buttons - center them in the popup
    let button_area = inner_chunks[button_idx];
    render_take_buttons(f, button_area, take_state.selected_button);

    // Help text - comes after buttons and optional spacer
    let help_idx = if take_state.is_range_order {
        button_idx + 2 // buttons at 11, spacer at 12, help at 13
    } else {
        button_idx + 1 // buttons at 8, help at 9
    };

    if help_idx < inner_chunks.len() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Use ", Style::default()),
                Span::styled(
                    "← →",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to switch, ", Style::default()),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to confirm", Style::default()),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            inner_chunks[help_idx],
        );
    }
}

fn render_compact_order_take(f: &mut ratatui::Frame, area: Rect, take_state: &TakeOrderState) {
    let [details_area, button_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3), // always reserve YES/NO controls
    ])
    .areas(area);
    let detailed_range_input = take_state.is_range_order && details_area.height >= 6;
    let compact_range_input = take_state.is_range_order && !detailed_range_input;
    let mut constraints = vec![
        Constraint::Length(1), // fiat amount
        Constraint::Length(1), // premium
    ];
    if detailed_range_input {
        constraints.push(Constraint::Length(1)); // amount label
        constraints.push(Constraint::Length(3)); // amount input
    } else if compact_range_input {
        constraints.push(Constraint::Length(1)); // compact amount input
    }
    let chunks = Layout::new(Direction::Vertical, constraints).split(details_area);
    let fiat = if take_state.is_range_order {
        format!(
            "{}-{} {}",
            take_state.order.min_amount.unwrap_or(0),
            take_state.order.max_amount.unwrap_or(0),
            take_state.order.fiat_code
        )
    } else {
        format!(
            "{} {}",
            take_state.order.fiat_amount, take_state.order.fiat_code
        )
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Fiat: "),
            Span::styled(fiat, Style::default().fg(PRIMARY_COLOR)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[0],
    );

    let (premium_text, premium_color) = format_premium(take_state.order.premium);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Premium: "),
            Span::styled(premium_text, Style::default().fg(premium_color)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

    if detailed_range_input {
        let min = take_state.order.min_amount.unwrap_or(0);
        let max = take_state.order.max_amount.unwrap_or(0);
        let currency = &take_state.order.fiat_code;
        f.render_widget(
            Paragraph::new(format!("Enter amount ({min}-{max} {currency}):"))
                .alignment(ratatui::layout::Alignment::Center),
            chunks[2],
        );

        let input_text = if take_state.amount_input.is_empty() {
            min.to_string()
        } else {
            take_state.amount_input.clone()
        };
        f.render_widget(
            Paragraph::new(format!("{input_text} {currency}"))
                .alignment(ratatui::layout::Alignment::Center)
                .block(Block::default().borders(Borders::ALL)),
            chunks[3],
        );
    } else if compact_range_input && chunks.len() > 2 {
        let min = take_state.order.min_amount.unwrap_or(0);
        let input_text = if take_state.amount_input.is_empty() {
            min.to_string()
        } else {
            take_state.amount_input.clone()
        };
        f.render_widget(
            Paragraph::new(format!(
                "Amount: {input_text} {}",
                take_state.order.fiat_code
            ))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[2],
        );
    }

    render_take_buttons(f, button_area, take_state.selected_button);
}

fn render_take_buttons(f: &mut ratatui::Frame, area: Rect, selected_button: bool) {
    let separator_width = 1;
    let button_width = ((area.width.saturating_sub(separator_width)) / 2).min(15);
    let total_button_width = (button_width * 2) + separator_width;
    let button_x = area.x + (area.width.saturating_sub(total_button_width)) / 2;
    let centered_button_area = Rect {
        x: button_x,
        y: area.y,
        width: total_button_width.min(area.width),
        height: area.height,
    };
    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(button_width),
            Constraint::Length(separator_width),
            Constraint::Length(button_width),
        ],
    )
    .split(centered_button_area);

    let yes_style = if selected_button {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };
    f.render_widget(
        Block::default().borders(Borders::ALL).style(yes_style),
        button_chunks[0],
    );
    let yes_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[0]);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✓ YES",
            Style::default()
                .fg(if selected_button {
                    Color::Black
                } else {
                    Color::Green
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        yes_inner[0],
    );

    let no_style = if !selected_button {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };
    f.render_widget(
        Block::default().borders(Borders::ALL).style(no_style),
        button_chunks[2],
    );
    let no_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[2]);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✗ NO",
            Style::default()
                .fg(if !selected_button {
                    Color::Black
                } else {
                    Color::Red
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        no_inner[0],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use mostro_core::prelude::SmallOrder;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

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

    fn render_take_order(width: u16, height: u16, is_range_order: bool) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let take_state = TakeOrderState {
            order: SmallOrder {
                fiat_code: "MXN".to_string(),
                fiat_amount: 500,
                min_amount: is_range_order.then_some(500),
                max_amount: is_range_order.then_some(1_000),
                premium: -3,
                payment_method: "SPEI".to_string(),
                ..Default::default()
            },
            amount_input: String::new(),
            is_range_order,
            validation_error: None,
            selected_button: true,
        };
        terminal
            .draw(|f| render_order_take(f, &take_state))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn take_order_shows_premium_for_fixed_and_range_orders() {
        for is_range_order in [false, true] {
            let buf = render_take_order(100, 30, is_range_order);
            assert!(buffer_contains(&buf, "Premium:"));
            assert!(buffer_contains(&buf, "-3%"));
            assert!(buffer_contains(&buf, "SPEI"));
            assert!(buffer_contains(&buf, "YES"));
            assert!(buffer_contains(&buf, "NO"));
        }
    }

    #[test]
    fn short_terminal_keeps_premium_and_actions_visible() {
        for is_range_order in [false, true] {
            let buf = render_take_order(60, 10, is_range_order);
            assert!(buffer_contains(&buf, "Premium:"));
            assert!(buffer_contains(&buf, "-3%"));
            assert!(buffer_contains(&buf, "YES"));
            assert!(buffer_contains(&buf, "NO"));
            if is_range_order {
                assert!(buffer_contains(&buf, "500 MXN"));
            }
        }
    }
}
