use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::{helpers, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_confirm_generate_new_keys(
    f: &mut ratatui::Frame,
    is_admin_mode: bool,
    selected_button: bool,
) {
    let role = if is_admin_mode { "Admin" } else { "User" };
    // `admin_key_confirm` uses a fixed-height message area (2 rows).
    // Keep the warning to exactly 2 short lines to avoid visual overflow/glitches.
    // Keep it plain ASCII/short lines: emoji glyphs can cause variable-width
    // rendering artefacts when this popup message is constrained to 2 rows.
    let custom_message = if is_admin_mode {
        "WARNING: Generating new Admin keys will change your identity.\n\
Save backup and restart Mostrix after saving."
            .to_string()
    } else {
        "WARNING: Generating new User keys will change your identity.\n\
Save backup and restart Mostrix after saving."
            .to_string()
    };

    // Reuse the generic YES/NO confirmation popup.
    crate::ui::admin_key_confirm::render_admin_key_confirm_with_message(
        f,
        &format!("Generate {} Keys", role),
        "",
        selected_button,
        Some(custom_message.as_str()),
    );
}

pub fn render_backup_new_keys(f: &mut ratatui::Frame, mnemonic: &str) {
    let area = f.area();
    let popup_width = 90u16;
    // Needs to fit: comment (2 lines) + mnemonic (1 line) + help line.
    let popup_height = 20u16;

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            "🧾 Save Backup",
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Render mnemonic as a single line (space-separated words).
    // Important: disable wrapping to avoid any clipping artefacts.
    let mnemonic_lines = mnemonic.split_whitespace().collect::<Vec<_>>().join(" ");

    let comment_text =
        "Write down these 12 words and keep them safe.\nYou will need them to restore keys.";

    // Fixed layout: 2 lines comment, 1 line mnemonic, 1 line help.
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(2), // top padding
            Constraint::Length(2), // comment (2 lines)
            Constraint::Length(2), // spacer
            Constraint::Length(2), // mnemonic (single line)
            Constraint::Min(0),    // remaining spacing
            Constraint::Length(1), // help
        ],
    )
    .split(inner);

    f.render_widget(
        Paragraph::new(comment_text)
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::White)),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new(mnemonic_lines)
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::White)),
        chunks[3],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Press ", Style::default().fg(Color::White)),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" or "),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to close"),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[5],
    );
}
