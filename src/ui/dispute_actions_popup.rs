//! Ctrl+K dispute-actions list for Disputes in Progress (INSERT and COMMAND).

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use super::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Ordered rows shown in the Ctrl+K dispute-actions popup.
pub const DISPUTE_ACTION_ROWS: &[&str] = &[
    "F  Resolve dispute",
    "R  Recover taken disputes",
    "C  Toggle Finalized / In Progress",
    "D  Remove from local DB",
];

#[must_use]
pub fn dispute_action_count() -> usize {
    DISPUTE_ACTION_ROWS.len()
}

/// Letter shortcut → row index (same letters as COMMAND Shift+chords / Del).
#[must_use]
pub fn dispute_action_index_for_key(c: char) -> Option<usize> {
    match c.to_ascii_lowercase() {
        'f' => Some(0),
        'r' => Some(1),
        'c' => Some(2),
        'd' => Some(3),
        _ => None,
    }
}

fn use_compact_dispute_actions(area: ratatui::layout::Rect) -> bool {
    let full_needed = (dispute_action_count() as u16).saturating_add(6);
    area.height < full_needed || area.width < 24
}

/// Renders the Disputes in Progress Ctrl+K action list.
pub fn render_dispute_actions_popup(f: &mut ratatui::Frame, selected_index: usize) {
    let area = f.area();
    let compact = use_compact_dispute_actions(area);
    let popup_width = if compact {
        area.width.clamp(1, 48)
    } else {
        48.min(area.width.saturating_sub(2).max(28))
    };
    let popup_height = if compact {
        area.height.max(1)
    } else {
        (dispute_action_count() as u16)
            .saturating_add(6)
            .min(area.height.saturating_sub(1).max(8))
    };

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Dispute actions ")
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

    let items: Vec<ListItem> = DISPUTE_ACTION_ROWS
        .iter()
        .map(|row| ListItem::new(Line::from(Span::raw(*row))))
        .collect();
    let mut state = ListState::default();
    state.select(Some(
        selected_index.min(dispute_action_count().saturating_sub(1)),
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
        assert_eq!(dispute_action_index_for_key('F'), Some(0));
        assert_eq!(dispute_action_index_for_key('d'), Some(3));
        assert_eq!(dispute_action_index_for_key('x'), None);
    }

    #[test]
    fn render_lists_core_actions() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_dispute_actions_popup(f, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Dispute actions"));
        assert!(buffer_contains(buf, "Resolve dispute"));
        assert!(buffer_contains(buf, "Recover taken"));
        assert!(buffer_contains(buf, "Esc"));
    }

    #[test]
    fn compact_layout_on_tiny_terminal_keeps_actions_visible() {
        let backend = TestBackend::new(20, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_dispute_actions_popup(f, 0))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Dispute actions"));
        assert!(
            buffer_contains(buf, "Resolve") || buffer_contains(buf, "Recover"),
            "essential action row must remain visible on 20×8"
        );
        assert!(
            !buffer_contains(buf, "to close"),
            "compact mode should omit the Esc close hint"
        );
    }
}
