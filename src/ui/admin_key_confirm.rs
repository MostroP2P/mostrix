use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Renders a generic key confirmation popup
pub fn render_admin_key_confirm(
    f: &mut ratatui::Frame,
    title: &str,
    key_string: &str,
    selected_button: bool,
) {
    render_admin_key_confirm_with_message(f, title, key_string, selected_button, None);
}

/// Renders a generic key confirmation popup with optional custom message
pub fn render_admin_key_confirm_with_message(
    f: &mut ratatui::Frame,
    title: &str,
    key_string: &str,
    selected_button: bool,
    custom_message: Option<&str>,
) {
    let area = f.area();
    let popup_width = 80;
    let popup_height = 12;

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1), // spacer
            Constraint::Length(2), // message (wrapped)
            Constraint::Length(1), // spacer
            Constraint::Length(1), // key display (truncated)
            Constraint::Length(1), // spacer
            Constraint::Length(3), // buttons
            Constraint::Length(1), // help text
        ],
    )
    .split(popup);

    // Confirmation message
    // This popup has a fixed 2-row message area. Rendering the message
    // with wrapping can create extra visual lines and bleed outside the frame.
    let message = custom_message.unwrap_or("Do you want to save this key in settings file?");
    let message_lines: Vec<Line> = message
        .lines()
        .map(|l| Line::from(Span::styled(l, Style::default().fg(Color::White))))
        .collect();
    f.render_widget(
        Paragraph::new(message_lines).alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

    // Display truncated key (show first 30 chars + ...)
    // Only show key if no custom message (for settings saves) or if custom message is provided but we still want to show it
    // For AddSolver, we hide the key display
    if custom_message.is_none() {
        let display_key = if key_string.len() > 30 {
            format!("{}...", &key_string[..30])
        } else {
            key_string.to_string()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Key: ", Style::default()),
                Span::styled(
                    display_key,
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[3],
        );
    }

    // Yes/No buttons
    let button_area = chunks[5];
    let button_width = 15;
    let separator_width = 1;
    let total_button_width = (button_width * 2) + separator_width;

    let button_x = button_area.x + (button_area.width.saturating_sub(total_button_width)) / 2;
    let centered_button_area = Rect {
        x: button_x,
        y: button_area.y,
        width: total_button_width.min(button_area.width),
        height: button_area.height,
    };

    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(button_width),
            Constraint::Length(separator_width),
            Constraint::Length(button_width),
        ],
    )
    .split(centered_button_area);

    // YES button
    let yes_style = if selected_button {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };

    let yes_block = Block::default().borders(Borders::ALL).style(yes_style);
    f.render_widget(yes_block, button_chunks[0]);

    let yes_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[0]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✓ YES",
            Style::default()
                .fg(if selected_button {
                    Color::Black
                } else {
                    Color::Green
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        yes_inner[0],
    );

    // NO button
    let no_style = if !selected_button {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };

    let no_block = Block::default().borders(Borders::ALL).style(no_style);
    f.render_widget(no_block, button_chunks[2]);

    let no_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[2]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✗ NO",
            Style::default()
                .fg(if !selected_button {
                    Color::Black
                } else {
                    Color::Red
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        no_inner[0],
    );

    // Help text - combine all messages into a single Paragraph
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Use ", Style::default()),
            Span::styled(
                "Left/Right",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to select, ", Style::default()),
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to confirm, ", Style::default()),
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to cancel", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[6],
    );
}

/// Confirm Shift+R recovery of the selected orphan dispute IDs.
pub fn render_recover_taken_disputes_confirm(
    f: &mut ratatui::Frame,
    recover_ids: &[uuid::Uuid],
    selected_button: bool,
) {
    let count = recover_ids.len();
    let area = f.area();
    let popup_width = 72.min(area.width);
    let popup_height = 16.min(area.height);
    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("🔄 Recover Taken Disputes")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let compact = inner.width < 40 || inner.height < 10;
    let ultra_compact = inner.height < 6;
    let constraints: &[Constraint] = if ultra_compact {
        &[Constraint::Min(1), Constraint::Length(3)]
    } else if compact {
        &[
            Constraint::Min(2),
            Constraint::Length(3),
            Constraint::Length(1),
        ]
    } else {
        &[
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ]
    };
    let chunks = Layout::new(Direction::Vertical, constraints).split(inner);
    let (body_area, button_area, help_area) = if ultra_compact {
        (chunks[0], chunks[1], None)
    } else if compact {
        (chunks[0], chunks[1], Some(chunks[2]))
    } else {
        (chunks[1], chunks[3], Some(chunks[4]))
    };

    let noun = if count == 1 { "dispute" } else { "disputes" };
    let preview: String = recover_ids
        .iter()
        .take(3)
        .map(|id| {
            let s = id.to_string();
            if s.len() > 13 {
                format!("{}…{}", &s[..8], &s[s.len().saturating_sub(4)..])
            } else {
                s
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let more = if count > 3 {
        format!(" (+{})", count - 3)
    } else {
        String::new()
    };
    let body = if compact {
        vec![Line::from(Span::styled(
            format!("📡 Re-request AdminTookDispute for {count} selected {noun}?"),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))]
    } else {
        vec![
            Line::from(Span::styled(
                format!("📡 Recover {count} selected {noun}"),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("🆔 {preview}{more}"),
                Style::default().fg(Color::Gray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "✨ Mostro will accept only if this admin owns them",
                Style::default().fg(PRIMARY_COLOR),
            )),
        ]
    };
    f.render_widget(
        Paragraph::new(body)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true }),
        body_area,
    );

    let button_width = if compact { 8u16 } else { 15 };
    let separator_width = 1;
    let total_button_width = (button_width * 2) + separator_width;
    let button_x = button_area.x + (button_area.width.saturating_sub(total_button_width)) / 2;
    let centered_button_area = Rect {
        x: button_x,
        y: button_area.y,
        width: total_button_width.min(button_area.width),
        height: button_area.height,
    };
    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(button_width),
            Constraint::Length(separator_width),
            Constraint::Length(button_width),
        ],
    )
    .split(centered_button_area);

    let yes_style = if selected_button {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };
    f.render_widget(
        Block::default().borders(Borders::ALL).style(yes_style),
        button_chunks[0],
    );
    let yes_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[0]);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✓ YES",
            Style::default()
                .fg(if selected_button {
                    Color::Black
                } else {
                    Color::Green
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        yes_inner[0],
    );

    let no_style = if !selected_button {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };
    f.render_widget(
        Block::default().borders(Borders::ALL).style(no_style),
        button_chunks[2],
    );
    let no_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[2]);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✗ NO",
            Style::default()
                .fg(if !selected_button {
                    Color::Black
                } else {
                    Color::Red
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        no_inner[0],
    );

    if let Some(help_area) = help_area {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Use ", Style::default()),
                Span::styled(
                    "Left/Right",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to select, ", Style::default()),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to confirm, ", Style::default()),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to cancel", Style::default()),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            help_area,
        );
    }
}

