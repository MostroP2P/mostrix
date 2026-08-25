use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::constants::*;
use super::{AppState, DisputeFilter, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::navigation::{AdminTab, Tab, UserRole, UserTab};

// 13 shortcuts, intro, close hint, borders, and one row of margin above and below.
const MY_TRADES_FULL_HELP_MIN_HEIGHT: u16 = 19;
const MY_TRADES_FULL_HELP_MIN_WIDTH: u16 = 60;

/// Renders the context-aware keyboard shortcuts popup (Ctrl+H, and Shift+H on My Trades).
pub fn render_help_popup(f: &mut ratatui::Frame, app: &AppState, tab: Tab) {
    let area = f.area();
    let (title, plain_lines) = help_content(app, tab);
    let narrow_my_trades =
        matches!(tab, Tab::User(UserTab::MyTrades)) && area.width < MY_TRADES_FULL_HELP_MIN_WIDTH;
    let compact_my_trades = matches!(tab, Tab::User(UserTab::MyTrades))
        && (area.height < MY_TRADES_FULL_HELP_MIN_HEIGHT || narrow_my_trades);

    // Match Settings Shift+H: compact rows, styled shortcut + description, full viewport height.
    let compact_chrome = matches!(
        tab,
        Tab::Admin(AdminTab::DisputesInProgress) | Tab::User(UserTab::MyTrades)
    );

    let (popup_width, popup_height) = if compact_chrome {
        (78u16.min(area.width), area.height.saturating_sub(2).max(6))
    } else {
        let line_count = plain_lines.len().max(1);
        (
            64u16,
            (line_count as u16 + 4).min(area.height.saturating_sub(2)),
        )
    };

    let popup = {
        let [p] = Layout::horizontal([Constraint::Length(popup_width)])
            .flex(Flex::Center)
            .areas(area);
        let [p] = Layout::vertical([Constraint::Length(popup_height)])
            .flex(Flex::Center)
            .areas(p);
        p
    };

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    if compact_chrome {
        let mut lines: Vec<Line<'static>> = Vec::new();
        if matches!(tab, Tab::Admin(AdminTab::DisputesInProgress)) {
            lines.push(help_disputes_in_progress_intro());
        } else if compact_my_trades {
            lines.extend(compact_my_trades_help(narrow_my_trades));
        } else {
            lines.push(help_my_trades_intro());
        }
        if !compact_my_trades {
            for s in plain_lines {
                lines.push(help_shortcut_line(&s));
            }
        }
        lines.push(Line::from(Span::styled(
            HELP_CLOSE_HINT,
            Style::default().fg(Color::DarkGray),
        )));
        let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true });
        f.render_widget(paragraph, inner);
    } else {
        let content: Vec<Line> = plain_lines
            .into_iter()
            .map(|s| Line::from(Span::styled(s, Style::default().fg(Color::White))))
            .collect();
        let mut all = content;
        all.push(Line::from(""));
        all.push(Line::from(Span::styled(
            HELP_CLOSE_HINT,
            Style::default().fg(Color::DarkGray),
        )));
        let paragraph = Paragraph::new(all).wrap(Wrap { trim: true });
        f.render_widget(paragraph, inner);
    }
}

/// Full reference for every Settings menu row (Shift+H on Settings).
pub fn render_settings_instructions_popup(f: &mut ratatui::Frame, user_role: UserRole) {
    let area = f.area();
    let (title, mut lines) = settings_instruction_lines(user_role);

    let intro = Line::from(vec![
        Span::styled(
            "Each row below matches one Settings list item. ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled("↑/↓", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" move · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" runs it.", Style::default().fg(Color::DarkGray)),
    ]);
    lines.insert(0, intro);

    lines.push(Line::from(Span::styled(
        SETTINGS_INSTRUCTIONS_CLOSE_HINT,
        Style::default().fg(Color::DarkGray),
    )));

    // Use (nearly) the full viewport height so wrapped text has room on short terminals. A naive
    // `line_count + borders` cap undersizes the block when there are few logical lines but long
    // wrapped rows, which made content clip on e.g. 24-row terminals.
    let popup_width = 78u16.min(area.width);
    let popup_height = area.height.saturating_sub(2).max(6);

    let popup = {
        let [p] = Layout::horizontal([Constraint::Length(popup_width)])
            .flex(Flex::Center)
            .areas(area);
        let [p] = Layout::vertical([Constraint::Length(popup_height)])
            .flex(Flex::Center)
            .areas(p);
        p
    };

    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let text = Text::from(lines);
    let paragraph = Paragraph::new(text).wrap(Wrap { trim: true });
    f.render_widget(paragraph, inner);
}

fn settings_instruction_block_style() -> (Style, Style) {
    let title = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);
    let body = Style::default().fg(Color::Gray);
    (title, body)
}

