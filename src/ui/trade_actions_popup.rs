//! Ctrl+K trade-actions list for My Trades (INSERT and COMMAND layers).

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use super::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Ordered rows shown in the Ctrl+K trade-actions popup.
pub const TRADE_ACTION_ROWS: &[&str] = &[
    "F  Mark fiat sent",
    "R  Release sats",
    "C  Cooperative cancel",
    "D  Open dispute",
    "V  Rate counterparty",
    "K  Reveal Shared key",
];

#[must_use]
pub fn trade_action_count() -> usize {
    TRADE_ACTION_ROWS.len()
}

/// Letter shortcut → row index (same letters as COMMAND Shift+chords).
#[must_use]
pub fn trade_action_index_for_key(c: char) -> Option<usize> {
    match c.to_ascii_lowercase() {
        'f' => Some(0),
        'r' => Some(1),
        'c' => Some(2),
        'd' => Some(3),
        'v' => Some(4),
        'k' => Some(5),
        _ => None,
    }
}

/// Whether the viewport can fit header + all actions + close hint.
fn use_compact_trade_actions(area: ratatui::layout::Rect) -> bool {
    // Borders (2) + header (1) + actions + close hint (2) + margin.
    let full_needed = (trade_action_count() as u16).saturating_add(6);
    area.height < full_needed || area.width < 24
}

/// Renders the My Trades Ctrl+K action list.
pub fn render_trade_actions_popup(f: &mut ratatui::Frame, selected_index: usize) {
    let area = f.area();
    let compact = use_compact_trade_actions(area);
    let popup_width = if compact {
        area.width.clamp(1, 42)
    } else {
        42.min(area.width.saturating_sub(2).max(24))
    };
    let popup_height = if compact {
        area.height.max(1)
    } else {
        (trade_action_count() as u16)
            .saturating_add(6)
            .min(area.height.saturating_sub(1).max(8))
    };

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Trade actions ")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let list_area = if compact {
        // Drop secondary header + Esc hint so the action list keeps usable height.
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

    let items: Vec<ListItem> = TRADE_ACTION_ROWS
        .iter()
        .map(|row| ListItem::new(Line::from(Span::raw(*row))))
        .collect();
    let mut state = ListState::default();
    state.select(Some(
        selected_index.min(trade_action_count().saturating_sub(1)),
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
        assert_eq!(trade_action_index_for_key('F'), Some(0));
        assert_eq!(trade_action_index_for_key('k'), Some(5));
        assert_eq!(trade_action_index_for_key('x'), None);
    }

    #[test]
    fn render_lists_core_actions() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_trade_actions_popup(f, 0)).unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Trade actions"));
        assert!(buffer_contains(buf, "Mark fiat sent"));
        assert!(buffer_contains(buf, "Release sats"));
        assert!(buffer_contains(buf, "Esc"));
    }

    #[test]
    fn compact_layout_on_tiny_terminal_keeps_actions_visible() {
        let backend = TestBackend::new(20, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_trade_actions_popup(f, 0)).unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Trade actions"));
        assert!(
            buffer_contains(buf, "Mark fiat") || buffer_contains(buf, "fiat sent"),
            "essential action row must remain visible on 20×8"
        );
        // Compact mode drops the Esc close hint to free list rows.
        assert!(
            !buffer_contains(buf, "to close"),
            "compact mode should omit the Esc close hint"
        );
    }
}
