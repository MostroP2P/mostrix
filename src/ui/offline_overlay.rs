use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::ui::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_offline_overlay(f: &mut ratatui::Frame, message: &str) {
    let area = f.area();
    let width = 70u16.min(area.width.saturating_sub(2)).max(20);
    let height = 7u16.min(area.height.saturating_sub(2)).max(5);
    let popup = helpers::create_centered_popup(area, width, height);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("⚠️ Offline")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Yellow));
    f.render_widget(block, popup);

    let inner = Rect {
        x: popup.x.saturating_add(1),
        y: popup.y.saturating_add(1),
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };

    let text = Text::from(vec![
        Line::from(vec![Span::styled(
            message,
            Style::default()
                .fg(Color::White)
                .bg(BACKGROUND_COLOR)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Retrying every 5s",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Tip: ", Style::default().fg(Color::Gray)),
            Span::raw("restore internet; Mostrix will reconnect automatically."),
        ]),
    ]);

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
    fn render_offline_overlay_shows_message_and_retry_hint() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_offline_overlay(f, "Relays unreachable right now"))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Offline"));
        assert!(buffer_contains(buf, "Relays unreachable right now"));
        assert!(buffer_contains(buf, "Retrying every 5s"));
    }
}
