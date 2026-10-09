//! Ctrl+K Observer actions list (Clear all / Save attachment / Dismiss error).
//!
//! The Shared key field stays editable outside this popup; Esc alone dismisses
//! the inline error without wiping secrets (see [`crate::ui::AppState::clear_observer_secrets`]).

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use super::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Ordered rows shown in the Ctrl+K Observer actions popup.
pub const OBSERVER_ACTION_ROWS: &[&str] =
    &["L  Clear all", "S  Save attachment", "E  Dismiss error"];

#[must_use]
pub fn observer_action_count() -> usize {
    OBSERVER_ACTION_ROWS.len()
}

/// Letter shortcut → row index.
#[must_use]
pub fn observer_action_index_for_key(c: char) -> Option<usize> {
    match c.to_ascii_lowercase() {
        'l' => Some(0),
        's' => Some(1),
        'e' => Some(2),
        _ => None,
    }
}

fn use_compact_observer_actions(area: ratatui::layout::Rect) -> bool {
    let full_needed = (observer_action_count() as u16).saturating_add(6);
    area.height < full_needed || area.width < 24
}

/// Renders the Observer Ctrl+K action list.
pub fn render_observer_actions_popup(f: &mut ratatui::Frame, selected_index: usize) {
    let area = f.area();
    let compact = use_compact_observer_actions(area);
    let popup_width = if compact {
        area.width.clamp(1, 42)
    } else {
        42.min(area.width.saturating_sub(2).max(28))
    };
    let popup_height = if compact {
        area.height.max(1)
    } else {
        (observer_action_count() as u16)
            .saturating_add(6)
            .min(area.height.saturating_sub(1).max(8))
    };

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Observer actions ")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let list_area = if compact {
        inner
    } else {
        let chunks = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(2),
            ],
        )
        .split(inner);

        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "↑↓ select · letter jump · Enter",
                Style::default().fg(Color::DarkGray),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[0],
        );

        helpers::render_help_text(f, chunks[2], "Press ", "Esc", " to close");
        chunks[1]
    };

    let items: Vec<ListItem> = OBSERVER_ACTION_ROWS
        .iter()
        .map(|row| ListItem::new(Line::from(Span::raw(*row))))
        .collect();
    let mut state = ListState::default();
    state.select(Some(
        selected_index.min(observer_action_count().saturating_sub(1)),
    ));
    f.render_stateful_widget(
        List::new(items).highlight_style(
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        list_area,
        &mut state,
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
    fn letter_shortcuts_map_to_rows() {
        assert_eq!(observer_action_index_for_key('L'), Some(0));
        assert_eq!(observer_action_index_for_key('e'), Some(2));
        assert_eq!(observer_action_index_for_key('x'), None);
    }

    #[test]
    fn render_lists_core_actions() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_observer_actions_popup(f, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Observer actions"));
        assert!(buffer_contains(buf, "Clear all"));
        assert!(buffer_contains(buf, "Save attachment"));
        assert!(buffer_contains(buf, "Esc"));
    }

    #[test]
    fn compact_layout_on_tiny_terminal_keeps_actions_visible() {
        let backend = TestBackend::new(20, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_observer_actions_popup(f, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Observer actions"));
        assert!(
            buffer_contains(buf, "Clear") || buffer_contains(buf, "Save"),
            "essential action row must remain visible on 20×8"
        );
        assert!(
            !buffer_contains(buf, "to close"),
            "compact mode should omit the Esc close hint"
        );
    }
}
