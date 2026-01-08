use std::sync::{Arc, Mutex};

use chrono::DateTime;
use mostro_core::prelude::*;
use ratatui::layout::Constraint;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use super::{apply_status_color, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Render the disputes tab showing a table of active disputes
/// This tab is only visible in admin mode
pub fn render_disputes_tab(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    disputes: &Arc<Mutex<Vec<Dispute>>>,
    selected_dispute_idx: usize,
) {
    let disputes_lock = disputes.lock().unwrap();

    if disputes_lock.is_empty() {
        let paragraph = Paragraph::new(Span::styled(
            "📭 No disputes found",
            Style::default().fg(Color::Yellow),
        ))
        .block(
            Block::default()
                .title("Disputes")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );
        f.render_widget(paragraph, area);
    } else {
        let header_cells = vec![
            Cell::from("🆔 Dispute ID").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("📊 Status").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("📅 Created").style(Style::default().add_modifier(Modifier::BOLD)),
        ];
        let header = Row::new(header_cells);

        let rows: Vec<Row> = disputes_lock
            .iter()
            .enumerate()
            .map(|(i, dispute)| {
                let id_cell = Cell::from(dispute.id.to_string());

                let status_str = dispute.status.clone();
                let status_cell =
                    Cell::from(status_str.clone()).style(apply_status_color(&status_str));

                let date = DateTime::from_timestamp(dispute.created_at, 0);
                let date_cell = Cell::from(
                    date.map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "Invalid date".to_string()),
                );

                let row = Row::new(vec![id_cell, status_cell, date_cell]);

                if i == selected_dispute_idx {
                    // Highlight the selected row
                    row.style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
                } else {
                    row
                }
            })
            .collect();

        let table = Table::new(
            rows,
            &[
                Constraint::Max(40), // Dispute ID
                Constraint::Max(15), // Status
                Constraint::Max(18), // Created
            ],
        )
        .header(header)
        .block(
            Block::default()
                .title("Disputes")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );
        f.render_widget(table, area);
    }
}
