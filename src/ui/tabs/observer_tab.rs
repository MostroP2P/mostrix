use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::ui::helpers::build_observer_scrollview_content;
use crate::ui::key_handler::chat_copy;
use crate::ui::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Below this width full field labels no longer fit; use abbreviated titles.
const OBSERVER_NARROW_WIDTH: u16 = 60;

/// High-contrast keycap groups that drop whole pairs when width is tight.
fn shortcut_bar(width: u16, hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    let mut used = 0;
    for (key, label) in hints {
        let gap = if spans.is_empty() { 0 } else { 2 };
        let key_text = format!(" {key} ");
        let label_text = format!(" {label}");
        let group_width = Span::raw(&key_text).width() + Span::raw(&label_text).width();
        if used + gap + group_width > usize::from(width) {
            break;
        }
        let key_style = if spans.is_empty() {
            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
        } else {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        };
        if gap > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            key_text,
            key_style.add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(label_text, Style::default().fg(Color::Gray)));
        used += gap + group_width;
    }
    Line::from(spans)
}

fn observer_command_bar(width: u16) -> Line<'static> {
    // Help/Copy before Clear-all so narrow terminals keep the same primary
    // discoverability as My Trades / Disputes.
    shortcut_bar(
        width,
        &[
            ("Enter", "Load"),
            ("Ctrl+H", "Help"),
            ("Ctrl+C", "Copy"),
            ("Esc", "Clear"),
            ("Ctrl+L", "All"),
        ],
    )
}

