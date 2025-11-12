use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::{BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_waiting(f: &mut ratatui::Frame) {
    let area = f.area();
    let popup_width = 50;
    let popup_height = 7;
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
            Constraint::Length(1), // spinner
        ],
    )
    .split(popup);

    f.render_widget(
        Paragraph::new(Line::from("Sending order and waiting for confirmation..."))
            .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[1],
    );

    // Simple spinner animation (could be enhanced)
    let spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
    let spinner_idx = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        / 100) as usize
        % spinner.chars().count();
    let spinner_char = spinner.chars().nth(spinner_idx).unwrap_or('⠋');
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            format!("{}", spinner_char),
            Style::default().fg(PRIMARY_COLOR),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        inner_chunks[2],
    );
}
