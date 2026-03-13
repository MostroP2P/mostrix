use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::{BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_status_bar(
    f: &mut ratatui::Frame,
    area: Rect,
    lines: &[String],
    pending_notifications: usize,
) {
    // Clear the area first to avoid leftover text
    f.render_widget(Clear, area);

    // Create blinking indicator for pending notifications
    // Blink every 500ms (on/off every 500ms)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let blink_on = (now / 500).is_multiple_of(2);

    // Build styled lines for the status bar
    let mut styled_lines: Vec<Line> = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let mut spans = vec![Span::styled(
            line.to_string(),
            Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR),
        )];

        // Add blinking notification indicator on the last line if there are pending notifications
        if idx == lines.len() - 1 && pending_notifications > 0 {
            let indicator_text = format!(" 🔔 {} new notification(s)", pending_notifications);
            let indicator_style = if blink_on {
                Style::default()
                    .bg(BACKGROUND_COLOR)
                    .fg(Color::Yellow)
                    .add_modifier(ratatui::style::Modifier::BOLD)
            } else {
                Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR)
            };
            spans.push(Span::styled(indicator_text, indicator_style));
        }

        styled_lines.push(Line::from(spans));
    }

    // Render all status lines as a single wrapping paragraph so long text can flow
    let bar = Paragraph::new(styled_lines)
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::NONE)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR)),
        );
    f.render_widget(bar, area);
}
