use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::ui::PRIMARY_COLOR;

/// Vertical scrollbar for a bordered table/list whose selection scrolls with
/// [`ratatui::widgets::TableState`] / [`ratatui::widgets::ListState`].
///
/// Draws only when `content_len` exceeds the visible body. The track is confined
/// to data rows (skipping the top border and optional header) so the thumb does
/// not overwrite corner glyphs. Thumb position is the viewport **offset** after
/// a stateful render, not the selected index.
pub fn render_table_list_scrollbar(
    f: &mut ratatui::Frame,
    area: Rect,
    content_len: usize,
    visible_rows: usize,
    header_rows: u16,
    viewport_offset: usize,
) {
    if content_len <= visible_rows || visible_rows == 0 {
        return;
    }
    let track = Rect {
        x: area.x,
        y: area.y + 1 + header_rows,
        width: area.width,
        height: visible_rows as u16,
    };
    let mut scrollbar_state = ScrollbarState::new(content_len)
        .viewport_content_length(visible_rows)
        .position(viewport_offset);
    f.render_stateful_widget(
        Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
        track,
        &mut scrollbar_state,
    );
}

/// Creates a centered popup area within the given area.
pub fn create_centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    let (popup_width, popup_height) = (width.min(area.width), height.min(area.height));
    let [popup] = Layout::horizontal([Constraint::Length(popup_width)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::vertical([Constraint::Length(popup_height)])
        .flex(Flex::Center)
        .areas(popup);
    popup
}

/// Renders help text with a styled key binding.
pub fn render_help_text(f: &mut ratatui::Frame, area: Rect, prefix: &str, key: &str, suffix: &str) {
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prefix, Style::default()),
            Span::styled(
                key,
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(suffix, Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        area,
    );
}

/// Render a pair of centered YES/NO buttons inside the given area.
/// `selected_button = true` highlights YES, `false` highlights NO.
pub fn render_yes_no_buttons(
    f: &mut ratatui::Frame,
    area: Rect,
    selected_button: bool,
    yes_label: &str,
    no_label: &str,
) {
    let button_width = 18;
    let separator_width = 1;
    let total_button_width = (button_width * 2) + separator_width;

    let button_x = area.x + (area.width.saturating_sub(total_button_width)) / 2;
    let centered_button_area = Rect {
        x: button_x,
        y: area.y,
        width: total_button_width.min(area.width),
        height: area.height,
    };

    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(button_width),
            Constraint::Length(separator_width),
            Constraint::Length(button_width),
        ],
    )
    .split(centered_button_area);

    let yes_style = if selected_button {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };

    let yes_block = ratatui::widgets::Block::default()
        .borders(Borders::ALL)
        .style(yes_style);
    f.render_widget(yes_block, button_chunks[0]);

    let yes_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[0]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            yes_label,
            Style::default()
                .fg(if selected_button {
                    Color::Black
                } else {
                    Color::Green
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        yes_inner[0],
    );

    let no_style = if !selected_button {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };

    let no_block = ratatui::widgets::Block::default()
        .borders(Borders::ALL)
        .style(no_style);
    f.render_widget(no_block, button_chunks[2]);

    let no_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[2]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            no_label,
            Style::default()
                .fg(if !selected_button {
                    Color::Black
                } else {
                    Color::Red
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        no_inner[0],
    );
}

/// Three buttons: YES (green), NO (red), CANCEL (yellow). `selected` is `0`, `1`, or `2`.
pub fn render_yes_no_cancel_buttons(
    f: &mut ratatui::Frame,
    area: Rect,
    selected: u8,
    yes_label: &str,
    no_label: &str,
    cancel_label: &str,
) {
    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            // spacer to avoid buttons from touching the frame border
            Constraint::Length(2),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            // spacer to avoid buttons from touching the frame border
            Constraint::Length(2),
        ],
    )
    .split(area);

    let mut render_one =
        |idx: u8, chunk: Rect, label: &str, current: u8, base_fg: Color, selected_bg: Color| {
            let is_on = current == idx;
            let block_style = if is_on {
                Style::default()
                    .bg(selected_bg)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(base_fg).add_modifier(Modifier::BOLD)
            };
            let block = ratatui::widgets::Block::default()
                .borders(Borders::ALL)
                .style(block_style);
            f.render_widget(block, chunk);
            let inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
                .margin(1)
                .split(chunk);
            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    label,
                    Style::default()
                        .fg(if is_on { Color::Black } else { base_fg })
                        .add_modifier(Modifier::BOLD),
                )]))
                .alignment(ratatui::layout::Alignment::Center),
                inner[0],
            );
        };

    render_one(
        0,
        button_chunks[1],
        yes_label,
        selected,
        Color::Green,
        Color::Green,
    );
    render_one(
        1,
        button_chunks[2],
        no_label,
        selected,
        Color::Red,
        Color::Red,
    );
    render_one(
        2,
        button_chunks[3],
        cancel_label,
        selected,
        Color::Yellow,
        Color::Yellow,
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

    fn cell_with_bg(buf: &ratatui::buffer::Buffer, bg: Color) -> bool {
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].style().bg == Some(bg) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn create_centered_popup_centers_within_area() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let popup = create_centered_popup(area, 40, 10);
        assert_eq!(popup.width, 40);
        assert_eq!(popup.height, 10);
        assert_eq!(popup.x, 20);
        // (24 - 10) / 2 = 7 via Flex::Center
        assert_eq!(popup.y, 7);
    }

    #[test]
    fn create_centered_popup_respects_area_origin() {
        let area = Rect {
            x: 10,
            y: 5,
            width: 60,
            height: 20,
        };
        let popup = create_centered_popup(area, 20, 8);
        assert_eq!(popup.width, 20);
        assert_eq!(popup.height, 8);
        assert_eq!(popup.x, 30);
        assert_eq!(popup.y, 11);
    }

    #[test]
    fn create_centered_popup_clamps_to_area() {
        let area = Rect {
            x: 2,
            y: 3,
            width: 20,
            height: 10,
        };
        let popup = create_centered_popup(area, 40, 20);
        assert_eq!(popup.width, 20);
        assert_eq!(popup.height, 10);
        assert_eq!(popup.x, 2);
        assert_eq!(popup.y, 3);
    }

    #[test]
    fn render_help_text_shows_prefix_key_and_suffix() {
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_help_text(f, f.area(), "Press ", "Enter", " to confirm"))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Press "), "missing prefix");
        assert!(buffer_contains(buf, "Enter"), "missing key");
        assert!(buffer_contains(buf, " to confirm"), "missing suffix");
    }

    #[test]
    fn render_yes_no_buttons_shows_labels_and_yes_selection() {
        let backend = TestBackend::new(50, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_yes_no_buttons(f, f.area(), true, "YES", "NO"))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "YES"), "missing YES label");
        assert!(buffer_contains(buf, "NO"), "missing NO label");
        assert!(
            cell_with_bg(buf, Color::Green),
            "YES selection should paint green background"
        );
        assert!(
            !cell_with_bg(buf, Color::Red),
            "NO should not be highlighted when YES is selected"
        );
    }

    #[test]
    fn render_yes_no_buttons_highlights_no_when_unselected() {
        let backend = TestBackend::new(50, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_yes_no_buttons(f, f.area(), false, "Confirm", "Cancel"))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Confirm"));
        assert!(buffer_contains(buf, "Cancel"));
        assert!(
            cell_with_bg(buf, Color::Red),
            "NO selection should paint red background"
        );
        assert!(
            !cell_with_bg(buf, Color::Green),
            "YES should not be highlighted when NO is selected"
        );
    }

    #[test]
    fn render_yes_no_cancel_buttons_shows_all_labels() {
        let backend = TestBackend::new(60, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_yes_no_cancel_buttons(f, f.area(), 0, "YES", "NO", "CANCEL");
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "YES"));
        assert!(buffer_contains(buf, "NO"));
        assert!(buffer_contains(buf, "CANCEL"));
    }

    #[test]
    fn render_yes_no_cancel_buttons_highlights_each_selection() {
        for (selected, expected_bg) in
            [(0u8, Color::Green), (1u8, Color::Red), (2u8, Color::Yellow)]
        {
            let backend = TestBackend::new(60, 5);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|f| {
                    render_yes_no_cancel_buttons(f, f.area(), selected, "YES", "NO", "CANCEL");
                })
                .unwrap();
            let buf = terminal.backend().buffer();
            assert!(
                cell_with_bg(buf, expected_bg),
                "selected={selected} should paint {expected_bg:?} background"
            );
        }
    }
}
