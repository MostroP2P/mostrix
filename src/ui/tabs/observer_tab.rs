use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::ui::constants::{CHAT_COPY_HINT, CHAT_COPY_START};
use crate::ui::helpers::build_observer_scrollview_content;
use crate::ui::key_handler::chat_copy;
use crate::ui::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Below this width the full field labels and footer (the longer footer line
/// needs ~104 columns) no longer fit; fall back to the abbreviated compact
/// labels/footer instead of silently clipping keyboard shortcuts.
const OBSERVER_NARROW_WIDTH: u16 = 60;

pub fn render_observer_tab(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    chat_copy::validate_selection(app);
    let selection = chat_copy::selected_index(app);
    let feedback = chat_copy::feedback_text(app);
    let copy_context = selection.is_some() || feedback.is_some();
    let copy_hint = if area.width < 34 {
        "↑↓ Select\nEnter Copy Esc Cancel\nCtrl+L Clear".to_string()
    } else {
        format!("{CHAT_COPY_HINT}\nCtrl+L:Clear all")
    };
    let compact = area.height < 16 || area.width < OBSERVER_NARROW_WIDTH;
    // Compact footer is 3 short lines (vs. 2 long ones) so shortcuts stay
    // readable instead of being cut off; field row stays 3 rows either way.
    let footer_height = if copy_context {
        Paragraph::new(feedback.unwrap_or(&copy_hint))
            .wrap(Wrap { trim: true })
            .line_count(area.width.max(1))
            .min(3) as u16
    } else if compact {
        3
    } else {
        2
    };
    let field_height = if copy_context && area.height < footer_height + 7 {
        0
    } else {
        3
    };
    let input_height = field_height + footer_height;
    // Borders consume 2 rows. Compact: 1 inner row (status/error). Full: 2 inner rows.
    let header_height = if copy_context && area.height < footer_height + 11 {
        0
    } else if compact {
        3
    } else {
        4
    };
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
                "Paste Shared key and press Enter to load chat",
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
                Span::raw("  –  paste Shared key (read-only). Never paste a signing key."),
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
        .title(if selection.is_some() {
            "Copy: Observer"
        } else {
            "Chat messages"
        })
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
            "No messages yet. Paste Shared key and press Enter to load."
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
        let mut content = build_observer_scrollview_content(
            &app.observer_messages,
            viewport_width,
            Some(max_content_width),
        );
        let selected_rows = content.select_message(selection);

        // Auto-scroll to bottom only when new messages arrive; preserve manual scroll otherwise.
        let visible_count = app.observer_messages.len();
        if visible_count > 0 {
            if let Some(last_count) = app.observer_scroll_tracker {
                if visible_count > last_count && selection.is_none() {
                    app.observer_scrollview_state.scroll_to_bottom();
                }
            } else if selection.is_none() {
                // First time we load messages, jump to bottom.
                app.observer_scrollview_state.scroll_to_bottom();
            }
            app.observer_scroll_tracker = Some(visible_count);
        } else {
            app.observer_scroll_tracker = Some(0);
        }

        if let Some(selected_rows) = selected_rows {
            content.keep_selection_visible(
                selected_rows,
                inner_area.height,
                &mut app.observer_scrollview_state,
            );
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
        [
            Constraint::Length(field_height),
            Constraint::Length(footer_height),
        ],
    )
    .split(chunks[2]);

    let focused_border = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);
    let title_style = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);

    let conv_title = if selection.is_some() {
        "Shared key (copying)"
    } else if compact {
        "Shared key (hex)"
    } else {
        "Shared key (64-char hex, read-only grant)"
    };

    let conv_input = Paragraph::new(app.observer_shared_key_input.as_str()).block(
        Block::default()
            .title(Span::styled(conv_title, title_style))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(focused_border),
    );
    f.render_widget(conv_input, input_chunks[0]);

    if copy_context {
        f.render_widget(
            Paragraph::new(feedback.unwrap_or(&copy_hint))
                .style(Style::default().fg(PRIMARY_COLOR))
                .wrap(Wrap { trim: true }),
            input_chunks[1],
        );
        return;
    }

    let footer_text = if compact {
        // Shortened so shortcuts stay visible instead of clipping on narrow terminals.
        "Ctrl+H:Help Ctrl+C:Copy Paste\n\
Enter:Load  Esc:Clear  Ctrl+L:All\n\
Ctrl+S:Save  \u{2191}\u{2193}/PgUp/PgDn:Scroll"
            .to_string()
    } else {
        let paste_hint = if cfg!(windows) {
            "Shift+Insert / Ctrl+V / Ctrl+Shift+V / right-click"
        } else {
            "Ctrl+V / Ctrl+Shift+V / middle-click"
        };
        format!(
            "{CHAT_COPY_START} | Ctrl+H: Help | Paste ({paste_hint})\n\
Enter: Load chat | Esc: Clear error | Ctrl+L: Clear all | Ctrl+S: Save attachment | ↑↓/PgUp/PgDn: Scroll"
        )
    };
    let footer = Paragraph::new(footer_text);
    f.render_widget(footer, input_chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::render_observer_tab;
    use crate::ui::key_handler::chat_copy;
    use crate::ui::PRIMARY_COLOR;
    use crate::ui::{AppState, UserRole};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
    fn observer_tab_prompts_for_shared_key_not_ecdh() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = AppState::new(UserRole::Admin);
        terminal
            .draw(|f| render_observer_tab(f, f.area(), &mut app))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "Shared key"),
            "missing Shared key field"
        );
        assert!(
            buffer_contains(buf, "Never paste a signing key"),
            "missing signing-key warning"
        );
        assert!(
            buffer_contains(buf, "read-only"),
            "missing read-only grant copy"
        );
        assert!(
            !buffer_contains(buf, "Signer pubkey"),
            "Signer pubkey input box should be removed"
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

    fn copy_key(app: &mut AppState, code: KeyCode, modifiers: KeyModifiers) {
        assert!(chat_copy::handle_key_with(
            app,
            &KeyEvent::new(code, modifiers),
            |_| false
        ));
    }

    fn highlighted(buffer: &ratatui::buffer::Buffer, text: &str) -> bool {
        (0..buffer.area.height).any(|row| {
            (0..buffer.area.width)
                .map(|column| &buffer[(column, row)])
                .filter(|cell| cell.bg == PRIMARY_COLOR)
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains(text)
        })
    }

    #[test]
    fn observer_copy_highlight_and_controls_survive_resize() {
        let mut app = chat_copy::tests::app_with_observer_messages();
        copy_key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        for (width, height) in [(120, 28), (80, 24), (80, 12), (40, 12), (30, 8), (120, 28)] {
            let buffer = render_observer(&mut app, width, height);
            assert!(
                highlighted(&buffer, "first"),
                "missing selection at {width}x{height}"
            );
            for hint in ["Enter", "Esc", "Ctrl+L"] {
                assert!(buffer_contains(&buffer, hint));
            }
            assert_eq!(app.observer_shared_key_input, "a".repeat(64));
        }
        for (width, height) in [(0, 0), (1, 1), (8, 3)] {
            render_observer(&mut app, width, height);
        }
    }

    #[test]
    fn observer_copy_scrolls_selection_and_ignores_incoming_autoscroll() {
        let mut app = chat_copy::tests::app_with_observer_messages();
        app.observer_messages[0].content = "first wrapped words ".repeat(60);
        copy_key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        let buffer = render_observer(&mut app, 40, 12);
        assert!(highlighted(&buffer, "first"));
        copy_key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        let buffer = render_observer(&mut app, 40, 12);
        assert!(highlighted(&buffer, "last"));
        assert!(app.observer_scrollview_state.offset().y > 0);
        let mut incoming = app.observer_messages[0].clone();
        incoming.content = "incoming words ".repeat(60);
        app.observer_messages.push(incoming);
        let buffer = render_observer(&mut app, 40, 12);
        assert!(highlighted(&buffer, "last"));
        assert_eq!(chat_copy::selected_index(&app), Some(1));
    }

    #[test]
    fn observer_copy_feedback_and_clear_remove_highlight() {
        for success in [true, false] {
            let mut app = chat_copy::tests::app_with_observer_messages();
            copy_key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
            assert!(chat_copy::handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                |_| success
            ));
            let buffer = render_observer(&mut app, 30, 8);
            assert!(buffer_contains(
                &buffer,
                if success {
                    "Copied to clipboard"
                } else {
                    "Clipboard unavailable"
                }
            ));
            assert!(!highlighted(&buffer, "first"));
            assert_eq!(app.observer_shared_key_input, "a".repeat(64));
            copy_key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
            copy_key(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
            let buffer = render_observer(&mut app, 80, 24);
            assert!(!buffer_contains(&buffer, "first"));
            assert!(!buffer_contains(&buffer, "Copied to clipboard"));
            assert!(app.observer_shared_key_input.is_empty());
        }
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
    fn observer_tab_compact_height_keeps_shared_key_field() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = AppState::new(UserRole::Admin);
        terminal
            .draw(|f| render_observer_tab(f, f.area(), &mut app))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "Shared key"),
            "compact layout dropped Shared key"
        );
    }

    #[test]
    fn observer_clear_shortcut_is_visible_in_full_and_compact_layouts() {
        for (width, height, shortcut) in [
            (120, 24, "Ctrl+L: Clear all"),
            (40, 24, "Ctrl+L:All"),
            (80, 12, "Ctrl+L:All"),
            (40, 12, "Ctrl+L:All"),
        ] {
            let buf = render_observer(&mut AppState::new(UserRole::Admin), width, height);
            assert!(
                buffer_contains(&buf, shortcut),
                "missing hint at {width}x{height}"
            );
            assert!(
                buffer_contains(&buf, "Ctrl+C:Copy"),
                "missing copy hint at {width}x{height}"
            );
            assert!(
                !buffer_contains(&buf, "Ctrl+C: Clear all") && !buffer_contains(&buf, "Ctrl+C:All"),
                "stale clear hint at {width}x{height}"
            );
        }
    }

    /// A narrow-but-tall terminal (plenty of height, insufficient width) should still
    /// switch to the compact layout: abbreviated field labels and a shortened footer
    /// so keyboard shortcuts stay fully on-screen instead of being clipped.
    #[test]
    fn observer_tab_narrow_width_uses_abbreviated_labels_and_footer() {
        let buf = render_observer(&mut AppState::new(UserRole::Admin), 40, 24);

        assert!(
            buffer_contains(&buf, "Shared key (hex)"),
            "narrow layout should use the abbreviated Shared key label"
        );
        assert!(
            !buffer_contains(&buf, "Shared key (64-char hex, read-only grant)"),
            "narrow layout should not use the full-width Shared key label"
        );
        // The long-form footer's second line never fits even at 80 columns, so its
        // presence here would indicate clipped text rather than a rendered shortcut.
        assert!(
            !buffer_contains(&buf, "Ctrl+S: Save attachment"),
            "narrow layout should not attempt to render the full-width footer"
        );

        for shortcut in [
            "Ctrl+H:Help",
            "Enter:Load",
            "Esc:Clear",
            "Ctrl+L:All",
            "Ctrl+S:Save",
            "Scroll",
        ] {
            assert!(
                buffer_contains(&buf, shortcut),
                "narrow footer is missing shortcut: {shortcut}"
            );
        }
    }
}
