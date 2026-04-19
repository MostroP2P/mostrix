use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::constants::*;
use super::{AppState, DisputeFilter, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::navigation::{AdminTab, Tab, UserRole, UserTab};

/// Renders the context-aware keyboard shortcuts popup (Ctrl+H).
pub fn render_help_popup(f: &mut ratatui::Frame, app: &AppState, tab: Tab) {
    let area = f.area();
    let (title, lines) = help_content(app, tab);
    let line_count = lines.len().max(1);
    let popup_width = 64u16;
    let popup_height = (line_count as u16 + 4).min(area.height.saturating_sub(2));

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

    let content: Vec<Line> = lines
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

/// Full reference for every Settings menu row (Shift+H on Settings).
pub fn render_settings_instructions_popup(f: &mut ratatui::Frame, user_role: UserRole) {
    let area = f.area();
    let (title, mut lines) = settings_instruction_lines(user_role);

    let intro = Line::from(vec![
        Span::styled(
            "Each block matches one Settings list row. ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled("↑/↓", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" move · ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter", Style::default().fg(PRIMARY_COLOR)),
        Span::styled(" runs it.", Style::default().fg(Color::DarkGray)),
    ]);
    lines.insert(0, intro);
    lines.insert(1, Line::from(""));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        SETTINGS_INSTRUCTIONS_CLOSE_HINT,
        Style::default().fg(Color::DarkGray),
    )));

    let line_count = lines.len().max(1);
    let popup_width = 78u16;
    let popup_height = (line_count as u16 + 4).min(area.height.saturating_sub(2));

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

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    f.render_widget(paragraph, inner);
}

fn settings_instruction_block_style() -> (Style, Style) {
    let title = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);
    let body = Style::default().fg(Color::Gray);
    (title, body)
}

/// One menu option: title row + indented body; optional blank line after (between entries).
fn push_settings_instruction_entry(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    description: &str,
    add_spacing_after: bool,
) {
    let (title_style, body_style) = settings_instruction_block_style();
    lines.push(Line::from(Span::styled(format!("  ▸ {name}"), title_style)));
    lines.push(Line::from(Span::styled(
        format!("      {description}"),
        body_style,
    )));
    if add_spacing_after {
        lines.push(Line::from(""));
    }
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
            "Set the Mostro daemon hex pubkey used for subscriptions and orders.",
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
            "Show your BIP-39 mnemonic from the local database. Treat as highly sensitive.",
        ),
        (
            "Add Dispute Solver",
            "Enter a solver npub to register with Mostro for dispute routing.",
        ),
        (
            "Change Admin Key",
            "Update admin_privkey in settings (dispute chat and classification).",
        ),
        (
            "Generate New Keys",
            "Rotate identity/trade keys. Confirm prompts and back up any new mnemonic.",
        ),
    ];

    let user_entries: &[(&str, &str)] = &[
        (
            "Switch Mode (User ↔ Admin)",
            "Switch to Admin when you need dispute tools. Saves user_mode and reloads tabs.",
        ),
        (
            "Change Mostro Pubkey",
            "Set the Mostro daemon hex pubkey used for subscriptions and orders.",
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
            "Show your BIP-39 mnemonic from the local database. Treat as highly sensitive.",
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
    let n = entries.len();
    for (i, (name, desc)) in entries.iter().enumerate() {
        push_settings_instruction_entry(&mut lines, name, desc, i + 1 < n);
    }

    (title, lines)
}

fn help_content(app: &AppState, tab: Tab) -> (String, Vec<String>) {
    match tab {
        Tab::Admin(AdminTab::DisputesInProgress) => {
            let is_finalized = app
                .admin_disputes_in_progress
                .get(app.selected_in_progress_idx)
                .and_then(crate::ui::helpers::is_dispute_finalized)
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
            ];
            if !is_finalized {
                lines.push(HELP_DIP_SHIFT_I_INPUT.to_string());
                lines.push(HELP_DIP_ENTER_SEND.to_string());
                lines.push(HELP_DIP_CTRL_S_ATTACH.to_string());
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
                HELP_MY_TRADES_SHIFT_I.to_string(),
                HELP_MY_TRADES_SHIFT_C_CANCEL.to_string(),
                HELP_MY_TRADES_SHIFT_F_FIAT_SENT.to_string(),
                HELP_MY_TRADES_SHIFT_R_RELEASE.to_string(),
                HELP_MY_TRADES_SHIFT_V_RATE.to_string(),
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