fn help_disputes_in_progress_intro() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "Sidebar: pick a dispute · ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled("↑/↓", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Tab", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" party · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Shift+C", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" filter.", Style::default().fg(Color::DarkGray)),
    ])
}

fn help_my_trades_intro() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "Sidebar: pick an order · ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled("Shift+I", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" chat · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Ctrl+H", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" / ", Style::default().fg(Color::DarkGray)),
        Span::styled("Shift+H", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" for this panel.", Style::default().fg(Color::DarkGray)),
    ])
}

fn compact_my_trades_help(narrow: bool) -> Vec<Line<'static>> {
    if narrow {
        let (title_style, _) = settings_instruction_block_style();
        return [
            "↑↓  Enter",
            "Tab  Shift+I",
            "Shift+C  Shift+F",
            "Shift+R  Shift+D",
        ]
        .into_iter()
        .map(|row| Line::from(Span::styled(row, title_style)))
        .collect();
    }

    [
        "↑↓ / Enter: Select order / send message",
        "Shift+I / Tab: Toggle input / Peer-Solver chat",
        "Shift+C / Shift+F: Cancel order / mark fiat sent",
        "Shift+R / Shift+D: Release sats / open dispute",
    ]
    .into_iter()
    .map(help_shortcut_line)
    .collect()
}

/// Split `Key: description` help strings into bold key + gray body (same as Settings Shift+H rows).
fn help_shortcut_line(s: &str) -> Line<'static> {
    let (title_style, body_style) = settings_instruction_block_style();
    match s.split_once(": ") {
        Some((key, rest)) => Line::from(vec![
            Span::styled(format!("▸ {key}: "), title_style),
            Span::styled(rest.to_string(), body_style),
        ]),
        None => Line::from(Span::styled(
            s.to_string(),
            Style::default().fg(Color::White),
        )),
    }
}

/// One menu option as a single wrapped line: bold title prefix + body (compact for small terminals).
fn push_settings_instruction_line(lines: &mut Vec<Line<'static>>, name: &str, description: &str) {
    let (title_style, body_style) = settings_instruction_block_style();
    lines.push(Line::from(vec![
        Span::styled(format!("▸ {name}: "), title_style),
        Span::styled(description.to_string(), body_style),
    ]));
}

fn settings_instruction_lines(user_role: UserRole) -> (String, Vec<Line<'static>>) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let title = match user_role {
        UserRole::Admin => "Settings (Admin) — All options",
        UserRole::User => "Settings (User) — All options",
    }
    .to_string();

    let admin_entries: &[(&str, &str)] = &[
        (
            "Switch Mode (User ↔ Admin)",
            "Toggle User vs Admin UI. Saves user_mode in settings.toml, reloads tabs, and may reload admin disputes.",
        ),
        (
            "Change Mostro Pubkey",
            "Set the Mostro daemon pubkey (npub or hex) used for subscriptions and orders.",
        ),
        (
            "Add Nostr Relay",
            "Append a wss:// relay; duplicates are skipped.",
        ),
        (
            "Add Currency Filter",
            "Add a fiat code (e.g. USD). The order book only shows matching orders.",
        ),
        (
            "Clear Currency Filters",
            "Remove all filters so every configured currency can appear again.",
        ),
        (
            "View Seed Words",
            "Show your BIP-39 mnemonic from the local database. Press C to copy. Treat as highly sensitive.",
        ),
        (
            "Add Dispute Solver",
            "Enter solver npub, use Left/Right to choose read or read-write, then confirm",
        ),
        (
            "Change Admin Key",
            "Set admin_privkey to the Mostro daemon nsec (operator actions + dispute chat).",
        ),
    ];

    let user_entries: &[(&str, &str)] = &[
        (
            "Switch Mode (User ↔ Admin)",
            "Switch to Admin when you need dispute tools. Saves user_mode and reloads tabs.",
        ),
        (
            "Change Mostro Pubkey",
            "Set the Mostro daemon pubkey (npub or hex) used for subscriptions and orders.",
        ),
        (
            "Add Nostr Relay",
            "Append a wss:// relay; duplicates are skipped.",
        ),
        (
            "Set Lightning Address (buyer)",
            "User mode only. Confirms save after fetching LNURL metadata (payRequest). On failure, settings are not updated.",
        ),
        (
            "Clear Lightning Address",
            "User mode only. Remove the saved buyer Lightning address from settings.toml.",
        ),
        (
            "Add Currency Filter",
            "Add a fiat code (e.g. USD). The order book only shows matching orders.",
        ),
        (
            "Clear Currency Filters",
            "Remove all filters so every configured currency can appear again.",
        ),
        (
            "View Seed Words",
            "Show your BIP-39 mnemonic from the local database. Press C to copy. Treat as highly sensitive.",
        ),
        (
            "Import Seed Words",
            "Wipe local session state and import a 12-word seed from another Mostro client, then restore orders from Mostro.",
        ),
        (
            "Restore Session",
            "Recover this identity's orders and disputes from Mostro after a reinstall or on a new machine.",
        ),
        (
            "Generate New Keys",
            "Rotate identity/trade keys. Confirm prompts and back up any new mnemonic.",
        ),
    ];

    let entries = match user_role {
        UserRole::Admin => admin_entries,
        UserRole::User => user_entries,
    };
    for (name, desc) in entries.iter() {
        push_settings_instruction_line(&mut lines, name, desc);
    }

    (title, lines)
}

