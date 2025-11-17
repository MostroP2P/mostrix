use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::ui::TakeOrderState;

/// Render the take order confirmation popup
pub fn render(f: &mut Frame, area: Rect, state: &TakeOrderState, confirmed: bool) {
    // Create popup area (centered)
    let popup_width = 70;
    let popup_height = if state.order.min_amount.is_some() && state.order.max_amount.is_some() {
        16 // Extra height for range amount input
    } else {
        13
    };

    let popup = centered_rect(popup_width, popup_height, area);

    // Clear the area
    f.render_widget(Clear, popup);

    // Create the main block
    let block = Block::default()
        .title(Span::styled(
            " Confirm Take Order ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Split inner area for content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // Order type
            Constraint::Length(1), // Amount
            Constraint::Length(1), // Fiat
            Constraint::Length(1), // Payment method
            Constraint::Length(1), // Spacer
            Constraint::Length(if state.order.min_amount.is_some() { 2 } else { 0 }), // Amount input (if range)
            Constraint::Length(1), // Spacer
            Constraint::Min(0),    // Flexible space
            Constraint::Length(3), // Buttons
        ])
        .split(inner);

    // Order details
    let order_type = state
        .order
        .kind
        .as_ref()
        .map(|k| k.to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let order_type_display = if order_type == "buy" {
        Span::styled("BUY", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("SELL", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
    };

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("Order Type: "),
            order_type_display,
        ]))
        .alignment(Alignment::Center),
        chunks[0],
    );

    // Amount display
    let amount_text = if let (Some(min), Some(max)) = (state.order.min_amount, state.order.max_amount) {
        format!("Amount Range: {} - {} sats", min, max)
    } else {
        format!("Amount: {} sats", state.order.amount)
    };
    f.render_widget(
        Paragraph::new(amount_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Yellow)),
        chunks[1],
    );

    // Fiat amount display
    let fiat_text = if let (Some(min), Some(max)) = (state.order.min_amount, state.order.max_amount) {
        format!("Fiat Range: {} - {} {}", min, max, state.order.fiat_code)
    } else {
        format!(
            "Fiat: {} {}",
            state.order.fiat_amount, state.order.fiat_code
        )
    };
    f.render_widget(
        Paragraph::new(fiat_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Cyan)),
        chunks[2],
    );

    // Payment method
    f.render_widget(
        Paragraph::new(format!("Payment: {}", state.order.payment_method))
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Magenta)),
        chunks[3],
    );

    // If range order, show amount input field
    if state.order.min_amount.is_some() && state.order.max_amount.is_some() {
        let amount_input_block = Block::default()
            .title(" Enter Amount ")
            .borders(Borders::ALL)
            .border_style(if !confirmed {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            });

        let amount_input = Paragraph::new(state.amount.as_str())
            .block(amount_input_block)
            .style(if !confirmed {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            });

        f.render_widget(amount_input, chunks[5]);
    }

    // Buttons
    let button_area = chunks[8];
    let button_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(button_area);

    // Yes button
    let yes_style = if !confirmed {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    };

    let yes_button = Paragraph::new("[ YES ]")
        .alignment(Alignment::Center)
        .style(yes_style);
    f.render_widget(yes_button, button_chunks[0]);

    // No button
    let no_style = if confirmed {
        Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Red)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    };

    let no_button = Paragraph::new("[ NO ]")
        .alignment(Alignment::Center)
        .style(no_style);
    f.render_widget(no_button, button_chunks[1]);

    // Help text
    let help_text = if state.order.min_amount.is_some() && !confirmed {
        "Enter amount | TAB: Switch | ENTER: Confirm | ESC: Cancel"
    } else {
        "TAB: Switch | ENTER: Confirm | ESC: Cancel"
    };

    let help_area = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width: inner.width,
        height: 1,
    };

    f.render_widget(
        Paragraph::new(help_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        help_area,
    );
}

/// Helper function to create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(percent_y)) / 2),
            Constraint::Length(percent_y),
            Constraint::Length((r.height.saturating_sub(percent_y)) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((r.width.saturating_sub(percent_x)) / 2),
            Constraint::Length(percent_x),
            Constraint::Length((r.width.saturating_sub(percent_x)) / 2),
        ])
        .split(popup_layout[1])[1]
}
