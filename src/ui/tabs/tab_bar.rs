use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Tabs};

use crate::ui::tabs::handoff_banner::{handoff_badge, BADGE_STYLE};
use crate::ui::{AdminTab, Tab, UserRole, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Renders the tab selector. `handoffs` is the number of disputes Serbero
/// handed to a person (0 outside admin mode).
pub fn render_tabs(
    f: &mut ratatui::Frame,
    area: Rect,
    active_tab: Tab,
    role: UserRole,
    handoffs: usize,
) {
    let tabs = Tabs::new(tab_titles(role, handoffs))
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

/// Tab labels for `role`. Disputes Pending is the first tab, so its handoff
/// badge (` 🙋 2`) stays visible when narrow terminals clip the last tabs.
pub fn tab_titles(role: UserRole, handoffs: usize) -> Vec<Line<'static>> {
    let pending = Tab::Admin(AdminTab::DisputesPending);
    let badge = handoff_badge(handoffs);
    Tab::get_titles(role)
        .into_iter()
        .enumerate()
        .map(|(index, title)| match &badge {
            Some(badge) if Tab::from_index(index, role) == pending => Line::from(vec![
                Span::raw(title),
                Span::styled(badge.clone(), BADGE_STYLE),
            ]),
            _ => Line::from(title),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn labels(role: UserRole, handoffs: usize) -> Vec<String> {
        tab_titles(role, handoffs)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn disputes_pending_counts_handoffs_and_other_labels_stay() {
        let labels = labels(UserRole::Admin, 2);

        assert_eq!(labels[0], "Disputes Pending 🙋 2");
        assert_eq!(&labels[1..], &Tab::get_titles(UserRole::Admin)[1..]);
    }

    #[test]
    fn no_badge_without_handoffs_or_outside_admin_mode() {
        assert_eq!(labels(UserRole::Admin, 0), Tab::get_titles(UserRole::Admin));
        assert_eq!(labels(UserRole::User, 3), Tab::get_titles(UserRole::User));
    }

    /// The badge keeps the "needs a person" yellow while another tab is active.
    #[test]
    fn the_badge_is_yellow_from_another_tab() {
        let mut terminal = Terminal::new(TestBackend::new(120, 3)).unwrap();

        terminal
            .draw(|f| {
                render_tabs(
                    f,
                    f.area(),
                    Tab::Admin(AdminTab::Observer),
                    UserRole::Admin,
                    1,
                )
            })
            .unwrap();

        let buf = terminal.backend().buffer();
        let badge = (0..buf.area.width)
            .find(|&x| buf[(x, 1)].symbol() == "🙋")
            .expect("badge rendered");
        assert_eq!(buf[(badge, 1)].fg, Color::Yellow);
        assert!(buf[(badge, 1)].modifier.contains(Modifier::BOLD));
    }
}
