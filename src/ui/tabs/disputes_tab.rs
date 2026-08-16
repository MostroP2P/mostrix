use std::sync::{Arc, Mutex};

use ratatui::layout::Constraint;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};

use crate::ui::helpers::{
    format_local_timestamp, get_initiated_disputes, render_table_list_scrollbar,
    selected_pending_display_idx,
};
use crate::ui::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::util::order_utils::DisputeRevision;

/// Render the Disputes Pending table (admin mode only).
///
/// Uses a persistent [`TableState`] (`app.disputes_table_state`) so ↑↓ keeps the
/// selected row in view without resetting the viewport each frame. Selection is
/// resolved by dispute UUID against the initiated-status projection
/// (`helpers/dispute_selection.rs`). Scrollbar via [`render_table_list_scrollbar`].
pub fn render_disputes_tab(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    disputes: &Arc<Mutex<Vec<DisputeRevision>>>,
    app: &mut AppState,
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

    let initiated = get_initiated_disputes(&disputes_lock);
    let valid_selected_idx =
        selected_pending_display_idx(app.selected_pending_dispute_id, &initiated).unwrap_or(0);

    if initiated.is_empty() {
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
        return;
    }

    // Compact layouts for small areas:
    // - full 40/20/25 when inner width ≥ 87
    // - id + status columns when 43 ≤ inner < 87
    // - single combined cell (short id + status) when inner < 43
    // Drop the header when height < 4 so a data row remains.
    let inner_width = area.width.saturating_sub(2);
    let show_created = inner_width >= 87;
    let ultra_compact = inner_width < 43;
    let show_header = area.height >= 4;

    let rows: Vec<Row> = initiated
        .iter()
        .map(|(_orig, dispute)| {
            if ultra_compact {
                // One cell: shortened UUID prefix + status (fits ~40-col bodies).
                let id = dispute.id.to_string();
                let short_id: String = id.chars().take(8).collect();
                Row::new(vec![Cell::from(format!("{short_id} {}", dispute.status))])
            } else {
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
            }
        })
        .collect();

    let constraints: Vec<Constraint> = if show_created {
        vec![
            Constraint::Length(40),
            Constraint::Length(20),
            Constraint::Length(25),
        ]
    } else if ultra_compact {
        vec![Constraint::Min(1)]
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
        let header_cells = if ultra_compact {
            vec![Cell::from("🆔 Dispute").style(Style::default().add_modifier(Modifier::BOLD))]
        } else {
            let mut cells = vec![
                Cell::from("🆔 Dispute ID").style(Style::default().add_modifier(Modifier::BOLD)),
                Cell::from("📊 Status").style(Style::default().add_modifier(Modifier::BOLD)),
            ];
            if show_created {
                cells.push(
                    Cell::from("📅 Created").style(Style::default().add_modifier(Modifier::BOLD)),
                );
            }
            cells
        };
        table = table.header(Row::new(header_cells));
    }

    // Persistent TableState keeps ↑ scroll smooth (same as Orders tab).
    app.disputes_table_state.select(Some(valid_selected_idx));
    f.render_stateful_widget(table, area, &mut app.disputes_table_state);

    let header_rows = u16::from(show_header);
    let visible_rows = area.height.saturating_sub(2 + header_rows) as usize;
    render_table_list_scrollbar(
        f,
        area,
        initiated.len(),
        visible_rows,
        header_rows,
        app.disputes_table_state.offset(),
    );
}

#[cfg(test)]
mod tests {
    use super::render_disputes_tab;
    use crate::ui::{AppState, UserRole};
    use crate::util::order_utils::DisputeRevision;
    use mostro_core::prelude::Dispute;
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

    fn initiated_dispute(nibble: u8) -> DisputeRevision {
        let mut dispute = Dispute::new(Uuid::new_v4(), "active".to_string());
        // Deterministic, visually distinct id prefix per row (e.g. 00000000-, 11111111-, ...)
        dispute.id = Uuid::from_bytes([nibble * 0x11; 16]);
        DisputeRevision::new(dispute, i64::from(nibble))
    }

