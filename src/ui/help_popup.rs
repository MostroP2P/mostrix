use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::constants::{
    FILTER_VIEW_FINALIZED, FILTER_VIEW_IN_PROGRESS, HELP_CLOSE_HINT, HELP_CNO_CHANGE_FIELD,
    HELP_CNO_ENTER_CONFIRM, HELP_CNO_TAB_NEXT, HELP_DIP_CTRL_S_ATTACH, HELP_DIP_END_BOTTOM,
    HELP_DIP_ENTER_SEND, HELP_DIP_SCROLL_CHAT, HELP_DIP_SELECT_DISPUTE, HELP_DIP_SHIFT_F_RESOLVE,
    HELP_DIP_SHIFT_I_INPUT, HELP_DIP_TAB_PARTY, HELP_DP_ENTER_TAKE, HELP_DP_SELECT_DISPUTE,
    HELP_EXIT_ENTER_CONFIRM, HELP_MSG_ENTER_OPEN, HELP_MSG_SELECT, HELP_MY_TRADES_NAV,
    HELP_OBS_CTRL_C_CLEAR, HELP_OBS_ENTER_LOAD, HELP_OBS_ESC_CLEAR_ERR, HELP_OBS_TAB_FIELD,
    HELP_ORDERS_ENTER_TAKE, HELP_ORDERS_SELECT, HELP_SETTINGS_ENTER_OPEN, HELP_SETTINGS_M_MODE,
    HELP_SETTINGS_SELECT_OPTION, HELP_TITLE_CREATE_NEW_ORDER, HELP_TITLE_DISPUTES_IN_PROGRESS,
    HELP_TITLE_DISPUTES_PENDING, HELP_TITLE_EXIT, HELP_TITLE_MESSAGES, HELP_TITLE_MY_TRADES,
    HELP_TITLE_OBSERVER, HELP_TITLE_ORDERS, HELP_TITLE_SETTINGS_ADMIN, HELP_TITLE_SETTINGS_USER,
};
use super::{AppState, DisputeFilter, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::navigation::{AdminTab, Tab, UserTab};

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
                HELP_OBS_TAB_FIELD.to_string(),
                HELP_OBS_ENTER_LOAD.to_string(),
                HELP_OBS_ESC_CLEAR_ERR.to_string(),
                HELP_OBS_CTRL_C_CLEAR.to_string(),
            ],
        ),
        Tab::Admin(AdminTab::Settings) => (
            HELP_TITLE_SETTINGS_ADMIN.to_string(),
            vec![
                HELP_SETTINGS_M_MODE.to_string(),
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
            vec![HELP_MY_TRADES_NAV.to_string()],
        ),
        Tab::User(UserTab::Messages) => (
            HELP_TITLE_MESSAGES.to_string(),
            vec![HELP_MSG_ENTER_OPEN.to_string(), HELP_MSG_SELECT.to_string()],
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
                HELP_SETTINGS_M_MODE.to_string(),
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
