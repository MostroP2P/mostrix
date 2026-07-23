use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph};

use crate::ui::{UserRole, BACKGROUND_COLOR, PRIMARY_COLOR};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMenuAction {
    SwitchMode,
    ChangeMostroPubkey,
    AddRelay,
    SetBuyerLnAddress,
    ClearBuyerLnAddress,
    AddCurrencyFilter,
    ClearCurrencyFilters,
    ViewSeedWords,
    AddDisputeSolver,
    ChangeAdminKey,
    GenerateNewKeys,
}

type SettingsMenuRow = (SettingsMenuAction, &'static str);

/// Single source of truth for Admin Settings rows (action + list label).
#[allow(clippy::redundant_static_lifetimes)]
const ADMIN_SETTINGS: [SettingsMenuRow; 9] = [
    (SettingsMenuAction::SwitchMode, "Switch Mode (User ↔ Admin)"),
    (
        SettingsMenuAction::ChangeMostroPubkey,
        "Change Mostro Pubkey",
    ),
    (SettingsMenuAction::AddRelay, "Add Nostr Relay"),
    (SettingsMenuAction::AddCurrencyFilter, "Add Currency Filter"),
    (
        SettingsMenuAction::ClearCurrencyFilters,
        "Clear Currency Filters",
    ),
    (SettingsMenuAction::ViewSeedWords, "View Seed Words"),
    (SettingsMenuAction::AddDisputeSolver, "Add Dispute Solver"),
    (SettingsMenuAction::ChangeAdminKey, "Change Admin Key"),
    (SettingsMenuAction::GenerateNewKeys, "Generate New Keys"),
];

/// Single source of truth for User Settings rows (action + list label).
#[allow(clippy::redundant_static_lifetimes)]
const USER_SETTINGS: [SettingsMenuRow; 9] = [
    (SettingsMenuAction::SwitchMode, "Switch Mode (User ↔ Admin)"),
    (
        SettingsMenuAction::ChangeMostroPubkey,
        "Change Mostro Pubkey",
    ),
    (SettingsMenuAction::AddRelay, "Add Nostr Relay"),
    (
        SettingsMenuAction::SetBuyerLnAddress,
        "Set Lightning Address (buyer)",
    ),
    (
        SettingsMenuAction::ClearBuyerLnAddress,
        "Clear Lightning Address",
    ),
    (SettingsMenuAction::AddCurrencyFilter, "Add Currency Filter"),
    (
        SettingsMenuAction::ClearCurrencyFilters,
        "Clear Currency Filters",
    ),
    (SettingsMenuAction::ViewSeedWords, "View Seed Words"),
    (SettingsMenuAction::GenerateNewKeys, "Generate New Keys"),
];

pub const ADMIN_SETTINGS_OPTIONS_COUNT: usize = ADMIN_SETTINGS.len();

pub const USER_SETTINGS_OPTIONS_COUNT: usize = USER_SETTINGS.len();

fn settings_rows(role: UserRole) -> &'static [SettingsMenuRow] {
    match role {
        UserRole::Admin => &ADMIN_SETTINGS,
        UserRole::User => &USER_SETTINGS,
    }
}

pub fn settings_action_for_index(user_role: UserRole, idx: usize) -> Option<SettingsMenuAction> {
    settings_rows(user_role).get(idx).map(|(action, _)| *action)
}

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
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));

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

    let rows = settings_rows(user_role);
    let list_items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            let style = if i == selected_option {
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(*label, style)))
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
            Span::styled("↑/↓", Style::default().fg(PRIMARY_COLOR)),
            Span::styled(" navigate · ", Style::default().fg(Color::White)),
            Span::styled("Enter", Style::default().fg(PRIMARY_COLOR)),
            Span::styled(" select · ", Style::default().fg(Color::White)),
            Span::styled("Shift+H", Style::default().fg(PRIMARY_COLOR)),
            Span::styled(" all options", Style::default().fg(Color::White)),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );
}
