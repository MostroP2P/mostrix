use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::ui::helpers::build_observer_scrollview_content;
use crate::ui::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_observer_tab(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3), // Header / status
            Constraint::Min(0),    // Chat messages
            Constraint::Length(4), // Input + footer
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
            Span::raw("  –  paste a shared key to fetch and view chat messages"),
        ]));

        if let Some(err) = &app.observer_error {
            lines.push(Line::from(vec![
                Span::styled(
                    "Error: ",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(err.as_str(), Style::default().fg(Color::Red)),
            ]));
        } else if !app.observer_messages.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("Loaded {} message(s)", app.observer_messages.len()),
                    Style::default().fg(Color::Green),
                ),
            ]));
        } else if app.observer_loading {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    "Fetching messages from relays...",
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    "Paste shared key and press Enter to load chat",
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

    // Chat view (reuses the same formatting as dispute chat) with scrollview.
    let chat_block = Block::default()
        .title("Chat messages")
        .borders(Borders::ALL);
    let chat_area = chunks[1];
    let inner_area = chat_block.inner(chat_area);
    f.render_widget(chat_block, chat_area);

    if app.observer_messages.is_empty() {
        let hint = if app.observer_loading {
            "Fetching messages..."
        } else {
            "No messages yet. Paste a shared key and press Enter to load."
        };
        let paragraph = Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::Gray),
        )));
        f.render_widget(paragraph, inner_area);
    } else {
        let content_width = inner_area.width.max(1);
        let max_content_width = (content_width / 2).max(1);
        let content = build_observer_scrollview_content(
            &app.observer_messages,
            content_width,
            Some(max_content_width),
        );

        // Auto-scroll to bottom only when new messages arrive; preserve manual scroll otherwise.
        let visible_count = app.observer_messages.len();
        if visible_count > 0 {
            if let Some(last_count) = app.observer_scroll_tracker {
                if visible_count > last_count {
                    app.observer_scrollview_state.scroll_to_bottom();
                }
            } else {
                // First time we load messages, jump to bottom.
                app.observer_scrollview_state.scroll_to_bottom();
            }
            app.observer_scroll_tracker = Some(visible_count);
        } else {
            app.observer_scroll_tracker = Some(0);
        }

        let mut scroll_view = ScrollView::new(Size::new(
            content.content_width,
            content.content_height.max(1),
        ))
        .vertical_scrollbar_visibility(ScrollbarVisibility::Always);

        let content_rect = Rect::new(0, 0, content.content_width, content.content_height.max(1));
        scroll_view.render_widget(
            Paragraph::new(content.lines).wrap(Wrap { trim: true }),
            content_rect,
        );
        f.render_stateful_widget(scroll_view, inner_area, &mut app.observer_scrollview_state);
    }

    // Shared key input + footer
    let input_chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Length(1)],
    )
    .split(chunks[2]);

    let key_border = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);
    let key_title_style = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);

    let key_title = Span::styled("Shared key (64-char hex)", key_title_style);
    let key_input = Paragraph::new(app.observer_shared_key_input.as_str()).block(
        Block::default()
            .title(key_title)
            .borders(Borders::ALL)
            .border_style(key_border),
    );
    f.render_widget(key_input, input_chunks[0]);

    let footer = Paragraph::new(
        "Paste shared key | Enter: Load chat | Esc: Clear error | Ctrl+C: Clear all | Ctrl+S: Save attachment | Up/Down: Scroll | PgUp/PgDn: Scroll page",
    );
    f.render_widget(footer, input_chunks[1]);
}