    /// When more pending disputes exist than table rows, selecting a late row
    /// must scroll the stateful table so that dispute stays visible.
    #[test]
    fn table_scrolls_to_keep_selected_dispute_visible() {
        let disputes: Vec<DisputeRevision> = (0..10).map(initiated_dispute).collect();
        let first_id = disputes[0].dispute.id.to_string();
        let last_id = disputes[9].dispute.id.to_string();
        let last_uuid = disputes[9].dispute.id;
        let disputes = Arc::new(Mutex::new(disputes));
        let mut app = AppState::new(UserRole::Admin);
        app.selected_pending_dispute_id = Some(last_uuid);

        // 8 high: 2 borders + 1 header leave 5 visible rows for 10 disputes.
        let backend = TestBackend::new(100, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, &mut app))
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
        let disputes: Vec<DisputeRevision> = (0..3).map(initiated_dispute).collect();
        let first_id = disputes[0].dispute.id.to_string();
        let first_uuid = disputes[0].dispute.id;
        let disputes = Arc::new(Mutex::new(disputes));
        let mut app = AppState::new(UserRole::Admin);
        app.selected_pending_dispute_id = Some(first_uuid);

        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, &mut app))
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

    /// Below 43 cols (inner width < 43) the two-column Min(20)+Length(20) layout
    /// cannot fit; a single cell with shortened id + status keeps both visible.
    #[test]
    fn ultra_narrow_area_uses_single_column_short_id_and_status() {
        let disputes: Vec<DisputeRevision> = (0..3).map(initiated_dispute).collect();
        let first_id = disputes[0].dispute.id.to_string();
        let short_id = &first_id[..8];
        let first_uuid = disputes[0].dispute.id;
        let disputes = Arc::new(Mutex::new(disputes));
        let mut app = AppState::new(UserRole::Admin);
        app.selected_pending_dispute_id = Some(first_uuid);

        // area width 42 → inner 40 < 43 → ultra-compact single column
        let backend = TestBackend::new(42, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, short_id),
            "shortened dispute id must stay visible under 43 cols"
        );
        assert!(
            buffer_contains(buf, "initiated"),
            "status must stay visible under 43 cols"
        );
        assert!(
            !buffer_contains(buf, "Created"),
            "Created column must not appear in ultra-compact layout"
        );
        assert!(
            !buffer_contains(buf, "Dispute ID"),
            "full Dispute ID header should yield to compact Dispute header"
        );
    }

    /// With no room below the header (height < 4) the header is dropped so at
    /// least one data row remains visible.
    #[test]
    fn short_area_drops_header_but_shows_selected_row() {
        let disputes: Vec<DisputeRevision> = (0..3).map(initiated_dispute).collect();
        let second_id = disputes[1].dispute.id.to_string();
        let second_uuid = disputes[1].dispute.id;
        let disputes = Arc::new(Mutex::new(disputes));
        let mut app = AppState::new(UserRole::Admin);
        app.selected_pending_dispute_id = Some(second_uuid);

        let backend = TestBackend::new(100, 3);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, &mut app))
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
        let disputes: Vec<DisputeRevision> = (0..10).map(initiated_dispute).collect();
        let last_uuid = disputes[9].dispute.id;
        let disputes = Arc::new(Mutex::new(disputes));
        let mut app = AppState::new(UserRole::Admin);
        app.selected_pending_dispute_id = Some(last_uuid);

        // Selected row 9 with 5 visible rows → table offset (5) != selected index (9)
        let backend = TestBackend::new(100, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, &mut app))
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

    /// Selecting the last pending dispute must park the scrollbar thumb against
    /// the end cap (`▼`) — same offset remapping as Orders.
    #[test]
    fn scrollbar_thumb_reaches_track_bottom_on_last_row() {
        let disputes: Vec<DisputeRevision> = (0..10).map(initiated_dispute).collect();
        let last_uuid = disputes[9].dispute.id;
        let disputes = Arc::new(Mutex::new(disputes));
        let mut app = AppState::new(UserRole::Admin);
        app.selected_pending_dispute_id = Some(last_uuid);

        // height 8 → borders+header leave 5 data rows; track y=2..6 with ▲…▼
        let backend = TestBackend::new(100, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        let right = buf.area.width - 1;
        let end_cap_y = buf.area.height - 2;
        let above_end = end_cap_y - 1;
        assert_eq!(
            buf[(right, end_cap_y)].symbol(),
            "▼",
            "scrollbar end cap must sit on the last track row"
        );
        assert_eq!(
            buf[(right, above_end)].symbol(),
            "█",
            "thumb must reach the cell above ▼ when the last dispute is selected"
        );
    }

    #[test]
    fn table_shows_first_disputes_when_selection_is_at_top() {
        let disputes: Vec<DisputeRevision> = (0..10).map(initiated_dispute).collect();
        let first_id = disputes[0].dispute.id.to_string();
        let last_id = disputes[9].dispute.id.to_string();
        let first_uuid = disputes[0].dispute.id;
        let disputes = Arc::new(Mutex::new(disputes));
        let mut app = AppState::new(UserRole::Admin);
        app.selected_pending_dispute_id = Some(first_uuid);

        let backend = TestBackend::new(100, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, &mut app))
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

    /// Persisted TableState keeps the viewport offset when moving selection up
    /// one row after scrolling to the bottom (Orders-tab alignment).
    #[test]
    fn persisted_table_state_keeps_viewport_when_moving_up() {
        let disputes: Vec<DisputeRevision> = (0..10).map(initiated_dispute).collect();
        let last_uuid = disputes[9].dispute.id;
        let eighth_uuid = disputes[8].dispute.id;
        let first_id = disputes[0].dispute.id.to_string();
        let disputes = Arc::new(Mutex::new(disputes));
        let mut app = AppState::new(UserRole::Admin);
        app.selected_pending_dispute_id = Some(last_uuid);

        let backend = TestBackend::new(100, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, &mut app))
            .expect("draw bottom");

        let offset_at_bottom = app.disputes_table_state.offset();
        assert!(offset_at_bottom > 0, "must have scrolled for last row");

        app.selected_pending_dispute_id = Some(eighth_uuid);
        terminal
            .draw(|f| render_disputes_tab(f, f.area(), &disputes, &mut app))
            .expect("draw up one");

        assert_eq!(
            app.disputes_table_state.offset(),
            offset_at_bottom,
            "moving up one row should not reset viewport to top"
        );
        let buf = terminal.backend().buffer();
        assert!(
            !buffer_contains(buf, &first_id[..8]),
            "first row must stay scrolled off after ↑ from bottom"
        );
    }
}
