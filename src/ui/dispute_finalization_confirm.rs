use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Render the dispute finalization confirmation popup
pub fn render_finalization_confirm(
    f: &mut ratatui::Frame,
    app: &super::AppState,
    dispute_id: &uuid::Uuid,
    is_settle: bool,
    selected_button: bool,
) {
    // Find the dispute by dispute_id (or fallback to order_id for backwards compatibility)
    let dispute = app
        .admin_disputes_in_progress
        .iter()
        .find(|d| d.dispute_id == dispute_id.to_string() || d.id == dispute_id.to_string());

    let Some(selected_dispute) = dispute else {
        // If dispute not found, show error with message
        let area = f.area();
        let popup_width = area.width.saturating_sub(area.width / 4);
        let popup_height = 10;
        let popup = helpers::create_centered_popup(area, popup_width, popup_height);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title("❌ Error")
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Red));

        let inner = block.inner(popup);
        f.render_widget(block, popup);

        let error_msg = format!("Dispute not found: {}", dispute_id);
        // Ensure wrap_width is at least 1 to avoid panic from chunks(0)
        let wrap_width = inner.width.saturating_sub(2).max(1) as usize;
        let error_lines: Vec<Line> = error_msg
            .chars()
            .collect::<Vec<_>>()
            .chunks(wrap_width)
            .map(|chunk| Line::from(chunk.iter().collect::<String>()))
            .collect();

        let mut lines = vec![];
        lines.push(Line::from(""));
        for line in error_lines {
            lines.push(line);
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Press ESC to close",
            Style::default().fg(Color::DarkGray),
        )]));

        let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
        f.render_widget(paragraph, inner);
        return;
    };

    let area = f.area();
    let popup_width = 70;
    let popup_height = 15;

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    // Determine action details
    let (action_title, action_description, action_color) = if is_settle {
        (
            "Pay Buyer (AdminSettle)",
            "This will settle the dispute in favor of the buyer.\nThe buyer will receive the full escrow amount.",
            Color::Green,
        )
    } else {
        (
            "Refund Seller (AdminCancel)",
            "This will cancel the order and refund the seller.\nThe seller will receive the full escrow amount back.",
            Color::Red,
        )
    };

    let block = Block::default()
        .title(format!("⚠️  Confirm {}", action_title))
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));

    let inner_area = block.inner(popup);
    f.render_widget(block, popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(1), // dispute ID
            Constraint::Length(1), // spacer
            Constraint::Length(3), // action description
            Constraint::Length(1), // spacer
            Constraint::Length(3), // buttons
            Constraint::Length(1), // help text
            Constraint::Length(1), // help text for esc
        ],
    )
    .split(inner_area);

    // Dispute ID
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Dispute ID: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &selected_dispute.dispute_id,
                Style::default().fg(PRIMARY_COLOR),
            ),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

    // Action description
    let description_lines: Vec<Line> = action_description
        .lines()
        .map(|line| Line::from(vec![Span::styled(line, Style::default().fg(Color::White))]))
        .collect();
    f.render_widget(
        Paragraph::new(description_lines).alignment(ratatui::layout::Alignment::Center),
        chunks[3],
    );

    // Yes/No buttons
    let button_area = chunks[5];
    let button_width = 15;
    let separator_width = 1;
    let total_button_width = (button_width * 2) + separator_width;

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
            Constraint::Length(separator_width),
            Constraint::Length(button_width),
        ],
    )
    .split(centered_button_area);

    // YES button - always use green when highlighted
    let yes_style = if selected_button {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(action_color)
            .add_modifier(Modifier::BOLD)
    };

    let yes_block = Block::default().borders(Borders::ALL).style(yes_style);
    f.render_widget(yes_block, button_chunks[0]);

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

    // NO button
    let no_style = if !selected_button {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };

    let no_block = Block::default().borders(Borders::ALL).style(no_style);
    f.render_widget(no_block, button_chunks[2]);

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

    // Help text - first line
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Use ", Style::default()),
            Span::styled(
                "Left/Right",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to select, ", Style::default()),
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to confirm", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[6],
    );

    // Help text for Esc key - second line
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to cancel", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[7],
    );
}
