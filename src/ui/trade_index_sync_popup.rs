//! Popup offering to sync trade index with Mostro and retry the failed command.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_trade_index_sync_confirm(f: &mut ratatui::Frame, selected_button: bool) {
    let area = f.area();
    let popup_width = 80;
    let popup_height = 14;

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("\u{1F504} Trade Index Out of Sync")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    let chunks = ratatui::layout::Layout::new(
        ratatui::layout::Direction::Vertical,
        [
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(4),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(3),
            ratatui::layout::Constraint::Length(1),
        ],
    )
    .split(popup);

    f.render_widget(
        Paragraph::new(vec![
            Line::from("Mostro rejected the command because this client's trade index"),
            Line::from("does not match the daemon's records."),
            Line::from(""),
            Line::from("Sync from Mostro, then retry the same action?"),
        ])
        .alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

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

    let buttons = Line::from(vec![
        Span::styled("  SYNC & RETRY  ", yes_style),
        Span::raw("    "),
        Span::styled("  CANCEL  ", no_style),
    ]);
    f.render_widget(
        Paragraph::new(buttons).alignment(ratatui::layout::Alignment::Center),
        chunks[3],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
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
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn trade_index_sync_popup_renders_sync_and_cancel() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_trade_index_sync_confirm(f, true))
            .expect("draw");
        let buf = terminal.backend().buffer();
        let flat: String = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| buf[(x, y)].symbol()))
            .collect();
        assert!(flat.contains("Trade Index Out of Sync"));
        assert!(flat.contains("SYNC & RETRY"));
        assert!(flat.contains("CANCEL"));
    }
}
