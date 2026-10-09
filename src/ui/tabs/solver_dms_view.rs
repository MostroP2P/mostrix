//! Read-only "SERBERO" pane of Disputes In Progress: the messages trusted
//! assistants sent about the selected dispute, newest first, with handoffs
//! and failed openings marked because a person has to act on them.

use crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::ui::constants::CHAT_COPY_START;
use crate::ui::helpers::{format_local_timestamp, ChatScrollViewContent};
use crate::ui::key_handler::chat_copy;
use crate::ui::{AppState, ChatParty, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::util::solver_dms::SolverDm;

/// Lines a PageUp / PageDown moves the pane.
const PAGE_LINES: u16 = 10;
/// Narrower panes drop timestamps and the long title, keeping subjects readable.
const WIDE_PANE_WIDTH: u16 = 50;
/// Marks messages that ask a person to take the dispute over.
const ACTION_MARKER: &str = "⚠ ";

/// Label of the pane's tab, with the message count when there are any.
pub fn solver_dms_tab_label(count: usize) -> String {
    if count == 0 {
        "SERBERO".to_string()
    } else {
        format!("SERBERO ({count})")
    }
}

/// Newest message first; each shows its time (dropped when `compact`) and
/// subject, then the body.
pub fn solver_dm_lines(dms: &[SolverDm], compact: bool) -> Vec<Line<'static>> {
    solver_dm_content(dms, compact, 1).lines
}

