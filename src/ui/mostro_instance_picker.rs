//! Settings popup: pick a trusted Mostro instance or type a custom pubkey.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState,
};

use super::helpers::create_centered_popup;
use super::mostro_instances::{
    picker_rows, region_for_pubkey, row_label, MostroInstancePicker, MostroInstancePickerRow,
};
use super::{BACKGROUND_COLOR, PRIMARY_COLOR};

/// Centered list popup for selecting a Mostro instance (trusted + custom).
pub fn render_mostro_instance_picker(
    f: &mut ratatui::Frame,
    picker: &MostroInstancePicker,
    current_hex: &str,
) {
    let area = f.area();
    let rows = picker_rows(&picker.filter);
    let content_rows = rows.len().clamp(1, 10) as u16;
    let width = 72u16.min(area.width).max(1);
    // filter line + list + hint; degrade on short terminals
    let height = (content_rows + 5).min(area.height).max(4.min(area.height));
    let popup = create_centered_popup(area, width, height);

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" 🌐 Select Mostro Instance ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let show_filter = inner.height >= 3;
    let show_hint = inner.height >= 4;
    let mut constraints = Vec::new();
    if show_filter {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(1));
    if show_hint {
        constraints.push(Constraint::Length(1));
    }
    let split = Layout::new(Direction::Vertical, constraints).split(inner);
    let mut idx = 0;
    if show_filter {
        let filter_text = if picker.filter.is_empty() {
            "Filter: (type region or npub/hex for custom)".to_string()
        } else {
            format!("Filter: {}", picker.filter)
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                filter_text,
                Style::default().fg(PRIMARY_COLOR),
            )),
            split[idx],
        );
        idx += 1;
    }
    let list_area = split[idx];

    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  no match — type a full npub/hex for custom",
                Style::default().fg(Color::DarkGray),
            )),
            list_area,
        );
    } else {
        let selected = picker.selected.min(rows.len() - 1);
        let current = current_hex.trim().to_ascii_lowercase();
        let items: Vec<ListItem> = rows
            .iter()
            .map(|row| {
                let (label, marker) = match row {
                    MostroInstancePickerRow::Trusted(n) => {
                        let active = n.pubkey.eq_ignore_ascii_case(&current);
                        let mark = if active { "★ " } else { "  " };
                        (format!("{mark}{}", row_label(n)), active)
                    }
                    MostroInstancePickerRow::Custom(raw) => {
                        (format!("  + use custom: {raw}"), false)
                    }
                };
                let style = if marker {
                    Style::default().fg(PRIMARY_COLOR)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect();

        let list = List::new(items)
            .style(Style::default().fg(Color::White).bg(BACKGROUND_COLOR))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› ");
        let mut state = ListState::default().with_selected(Some(selected));
        f.render_stateful_widget(list, list_area, &mut state);

        if rows.len() > list_area.height as usize {
            let mut sb_state = ScrollbarState::new(rows.len()).position(selected);
            f.render_stateful_widget(
                Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
                list_area,
                &mut sb_state,
            );
        }
    }

    if show_hint {
        let hint = if let Some(region) = region_for_pubkey(current_hex) {
            format!("Current: {region}  |  ↑↓  Enter select  Esc  type to filter")
        } else if current_hex.trim().is_empty() {
            "↑↓ navigate  Enter select  type filter or custom npub/hex  Esc".to_string()
        } else {
            "Current: custom  |  ↑↓  Enter select  Esc  type to filter".to_string()
        };
        f.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
            split[split.len() - 1],
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
    fn picker_renders_trusted_regions() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let picker = MostroInstancePicker::default();
        terminal
            .draw(|f| {
                render_mostro_instance_picker(
                    f,
                    &picker,
                    "82fa8cb978b43c79b2156585bac2c011176a21d2aead6d9f7c575c005be88390",
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Select Mostro Instance"));
        assert!(buffer_contains(buf, "Default"));
        assert!(buffer_contains(buf, "Cuba"));
        assert!(buffer_contains(buf, "★"));
    }

    #[test]
    fn picker_shows_custom_row_for_unmatched_filter() {
        let backend = TestBackend::new(80, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let picker = MostroInstancePicker {
            filter: "npub1customonly".to_string(),
            selected: 0,
        };
        terminal
            .draw(|f| render_mostro_instance_picker(f, &picker, ""))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "use custom"));
    }
}