fn observer_copy_controls(width: u16) -> Text<'static> {
    let hints = [
        ("↑↓", "Select"),
        ("Enter", "Copy"),
        ("Esc", "Cancel"),
        ("Ctrl+L", "Clear"),
    ];
    let full = shortcut_bar(u16::MAX, &hints);
    if full.width() <= usize::from(width) {
        Text::from(full)
    } else {
        Text::from(
            hints
                .iter()
                .map(|&(key, label)| {
                    let line = shortcut_bar(width, &[(key, label)]);
                    if line.spans.is_empty() {
                        shortcut_bar(width, &[(key, "")])
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>(),
        )
    }
}

pub fn render_observer_tab(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    chat_copy::validate_selection(app);
    let selection = chat_copy::selected_index(app);
    let selection_range = chat_copy::selected_range(app);
    let feedback = chat_copy::feedback_text(app);
    let copy_context = selection.is_some() || feedback.is_some();
    let copy_controls = if let Some(text) = feedback {
        Text::styled(text, Style::default().fg(PRIMARY_COLOR))
    } else {
        observer_copy_controls(area.width)
    };
    let compact = area.height < 16 || area.width < OBSERVER_NARROW_WIDTH;
    // One keycap command row; copy mode may wrap Select/Copy/Cancel/Clear.
    let footer_height = if copy_context {
        Paragraph::new(copy_controls.clone())
            .wrap(Wrap { trim: true })
            .line_count(area.width.max(1))
            .min(4) as u16
    } else {
        1
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
    let chat_area = chunks[1];
    let has_attachment = app.observer_messages.iter().any(|m| m.attachment.is_some());
    let mut chat_hints = Vec::new();
    if has_attachment {
        chat_hints.push(("Ctrl+S", "Save file"));
    }
    chat_hints.push(("Ctrl+V", "Paste"));
    chat_hints.push(("PgUp/PgDn", "Scroll"));
    let chat_border_hints = if copy_context {
        Line::default()
    } else {
        shortcut_bar(chat_area.width.saturating_sub(2), &chat_hints)
    };
    let chat_block = Block::default()
        .title(if selection.is_some() {
            "Copy: Observer"
        } else {
            "Chat messages"
        })
        .title_bottom(chat_border_hints)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));
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
        let selected_rows = content.select_messages(selection_range, selection);
        app.observer_line_starts = content.line_start_per_message.clone();

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
            Paragraph::new(copy_controls).wrap(Wrap { trim: true }),
            input_chunks[1],
        );
        return;
    }

    if footer_height > 0 {
        f.render_widget(
            Paragraph::new(observer_command_bar(input_chunks[1].width)),
            input_chunks[1],
        );
    }
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
    fn shortcut_bar_keeps_complete_groups_within_display_width() {
        let hints = [("Enter", "Load"), ("Ctrl+L", "All"), ("Ctrl+H", "Help")];
        for width in 0..120 {
            let line = super::shortcut_bar(width, &hints);
            assert!(line.width() <= usize::from(width));
            let text = line.to_string();
            assert_eq!(text.contains("Ctrl+L"), text.contains("All"));
            assert_eq!(text.contains("Ctrl+H"), text.contains("Help"));
        }
        assert_eq!(super::shortcut_bar(12, &hints).to_string(), " Enter  Load");
        assert!(super::shortcut_bar(11, &hints).spans.is_empty());
        let line = super::shortcut_bar(80, &hints);
        assert_eq!(line.spans[0].style.bg, Some(PRIMARY_COLOR));
        assert_eq!(
            line.spans[3].style.bg,
            Some(ratatui::style::Color::DarkGray)
        );
    }

    #[test]
    fn command_bar_and_copy_controls_fit_narrow_widths() {
        for width in 0..120 {
            let line = super::observer_command_bar(width);
            assert!(line.width() <= usize::from(width));
            let text = line.to_string();
            if width >= 28 {
                assert!(text.contains("Load"));
                assert!(text.contains("Clear") || text.contains("Help"));
            }
            let controls = super::observer_copy_controls(width);
            assert!(controls
                .lines
                .iter()
                .all(|line| line.width() <= usize::from(width)));
            if width >= 14 {
                let text = controls.to_string();
                for label in ["Select", "Copy", "Cancel"] {
                    assert!(text.contains(label), "missing {label} at width {width}");
                }
            }
        }
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
    fn observer_command_bar_shows_primary_actions_on_one_row() {
        for (width, height) in [(120, 24), (80, 24), (80, 12), (40, 24), (40, 12)] {
            let buf = render_observer(&mut AppState::new(UserRole::Admin), width, height);
            for label in ["Load", "Help"] {
                assert!(
                    buffer_contains(&buf, label),
                    "missing {label} at {width}x{height}"
                );
            }
            // Load+Help ≈ 27 cols; Copy needs ≈ 42; Clear/All need ≈ 60+.
            if width >= 50 {
                assert!(
                    buffer_contains(&buf, "Copy"),
                    "missing Copy at {width}x{height}"
                );
            }
            if width >= 70 {
                for label in ["Clear", "All"] {
                    assert!(
                        buffer_contains(&buf, label),
                        "missing {label} at {width}x{height}"
                    );
                }
            }
            assert!(
                !buffer_contains(&buf, "Ctrl+L: Clear all") && !buffer_contains(&buf, "Ctrl+L:All"),
                "stale colon-style clear hint at {width}x{height}"
            );
            assert!(
                !buffer_contains(&buf, "Ctrl+C:Copy") && !buffer_contains(&buf, "Ctrl+C: Copy"),
                "stale colon-style copy hint at {width}x{height}"
            );
        }
    }

    #[test]
    fn observer_paste_and_scroll_hints_stay_on_the_chat_border() {
        let buf = render_observer(&mut AppState::new(UserRole::Admin), 120, 24);
        assert!(
            buffer_contains(&buf, "Paste"),
            "Paste hint must stay on the chat border"
        );
        assert!(
            buffer_contains(&buf, "Scroll"),
            "Scroll hint must stay on the chat border"
        );
        assert!(
            buffer_contains(&buf, "Load"),
            "primary command bar must remain visible"
        );
    }

    /// A narrow-but-tall terminal should still use abbreviated field labels;
    /// the keycap command bar width-truncates whole groups instead of clipping.
    #[test]
    fn observer_tab_narrow_width_uses_abbreviated_labels_and_keycap_bar() {
        let buf = render_observer(&mut AppState::new(UserRole::Admin), 40, 24);

        assert!(
            buffer_contains(&buf, "Shared key (hex)"),
            "narrow layout should use the abbreviated Shared key label"
        );
        assert!(
            !buffer_contains(&buf, "Shared key (64-char hex, read-only grant)"),
            "narrow layout should not use the full-width Shared key label"
        );
        assert!(
            !buffer_contains(&buf, "Ctrl+S: Save attachment"),
            "narrow layout should not attempt to render the old full-width footer"
        );

        for label in ["Load", "Help"] {
            assert!(
                buffer_contains(&buf, label),
                "narrow command bar is missing label: {label}"
            );
        }
        assert!(
            buffer_contains(&buf, "Paste") || buffer_contains(&buf, "Scroll"),
            "narrow chat border should keep at least one contextual hint"
        );
    }
}
