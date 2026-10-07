use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::{BACKGROUND_COLOR, PRIMARY_COLOR};

fn primary_style() -> Style {
    Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR)
}

fn reputation_star_style() -> Style {
    Style::default()
        .bg(BACKGROUND_COLOR)
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Split a status line so `⭐` + rating use yellow; the rest stays primary.
fn status_line_spans(line: &str) -> Vec<Span<'static>> {
    const STAR: &str = "⭐ ";
    let Some(star_at) = line.find(STAR) else {
        return vec![Span::styled(line.to_string(), primary_style())];
    };
    let mut spans = Vec::new();
    if star_at > 0 {
        spans.push(Span::styled(line[..star_at].to_string(), primary_style()));
    }
    let after_star = &line[star_at..];
    match after_star.find(" · ") {
        Some(sep) => {
            spans.push(Span::styled(
                after_star[..sep].to_string(),
                reputation_star_style(),
            ));
            spans.push(Span::styled(after_star[sep..].to_string(), primary_style()));
        }
        None => spans.push(Span::styled(
            after_star.to_string(),
            reputation_star_style(),
        )),
    }
    spans
}

/// Draw the multi-line bottom status bar (Mostro name/pubkey, optional own
/// reputation segment, relays, currencies) plus a blinking notification badge.
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
        let mut spans = status_line_spans(line);

        // Add blinking notification indicator on the last line if there are pending notifications
        if idx == lines.len() - 1 && pending_notifications > 0 {
            let indicator_text = format!(" 🔔 {} new notification(s)", pending_notifications);
            let indicator_style = if blink_on {
                Style::default()
                    .bg(BACKGROUND_COLOR)
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                primary_style()
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_contains(buf: &ratatui::buffer::Buffer, needle: &str) -> bool {
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
            flat.push('\n');
        }
        flat.contains(needle)
    }

    #[test]
    fn render_status_bar_without_notifications() {
        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let lines = vec!["Connected to relays".to_string()];
        terminal
            .draw(|f| render_status_bar(f, f.area(), &lines, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Connected to relays"));
        assert!(!buffer_contains(buf, "new notification"));
    }

    #[test]
    fn render_status_bar_with_pending_notifications() {
        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let lines = vec!["Status line".to_string()];
        terminal
            .draw(|f| render_status_bar(f, f.area(), &lines, 3))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Status line"));
        assert!(buffer_contains(buf, "new notification"));
    }

    #[test]
    fn render_status_bar_shows_own_reputation_segment() {
        let backend = TestBackend::new(100, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let lines = vec![
            "🧌 Mostro name: demo | Pubkey: npub1abc | ⭐ 4.8 · 🗳 23 · since Nov 2023".to_string(),
        ];
        terminal
            .draw(|f| render_status_bar(f, f.area(), &lines, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "⭐"));
        assert!(buffer_contains(buf, "4.8"));
        assert!(buffer_contains(buf, "🗳"));
        assert!(buffer_contains(buf, "since Nov 2023"));
        assert!(!buffer_contains(buf, "reputation: none"));
    }

    #[test]
    fn status_line_spans_paints_star_rating_yellow() {
        let spans = status_line_spans("name | ⭐ 4.8 · 🗳 23 · since Nov 2023");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content.as_ref(), "name | ");
        assert_eq!(spans[1].content.as_ref(), "⭐ 4.8");
        assert_eq!(spans[1].style.fg, Some(Color::Yellow));
        assert!(spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert!(spans[2].content.as_ref().starts_with(" · 🗳"));
        assert_eq!(spans[2].style.fg, Some(PRIMARY_COLOR));
    }
}
