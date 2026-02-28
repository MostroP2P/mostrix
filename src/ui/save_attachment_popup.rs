//! Save Attachment popup: list of attachments in the current dispute chat (Ctrl+S).
//! User selects with Up/Down and presses Enter to save.

use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::chat::ChatAttachmentType;
use super::constants::SAVE_ATTACHMENT_POPUP_HINT;
use super::helpers::get_visible_attachment_messages;
use super::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

const POPUP_WIDTH: u16 = 56;
const TITLE: &str = "📎 Save attachment";

/// Renders the Save Attachment popup with a selectable list.
/// `selected_idx` is the index into the visible attachment list (clamped inside).
pub fn render_save_attachment_popup(f: &mut ratatui::Frame, app: &AppState, selected_idx: usize) {
    let dispute_id_key = match app
        .admin_disputes_in_progress
        .get(app.selected_in_progress_idx)
    {
        Some(d) => d.dispute_id.as_str(),
        None => return,
    };

    let list = get_visible_attachment_messages(app, dispute_id_key);
    if list.is_empty() {
        return;
    }

    let selected_idx = selected_idx.min(list.len().saturating_sub(1));
    let line_count = list.len();
    let popup_height = (line_count as u16 + 4).min(f.area().height.saturating_sub(2));

    let area = f.area();
    let popup = {
        let [p] = Layout::horizontal([Constraint::Length(POPUP_WIDTH)])
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
            TITLE,
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let content: Vec<Line> = list
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let att = msg.attachment.as_ref().unwrap();
            let icon = match att.file_type {
                ChatAttachmentType::Image => "🖼",
                ChatAttachmentType::File => "📎",
            };
            let text = format!("{} {}", icon, att.filename);
            let style = if i == selected_idx {
                Style::default().fg(BACKGROUND_COLOR).bg(PRIMARY_COLOR)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(Span::styled(text, style))
        })
        .collect();

    let mut all = content;
    all.push(Line::from(""));
    all.push(Line::from(Span::styled(
        SAVE_ATTACHMENT_POPUP_HINT,
        Style::default().fg(Color::DarkGray),
    )));
    let paragraph = Paragraph::new(all).wrap(Wrap { trim: true });
    f.render_widget(paragraph, inner);
}
