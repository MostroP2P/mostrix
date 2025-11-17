use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::{BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_waiting(f: &mut ratatui::Frame) {
    let area = f.area();
    let popup_width = 60;
    let popup_height = 10;
    let popup_x = area.x + (area.width - popup_width) / 2;
    let popup_y = area.y + (area.height - popup_height) / 2;
    let popup = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    let block = Block::default()
        .title("⏳ Waiting for Mostro")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    let inner_chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(1), // message
            Constraint::Length(1), // spacer
            Constraint::Length(1), // spinner
            Constraint::Length(1), // dots animation
            Constraint::Length(1), // spacer
            Constraint::Length(1), // hint
        ],
    )
    .split(popup);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Sending order and waiting for confirmation...",
            Style::default().add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[1],
    );

    // Enhanced spinner animation with multiple frames
    let elapsed_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    // Braille spinner (faster rotation)
    let spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
    let spinner_idx = ((elapsed_millis / 80) as usize) % spinner.chars().count();
    let spinner_char = spinner.chars().nth(spinner_idx).unwrap_or('⠋');

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            format!("  {}  ", spinner_char),
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[3],
    );

    // Animated dots for extra visual feedback
    let dots_count = ((elapsed_millis / 400) as usize % 4) + 1;
    let dots = ".".repeat(dots_count);
    let dots_line = Line::from(vec![
        Span::styled("Processing", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:<4}", dots), Style::default().fg(PRIMARY_COLOR)),
    ]);

    f.render_widget(
        Paragraph::new(dots_line).alignment(ratatui::layout::Alignment::Center),
        inner_chunks[4],
    );

    // Hint
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Please wait, this may take a few seconds",
            Style::default().fg(Color::DarkGray),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[6],
    );
}
