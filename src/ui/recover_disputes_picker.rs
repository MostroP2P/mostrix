//! Shift+R orphan picker: choose which relay `in-progress` disputes to re-take.
//!
//! Relay kind-38386 does not publish the assigned solver, so Mostrix cannot
//! auto-filter "mine". The admin picks explicitly (↑↓ + Space) before any
//! `AdminTakeDispute` is sent.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Wrap,
};
use uuid::Uuid;

use super::helpers::{self, render_table_list_scrollbar};
use super::{BACKGROUND_COLOR, PRIMARY_COLOR};

pub const RECOVER_PICKER_HINT: &str =
    "↑↓ Navigate  Space Toggle  Enter Confirm  Esc Cancel  (no Select-All)";

/// IDs that will be recovered: all checked rows, or the cursor row if none checked.
pub fn recover_ids_from_selection(
    candidates: &[Uuid],
    cursor: usize,
    checked: &[bool],
) -> Vec<Uuid> {
    let marked: Vec<Uuid> = candidates
        .iter()
        .zip(checked.iter())
        .filter_map(|(id, on)| if *on { Some(*id) } else { None })
        .collect();
    if !marked.is_empty() {
        return marked;
    }
    candidates.get(cursor).copied().into_iter().collect()
}

pub fn move_recover_cursor(cursor: &mut usize, len: usize, down: bool) {
    if len == 0 {
        *cursor = 0;
        return;
    }
    if down {
        *cursor = (*cursor + 1).min(len - 1);
    } else {
        *cursor = cursor.saturating_sub(1);
    }
}

pub fn toggle_recover_checked(checked: &mut [bool], cursor: usize) {
    if let Some(slot) = checked.get_mut(cursor) {
        *slot = !*slot;
    }
}

fn short_id(id: &Uuid) -> String {
    let s = id.to_string();
    if s.len() > 13 {
        format!("{}…{}", &s[..8], &s[s.len().saturating_sub(4)..])
    } else {
        s
    }
}

/// Render the Shift+R candidate list (relay in-progress missing from local DB).
pub fn render_recover_taken_disputes_picker(
    f: &mut ratatui::Frame,
    candidates: &[Uuid],
    cursor: usize,
    checked: &[bool],
) {
    let area = f.area();
    let popup_width = 72.min(area.width);
    let popup_height = 18.min(area.height);
    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let checked_count = checked.iter().filter(|c| **c).count();
    let title = format!(
        "🔄 Recover Taken Disputes ({}/{})",
        checked_count,
        candidates.len()
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(1),
        ],
    )
    .split(inner);

    let intro = Paragraph::new(vec![
        Line::from(Span::styled(
            "Relay in-progress disputes missing from your local DB.",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            "Select only ones you believe were assigned to you.",
            Style::default().fg(Color::Gray),
        )),
    ])
    .wrap(Wrap { trim: true });
    f.render_widget(intro, chunks[0]);

    let list_area = chunks[1];
    let items: Vec<ListItem> = candidates
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let mark = if checked.get(i).copied().unwrap_or(false) {
                "[x]"
            } else {
                "[ ]"
            };
            let full = id.to_string();
            let label = if list_area.width.saturating_sub(8) as usize >= full.len() {
                full
            } else {
                short_id(id)
            };
            let style = if i == cursor {
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(format!(" {mark} {label}"), style)))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(PRIMARY_COLOR)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ")
        .highlight_spacing(HighlightSpacing::Always);
    let mut state =
        ListState::default().with_selected(Some(cursor.min(candidates.len().saturating_sub(1))));
    f.render_stateful_widget(list, list_area, &mut state);

    let visible = list_area.height as usize;
    let offset = state.offset();
    render_table_list_scrollbar(f, list_area, candidates.len(), visible, 0, offset);

    f.render_widget(
        Paragraph::new(Span::styled(
            RECOVER_PICKER_HINT,
            Style::default().fg(Color::DarkGray),
        ))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[2],
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
    fn recover_ids_uses_checked_when_any_marked() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let c = Uuid::from_u128(3);
        let ids = recover_ids_from_selection(&[a, b, c], 0, &[false, true, true]);
        assert_eq!(ids, vec![b, c]);
    }

    #[test]
    fn recover_ids_falls_back_to_cursor_when_none_checked() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let ids = recover_ids_from_selection(&[a, b], 1, &[false, false]);
        assert_eq!(ids, vec![b]);
    }

    #[test]
    fn move_cursor_clamps_at_ends() {
        let mut c = 0;
        move_recover_cursor(&mut c, 3, false);
        assert_eq!(c, 0);
        move_recover_cursor(&mut c, 3, true);
        assert_eq!(c, 1);
        c = 2;
        move_recover_cursor(&mut c, 3, true);
        assert_eq!(c, 2);
    }

    #[test]
    fn picker_renders_checkboxes_and_hint() {
        let a = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let b = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_recover_taken_disputes_picker(f, &[a, b], 1, &[true, false]);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Recover Taken Disputes"));
        assert!(buffer_contains(buf, "[x]"));
        assert!(buffer_contains(buf, "[ ]"));
        assert!(buffer_contains(buf, "Space"));
        assert!(buffer_contains(buf, "1/2"));
    }

    #[test]
    fn picker_keeps_actions_on_narrow_short_terminal() {
        let id = Uuid::from_u128(9);
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_recover_taken_disputes_picker(f, &[id], 0, &[false]);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "Enter") || buffer_contains(buf, "Space"),
            "picker chrome must stay readable on 40x10"
        );
    }
}
