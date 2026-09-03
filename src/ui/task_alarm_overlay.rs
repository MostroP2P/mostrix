use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::ui::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Non-blocking overlay for recoverable background-task failures (per-task respawn in progress).
///
/// Degrades on short/narrow terminals: drop tip and spacer first so the message and
/// retry status stay visible (see AGENTS.md TUI guidelines).
pub fn render_task_alarm_overlay(f: &mut ratatui::Frame, message: &str) {
    let area = f.area();
    let width = 72u16.min(area.width.saturating_sub(2)).max(18);
    // Prefer fitting the frame over a fixed decorative height.
    let height = 7u16.min(area.height.saturating_sub(2)).max(3);
    let popup = helpers::create_centered_popup(area, width, height);

    f.render_widget(Clear, popup);

    let title = if popup.width < 28 {
        "⚠ Task restarting"
    } else {
        "⚠ Background task restarting"
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Yellow));
    f.render_widget(block, popup);

    let inner = Rect {
        x: popup.x.saturating_add(1),
        y: popup.y.saturating_add(1),
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };

    let status_line = Line::from(vec![
        Span::styled("Status: ", Style::default().fg(Color::Gray)),
        Span::styled(
            "Auto-retry with backoff",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let message_line = Line::from(vec![Span::styled(
        message,
        Style::default()
            .fg(Color::White)
            .bg(BACKGROUND_COLOR)
            .add_modifier(Modifier::BOLD),
    )]);

    // Short frames: message + status only. Narrow-but-tall: drop tip.
    let text = if inner.height <= 2 {
        Text::from(vec![message_line, status_line])
    } else if inner.height <= 4 || inner.width < 40 {
        Text::from(vec![message_line, Line::from(""), status_line])
    } else {
        Text::from(vec![
            message_line,
            Line::from(""),
            status_line,
            Line::from(vec![
                Span::styled("Tip: ", Style::default().fg(Color::Gray)),
                Span::raw("other relays/DMs keep running; restart Mostrix if this repeats."),
            ]),
        ])
    };

    f.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .style(Style::default().bg(BACKGROUND_COLOR)),
        inner,
    );
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
    fn render_task_alarm_overlay_shows_message_and_retry_hint() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_task_alarm_overlay(f, "trade DM listener stopped unexpectedly");
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Background task restarting"));
        assert!(buffer_contains(
            buf,
            "trade DM listener stopped unexpectedly"
        ));
        assert!(buffer_contains(buf, "Auto-retry with backoff"));
    }

    #[test]
    fn render_task_alarm_overlay_keeps_message_and_status_on_short_frame() {
        let backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_task_alarm_overlay(f, "order book scheduler stopped");
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "order book scheduler stopped"));
        assert!(buffer_contains(buf, "Auto-retry with backoff"));
    }

    #[test]
    fn render_task_alarm_overlay_keeps_message_and_status_on_narrow_frame() {
        let backend = TestBackend::new(22, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_task_alarm_overlay(f, "chat router stopped");
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Task restarting"));
        // Wrapped across rows; assert fragments rather than the full contiguous string.
        assert!(buffer_contains(buf, "chat router"));
        assert!(buffer_contains(buf, "stopped"));
        assert!(buffer_contains(buf, "Auto-retry"));
    }
}
