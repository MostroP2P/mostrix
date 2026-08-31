//! Import seed words UI: input popup and destructive confirmation.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::{helpers, KeyInputState, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_import_seed_input(f: &mut ratatui::Frame, key_state: &KeyInputState) {
    key_input_popup_with_wrap(
        f,
        "\u{1F511} Import Seed Words",
        "Paste or type your 12 BIP-39 words (spaces between words):",
        "word1 word2 ... word12",
        key_state,
    );
}

fn key_input_popup_with_wrap(
    f: &mut ratatui::Frame,
    title: &str,
    label: &str,
    placeholder: &str,
    key_state: &KeyInputState,
) {
    let area = f.area();
    let compact = area.height < 14 || area.width < 50;
    let popup_width = area.width.saturating_sub(2).clamp(24, 86);
    let popup_height = if compact {
        10.min(area.height.max(1))
    } else {
        14.min(area.height.max(1))
    };

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    let constraints: Vec<ratatui::layout::Constraint> = if compact {
        vec![
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(3),
            ratatui::layout::Constraint::Length(1),
        ]
    } else {
        vec![
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(2),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(4),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ]
    };
    let chunks = ratatui::layout::Layout::new(ratatui::layout::Direction::Vertical, constraints)
        .split(popup);

    let warning = if compact {
        "Import wipes local session data."
    } else {
        "\u{26A0}  Importing replaces this identity and wipes local orders, chats, and LN address."
    };

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            warning,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

    if !compact {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                label,
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            )]))
            .alignment(ratatui::layout::Alignment::Center),
            chunks[2],
        );
    }

    let input_display = if key_state.key_input.is_empty() {
        placeholder.to_string()
    } else {
        key_state.key_input.clone()
    };

    let input_style = if key_state.focused {
        Style::default()
            .fg(PRIMARY_COLOR)
            .bg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let input_chunk = if compact { chunks[2] } else { chunks[3] };
    f.render_widget(
        Paragraph::new(input_display)
            .style(input_style)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(if key_state.focused {
                        Style::default().fg(PRIMARY_COLOR)
                    } else {
                        Style::default()
                    }),
            ),
        input_chunk,
    );

    let help_chunk = if compact { chunks[3] } else { chunks[5] };
    let help = if compact {
        Line::from(vec![
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" continue · "),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" cancel"),
        ])
    } else {
        Line::from(vec![
            Span::styled("Paste ", Style::default().fg(Color::White)),
            Span::styled(
                "Ctrl+V",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" / right-click, "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to continue, "),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to cancel"),
        ])
    };
    f.render_widget(
        Paragraph::new(help).alignment(ratatui::layout::Alignment::Center),
        help_chunk,
    );
}

pub fn render_confirm_import_seed(f: &mut ratatui::Frame, selected_button: bool) {
    crate::ui::admin_key_confirm::render_admin_key_confirm_with_message(
        f,
        "\u{1F511} Confirm Import Seed",
        "",
        selected_button,
        Some(
            "WARNING: This wipes local users/orders/disputes, chat files, downloads,\n\
and Lightning address, then imports this seed and restores from Mostro.",
        ),
    );
}

#[cfg(test)]
mod tests {
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
    fn import_seed_input_renders_title_and_warning() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = KeyInputState {
            key_input: String::new(),
            focused: true,
            just_pasted: false,
        };
        terminal
            .draw(|f| render_import_seed_input(f, &state))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Import Seed Words"));
        assert!(buffer_contains(buf, "wipes local"));
        assert!(buffer_contains(buf, "Ctrl+V"));
    }

    #[test]
    fn confirm_import_seed_renders_warning() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_confirm_import_seed(f, true))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Confirm Import Seed"));
        assert!(buffer_contains(buf, "WARNING"));
    }

    #[test]
    fn import_seed_input_fits_narrow_short_terminal() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = KeyInputState {
            key_input: String::new(),
            focused: true,
            just_pasted: false,
        };
        terminal
            .draw(|f| render_import_seed_input(f, &state))
            .unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "Import Seed"));
        assert!(buffer_contains(buf, "wipes"));
    }
}
