use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Tabs};

use crate::ui::{Tab, UserRole, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_tabs(f: &mut ratatui::Frame, area: Rect, active_tab: Tab, role: UserRole) {
    let titles = Tab::get_titles(role);
    let tab_titles: Vec<Line> = titles.iter().map(|t| Line::from(t.as_str())).collect();

    let tabs = Tabs::new(tab_titles)
        .select(active_tab.as_index())
        .block(
            // Keep the top tab selector a plain white, square frame so it stays
            // visually distinct from the green rounded content frames below.
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::White)),
        )
        .highlight_style(
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, area);
}