fn help_content(app: &AppState, tab: Tab) -> (String, Vec<String>) {
    match tab {
        Tab::Admin(AdminTab::DisputesInProgress) => {
            let is_finalized = crate::ui::helpers::selected_filtered_dispute(app)
                .and_then(|d| crate::ui::helpers::is_dispute_finalized(&d))
                .unwrap_or(false);
            let filter_hint = match app.dispute_filter {
                DisputeFilter::InProgress => FILTER_VIEW_FINALIZED,
                DisputeFilter::Finalized => FILTER_VIEW_IN_PROGRESS,
            };
            let mut lines = vec![
                filter_hint.to_string(),
                HELP_DIP_TAB_PARTY.to_string(),
                HELP_DIP_SELECT_DISPUTE.to_string(),
                HELP_DIP_SCROLL_CHAT.to_string(),
                HELP_DIP_END_BOTTOM.to_string(),
                HELP_DIP_SHIFT_F_RESOLVE.to_string(),
                HELP_DIP_SHIFT_R_RECOVER.to_string(),
            ];
            if !is_finalized {
                lines.push(HELP_DIP_SHIFT_I_INPUT.to_string());
                lines.push(HELP_DIP_ENTER_SEND.to_string());
                lines.push(HELP_DIP_PASTE_CHAT.to_string());
                lines.push(HELP_DIP_DELETE_LOCAL.to_string());
                lines.push(HELP_DIP_CTRL_S_ATTACH.to_string());
            } else {
                lines.push(HELP_DIP_DELETE_LOCAL.to_string());
            }
            (HELP_TITLE_DISPUTES_IN_PROGRESS.to_string(), lines)
        }
        Tab::Admin(AdminTab::DisputesPending) => (
            HELP_TITLE_DISPUTES_PENDING.to_string(),
            vec![
                HELP_DP_ENTER_TAKE.to_string(),
                HELP_DP_SELECT_DISPUTE.to_string(),
            ],
        ),
        Tab::Admin(AdminTab::Observer) => (
            HELP_TITLE_OBSERVER.to_string(),
            vec![
                HELP_OBS_ENTER_LOAD.to_string(),
                HELP_OBS_PASTE_SHARED_KEY.to_string(),
                HELP_OBS_SCROLL_LINE.to_string(),
                HELP_OBS_SCROLL_PAGE.to_string(),
                HELP_OBS_ESC_CLEAR_ERR.to_string(),
                HELP_OBS_CTRL_C_CLEAR.to_string(),
                HELP_OBS_CTRL_S_ATTACH.to_string(),
            ],
        ),
        Tab::Admin(AdminTab::Settings) => (
            HELP_TITLE_SETTINGS_ADMIN.to_string(),
            vec![
                HELP_SETTINGS_SWITCH_FROM_MENU.to_string(),
                HELP_SETTINGS_SHIFT_H_FULL.to_string(),
                HELP_SETTINGS_SELECT_OPTION.to_string(),
                HELP_SETTINGS_ENTER_OPEN.to_string(),
            ],
        ),
        Tab::Admin(AdminTab::Exit) => (
            HELP_TITLE_EXIT.to_string(),
            vec![HELP_EXIT_ENTER_CONFIRM.to_string()],
        ),
        Tab::User(UserTab::Orders) => (
            HELP_TITLE_ORDERS.to_string(),
            vec![
                HELP_ORDERS_ENTER_TAKE.to_string(),
                HELP_ORDERS_SELECT.to_string(),
            ],
        ),
        Tab::User(UserTab::MyTrades) => (
            HELP_TITLE_MY_TRADES.to_string(),
            vec![
                HELP_MY_TRADES_NAV.to_string(),
                HELP_MY_TRADES_ENTER_SEND.to_string(),
                HELP_MY_TRADES_TAB_CHAT.to_string(),
                HELP_MY_TRADES_SHIFT_I.to_string(),
                HELP_MY_TRADES_SHIFT_C_CANCEL.to_string(),
                HELP_MY_TRADES_SHIFT_F_FIAT_SENT.to_string(),
                HELP_MY_TRADES_SHIFT_R_RELEASE.to_string(),
                HELP_MY_TRADES_SHIFT_V_RATE.to_string(),
                HELP_MY_TRADES_SHIFT_D_DISPUTE.to_string(),
                HELP_MY_TRADES_SHIFT_U_REFRESH.to_string(),
                HELP_MY_TRADES_SHIFT_K_KCONV.to_string(),
                HELP_MY_TRADES_CTRL_S_ATTACH.to_string(),
                HELP_MY_TRADES_CTRL_O_SEND.to_string(),
                HELP_MY_TRADES_CTRL_SHIFT_O_RETRY.to_string(),
                HELP_MY_TRADES_SHIFT_H_HELP.to_string(),
            ],
        ),
        Tab::User(UserTab::Messages) => (
            HELP_TITLE_MESSAGES.to_string(),
            vec![HELP_MSG_ENTER_OPEN.to_string(), HELP_MSG_SELECT.to_string()],
        ),
        Tab::User(UserTab::MostroInfo) | Tab::Admin(AdminTab::MostroInfo) => (
            "Mostro instance info".to_string(),
            vec!["View Mostro daemon status and accepted fiat currencies.".to_string()],
        ),
        Tab::User(UserTab::CreateNewOrder) => (
            HELP_TITLE_CREATE_NEW_ORDER.to_string(),
            vec![
                HELP_CNO_CHANGE_FIELD.to_string(),
                HELP_CNO_TAB_NEXT.to_string(),
                HELP_CNO_ENTER_CONFIRM.to_string(),
            ],
        ),
        Tab::User(UserTab::Settings) => (
            HELP_TITLE_SETTINGS_USER.to_string(),
            vec![
                HELP_SETTINGS_SWITCH_FROM_MENU.to_string(),
                HELP_SETTINGS_SHIFT_H_FULL.to_string(),
                HELP_SETTINGS_SELECT_OPTION.to_string(),
                HELP_SETTINGS_ENTER_OPEN.to_string(),
            ],
        ),
        Tab::User(UserTab::Exit) => (
            HELP_TITLE_EXIT.to_string(),
            vec![HELP_EXIT_ENTER_CONFIRM.to_string()],
        ),
    }
}

