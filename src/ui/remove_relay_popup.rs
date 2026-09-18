use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState,
};

use super::helpers::create_centered_popup;
use super::{BACKGROUND_COLOR, PRIMARY_COLOR};

/// Centered dropdown-style popup to pick a relay to remove, mirroring the
/// create-order currency picker (List + scrollbar + hint line).
pub fn render_remove_relay_popup(f: &mut ratatui::Frame, relays: &[String], selected: usize) {
    let area = f.area();
    let content_rows = relays.len().clamp(1, 8) as u16;
    let width = 60u16.min(area.width).max(1);
    // Size to available height (borders + rows); never subtract a margin that
    // could starve the single relay row on short terminals (e.g. 20x4).
    let height = (content_rows + 3).min(area.height).max(3.min(area.height));
    let popup = create_centered_popup(area, width, height);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" \u{1f4e1} Remove Relay ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Drop the hint line when height is tight so at least one relay row stays visible.
    let show_hint = inner.height >= 2;
    let (list_area, hint_area) = if show_hint {
        let split = Layout::new(
            Direction::Vertical,
            [Constraint::Min(1), Constraint::Length(1)],
        )
        .split(inner);
        (split[0], Some(split[1]))
    } else {
        (inner, None)
    };

    if relays.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  no relays configured",
                Style::default().fg(Color::DarkGray),
            ))
            .style(Style::default().bg(BACKGROUND_COLOR)),
            list_area,
        );
    } else {
        let selected = selected.min(relays.len() - 1);
        let items: Vec<ListItem> = relays
            .iter()
            .map(|relay| ListItem::new(Line::from(Span::raw(relay.clone()))))
            .collect();

        let list = List::new(items)
            .style(Style::default().fg(Color::White).bg(BACKGROUND_COLOR))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("\u{203a} ");
        let mut state = ListState::default().with_selected(Some(selected));
        f.render_stateful_widget(list, list_area, &mut state);

        if relays.len() > list_area.height as usize {
            let mut sb_state = ScrollbarState::new(relays.len()).position(selected);
            f.render_stateful_widget(
                Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
                list_area,
                &mut sb_state,
            );
        }
    }

    if let Some(hint_area) = hint_area {
        f.render_widget(
            Paragraph::new(Span::styled(
                "\u{2191}\u{2193} move \u{2022} Enter remove \u{2022} Esc close",
                Style::default().fg(Color::DarkGray),
            ))
            .style(Style::default().bg(BACKGROUND_COLOR)),
            hint_area,
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

    #[test]
    fn render_lists_relays_and_chrome() {
        let relays = vec![
            "wss://relay.mostro.network".to_string(),
            "wss://relay.shadowbip.com".to_string(),
        ];
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_remove_relay_popup(f, &relays, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Remove Relay"));
        assert!(buffer_contains(buf, "relay.mostro.network"));
        assert!(buffer_contains(buf, "relay.shadowbip.com"));
        assert!(buffer_contains(buf, "Enter remove"));
    }

    #[test]
    fn render_highlights_selected_relay() {
        let relays = vec![
            "wss://relay.mostro.network".to_string(),
            "wss://relay.shadowbip.com".to_string(),
        ];
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_remove_relay_popup(f, &relays, 1))
            .unwrap();
        let buf = terminal.backend().buffer();
        // The selected row is painted with the primary color background.
        let mut highlighted = false;
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                if cell.symbol().contains('s') && cell.bg == PRIMARY_COLOR {
                    highlighted = true;
                }
            }
        }
        assert!(highlighted, "selected relay row should use the primary bg");
    }

    #[test]
    fn render_empty_shows_placeholder() {
        let relays: Vec<String> = Vec::new();
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_remove_relay_popup(f, &relays, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "no relays configured"));
    }

    #[test]
    fn render_keeps_relay_row_on_short_terminal() {
        // On a 20x4 terminal the hint line is dropped but a relay row stays visible.
        let relays = vec!["wss://relay.mostro.network".to_string()];
        let backend = TestBackend::new(20, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_remove_relay_popup(f, &relays, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "relay"),
            "a relay row must remain visible on short terminals"
        );
    }
}
