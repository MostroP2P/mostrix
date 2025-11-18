use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::{TakeOrderState, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_order_take(f: &mut ratatui::Frame, take_state: &TakeOrderState) {
    let area = f.area();
    let popup_width = area.width.saturating_sub(area.width / 4);
    // Adjust height based on whether it's a range order (needs input field and error)
    // Calculate total height needed: sum of all constraints + borders (2 lines)
    // Base constraints: spacer(1) + title(2) + separator(1) + kind(1) + currency(1) + fiat(1) + payment(1) + separator(1) + buttons(3) + help(1) = 13
    // For range: + label(1) + input(3) + error(1) = +5 (always reserve error space to prevent resizing)
    // Borders: top(1) + bottom(1) = 2
    // IMPORTANT: Always use fixed height for range orders to prevent popup from moving when typing
    let popup_height = if take_state.is_range_order {
        22 // Base(13) + range(5) + borders(2) + padding(2) = 22 (fixed, never changes)
    } else {
        15 // Base(13) + borders(2) = 15
    };
    let popup_x = area.x + (area.width - popup_width) / 2;
    let popup_y = area.y + (area.height - popup_height) / 2;
    let popup = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    let mut constraints = vec![
        Constraint::Length(1), // spacer
        Constraint::Length(2), // title
        Constraint::Length(1), // separator
        Constraint::Length(1), // kind
        Constraint::Length(1), // currency
        Constraint::Length(1), // fiat amount (or range)
        Constraint::Length(1), // payment method
    ];

    // Add input field and error for range orders
    // Always reserve space for error message to prevent layout changes when typing
    if take_state.is_range_order {
        constraints.push(Constraint::Length(1)); // label
        constraints.push(Constraint::Length(3)); // input box (with borders)
        constraints.push(Constraint::Length(1)); // error message (always reserve space, even if empty)
    }

    constraints.push(Constraint::Length(1)); // separator
    constraints.push(Constraint::Length(3)); // YES/NO buttons (need more space for borders and content)
    constraints.push(Constraint::Length(1)); // help text

    let inner_chunks = Layout::new(Direction::Vertical, constraints).split(popup);

    let block = Block::default()
        .title("📥 Take Order")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

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
        format!("{} {}", take_state.order.fiat_amount, take_state.order.fiat_code)
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

    // Input field for range orders
    // Calculate button index: buttons come after separator
    // For range orders, always use fixed index since we always reserve error space
    let button_idx = if take_state.is_range_order {
        10 // separator at 9 (after error space), buttons at 10 (fixed, never changes)
    } else {
        8 // separator at 7, buttons at 8
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
            inner_chunks[7],
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
        let input_area = inner_chunks[8];
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
        let inner_input = Layout::new(
            Direction::Horizontal,
            [Constraint::Min(0)],
        )
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
        let error_chunk = inner_chunks[9];
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
    
    // Calculate button width (each button + separator)
    // Each button should be about 12-15 chars wide, plus 1 for separator
    let button_width = 15; // Width for each button
    let separator_width = 1;
    let total_button_width = (button_width * 2) + separator_width;
    
    // Center the buttons horizontally
    let button_x = button_area.x + (button_area.width.saturating_sub(total_button_width)) / 2;
    let centered_button_area = Rect {
        x: button_x,
        y: button_area.y,
        width: total_button_width.min(button_area.width),
        height: button_area.height,
    };
    
    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(button_width),
            Constraint::Length(separator_width), // separator
            Constraint::Length(button_width),
        ],
    )
    .split(centered_button_area);

    // YES button
    let yes_style = if take_state.selected_button {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };

    let yes_block = Block::default()
        .borders(Borders::ALL)
        .style(yes_style);
    f.render_widget(yes_block, button_chunks[0]);

    let yes_inner = Layout::new(
        Direction::Vertical,
        [Constraint::Min(0)],
    )
    .margin(1)
    .split(button_chunks[0]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✓ YES",
            Style::default()
                .fg(if take_state.selected_button {
                    Color::Black
                } else {
                    Color::Green
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        yes_inner[0],
    );

    // NO button
    let no_style = if !take_state.selected_button {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD)
    };

    let no_block = Block::default()
        .borders(Borders::ALL)
        .style(no_style);
    f.render_widget(no_block, button_chunks[2]);

    let no_inner = Layout::new(
        Direction::Vertical,
        [Constraint::Min(0)],
    )
    .margin(1)
    .split(button_chunks[2]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✗ NO",
            Style::default()
                .fg(if !take_state.selected_button {
                    Color::Black
                } else {
                    Color::Red
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        no_inner[0],
    );

    // Help text
    let help_idx = button_idx + 1;
    if help_idx < inner_chunks.len() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Use ", Style::default()),
                Span::styled("← →", Style::default().fg(PRIMARY_COLOR).add_modifier(Modifier::BOLD)),
                Span::styled(" to switch, ", Style::default()),
                Span::styled("Enter", Style::default().fg(PRIMARY_COLOR).add_modifier(Modifier::BOLD)),
                Span::styled(" to confirm", Style::default()),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            inner_chunks[help_idx],
        );
    } else if !take_state.is_range_order {
        // For non-range orders, show help text at the end
        let last_idx = inner_chunks.len() - 1;
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Use ", Style::default()),
                Span::styled("← →", Style::default().fg(PRIMARY_COLOR).add_modifier(Modifier::BOLD)),
                Span::styled(" to switch, ", Style::default()),
                Span::styled("Enter", Style::default().fg(PRIMARY_COLOR).add_modifier(Modifier::BOLD)),
                Span::styled(" to confirm", Style::default()),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            inner_chunks[last_idx],
        );
    }
}