#[cfg(test)]
mod help_content_tests {
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
    fn my_trades_help_lists_the_dispute_shortcut() {
        let app = AppState::new(UserRole::User);
        let (_, lines) = help_content(&app, Tab::User(UserTab::MyTrades));
        assert!(
            lines.iter().any(|l| l == HELP_MY_TRADES_SHIFT_D_DISPUTE),
            "Shift+D missing from My Trades help: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l == HELP_MY_TRADES_SHIFT_U_REFRESH),
            "Shift+U missing from My Trades help: {lines:?}"
        );
        assert!(
            lines.iter().any(|l| l == HELP_MY_TRADES_SHIFT_K_KCONV),
            "Shift+K missing from My Trades help: {lines:?}"
        );
    }

    #[test]
    fn observer_help_lists_shared_key_load() {
        let app = AppState::new(UserRole::Admin);
        let (_, lines) = help_content(&app, Tab::Admin(AdminTab::Observer));
        assert!(
            lines.iter().any(|l| l == HELP_OBS_ENTER_LOAD),
            "Shared key load missing from Observer help: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.contains("Tab: Switch")),
            "Tab focus shortcut should be removed from Observer help now that only one field exists: {lines:?}"
        );
    }

    #[test]
    fn short_my_trades_help_keeps_essential_shortcuts_and_close_hint_visible() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = AppState::new(UserRole::User);

        terminal
            .draw(|f| render_help_popup(f, &app, Tab::User(UserTab::MyTrades)))
            .unwrap();

        let buf = terminal.backend().buffer();
        for expected in [
            "Enter",
            "Shift+I",
            "Tab",
            "Shift+C",
            "Shift+F",
            "Shift+R",
            "Shift+D",
            HELP_CLOSE_HINT,
        ] {
            assert!(
                buffer_contains(buf, expected),
                "missing {expected:?} from compact My Trades help"
            );
        }
    }

    #[test]
    fn narrow_short_my_trades_help_keeps_shortcuts_and_close_hint_visible() {
        let backend = TestBackend::new(20, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = AppState::new(UserRole::User);

        terminal
            .draw(|f| render_help_popup(f, &app, Tab::User(UserTab::MyTrades)))
            .unwrap();

        let buf = terminal.backend().buffer();
        for expected in [
            "Enter",
            "Tab",
            "Shift+I",
            "Shift+C",
            "Shift+F",
            "Shift+R",
            "Shift+D",
            "Esc, Enter or",
            "Ctrl+H to close",
        ] {
            assert!(
                buffer_contains(buf, expected),
                "missing {expected:?} from narrow compact My Trades help"
            );
        }
    }
}
