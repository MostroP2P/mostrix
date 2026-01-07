use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::{UserRole, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_settings_tab(
    f: &mut ratatui::Frame,
    area: Rect,
    user_role: UserRole,
    selected_option: usize,
) {
    let block = Block::default()
        .title("⚙️  Settings")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(3), // mode section
            Constraint::Length(1), // spacer
            Constraint::Min(0),    // rest
        ],
    )
    .split(inner_area);

    // Current mode display
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Current Mode: ", Style::default()),
            Span::styled(
                match user_role {
                    UserRole::User => "User",
                    UserRole::Admin => "Admin",
                },
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

    // Options based on user role
    let options = if user_role == UserRole::Admin {
        vec![
            "Change Mostro Pubkey",
            "Add Nostr Relay",
            "Add Dispute Solver",
            "Change Admin Key",
        ]
    } else {
        vec!["Change Mostro Pubkey", "Add Nostr Relay"]
    };

    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(idx, opt)| {
            let style = if idx == selected_option {
                Style::default()
                    .bg(PRIMARY_COLOR)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![Span::styled(*opt, style)]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
        .highlight_symbol(">> ");

    f.render_stateful_widget(
        list,
        chunks[3],
        &mut ratatui::widgets::ListState::default().with_selected(Some(selected_option)),
    );

    // Instructions footer
    if user_role == UserRole::User {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Press ", Style::default()),
                Span::styled(
                    "M",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to switch to Admin mode", Style::default()),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[3], // Re-using chunks[3] might overlap, but list is Min(0)
        );
    }
}
