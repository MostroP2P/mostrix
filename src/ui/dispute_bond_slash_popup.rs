use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::util::order_utils::BondSlashChoice;

/// Centered bond-slash submenu overlay on top of the finalize popup.
pub fn render_bond_slash_overlay(
    f: &mut ratatui::Frame,
    parent_area: Rect,
    selected_choice_index: usize,
) {
    let popup_width = 52.min(parent_area.width.saturating_sub(4));
    let popup_height = 12.min(parent_area.height.saturating_sub(2));
    let popup = helpers::create_centered_popup(parent_area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("⚔️ Bond resolution")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ],
    )
    .split(inner);

    f.render_widget(
        Paragraph::new("Choose how to resolve anti-abuse bonds:")
            .alignment(ratatui::layout::Alignment::Center),
        chunks[0],
    );

    let mut choice_lines: Vec<Line> = Vec::with_capacity(BondSlashChoice::ALL.len());
    for (i, choice) in BondSlashChoice::ALL.iter().enumerate() {
        let selected = i == selected_choice_index;
        let style = if selected {
            Style::default()
                .bg(PRIMARY_COLOR)
                .fg(BACKGROUND_COLOR)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if selected { "▶ " } else { "  " };
        choice_lines.push(Line::from(vec![Span::styled(
            format!("{}{}", prefix, choice.label()),
            style,
        )]));
    }
    f.render_widget(
        Paragraph::new(choice_lines).alignment(ratatui::layout::Alignment::Center),
        chunks[3],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "↑↓",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" select  "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" apply  "),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" back"),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );
}
