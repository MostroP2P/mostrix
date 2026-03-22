use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Renders an exit confirmation popup
pub fn render_exit_confirm(f: &mut ratatui::Frame, selected_button: bool) {
    let area = f.area();
    let popup_width = 60;
    let popup_height = 11; // Increased height to ensure help text fits inside

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    // Clear the entire popup area to remove any background content (including Exit tab text)
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Exit Mostrix ")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));

    // Get inner area (inside borders) for content layout
    let inner_area = block.inner(popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(1), // message
            Constraint::Length(1), // spacer
            Constraint::Length(3), // buttons
            Constraint::Length(1), // help text (must be inside borders)
            Constraint::Length(1), // help text for esc key
        ],
    )
    .split(inner_area);

    // Render the block after calculating inner area
    f.render_widget(block, popup);

    // Confirmation message
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Are you sure you want to exit Mostrix?",
            Style::default().fg(Color::White),
        )]))
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(ratatui::widgets::Wrap { trim: true }),
        chunks[1],
    );

    // YES/NO buttons
    helpers::render_yes_no_buttons(f, chunks[3], selected_button, "✓ YES", "✗ NO");

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
        chunks[4],
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
        chunks[5],
    );
}
