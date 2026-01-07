use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{helpers, KeyInputState, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_key_input_popup(
    f: &mut ratatui::Frame,
    title: &str,
    label: &str,
    placeholder: &str,
    key_state: &KeyInputState,
    is_sensitive: bool,
) {
    let area = f.area();
    let popup_width = 80;
    let popup_height = if is_sensitive { 12 } else { 10 };

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);

    // Clear the popup area
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1),                                // spacer
            Constraint::Length(if is_sensitive { 2 } else { 1 }), // warning (if sensitive) or label
            Constraint::Length(1),                                // spacer
            Constraint::Length(3),                                // input field
            Constraint::Length(1),                                // spacer
            Constraint::Length(1),                                // help text
            Constraint::Length(1),                                // help text
        ],
    )
    .split(popup);

    // Warning for sensitive data (emphasized)
    if is_sensitive {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "⚠️  SENSITIVE DATA: Private keys are confidential!",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[1],
        );
    }

    // Label
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            label,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[if is_sensitive { 2 } else { 1 }],
    );

    // Input field
    let input_display = if key_state.key_input.is_empty() {
        placeholder.to_string()
    } else {
        key_state.key_input.clone()
    };

    let input_style = if key_state.focused {
        Style::default()
            .fg(PRIMARY_COLOR)
            .bg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    f.render_widget(
        Paragraph::new(input_display)
            .style(input_style)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(if key_state.focused {
                        Style::default().fg(PRIMARY_COLOR)
                    } else {
                        Style::default()
                    }),
            ),
        chunks[3], // Input field is always at chunks[3]
    );

    // Help text
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Paste key (", Style::default()),
            Span::styled(
                "Ctrl+Shift+V",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" or right-click), then press ", Style::default()),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to submit", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[5],
    );

    // Esc help text
    helpers::render_help_text(f, chunks[6], "Press ", "Esc", " to cancel");
}
