use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

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

fn str_width(s: &str) -> usize {
    Span::raw(s).width()
}

/// Clip `s` to `max` display columns, ending with `…` when cut.
fn truncate_to_width(s: &str, max: usize) -> String {
    if str_width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let budget = max - 1;
    let mut out = String::new();
    let mut w = 0;
    let mut buf = [0u8; 4];
    for ch in s.chars() {
        let cw = str_width(ch.encode_utf8(&mut buf));
        if w + cw > budget {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

/// Draw the bottom status bar (Mostro name, optional own reputation, pubkey,
/// relays, currencies) plus a blinking notification badge.
///
/// One row per line, truncated to the area width, so long lines never push
/// later lines out of the fixed-height bar. The badge goes on the last visible
/// row and its width is reserved before truncating that row's text.
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

    let width = area.width as usize;
    let visible = lines.len().min(area.height as usize);
    let mut styled_lines: Vec<Line> = Vec::new();
    for (idx, line) in lines.iter().take(visible).enumerate() {
        let badge = (idx + 1 == visible && pending_notifications > 0)
            .then(|| format!(" 🔔 {} new notification(s)", pending_notifications));
        let badge_width = badge.as_deref().map_or(0, str_width);
        let text = truncate_to_width(line, width.saturating_sub(badge_width));
        let mut spans = status_line_spans(&text);

        if let Some(indicator_text) = badge {
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

    let bar = Paragraph::new(styled_lines).block(
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
            "🧌 Mostro name: demo | ⭐ 4.8 · 🗳 23 · since Nov 2023 | Pubkey: npub1abc".to_string(),
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

    fn production_status_lines() -> Vec<String> {
        vec![
            format!(
                "🧌 Mostro name: demo | ⭐ 4.8 · 🗳 23 · since Nov 2023 | Pubkey: {}",
                "a".repeat(64)
            ),
            "🔗 Relays: wss://relay.mostro.network, wss://nos.lol".to_string(),
            "💱 Currencies: USD, EUR, ARS - Filters: All currencies are accepted | Shift+F: Order filters | Shift+X: Clear order filters".to_string(),
        ]
    }

    #[test]
    fn three_row_status_bar_keeps_every_line_and_badge_at_common_widths() {
        for width in [80, 120] {
            let backend = TestBackend::new(width, 3);
            let mut terminal = Terminal::new(backend).unwrap();
            let lines = production_status_lines();
            terminal
                .draw(|f| render_status_bar(f, f.area(), &lines, 2))
                .unwrap();
            let buf = terminal.backend().buffer();
            assert!(buffer_contains(buf, "4.8"), "reputation at {width} cols");
            assert!(buffer_contains(buf, "Relays"), "relays at {width} cols");
            assert!(
                buffer_contains(buf, "Currencies"),
                "currencies at {width} cols"
            );
            assert!(
                buffer_contains(buf, "2 new notification(s)"),
                "badge at {width} cols"
            );
        }
    }

    #[test]
    fn short_status_bar_moves_badge_to_last_visible_row() {
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let lines = production_status_lines();
        terminal
            .draw(|f| render_status_bar(f, f.area(), &lines, 1))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Mostro name"));
        assert!(buffer_contains(buf, "1 new notification(s)"));
    }

    #[test]
    fn truncate_to_width_respects_display_columns() {
        assert_eq!(truncate_to_width("short", 10), "short");
        assert_eq!(truncate_to_width("abcdef", 4), "abc…");
        assert_eq!(truncate_to_width("abc", 0), "");
        let cut = truncate_to_width("⭐⭐⭐", 4);
        assert!(str_width(&cut) <= 4);
        assert!(cut.ends_with('…'));
    }
}
