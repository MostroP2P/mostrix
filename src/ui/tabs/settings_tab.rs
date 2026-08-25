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
    RestoreSession,
    AddDisputeSolver,
    ChangeAdminKey,
    GenerateNewKeys,
}

type SettingsMenuRow = (SettingsMenuAction, &'static str);

/// Single source of truth for Admin Settings rows (action + list label).
///
/// Admin mode intentionally omits **Generate New Keys**: `AdminAddSolver` and
/// other operator actions must be signed with the Mostro daemon nsec, which is
/// set via **Change Admin Key** — generating a fresh keypair would overwrite
/// `admin_privkey` with a key the daemon rejects.
#[allow(clippy::redundant_static_lifetimes)]
const ADMIN_SETTINGS: [SettingsMenuRow; 8] = [
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
];

/// Single source of truth for User Settings rows (action + list label).
#[allow(clippy::redundant_static_lifetimes)]
const USER_SETTINGS: [SettingsMenuRow; 10] = [
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
    (SettingsMenuAction::RestoreSession, "Restore Session"),
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

/// Package version from `Cargo.toml` (`CARGO_PKG_VERSION`).
pub const MOSTRIX_VERSION: &str = env!("CARGO_PKG_VERSION");

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
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    // On short terminals, keep mode + list + nav; drop the version row first.
    let show_version = inner_area.height >= 8;
    let chunks = if show_version {
        Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(1), // spacer
                Constraint::Length(1), // mode
                Constraint::Length(1), // version
                Constraint::Length(1), // spacer
                Constraint::Min(0),    // list area
                Constraint::Length(1), // footer
            ],
        )
        .split(inner_area)
    } else {
        Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(1), // spacer
                Constraint::Length(1), // mode
                Constraint::Length(1), // spacer
                Constraint::Min(0),    // list area
                Constraint::Length(1), // footer
            ],
        )
        .split(inner_area)
    };

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

    let (list_chunk, footer_chunk) = if show_version {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "Mostrix ",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    format!("v{MOSTRIX_VERSION}"),
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[2],
        );
        (chunks[4], chunks[5])
    } else {
        (chunks[3], chunks[4])
    };

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
        let centered = Layout::new(
            Direction::Horizontal,
            [
                Constraint::Fill(1),
                Constraint::Length(50),
                Constraint::Fill(1),
            ],
        )
        .flex(Flex::Center)
        .split(list_chunk);
        centered[1]
    } else {
        // use full width on narrow terminals
        list_chunk
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
        footer_chunk,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

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

    #[test]
    fn admin_settings_omit_generate_new_keys() {
        assert_eq!(ADMIN_SETTINGS_OPTIONS_COUNT, 8);
        assert!(ADMIN_SETTINGS
            .iter()
            .all(|(action, _)| *action != SettingsMenuAction::GenerateNewKeys));
        assert!(matches!(
            settings_action_for_index(UserRole::Admin, 7),
            Some(SettingsMenuAction::ChangeAdminKey)
        ));
        assert!(settings_action_for_index(UserRole::Admin, 8).is_none());
    }

    #[test]
    fn user_settings_keep_generate_new_keys() {
        assert!(USER_SETTINGS
            .iter()
            .any(|(action, _)| *action == SettingsMenuAction::GenerateNewKeys));
        assert_eq!(
            settings_action_for_index(UserRole::User, USER_SETTINGS_OPTIONS_COUNT - 1),
            Some(SettingsMenuAction::GenerateNewKeys)
        );
    }

    #[test]
    fn render_shows_mostrix_version_on_tall_terminal() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_settings_tab(f, f.area(), UserRole::User, 0);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Mostrix"));
        assert!(buffer_contains(buf, &format!("v{MOSTRIX_VERSION}")));
        assert!(buffer_contains(buf, "Current Mode"));
        assert!(buffer_contains(buf, "navigate"));
    }

    #[test]
    fn render_hides_version_on_short_terminal() {
        let backend = TestBackend::new(80, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_settings_tab(f, f.area(), UserRole::User, 0);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // Outer frame is 6 rows; after borders inner height is 4 (< 8), so version is omitted.
        assert!(!buffer_contains(buf, "Mostrix"));
        assert!(buffer_contains(buf, "Current Mode"));
    }

    #[test]
    fn restore_session_is_a_user_option_but_not_an_admin_one() {
        assert!(USER_SETTINGS
            .iter()
            .any(|(a, _)| *a == SettingsMenuAction::RestoreSession));
        // Admin mode signs with admin_privkey, not the identity mnemonic Mostro
        // indexes users by, so a restore there would recover nothing.
        assert!(!ADMIN_SETTINGS
            .iter()
            .any(|(a, _)| *a == SettingsMenuAction::RestoreSession));
    }

    #[test]
    fn user_menu_keeps_restore_next_to_the_key_management_rows() {
        let labels: Vec<&str> = USER_SETTINGS.iter().map(|(_, l)| *l).collect();
        let seed = labels.iter().position(|l| *l == "View Seed Words").unwrap();
        let restore = labels.iter().position(|l| *l == "Restore Session").unwrap();
        let generate = labels
            .iter()
            .position(|l| *l == "Generate New Keys")
            .unwrap();
        assert!(seed < restore && restore < generate);
    }
}
