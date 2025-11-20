use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::{BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_status_bar(f: &mut ratatui::Frame, area: Rect, line: &str, pending_notifications: usize) {
    // Create blinking indicator for pending notifications
    // Blink every 500ms (on/off every 500ms)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let blink_on = (now / 500).is_multiple_of(2);
    
    let mut spans = vec![Span::styled(
        line.to_string(),
        Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR),
    )];
    
    // Add blinking notification indicator if there are pending notifications
    if pending_notifications > 0 {
        let indicator_text = format!(" 🔔 {} new notification(s)", pending_notifications);
        let indicator_style = if blink_on {
            Style::default()
                .bg(BACKGROUND_COLOR)
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default()
                .bg(BACKGROUND_COLOR)
                .fg(PRIMARY_COLOR)
        };
        spans.push(Span::styled(indicator_text, indicator_style));
    }
    
    let bar = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR)),
    );
    f.render_widget(bar, area);
}
