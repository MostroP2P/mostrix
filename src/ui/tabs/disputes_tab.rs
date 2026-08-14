use std::str::FromStr;
use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Table, TableState,
};

use crate::ui::helpers::format_local_timestamp;
use crate::ui::{BACKGROUND_COLOR, PRIMARY_COLOR};

/// Render the disputes tab showing a table of active disputes
/// This tab is only visible in admin mode
pub fn render_disputes_tab(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    disputes: &Arc<Mutex<Vec<Dispute>>>,
    selected_dispute_idx: usize,
) {
    let disputes_lock = match disputes.lock() {
        Ok(g) => g,
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned disputes lock: {e}). Please restart the app."
            ));
            let paragraph = Paragraph::new(Span::styled(
                "❌ Internal error. Please restart Mostrix.",
                Style::default().fg(Color::Red),
            ))
            .block(
                Block::default()
                    .title("Disputes Pending")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(PRIMARY_COLOR))
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            );
            f.render_widget(paragraph, area);
            return;
        }
    };

    // Filter to only show disputes with "initiated" status
    let initiated_disputes: Vec<&Dispute> = disputes_lock
        .iter()
        .filter(|dispute| {
            DisputeStatus::from_str(dispute.status.as_str())
                .map(|s| s == DisputeStatus::Initiated)
                .unwrap_or(false)
        })
        .collect();

    // Ensure selected index is within bounds of filtered list
    let valid_selected_idx = if initiated_disputes.is_empty() {
        0
    } else {
        selected_dispute_idx.min(initiated_disputes.len().saturating_sub(1))
    };

    if initiated_disputes.is_empty() {
        let paragraph = Paragraph::new(Span::styled(
            "📭 No disputes found",
            Style::default().fg(Color::Yellow),
        ))
        .block(
            Block::default()
                .title("Disputes Pending")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(PRIMARY_COLOR))
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );
        f.render_widget(paragraph, area);
    } else {
        // Compact layouts for small areas: drop the Created column when the
        // full 40/20/25 layout (87 cols with spacing) cannot fit inside the
        // borders, and drop the header when it would leave no room for data.
        let inner_width = area.width.saturating_sub(2);
        let show_created = inner_width >= 87;
        let show_header = area.height >= 4;

        let rows: Vec<Row> = initiated_disputes
            .iter()
            .map(|dispute| {
                let mut cells = vec![
                    Cell::from(dispute.id.to_string()),
                    Cell::from(dispute.status.clone()),
                ];
                if show_created {
                    cells.push(Cell::from(
                        format_local_timestamp(dispute.created_at, "%Y-%m-%d %H:%M")
                            .unwrap_or_else(|| "Invalid date".to_string()),
                    ));
                }
                Row::new(cells)
            })
            .collect();

        let constraints: Vec<Constraint> = if show_created {
            vec![
                Constraint::Length(40),
                Constraint::Length(20),
                Constraint::Length(25),
            ]
        } else {
            // Narrow: dispute id takes the remaining width, status stays visible
            vec![Constraint::Min(20), Constraint::Length(20)]
        };

        let mut table = Table::new(rows, constraints)
            .block(
                Block::default()
                    .title("Disputes Pending")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(PRIMARY_COLOR))
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            )
            .row_highlight_style(
                Style::default()
                    .bg(PRIMARY_COLOR)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            );

        if show_header {
            let mut header_cells = vec![
                Cell::from("🆔 Dispute ID").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("📊 Status").style(Style::default().add_modifier(Modifier::BOLD)),
            ];
            if show_created {
                header_cells.push(
                    Cell::from("📅 Created").style(Style::default().add_modifier(Modifier::BOLD)),
                );
            }
            table = table.header(Row::new(header_cells));
        }

        // Stateful render keeps the selected row in view when the table overflows
        // (same pattern as the Disputes In Progress sidebar).
        let mut table_state = TableState::default().with_selected(Some(valid_selected_idx));
        f.render_stateful_widget(table, area, &mut table_state);

        // Visible rows = area minus borders (2) and header (1 when shown)
        let header_rows = u16::from(show_header);
        let visible_rows = area.height.saturating_sub(2 + header_rows) as usize;
        if initiated_disputes.len() > visible_rows && visible_rows > 0 {
            // Track the real viewport: position comes from the offset the
            // stateful render just computed, and the scrollbar is confined to
            // the data rows so it does not overwrite the borders or header.
            let track = Rect {
                x: area.x,
                y: area.y + 1 + header_rows,
                width: area.width,
                height: visible_rows as u16,
            };
            let mut scrollbar_state = ScrollbarState::new(initiated_disputes.len())
                .viewport_content_length(visible_rows)
                .position(table_state.offset());
            f.render_stateful_widget(
                Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
                track,
                &mut scrollbar_state,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render_disputes_tab;
    use mostro_core::prelude::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

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

    fn initiated_dispute(nibble: u8) -> Dispute {
        let mut dispute = Dispute::new(Uuid::new_v4(), "active".to_string());
        // Deterministic, visually distinct id prefix per row (e.g. 00000000-, 11111111-, ...)
        dispute.id = Uuid::from_bytes([nibble * 0x11; 16]);
        dispute
    }

    /// When more pending disputes exist than table rows, selecting a late row
    /// must scroll the stateful table so that dispute stays visible (same
    /// behavior as the Disputes In Progress sidebar).
    #[test]
    fn table_scrolls_to_keep_selected_dispute_visible() {
        let disputes: Vec<Dispute> = (0..10).map(initiated_dispute).collect();
        let first_id = disputes[0].id.to_string();
        let last_id = disputes[9].id.to_string();
        let disputes = Arc::new(Mutex::new(disputes));

        // 8 high: 2 borders + 1 header leave 5 visible rows for 10 disputes.
        let backend = TestBackend::new(100, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, 9))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, &last_id[..8]),
            "selected late dispute must be visible after table scroll"
        );
        assert!(
            !buffer_contains(buf, &first_id[..8]),
            "first dispute should scroll off-screen when selecting the last"
        );
    }

    /// Narrow terminals drop the Created column so dispute id and status
    /// stay readable instead of being clipped by the fixed 40/20/25 layout.
    #[test]
    fn narrow_area_drops_created_column_but_keeps_id_and_status() {
        let disputes: Vec<Dispute> = (0..3).map(initiated_dispute).collect();
        let first_id = disputes[0].id.to_string();
        let disputes = Arc::new(Mutex::new(disputes));

        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, 0))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, &first_id[..8]),
            "dispute id must stay visible in narrow layout"
        );
        assert!(
            buffer_contains(buf, "initiated"),
            "status must stay visible in narrow layout"
        );
        assert!(
            !buffer_contains(buf, "Created"),
            "Created column should be dropped when the area is narrow"
        );
    }

    /// With no room below the header (height < 4) the header is dropped so at
    /// least one data row remains visible.
    #[test]
    fn short_area_drops_header_but_shows_selected_row() {
        let disputes: Vec<Dispute> = (0..3).map(initiated_dispute).collect();
        let second_id = disputes[1].id.to_string();
        let disputes = Arc::new(Mutex::new(disputes));

        let backend = TestBackend::new(100, 3);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, 1))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            !buffer_contains(buf, "Dispute ID"),
            "header should be dropped when the area is too short"
        );
        assert!(
            buffer_contains(buf, &second_id[..8]),
            "selected dispute row must be visible without the header"
        );
    }

    /// When the selection forces the table to scroll, the scrollbar must not
    /// overwrite the block borders or the header row.
    #[test]
    fn scrollbar_preserves_borders_and_header_when_scrolled() {
        let disputes: Vec<Dispute> = (0..10).map(initiated_dispute).collect();
        let disputes = Arc::new(Mutex::new(disputes));

        // Selected row 9 with 5 visible rows → table offset (5) != selected index (9)
        let backend = TestBackend::new(100, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, 9))
            .expect("draw");

        let buf = terminal.backend().buffer();
        let right = buf.area.width - 1;
        assert_eq!(buf[(right, 0)].symbol(), "╮", "top-right corner intact");
        assert_eq!(
            buf[(right, buf.area.height - 1)].symbol(),
            "╯",
            "bottom-right corner intact"
        );
        assert_eq!(
            buf[(right, 1)].symbol(),
            "│",
            "header row border must not be overwritten by the scrollbar"
        );
        assert!(
            buffer_contains(buf, "Dispute ID"),
            "header must still render while scrolled"
        );
    }

    #[test]
    fn table_shows_first_disputes_when_selection_is_at_top() {
        let disputes: Vec<Dispute> = (0..10).map(initiated_dispute).collect();
        let first_id = disputes[0].id.to_string();
        let last_id = disputes[9].id.to_string();
        let disputes = Arc::new(Mutex::new(disputes));

        let backend = TestBackend::new(100, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, 0))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, &first_id[..8]),
            "first dispute must stay visible when selected"
        );
        assert!(
            !buffer_contains(buf, &last_id[..8]),
            "last dispute should not appear while scrolled to the top"
        );
    }
}
