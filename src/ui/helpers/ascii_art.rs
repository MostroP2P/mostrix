use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Empty-mailbox art for the Messages tab empty state. All lines share the same
/// visual width so [`render_centered_lines`] keeps the shape aligned when it
/// centers each row independently.
pub const MAILBOX_EMPTY_ART: &[&str] = &[
    "   ╭──────────────╮   ",
    "   │  ╲        ╱  │   ",
    "   │   (empty)    │   ",
    "   │              │   ",
    "   ╰──────┬┬──────╯   ",
    "          ││          ",
    "          ││          ",
];

/// Renders each line centered horizontally within `area`, one row per line.
pub fn render_centered_lines<F>(f: &mut ratatui::Frame, area: Rect, lines: &[&str], style_line: F)
where
    F: Fn(&str) -> Vec<Span<'static>>,
{
    if lines.is_empty() {
        return;
    }

    let logo_height = lines.len() as u16;
    let start_y = if logo_height < area.height {
        area.y + (area.height.saturating_sub(logo_height)) / 2
    } else {
        area.y
    };

    for (idx, line) in lines.iter().enumerate() {
        let y = start_y + idx as u16;
        if y >= area.y.saturating_add(area.height) {
            break;
        }

        let line_width = line.chars().count() as u16;
        let start_x = if line_width < area.width {
            area.x + (area.width.saturating_sub(line_width)) / 2
        } else {
            area.x
        };

        let centered_rect = Rect {
            x: start_x,
            y,
            width: line_width.min(area.width),
            height: 1,
        };

        f.render_widget(Paragraph::new(Line::from(style_line(line))), centered_rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_art_lines_share_width() {
        // Equal widths keep the shape aligned when each row is centered independently.
        let widths: Vec<usize> = MAILBOX_EMPTY_ART
            .iter()
            .map(|l| l.chars().count())
            .collect();
        assert!(!widths.is_empty());
        assert!(
            widths.iter().all(|&w| w == widths[0]),
            "mailbox art rows must share a width, got {widths:?}"
        );
    }
}
