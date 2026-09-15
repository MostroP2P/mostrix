use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
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
    let message = custom_message.unwrap_or("Do you want to save this key in settings file?");
    let mut body: Vec<Line<'static>> = message
        .lines()
        .map(|l| {
            Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(Color::White),
            ))
        })
        .collect();
    let mut compact_body = body.clone();

    // The key is only shown for plain settings saves (no custom message).
    if custom_message.is_none() {
        let display_key = if key_string.len() > 30 {
            format!("{}...", &key_string[..30])
        } else {
            key_string.to_string()
        };
        let key_line = Line::from(vec![
            Span::styled("Key: ", Style::default()),
            Span::styled(
                display_key,
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        body.push(Line::from(""));
        body.push(key_line.clone());
        compact_body.push(key_line);
    }

    render_yes_no_confirm(f, title, (80, 12), body, compact_body, selected_button);
}

/// Confirm Shift+R recovery of the selected orphan dispute IDs.
pub fn render_recover_taken_disputes_confirm(
    f: &mut ratatui::Frame,
    recover_ids: &[uuid::Uuid],
    selected_button: bool,
) {
    let count = recover_ids.len();
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
    let body = vec![
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
    ];
    let compact_body = vec![Line::from(Span::styled(
        format!("📡 Re-request AdminTookDispute for {count} selected {noun}?"),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ))];

    render_yes_no_confirm(
        f,
        "🔄 Recover Taken Disputes",
        (72, 16),
        body,
        compact_body,
        selected_button,
    );
}

/// Confirmation before AddInvoice when Settings contain a buyer Lightning address (taller body + wrap).
pub fn render_saved_ln_address_invoice_confirm(
    f: &mut ratatui::Frame,
    selected_button: bool,
    body: &str,
) {
    let lines: Vec<Line<'static>> = body
        .lines()
        .map(|l| {
            Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(Color::White),
            ))
        })
        .collect();
    // Compact: drop the blank separators so the address and question keep the rows.
    let compact: Vec<Line<'static>> = lines.iter().filter(|l| l.width() > 0).cloned().collect();
    render_yes_no_confirm(
        f,
        "⚡ Use saved Lightning address?",
        (82, 17),
        lines,
        compact,
        selected_button,
    );
}

const CONFIRM_BUTTON_WIDTH: u16 = 15;
const CONFIRM_COMPACT_BUTTON_WIDTH: u16 = 8;
/// Below this inner width the two 15-column buttons (plus margin) do not fit.
const CONFIRM_COMPACT_MIN_WIDTH: u16 = 34;

/// Shared YES/NO confirmation layout that degrades on small terminals.
///
/// * **Full** — spacer, `body`, spacer, buttons, full help (wrapped). The popup
///   grows past `base_height` when the wrapped body needs it.
/// * **Compact** (inner width < 34 or not tall enough for full) — `compact_body`,
///   narrow buttons, short key hint (wrapped onto a second row only if needed).
/// * **Ultra compact** (inner height < 5) — `compact_body` and buttons only, so
///   the selectable controls are never the part that gets clipped.
///
/// Everything is laid out inside the border (`block.inner`), so long text wraps
/// instead of overwriting the frame.
fn render_yes_no_confirm(
    f: &mut ratatui::Frame,
    title: &str,
    (max_width, base_height): (u16, u16),
    body: Vec<Line<'static>>,
    compact_body: Vec<Line<'static>>,
    selected_button: bool,
) {
    let area = f.area();
    let popup_width = max_width.min(area.width);
    let inner_width = popup_width.saturating_sub(2);
    let full_help = confirm_help_line(false);
    let body_rows = helpers::wrapped_rows(&body, inner_width);
    let full_help_rows = helpers::wrapped_rows(std::slice::from_ref(&full_help), inner_width);
    // spacer + body + spacer + buttons + help
    let full_inner_needed = 1 + body_rows + 1 + 3 + full_help_rows;
    let popup_height = base_height
        .max(full_inner_needed.saturating_add(2))
        .min(area.height);

    let popup = helpers::create_centered_popup(area, popup_width, popup_height);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let compact = inner.width < CONFIRM_COMPACT_MIN_WIDTH || inner.height < full_inner_needed;
    let ultra_compact = inner.height < 5;

    let (body_area, button_area, help) = if ultra_compact {
        let [b, btn] = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(inner);
        (b, btn, None)
    } else if compact {
        let short_help = confirm_help_line(true);
        // A second hint row only when it does not steal a row from the body.
        let compact_body_rows = helpers::wrapped_rows(&compact_body, inner.width).max(1);
        let help_rows = if inner.height >= compact_body_rows + 3 + 2 {
            helpers::wrapped_rows(std::slice::from_ref(&short_help), inner.width).clamp(1, 2)
        } else {
            1
        };
        let [b, btn, h] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(help_rows),
        ])
        .areas(inner);
        (b, btn, Some((h, short_help)))
    } else {
        let [_, b, _, btn, h] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(body_rows),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(full_help_rows),
        ])
        .areas(inner);
        (b, btn, Some((h, full_help)))
    };

    let shown_body = if compact { compact_body } else { body };
    f.render_widget(
        Paragraph::new(Text::from(shown_body))
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true }),
        body_area,
    );

    let button_width = if compact {
        CONFIRM_COMPACT_BUTTON_WIDTH
    } else {
        CONFIRM_BUTTON_WIDTH
    };
    helpers::render_yes_no_buttons_with_width(
        f,
        button_area,
        button_width,
        selected_button,
        "✓ YES",
        "✗ NO",
    );

    if let Some((help_area, help_line)) = help {
        f.render_widget(
            Paragraph::new(help_line)
                .alignment(ratatui::layout::Alignment::Center)
                .wrap(Wrap { trim: true }),
            help_area,
        );
    }
}

