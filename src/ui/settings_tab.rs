use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::{UserRole, BACKGROUND_COLOR, PRIMARY_COLOR};

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

    // Determine list area: center when there's enough width, otherwise use full width
    let list_width = 30u16; // Desired width for the options list
    let list_area = if chunks[3].width <= list_width {
        // Terminal is narrow: use the full available width to avoid clipping text
        chunks[3]
    } else {
        // Terminal is wide enough: center the list horizontally
        let [centered_area] = Layout::horizontal([Constraint::Length(list_width)])
            .flex(Flex::Center)
            .areas(chunks[3]);
        centered_area
    };

    f.render_stateful_widget(
        list,
        list_area,
        &mut ratatui::widgets::ListState::default().with_selected(Some(selected_option)),
    );

    // Instructions footer
    let footer_text = if user_role == UserRole::User {
        vec![
            Span::styled("Press ", Style::default()),
            Span::styled(
                "M",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to switch to Admin mode", Style::default()),
        ]
    } else {
        vec![
            Span::styled("Press ", Style::default()),
            Span::styled(
                "M",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to switch to User mode", Style::default()),
        ]
    };

    f.render_widget(
        Paragraph::new(Line::from(footer_text)).alignment(ratatui::layout::Alignment::Center),
        chunks[4], // Footer area
    );
}