/// Confirmation before AddInvoice when Settings contain a buyer Lightning address (taller body + wrap).
pub fn render_saved_ln_address_invoice_confirm(
    f: &mut ratatui::Frame,
    selected_button: bool,
    body: &str,
) {
    let area = f.area();
    let popup_width = 82;
    let popup_height = 17;
    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("⚡ Use saved Lightning address?")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1),
            Constraint::Length(9),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ],
    )
    .split(popup);

    f.render_widget(
        Paragraph::new(body)
            .style(Style::default().fg(Color::White))
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true }),
        chunks[1],
    );

    let button_area = chunks[3];
    let button_width = 15;
    let separator_width = 1;
    let total_button_width = (button_width * 2) + separator_width;

    let button_x = button_area.x + (button_area.width.saturating_sub(total_button_width)) / 2;
    let centered_button_area = Rect {
        x: button_x,
        y: button_area.y,
        width: total_button_width.min(button_area.width),
        height: button_area.height,
    };

    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(button_width),
            Constraint::Length(separator_width),
            Constraint::Length(button_width),
        ],
    )
    .split(centered_button_area);

    let yes_style = if selected_button {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };

    let yes_block = Block::default().borders(Borders::ALL).style(yes_style);
    f.render_widget(yes_block, button_chunks[0]);

    let yes_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[0]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✓ YES",
            Style::default()
                .fg(if selected_button {
                    Color::Black
                } else {
                    Color::Green
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        yes_inner[0],
    );

    let no_style = if !selected_button {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };

    let no_block = Block::default().borders(Borders::ALL).style(no_style);
    f.render_widget(no_block, button_chunks[2]);

    let no_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[2]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "✗ NO",
            Style::default()
                .fg(if !selected_button {
                    Color::Black
                } else {
                    Color::Red
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        no_inner[0],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Use ", Style::default()),
            Span::styled(
                "Left/Right",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to select, ", Style::default()),
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to confirm, ", Style::default()),
            Span::styled("Press ", Style::default()),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to cancel", Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );
}

#[cfg(test)]
mod tests {
    use super::render_recover_taken_disputes_confirm;
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
    fn recover_confirm_shows_centered_count_and_actions() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let ids: Vec<uuid::Uuid> = (0..39).map(|n| uuid::Uuid::from_u128(n + 1)).collect();
        terminal
            .draw(|f| render_recover_taken_disputes_confirm(f, &ids, true))
            .expect("draw");
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Recover Taken Disputes"));
        assert!(buffer_contains(buf, "Recover 39 selected disputes"));
        assert!(buffer_contains(buf, "YES"));
        assert!(buffer_contains(buf, "NO"));
    }

    #[test]
    fn recover_confirm_keeps_actions_visible_on_narrow_short_terminal() {
        let backend = TestBackend::new(30, 8);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let ids = vec![uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(2)];
        terminal
            .draw(|f| render_recover_taken_disputes_confirm(f, &ids, true))
            .expect("draw");
        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "YES"),
            "selected YES action must stay visible on 30x8"
        );
    }
}
