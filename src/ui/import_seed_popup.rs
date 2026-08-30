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
    let popup_width = 86;
    let popup_height = 14;

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    let chunks = ratatui::layout::Layout::new(
        ratatui::layout::Direction::Vertical,
        [
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(2),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(4),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
            ratatui::layout::Constraint::Length(1),
        ],
    )
    .split(popup);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "\u{26A0}  Importing replaces this identity and wipes local orders, chats, and LN address.",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

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
        chunks[3],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Press ", Style::default().fg(Color::White)),
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
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[5],
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
}
