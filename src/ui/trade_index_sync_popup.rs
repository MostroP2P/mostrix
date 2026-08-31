//! Popup offering to sync trade index with Mostro and retry the failed command.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::ui::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_trade_index_sync_confirm(f: &mut ratatui::Frame, selected_button: bool) {
    let area = f.area();
    let popup_width = area.width.saturating_sub(2).clamp(24, 80);
    let compact = area.height < 14 || area.width < 56;
    let narrow = area.width < 48;
    let popup_height = if compact {
        area.height.saturating_sub(1).clamp(8, 11)
    } else {
        area.height.saturating_sub(2).clamp(12, 14)
    };

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("\u{1F504} Trade Index Out of Sync")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let button_rows = if narrow { 2 } else { 1 };
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Min(if compact { 2 } else { 3 }),
            Constraint::Length(button_rows),
            Constraint::Length(1),
        ],
    )
    .split(inner);

    let message = if compact {
        "Trade index mismatch with Mostro.\nSync and retry?"
    } else {
        "Mostro rejected the command because this client's trade index does not match the daemon's records.\n\nSync from Mostro, then retry the same action?"
    };
    f.render_widget(
        Paragraph::new(message)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[0],
    );

    render_sync_buttons(f, chunks[1], selected_button, narrow);

    let help = if compact {
        Line::from(vec![
            Span::styled(
                "←/→",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" · "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" · "),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                "Left/Right",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" select, "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" confirm, "),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" cancel"),
        ])
    };
    f.render_widget(
        Paragraph::new(help).alignment(ratatui::layout::Alignment::Center),
        chunks[2],
    );
}

fn render_sync_buttons(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    selected_button: bool,
    narrow: bool,
) {
    let yes_style = if selected_button {
        Style::default()
            .fg(Color::Black)
            .bg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let no_style = if selected_button {
        Style::default().fg(Color::White)
    } else {
        Style::default()
            .fg(Color::Black)
            .bg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD)
    };

    if narrow {
        let rows = Layout::new(
            Direction::Vertical,
            [Constraint::Length(1), Constraint::Length(1)],
        )
        .split(area);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" SYNC & RETRY ", yes_style)))
                .alignment(ratatui::layout::Alignment::Center),
            rows[0],
        );
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(" CANCEL ", no_style)))
                .alignment(ratatui::layout::Alignment::Center),
            rows[1],
        );
    } else {
        let buttons = Line::from(vec![
            Span::styled("  SYNC & RETRY  ", yes_style),
            Span::raw("    "),
            Span::styled("  CANCEL  ", no_style),
        ]);
        f.render_widget(
            Paragraph::new(buttons).alignment(ratatui::layout::Alignment::Center),
            area,
        );
    }
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

    fn assert_popup_chrome(buf: &ratatui::buffer::Buffer) {
        assert!(buffer_contains(buf, "Trade Index Out of Sync"));
        assert!(buffer_contains(buf, "SYNC & RETRY"));
        assert!(buffer_contains(buf, "CANCEL"));
        assert!(buffer_contains(buf, "Enter"));
        assert!(buffer_contains(buf, "Esc"));
    }

    #[test]
    fn trade_index_sync_popup_renders_sync_and_cancel() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_trade_index_sync_confirm(f, true))
            .expect("draw");
        assert_popup_chrome(terminal.backend().buffer());
    }

    #[test]
    fn trade_index_sync_popup_renders_on_narrow_terminal() {
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_trade_index_sync_confirm(f, false))
            .expect("draw");
        let buf = terminal.backend().buffer();
        assert_popup_chrome(buf);
        assert!(buffer_contains(buf, "mismatch"));
    }

    #[test]
    fn trade_index_sync_popup_renders_on_short_terminal() {
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_trade_index_sync_confirm(f, true))
            .expect("draw");
        let buf = terminal.backend().buffer();
        assert_popup_chrome(buf);
        assert!(buffer_contains(buf, "retry"));
    }
}
