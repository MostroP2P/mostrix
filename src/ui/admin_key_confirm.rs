use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Renders a generic key confirmation popup
pub fn render_admin_key_confirm(
    f: &mut ratatui::Frame,
    title: &str,
    key_string: &str,
    selected_button: bool,
) {
    render_admin_key_confirm_with_message(f, title, key_string, selected_button, None);
}

/// Renders a generic key confirmation popup with optional custom message
pub fn render_admin_key_confirm_with_message(
    f: &mut ratatui::Frame,
    title: &str,
    key_string: &str,
    selected_button: bool,
    custom_message: Option<&str>,
) {
    let area = f.area();
    let popup_width = 80;
    let popup_height = 12;

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(2), // message (wrapped)
            Constraint::Length(1), // spacer
            Constraint::Length(1), // key display (truncated)
            Constraint::Length(1), // spacer
            Constraint::Length(3), // buttons
            Constraint::Length(1), // help text
        ],
    )
    .split(popup);

    // Confirmation message
    let message = custom_message.unwrap_or("Do you want to save this key in settings file?");
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            message,
            Style::default().fg(Color::White),
        )]))
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true }),
        chunks[1],
    );

    // Display truncated key (show first 30 chars + ...)
    // Only show key if no custom message (for settings saves) or if custom message is provided but we still want to show it
    // For AddSolver, we hide the key display
    if custom_message.is_none() {
        let display_key = if key_string.len() > 30 {
            format!("{}...", &key_string[..30])
        } else {
            key_string.to_string()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Key: ", Style::default()),
                Span::styled(
                    display_key,
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[3],
        );
    }

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

    // YES button
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

    // Help text - combine all messages into a single Paragraph
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
            Span::styled(" to confirm, ", Style::default()),
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
        chunks[6],
    );
}
