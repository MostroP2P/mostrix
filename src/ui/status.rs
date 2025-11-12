use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::{BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_status_bar(f: &mut ratatui::Frame, area: Rect, line: &str) {
    let bar = Paragraph::new(Line::from(line.to_string())).block(
        Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR)),
    );
    f.render_widget(bar, area);
}
