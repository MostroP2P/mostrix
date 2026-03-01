use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::ui::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserverFocus {
    FilePath,
    SharedKey,
}

pub fn render_observer_tab(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),  // Header / status
            Constraint::Min(0),     // Decrypted chat
            Constraint::Length(10), // Inputs (taller so chatbox doesn't dominate)
        ],
    )
    .split(area);

    // Header / status
    let status_lines = {
        let mut lines = Vec::new();
        lines.push(Line::from(vec![
            Span::styled(
                "Observer Mode",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  –  open a Blossom-encrypted chat file with a shared key"),
        ]));

        if !app.observer_file_path_input.trim().is_empty() {
            lines.push(Line::from(vec![
                Span::styled("File: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    app.observer_file_path_input.as_str(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        if let Some(err) = &app.observer_error {
            lines.push(Line::from(vec![
                Span::styled(
                    "Error: ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(err.as_str(), Style::default().fg(Color::Red)),
            ]));
        } else if !app.observer_chat_lines.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("Decrypted {} line(s)", app.observer_chat_lines.len()),
                    Style::default().fg(Color::Green),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    "Waiting for file + key (press Enter to load)",
                    Style::default().fg(Color::Gray),
                ),
            ]));
        }

        lines
    };

    let header = Paragraph::new(status_lines).block(
        Block::default()
            .title(Span::styled(
                "🔍 Observer",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR)),
    );
    f.render_widget(header, chunks[0]);

    // Decrypted chat view
    let chat_items: Vec<ListItem> = if app.observer_chat_lines.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No decrypted content yet. After you set file and key, press Enter to load.",
            Style::default().fg(Color::Gray),
        )))]
    } else {
        app.observer_chat_lines
            .iter()
            .map(|line| {
                ListItem::new(Line::from(Span::styled(
                    line.as_str(),
                    Style::default().fg(Color::White),
                )))
            })
            .collect()
    };

    let chat_block = Block::default()
        .title("Decrypted chat preview")
        .borders(Borders::ALL);
    let chat_list = List::new(chat_items).block(chat_block);
    f.render_widget(chat_list, chunks[1]);

    // Inputs (taller rows for file path and shared key)
    let input_chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ],
    )
    .split(chunks[2]);

    let (file_border, file_title_style) = if app.observer_focus == ObserverFocus::FilePath {
        (
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(Color::Gray),
            Style::default().fg(Color::Gray),
        )
    };

    let (key_border, key_title_style) = if app.observer_focus == ObserverFocus::SharedKey {
        (
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(Color::Gray),
            Style::default().fg(Color::Gray),
        )
    };

    let file_title = Span::styled(
        "File path (relative to ~/.mostrix/downloads or absolute)",
        file_title_style,
    );
    let file_input = Paragraph::new(app.observer_file_path_input.as_str()).block(
        Block::default()
            .title(file_title)
            .borders(Borders::ALL)
            .border_style(file_border),
    );
    f.render_widget(file_input, input_chunks[0]);

    let key_title = Span::styled("Shared key (32-byte hex)", key_title_style);
    let key_input = Paragraph::new(app.observer_shared_key_input.as_str()).block(
        Block::default()
            .title(key_title)
            .borders(Borders::ALL)
            .border_style(key_border),
    );
    f.render_widget(key_input, input_chunks[1]);

    let footer = Paragraph::new(
        "Tab: Switch field | Enter: Load & decrypt | Esc: Clear error | Ctrl+C: Clear all",
    );
    f.render_widget(footer, input_chunks[2]);
}
