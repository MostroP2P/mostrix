use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::ui::helpers::build_observer_scrollview_content;
use crate::ui::{AppState, ObserverInputField, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_observer_tab(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let compact = area.height < 16;
    let input_height = if compact { 5 } else { 8 };
    // Borders consume 2 rows. Compact: 1 inner row (status/error). Full: 2 inner rows.
    let header_height = if compact { 3 } else { 4 };
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(header_height),
            Constraint::Min(0), // Chat messages
            Constraint::Length(input_height),
        ],
    )
    .split(area);

    // Header / status — keep the dynamic row visible (do not clip it behind the title).
    let status_line = if let Some(err) = &app.observer_error {
        Line::from(vec![
            Span::styled(
                "Error: ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(err.as_str(), Style::default().fg(Color::Red)),
        ])
    } else if !app.observer_messages.is_empty() {
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("Loaded {} message(s)", app.observer_messages.len()),
                Style::default().fg(Color::Green),
            ),
        ])
    } else if app.observer_loading {
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Fetching messages from relays...",
                Style::default().fg(Color::Yellow),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Paste K_conv and press Enter to load chat",
                Style::default().fg(Color::Gray),
            ),
        ])
    };

    let status_lines = if compact {
        vec![status_line]
    } else {
        vec![
            Line::from(vec![
                Span::styled(
                    "Observer Mode",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  –  paste K_conv (read-only). Never paste K_sign."),
            ]),
            status_line,
        ]
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
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(PRIMARY_COLOR))
            .style(Style::default().bg(BACKGROUND_COLOR)),
    );
    f.render_widget(header, chunks[0]);

    // Chat view (reuses the same formatting as dispute chat) with scrollview.
    let chat_block = Block::default()
        .title("Chat messages")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));
    let chat_area = chunks[1];
    let inner_area = chat_block.inner(chat_area);
    f.render_widget(chat_block, chat_area);

    if app.observer_messages.is_empty() {
        let hint = if app.observer_loading {
            "Fetching messages..."
        } else {
            "No messages yet. Paste K_conv and press Enter to load."
        };
        let paragraph = Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::Gray),
        )));
        f.render_widget(paragraph, inner_area);
    } else {
        // Match the disputes chat behavior: use the full inner width (minus one column)
        // so right-aligned messages don't lose their last character.
        let viewport_width = inner_area.width.saturating_sub(1).max(1);
        let max_content_width = (viewport_width / 2).max(1);
        let content = build_observer_scrollview_content(
            &app.observer_messages,
            viewport_width,
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

    // K_conv / optional pub(K_sign) inputs + footer
    let show_both_fields = !compact;
    let input_chunks = if show_both_fields {
        Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(2),
            ],
        )
        .split(chunks[2])
    } else {
        Layout::new(
            Direction::Vertical,
            [Constraint::Length(3), Constraint::Length(2)],
        )
        .split(chunks[2])
    };

    let focused_border = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);
    let idle_border = Style::default().fg(Color::Gray);
    let title_style = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);

    let conv_focused = app.observer_input_focus == ObserverInputField::ConvKey;
    let conv_input = Paragraph::new(app.observer_shared_key_input.as_str()).block(
        Block::default()
            .title(Span::styled(
                "K_conv (64-char hex, read-only grant)",
                title_style,
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if conv_focused {
                focused_border
            } else {
                idle_border
            }),
    );
    let sign_input = Paragraph::new(app.observer_sign_pubkey_input.as_str()).block(
        Block::default()
            .title(Span::styled(
                "pub(K_sign) optional locator (hex/npub)",
                title_style,
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if !conv_focused {
                focused_border
            } else {
                idle_border
            }),
    );

    if show_both_fields {
        f.render_widget(conv_input, input_chunks[0]);
        f.render_widget(sign_input, input_chunks[1]);
    } else if conv_focused {
        f.render_widget(conv_input, input_chunks[0]);
    } else {
        f.render_widget(sign_input, input_chunks[0]);
    }

    let paste_hint = if cfg!(windows) {
        "Shift+Insert / Ctrl+V / Ctrl+Shift+V / right-click"
    } else {
        "Ctrl+V / Ctrl+Shift+V / middle-click"
    };
    let footer = Paragraph::new(format!(
        "Ctrl+H: Help | Tab: Switch K_conv / pub(K_sign) | Paste ({paste_hint})\n\
Enter: Load chat | Esc: Clear error | Ctrl+C: Clear all | Ctrl+S: Save attachment | ↑↓/PgUp/PgDn: Scroll"
    ));
    let footer_idx = if show_both_fields { 2 } else { 1 };
    f.render_widget(footer, input_chunks[footer_idx]);
}

#[cfg(test)]
mod tests {
    use super::render_observer_tab;
    use crate::ui::{AppState, UserRole};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_contains(buf: &ratatui::buffer::Buffer, needle: &str) -> bool {
        let mut hay = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                hay.push_str(buf[(x, y)].symbol());
            }
        }
        hay.contains(needle)
    }

    #[test]
    fn observer_tab_prompts_for_k_conv_not_ecdh() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = AppState::new(UserRole::Admin);
        terminal
            .draw(|f| render_observer_tab(f, f.area(), &mut app))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "K_conv"), "missing K_conv field");
        assert!(
            buffer_contains(buf, "Never paste K_sign") || buffer_contains(buf, "K_sign"),
            "missing K_sign warning"
        );
        assert!(
            buffer_contains(buf, "read-only"),
            "missing read-only grant copy"
        );
    }

    fn render_observer(app: &mut AppState, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_observer_tab(f, f.area(), app))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn observer_header_shows_loading_and_error_at_standard_and_compact_heights() {
        let mut app = AppState::new(UserRole::Admin);
        app.observer_loading = true;
        let buf = render_observer(&mut app, 80, 24);
        assert!(
            buffer_contains(&buf, "Fetching messages"),
            "standard height clipped loading status"
        );
        let buf = render_observer(&mut app, 80, 12);
        assert!(
            buffer_contains(&buf, "Fetching messages"),
            "compact height clipped loading status"
        );

        app.observer_loading = false;
        app.observer_error = Some("invalid-k-conv".to_string());
        let buf = render_observer(&mut app, 80, 24);
        assert!(
            buffer_contains(&buf, "invalid-k-conv"),
            "standard height clipped K_conv error"
        );
        let buf = render_observer(&mut app, 80, 12);
        assert!(
            buffer_contains(&buf, "invalid-k-conv"),
            "compact height clipped K_conv error"
        );
    }

    #[test]
    fn observer_tab_compact_height_keeps_k_conv_field() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = AppState::new(UserRole::Admin);
        terminal
            .draw(|f| render_observer_tab(f, f.area(), &mut app))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "K_conv"),
            "compact layout dropped K_conv"
        );
    }
}
