use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::ui::{UserRole, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Number of settings options for Admin role
pub const ADMIN_SETTINGS_OPTIONS_COUNT: usize = 6; // Change Mostro Pubkey, Add Nostr Relay, Add Currency Filter, Clear Currency Filters, Add Dispute Solver, Change Admin Key

/// Number of settings options for User role
pub const USER_SETTINGS_OPTIONS_COUNT: usize = 4; // Change Mostro Pubkey, Add Nostr Relay, Add Currency Filter, Clear Currency Filters

/// Render the Settings tab UI
///
/// Displays settings options based on user role (User or Admin).
/// The options list is centered when terminal width allows, otherwise uses full width
/// to prevent text clipping on narrow terminals.
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
            Constraint::Min(0),    // list area
            Constraint::Length(1), // footer
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
            "Add Currency Filter",
            "Clear Currency Filters",
            "Add Dispute Solver",
            "Change Admin Key",
        ]
    } else {
        vec![
            "Change Mostro Pubkey",
            "Add Nostr Relay",
            "Add Currency Filter",
            "Clear Currency Filters",
        ]
    };

    let list_items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, option)| {
            let style = if i == selected_option {
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(*option, style)))
        })
        .collect();

    // Determine the list width based on terminal width to keep it readable
    let list_width = if inner_area.width > 60 {
        // center the list for wide terminals
        let chunks = Layout::new(
            Direction::Horizontal,
            [
                Constraint::Fill(1),
                Constraint::Length(50),
                Constraint::Fill(1),
            ],
        )
        .flex(Flex::Center)
        .split(chunks[3]);
        chunks[1]
    } else {
        // use full width on narrow terminals
        chunks[3]
    };

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::NONE)
                .style(Style::default().bg(BACKGROUND_COLOR)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(list, list_width);

    // Footer hint
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Use ", Style::default().fg(Color::White)),
            Span::styled("↑/↓", Style::default().fg(PRIMARY_COLOR)),
            Span::styled(" to navigate, ", Style::default().fg(Color::White)),
            Span::styled("Enter", Style::default().fg(PRIMARY_COLOR)),
            Span::styled(" to select", Style::default().fg(Color::White)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );
}