fn solver_dm_content(dms: &[SolverDm], compact: bool, width: u16) -> ChatScrollViewContent {
    let mut lines = Vec::new();
    let mut starts = Vec::new();
    for dm in dms.iter().rev() {
        starts.push(lines.len());
        let at = format_local_timestamp(dm.created_at, "%Y-%m-%d %H:%M")
            .unwrap_or_else(|| "unknown time".to_string());
        let (marker, subject_style) = if dm.needs_action() {
            (
                ACTION_MARKER,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (
                "",
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            )
        };
        let subject = Span::styled(format!("{marker}{}", dm.subject), subject_style);
        lines.push(if compact {
            Line::from(subject)
        } else {
            Line::from(vec![
                Span::styled(format!("[{at}] "), Style::default().fg(Color::Gray)),
                subject,
            ])
        });
        // The first line is the `Dispute <id> · <subject>` header shown above.
        lines.extend(dm.text.lines().skip(1).map(|l| Line::raw(l.to_string())));
        lines.push(Line::raw(""));
    }
    ChatScrollViewContent {
        content_height: lines.len().min(u16::MAX as usize) as u16,
        content_width: width.max(1),
        lines,
        line_start_per_message: starts,
    }
}

/// Renders the pane for `dispute_id` into `area`.
pub fn render_solver_dms(f: &mut ratatui::Frame, area: Rect, app: &mut AppState, dispute_id: &str) {
    chat_copy::validate_selection(app);
    let selection = chat_copy::selected_index(app).filter(|_| {
        app.admin_show_solver_dms && app.selected_dispute_id.as_deref() == Some(dispute_id)
    });
    if app.solver_dm_scroll_dispute.as_deref() != Some(dispute_id) {
        app.solver_dm_scroll = 0;
        app.solver_dm_scroll_dispute = Some(dispute_id.to_string());
    }
    let dms = app
        .solver_dms
        .get(dispute_id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let noun = if dms.len() == 1 {
        "message"
    } else {
        "messages"
    };
    let wide = area.width >= WIDE_PANE_WIDTH;
    let title = if selection.is_some() {
        "Copy: Serbero".to_string()
    } else if wide {
        format!(
            "Serbero · {} {noun} · newest first · {CHAT_COPY_START}",
            dms.len()
        )
    } else {
        format!("Serbero ({}) | {CHAT_COPY_START}", dms.len())
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));

    if dms.is_empty() {
        app.solver_dm_scroll = 0;
        let empty = Paragraph::new(vec![
            Line::styled(
                "No assistant messages for this dispute.",
                Style::default().fg(Color::Yellow),
            ),
            Line::raw("Messages come from the trusted_dm_senders in settings.toml."),
        ])
        .wrap(Wrap { trim: true })
        .block(block);
        f.render_widget(empty, area);
        return;
    }

    let inner = block.inner(area);
    let mut content = solver_dm_content(dms, !wide, inner.width);
    let selected_rows = content.select_message_with_wrap(selection, Wrap { trim: false });
    // Scroll counts wrapped rows: stop once the last row reaches the bottom.
    let rows = usize::from(content.content_height);
    let max_scroll = rows.saturating_sub(usize::from(inner.height));
    app.solver_dm_scroll = app
        .solver_dm_scroll
        .min(u16::try_from(max_scroll).unwrap_or(u16::MAX));
    if let Some(selected_rows) = selected_rows {
        app.solver_dm_scroll =
            content.selection_scroll_offset(selected_rows, inner.height, app.solver_dm_scroll);
    }
    let pane = Paragraph::new(content.lines).wrap(Wrap { trim: false });
    f.render_widget(block, area);
    f.render_widget(pane.scroll((app.solver_dm_scroll, 0)), inner);
}

/// Scrolls the pane; `End` returns to the newest message. Returns `true`
/// when the key was used.
pub fn scroll_solver_dms(app: &mut AppState, code: KeyCode) -> bool {
    if !app.admin_show_solver_dms {
        return false;
    }
    app.solver_dm_scroll = match code {
        KeyCode::PageDown => app.solver_dm_scroll.saturating_add(PAGE_LINES),
        KeyCode::PageUp => app.solver_dm_scroll.saturating_sub(PAGE_LINES),
        KeyCode::End => 0,
        _ => return false,
    };
    true
}

/// Tab order of the dispute panes: BUYER → SELLER → SERBERO → BUYER.
/// Returns the party chat to show and whether the SERBERO pane is on.
pub fn next_dispute_pane(party: ChatParty, serbero: bool, forward: bool) -> (ChatParty, bool) {
    match (party, serbero, forward) {
        (_, true, true) => (ChatParty::Buyer, false),
        (_, true, false) => (ChatParty::Seller, false),
        (ChatParty::Buyer, false, true) => (ChatParty::Seller, false),
        (ChatParty::Seller, false, true) => (ChatParty::Seller, true),
        (ChatParty::Buyer, false, false) => (ChatParty::Seller, true),
        (ChatParty::Seller, false, false) => (ChatParty::Buyer, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UserRole;
    use crossterm::event::{KeyEvent, KeyModifiers};
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

    fn dm(event_id: &str, subject: &str, text: &str, created_at: i64) -> SolverDm {
        SolverDm {
            event_id: event_id.into(),
            sender_pubkey: String::new(),
            recipient_pubkey: String::new(),
            dispute_id: Some("d1".into()),
            subject: subject.into(),
            text: text.into(),
            created_at,
        }
    }

    fn render(app: &mut AppState, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let dispute_id = app
            .selected_dispute_id
            .clone()
            .unwrap_or_else(|| "d1".into());
        terminal
            .draw(|f| render_solver_dms(f, f.area(), app, &dispute_id))
            .unwrap();
        terminal.backend().buffer().clone()
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

    fn copy_key(app: &mut AppState, code: KeyCode, modifiers: KeyModifiers) {
        assert!(chat_copy::handle_key_with(
            app,
            &KeyEvent::new(code, modifiers),
            |_| false
        ));
    }

    #[test]
    fn solver_dm_copy_highlight_survives_arrival_navigation_and_resize() {
        let mut app = chat_copy::tests::app_with_solver_dms();
        copy_key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        for (width, height) in [(80, 12), (40, 8), (30, 5), (80, 12)] {
            let buffer = render(&mut app, width, height);
            assert!(
                highlighted(&buffer, "newest"),
                "missing selection at {width}x{height}"
            );
            assert!(buffer_contains(&buffer, "Copy: Serbero"));
        }
        let mut incoming = app.solver_dms["dispute"][1].clone();
        incoming.event_id = "incoming".into();
        incoming.text = "header\nincoming".into();
        app.solver_dms.get_mut("dispute").unwrap().push(incoming);
        let buffer = render(&mut app, 30, 5);
        assert!(highlighted(&buffer, "newest"));
        assert_eq!(chat_copy::selected_index(&app), Some(1));
        copy_key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        let buffer = render(&mut app, 30, 5);
        assert!(highlighted(&buffer, "older"));
        assert!(app.solver_dm_scroll > 0);
        copy_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        let buffer = render(&mut app, 30, 5);
        assert!(!highlighted(&buffer, "older"));
        assert!(scroll_solver_dms(&mut app, KeyCode::End));
        assert_eq!(app.solver_dm_scroll, 0);
    }

    #[test]
    fn solver_dm_copy_preserves_wrapping_and_handles_oversized_messages() {
        let mut app = chat_copy::tests::app_with_solver_dms();
        app.solver_dms.get_mut("dispute").unwrap()[1].text =
            format!("header\n    selected {}", "wide text ".repeat(60));
        copy_key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        for (width, height) in [(80, 12), (40, 8), (30, 5)] {
            let buffer = render(&mut app, width, height);
            assert!(highlighted(&buffer, "selected"));
            assert_eq!(app.solver_dm_scroll, 0);
        }
        for (width, height) in [(0, 0), (1, 1), (8, 3)] {
            render(&mut app, width, height);
        }
        let mut content = solver_dm_content(&app.solver_dms["dispute"], true, 12);
        let expected_rows = Paragraph::new(content.lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(12);
        content.select_message_with_wrap(Some(0), Wrap { trim: false });
        assert_eq!(usize::from(content.content_height), expected_rows);
    }

    #[test]
    fn the_tab_label_counts_messages() {
        assert_eq!(solver_dms_tab_label(0), "SERBERO");
        assert_eq!(solver_dms_tab_label(3), "SERBERO (3)");
    }

    #[test]
    fn lines_show_the_newest_message_first_without_repeating_the_header() {
        let dms = [
            dm(
                "a",
                "mediating",
                "Dispute d1 · mediating\nSerbero is mediating",
                100,
            ),
            dm(
                "b",
                "handed off: fraud_signal",
                "Dispute d1 · handed off: fraud_signal\nBuyer — says sent",
                200,
            ),
        ];

        let text: Vec<String> = solver_dm_lines(&dms, false)
            .iter()
            .map(|l| l.to_string())
            .collect();

        let handoff = text
            .iter()
            .position(|l| l.contains("⚠ handed off: fraud_signal"))
            .unwrap();
        let mediating = text
            .iter()
            .position(|l| l.contains("mediating") && !l.contains("Serbero"))
            .unwrap();
        assert!(handoff < mediating, "{text:?}");
        assert!(text.iter().any(|l| l == "Buyer — says sent"));
        assert!(
            !text.iter().any(|l| l.starts_with("Dispute d1 ·")),
            "{text:?}"
        );
    }

    #[test]
    fn the_pane_shows_the_dispute_messages() {
        let mut app = AppState::new(UserRole::Admin);
        app.solver_dms.insert(
            "d1".into(),
            vec![dm(
                "b",
                "handed off: conflicting_claims",
                "Dispute d1 · handed off: conflicting_claims\nTopic: payment",
                200,
            )],
        );

        let buf = render(&mut app, 70, 12);

        assert!(buffer_contains(&buf, "handed off: conflicting_claims"));
        assert!(buffer_contains(&buf, "Topic: payment"));
        assert!(buffer_contains(&buf, "1 message"));
    }

    #[test]
    fn an_empty_pane_explains_where_messages_come_from() {
        let mut app = AppState::new(UserRole::Admin);

        let buf = render(&mut app, 80, 8);

        assert!(buffer_contains(&buf, "No assistant messages"));
        assert!(buffer_contains(&buf, "trusted_dm_senders"));
    }

    #[test]
    fn a_narrow_short_pane_still_shows_the_subject() {
        // Narrow panes drop the timestamp so the subject fits on one line.
        let mut app = AppState::new(UserRole::Admin);
        app.solver_dms.insert(
            "d1".into(),
            vec![dm(
                "b",
                "handed off: x",
                "Dispute d1 · handed off: x\nbody",
                200,
            )],
        );

        let buf = render(&mut app, 30, 5);

        assert!(buffer_contains(&buf, "handed off"));
    }

    #[test]
    fn switching_disputes_starts_at_the_newest_message() {
        let mut app = AppState::new(UserRole::Admin);
        let many = (0..30)
            .map(|i| dm(&format!("e{i}"), "update", "Dispute d1 · update\nline", i))
            .collect::<Vec<_>>();
        app.solver_dms.insert("d1".into(), many.clone());
        app.solver_dms.insert("d2".into(), many);
        render(&mut app, 70, 12);
        app.solver_dm_scroll = 20;

        let mut terminal = Terminal::new(TestBackend::new(70, 12)).unwrap();
        terminal
            .draw(|f| render_solver_dms(f, f.area(), &mut app, "d2"))
            .unwrap();

        assert_eq!(app.solver_dm_scroll, 0);
    }

    #[test]
    fn paging_scrolls_and_end_returns_to_the_newest() {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_show_solver_dms = true;

        assert!(scroll_solver_dms(&mut app, KeyCode::PageDown));
        assert_eq!(app.solver_dm_scroll, PAGE_LINES);
        assert!(scroll_solver_dms(&mut app, KeyCode::PageUp));
        assert!(scroll_solver_dms(&mut app, KeyCode::PageUp));
        assert_eq!(app.solver_dm_scroll, 0);
        app.solver_dm_scroll = 25;
        assert!(scroll_solver_dms(&mut app, KeyCode::End));
        assert_eq!(app.solver_dm_scroll, 0);
    }

    #[test]
    fn the_end_of_a_long_wrapped_message_is_reachable() {
        let mut app = AppState::new(UserRole::Admin);
        let body = format!("Dispute d1 · transcript\n{}TAIL", "word ".repeat(120));
        app.solver_dms
            .insert("d1".into(), vec![dm("t", "transcript", &body, 1)]);
        render(&mut app, 30, 8);
        app.solver_dm_scroll = u16::MAX;

        let buf = render(&mut app, 30, 8);

        assert!(
            buffer_contains(&buf, "TAIL"),
            "last wrapped row must be visible"
        );
        assert!(
            app.solver_dm_scroll > 2,
            "scroll kept past the logical line count"
        );
    }

    #[test]
    fn scrolling_is_ignored_while_a_party_chat_is_shown() {
        let mut app = AppState::new(UserRole::Admin);

        assert!(!scroll_solver_dms(&mut app, KeyCode::PageDown));
        assert_eq!(app.solver_dm_scroll, 0);
    }

    #[test]
    fn tab_cycles_buyer_seller_serbero() {
        assert_eq!(
            next_dispute_pane(ChatParty::Buyer, false, true),
            (ChatParty::Seller, false)
        );
        assert_eq!(
            next_dispute_pane(ChatParty::Seller, false, true),
            (ChatParty::Seller, true)
        );
        assert_eq!(
            next_dispute_pane(ChatParty::Seller, true, true),
            (ChatParty::Buyer, false)
        );
        assert_eq!(
            next_dispute_pane(ChatParty::Buyer, false, false),
            (ChatParty::Seller, true)
        );
        assert_eq!(
            next_dispute_pane(ChatParty::Seller, true, false),
            (ChatParty::Seller, false)
        );
        assert_eq!(
            next_dispute_pane(ChatParty::Seller, false, false),
            (ChatParty::Buyer, false)
        );
    }
}