/// Key hint under the buttons; `compact` fits in 38 columns.
fn confirm_help_line(compact: bool) -> Line<'static> {
    let key = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);
    if compact {
        Line::from(vec![
            Span::styled("←/→", key),
            Span::styled(" select · ", Style::default()),
            Span::styled("Enter", key),
            Span::styled(" ok · ", Style::default()),
            Span::styled("Esc", key),
            Span::styled(" cancel", Style::default()),
        ])
    } else {
        Line::from(vec![
            Span::styled("Use ", Style::default()),
            Span::styled("Left/Right", key),
            Span::styled(" to select, ", Style::default()),
            Span::styled("Enter", key),
            Span::styled(" to confirm, ", Style::default()),
            Span::styled("Esc", key),
            Span::styled(" to cancel", Style::default()),
        ])
    }
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

    fn draw(
        width: u16,
        height: u16,
        render: impl FnOnce(&mut ratatui::Frame),
    ) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(render).expect("draw");
        terminal.backend().buffer().clone()
    }

    /// Popup text must stay inside the frame: every row between the top and
    /// bottom corners starts and ends with a border glyph.
    fn assert_frame_intact(buf: &ratatui::buffer::Buffer, label: &str) {
        let width = buf.area.width;
        let rows: Vec<u16> = (0..buf.area.height)
            .filter(|&y| (0..width).any(|x| buf[(x, y)].symbol() != " "))
            .collect();
        let (top, bottom) = (rows[0], *rows.last().unwrap());
        let left = (0..width)
            .find(|&x| buf[(x, top)].symbol() == "┌")
            .expect("top-left corner");
        let right = (0..width)
            .rev()
            .find(|&x| buf[(x, top)].symbol() == "┐")
            .expect("top-right corner");
        for y in top + 1..bottom {
            assert_eq!(
                buf[(left, y)].symbol(),
                "│",
                "{label}: left border overwritten on row {y}"
            );
            assert_eq!(
                buf[(right, y)].symbol(),
                "│",
                "{label}: right border overwritten on row {y}"
            );
        }
    }

    #[test]
    fn restore_session_confirm_fits_small_terminals() {
        let question =
            "Ask Mostro to restore this identity's orders and disputes into the local database?";
        for (width, height) in [(40u16, 12u16), (40, 24)] {
            let buf = draw(width, height, |f| {
                super::render_admin_key_confirm_with_message(
                    f,
                    "Restore Session",
                    "",
                    true,
                    Some(question),
                )
            });
            let label = format!("restore confirm {width}x{height}");
            assert_frame_intact(&buf, &label);
            // The wrapped question is shown in full, not truncated.
            assert!(buffer_contains(&buf, "Ask Mostro"), "{label}");
            assert!(buffer_contains(&buf, "database?"), "{label}");
            assert!(
                buffer_contains(&buf, "YES") && buffer_contains(&buf, "NO"),
                "{label}"
            );
            assert!(buffer_contains(&buf, "Esc"), "{label}: key hint missing");
        }
    }

    #[test]
    fn key_confirm_keeps_key_buttons_and_hint_on_small_terminals() {
        for (width, height) in [(40u16, 12u16), (40, 24)] {
            let buf = draw(width, height, |f| {
                super::render_admin_key_confirm(
                    f,
                    "Confirm Relay",
                    "wss://relay.mostro.network",
                    true,
                )
            });
            let label = format!("key confirm {width}x{height}");
            assert_frame_intact(&buf, &label);
            assert!(
                buffer_contains(&buf, "wss://relay.mostro.network"),
                "{label}"
            );
            assert!(
                buffer_contains(&buf, "YES") && buffer_contains(&buf, "NO"),
                "{label}"
            );
            assert!(buffer_contains(&buf, "Esc"), "{label}");
        }
    }

    #[test]
    fn saved_ln_address_confirm_keeps_address_and_actions_on_small_terminals() {
        let body = "Saved Lightning address:\nyou@wallet.example.com\n\n\
Confirm using this address from Settings as your invoice?";
        for (width, height) in [(40u16, 12u16), (40, 24)] {
            let buf = draw(width, height, |f| {
                super::render_saved_ln_address_invoice_confirm(f, false, body)
            });
            let label = format!("saved ln confirm {width}x{height}");
            assert_frame_intact(&buf, &label);
            assert!(buffer_contains(&buf, "you@wallet.example.com"), "{label}");
            assert!(
                buffer_contains(&buf, "YES") && buffer_contains(&buf, "NO"),
                "{label}"
            );
        }
    }
}
