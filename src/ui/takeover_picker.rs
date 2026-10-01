//! Ctrl+T take-over picker: disputes Serbero told this solver about.
//!
//! Serbero (a read-only solver) takes disputes to mediate them and DMs the
//! solvers about each one (see [`crate::util::solver_dms`]). A write solver
//! can take such a dispute over; mostrod decides, refusing when the current
//! holder can write. The picker lists disputes with an assistant message in
//! the last [`TAKEOVER_WINDOW_SECS`] that the relay shows `in-progress` and
//! that are not already in the local DB, handoffs first. The handoffs alone
//! ([`handoff_candidates`]) feed the Disputes Pending banner and tab badge.

use std::collections::HashSet;
use std::str::FromStr;

use mostro_core::prelude::{Dispute, DisputeStatus};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use uuid::Uuid;

use super::helpers;
use super::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::util::solver_dms::SolverDmsByDispute;

/// How recent an assistant message must be for its dispute to be offered.
pub const TAKEOVER_WINDOW_SECS: i64 = 12 * 3600;

/// Rows at least this wide show the full dispute id and the message age.
/// (36-char id + subject + age ≈ 86 columns with the highlight symbol).
const WIDE_PICKER_WIDTH: u16 = 86;

/// Narrower rows put the subject above the short id so it is never pushed out.
const STACKED_PICKER_WIDTH: u16 = 32;

pub const TAKEOVER_PICKER_HINT: &str = "↑↓ Navigate  Enter Take over  Esc Cancel";

/// Keys-only hint for rows too narrow for [`TAKEOVER_PICKER_HINT`].
const TAKEOVER_PICKER_HINT_COMPACT: &str = "↑↓  Enter  Esc";

/// A dispute the solver may take over, with the assistant's latest word on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeoverCandidate {
    pub dispute_id: Uuid,
    /// Subject of the newest assistant message, e.g. `handed off: …`.
    pub subject: String,
    pub last_message_at: i64,
    /// Subject of the newest message in the window that asks a person to act
    /// (a handoff or failed opening), e.g. `handed off: fraud_signal`. Later
    /// messages such as the transcript do not hide it.
    pub action_subject: Option<String>,
}

impl TakeoverCandidate {
    /// Serbero asked a person to take this dispute over.
    pub fn needs_action(&self) -> bool {
        self.action_subject.is_some()
    }
}

/// Disputes with an assistant message since `now - TAKEOVER_WINDOW_SECS`,
/// `in-progress` on the relay and missing from `local_ids`; the ones that
/// need a person first, then newest first.
pub fn takeover_candidates(
    solver_dms: &SolverDmsByDispute,
    relay: &[Dispute],
    local_ids: &HashSet<String>,
    now: i64,
) -> Vec<TakeoverCandidate> {
    let since = now - TAKEOVER_WINDOW_SECS;
    let mut candidates: Vec<TakeoverCandidate> = relay
        .iter()
        .filter(|d| {
            DisputeStatus::from_str(&d.status).is_ok_and(|s| s == DisputeStatus::InProgress)
        })
        .filter(|d| !local_ids.contains(&d.id.to_string()))
        .filter_map(|d| {
            let recent: Vec<_> = solver_dms
                .get(&d.id.to_string())?
                .iter()
                .filter(|m| m.created_at >= since)
                .collect();
            let newest = recent.last()?;
            Some(TakeoverCandidate {
                dispute_id: d.id,
                subject: newest.subject.clone(),
                last_message_at: newest.created_at,
                action_subject: recent
                    .iter()
                    .rev()
                    .find(|m| m.needs_action())
                    .map(|m| m.subject.clone()),
            })
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.needs_action()
            .cmp(&a.needs_action())
            .then(b.last_message_at.cmp(&a.last_message_at))
    });
    candidates
}

/// [`takeover_candidates`] for this solver: its assistant messages, leaving
/// out the disputes already in its local DB (`admin_disputes_in_progress`).
pub fn solver_takeover_candidates(
    app: &AppState,
    relay: &[Dispute],
    now: i64,
) -> Vec<TakeoverCandidate> {
    let local_ids: HashSet<String> = app
        .admin_disputes_in_progress
        .iter()
        .map(|d| d.dispute_id.clone())
        .collect();
    takeover_candidates(&app.solver_dms, relay, &local_ids, now)
}

/// Disputes Serbero handed to a person: the candidates that need action, in
/// picker order. The Disputes Pending banner and tab badge show these, so
/// they always match the `⚠` rows Ctrl+T lists first.
pub fn handoff_candidates(app: &AppState, relay: &[Dispute], now: i64) -> Vec<TakeoverCandidate> {
    let mut candidates = solver_takeover_candidates(app, relay, now);
    candidates.retain(TakeoverCandidate::needs_action);
    candidates
}

/// Moves the picker cursor one row, clamped to the list.
pub fn move_takeover_cursor(cursor: &mut usize, len: usize, down: bool) {
    if len == 0 {
        *cursor = 0;
    } else if down {
        *cursor = (*cursor + 1).min(len - 1);
    } else {
        *cursor = cursor.saturating_sub(1);
    }
}

/// `just now`, `25 min ago`, `3 h ago`.
fn age(at: i64, now: i64) -> String {
    let secs = (now - at).max(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", secs / 60),
        _ => format!("{} h ago", secs / 3600),
    }
}

/// One picker row: full id, subject and age when wide; short id and subject
/// when narrow; subject over the short id when there is no room for both.
fn candidate_item(candidate: &TakeoverCandidate, now: i64, width: u16) -> ListItem<'static> {
    if width >= STACKED_PICKER_WIDTH {
        return ListItem::new(candidate_line(candidate, now, width >= WIDE_PICKER_WIDTH));
    }
    let (marker, style) = subject_marker_style(candidate);
    ListItem::new(vec![
        Line::styled(format!("{marker}{}", candidate.subject), style),
        Line::styled(
            candidate.dispute_id.to_string()[..8].to_string(),
            Style::default().fg(Color::Gray),
        ),
    ])
}

fn subject_marker_style(candidate: &TakeoverCandidate) -> (&'static str, Style) {
    if candidate.needs_action() {
        ("⚠ ", Style::default().fg(Color::Yellow))
    } else {
        ("", Style::default().fg(Color::White))
    }
}

fn candidate_line(candidate: &TakeoverCandidate, now: i64, wide: bool) -> Line<'static> {
    let full_id = candidate.dispute_id.to_string();
    let id = if wide {
        full_id
    } else {
        full_id[..8].to_string()
    };
    let (marker, subject_style) = subject_marker_style(candidate);
    let mut spans = vec![
        Span::styled(format!("{id}  "), Style::default().fg(PRIMARY_COLOR)),
        Span::styled(format!("{marker}{}", candidate.subject), subject_style),
    ];
    if wide {
        spans.push(Span::styled(
            format!("  · {}", age(candidate.last_message_at, now)),
            Style::default().fg(Color::Gray),
        ));
    }
    Line::from(spans)
}

/// Renders the picker popup.
pub fn render_takeover_picker(
    f: &mut ratatui::Frame,
    candidates: &[TakeoverCandidate],
    cursor: usize,
    now: i64,
) {
    let area = f.area();
    let popup = helpers::create_centered_popup(area, 96.min(area.width), 18.min(area.height));
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(format!("🛟 Take over a dispute ({})", candidates.len()))
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Short popups drop the intro so at least one candidate row stays visible.
    let intro_height = if inner.height >= 8 { 2 } else { 0 };
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(intro_height),
            Constraint::Min(1),
            Constraint::Length(1),
        ],
    )
    .split(inner);

    f.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "In-progress disputes Serbero wrote to you about (last 12 h).",
                Style::default().fg(Color::White),
            ),
            Line::styled(
                "Mostro refuses if the solver holding it can write.",
                Style::default().fg(Color::Gray),
            ),
        ])
        .wrap(Wrap { trim: true }),
        chunks[0],
    );

    let items: Vec<ListItem> = candidates
        .iter()
        .map(|c| candidate_item(c, now, chunks[1].width))
        .collect();
    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD))
        .highlight_symbol("> ");
    let mut state =
        ListState::default().with_selected(Some(cursor.min(candidates.len().saturating_sub(1))));
    f.render_stateful_widget(list, chunks[1], &mut state);

    let full_hint = Line::raw(TAKEOVER_PICKER_HINT);
    let hint = if usize::from(chunks[2].width) >= full_hint.width() {
        TAKEOVER_PICKER_HINT
    } else {
        TAKEOVER_PICKER_HINT_COMPACT
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(Color::Gray))),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AdminDispute;
    use crate::ui::UserRole;
    use crate::util::solver_dms::{index_by_dispute, SolverDm};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    const NOW: i64 = 1_000_000;

    /// A candidate whose newest message is a handoff with `subject`.
    fn handed_off(dispute: Uuid, subject: &str, at: i64) -> TakeoverCandidate {
        TakeoverCandidate {
            dispute_id: dispute,
            subject: subject.to_string(),
            last_message_at: at,
            action_subject: Some(subject.to_string()),
        }
    }

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

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn msg(dispute: Uuid, subject: &str, at: i64) -> SolverDm {
        SolverDm {
            event_id: format!("{dispute}-{at}"),
            sender_pubkey: String::new(),
            recipient_pubkey: String::new(),
            dispute_id: Some(dispute.to_string()),
            subject: subject.to_string(),
            text: String::new(),
            created_at: at,
        }
    }

    fn relay(dispute: Uuid, status: &str) -> Dispute {
        Dispute {
            id: dispute,
            status: status.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn only_recent_in_progress_disputes_missing_locally_are_offered() {
        let dms = index_by_dispute(vec![
            msg(id(1), "mediating", NOW - 60),
            msg(id(2), "mediating", NOW - TAKEOVER_WINDOW_SECS - 1),
            msg(id(3), "mediating", NOW - 60),
            msg(id(4), "mediating", NOW - 60),
        ]);
        let relay = [
            relay(id(1), "in-progress"),
            relay(id(2), "in-progress"),
            relay(id(3), "settled"),
            relay(id(4), "in-progress"),
        ];
        let local: HashSet<String> = [id(4).to_string()].into();

        let ids: Vec<Uuid> = takeover_candidates(&dms, &relay, &local, NOW)
            .into_iter()
            .map(|c| c.dispute_id)
            .collect();

        assert_eq!(ids, [id(1)]);
    }

    #[test]
    fn handoffs_come_first_then_the_newest() {
        let dms = index_by_dispute(vec![
            msg(id(1), "mediating", NOW - 10),
            msg(id(2), "handed off: fraud_signal", NOW - 500),
            msg(id(2), "transcript (4 messages, times UTC)", NOW - 499),
            msg(id(3), "mediating", NOW - 100),
        ]);
        let relay = [
            relay(id(1), "in-progress"),
            relay(id(2), "in-progress"),
            relay(id(3), "in-progress"),
        ];

        let candidates = takeover_candidates(&dms, &relay, &HashSet::new(), NOW);

        let ids: Vec<Uuid> = candidates.iter().map(|c| c.dispute_id).collect();
        assert_eq!(ids, [id(2), id(1), id(3)]);
        assert!(candidates[0].needs_action());
        assert!(!candidates[1].needs_action());
        assert_eq!(candidates[0].subject, "transcript (4 messages, times UTC)");
        assert_eq!(candidates[0].last_message_at, NOW - 499);
    }

    #[test]
    fn the_newest_handoff_is_kept_past_later_messages() {
        let dms = index_by_dispute(vec![
            msg(id(1), "mediation could not start", NOW - 900),
            msg(id(1), "handed off: fraud_signal", NOW - 500),
            msg(id(1), "transcript (4 messages, times UTC)", NOW - 499),
            msg(id(1), "new messages since handoff (2)", NOW - 100),
        ]);

        let candidates =
            takeover_candidates(&dms, &[relay(id(1), "in-progress")], &HashSet::new(), NOW);

        assert_eq!(
            candidates[0].action_subject.as_deref(),
            Some("handed off: fraud_signal")
        );
        assert_eq!(candidates[0].subject, "new messages since handoff (2)");
    }

    fn solver(dms: Vec<SolverDm>, local: &[Uuid]) -> AppState {
        let mut app = AppState::new(UserRole::Admin);
        app.solver_dms = index_by_dispute(dms);
        app.admin_disputes_in_progress = local
            .iter()
            .map(|d| AdminDispute {
                dispute_id: d.to_string(),
                ..Default::default()
            })
            .collect();
        app
    }

    #[test]
    fn handoffs_are_the_candidates_a_person_has_to_act_on() {
        let app = solver(
            vec![
                msg(id(1), "handed off: conflicting_claims", NOW - 300),
                msg(id(1), "transcript (18 messages, times UTC)", NOW - 299),
                msg(id(2), "mediating", NOW - 60),
                msg(id(3), "handed off: fraud_signal", NOW - 60),
                msg(id(4), "handed off: unresponsive", NOW - 60),
                msg(id(5), "mediation could not start", NOW - 100),
                msg(
                    id(6),
                    "handed off: round_limit",
                    NOW - TAKEOVER_WINDOW_SECS - 1,
                ),
            ],
            &[id(3)],
        );
        let relay = [
            relay(id(1), "in-progress"),
            relay(id(2), "in-progress"),
            relay(id(3), "in-progress"),
            relay(id(4), "settled"),
            relay(id(5), "in-progress"),
            relay(id(6), "in-progress"),
        ];

        let handoffs = handoff_candidates(&app, &relay, NOW);

        let ids: Vec<Uuid> = handoffs.iter().map(|c| c.dispute_id).collect();
        assert_eq!(ids, [id(5), id(1)], "newest first");
        assert_eq!(
            handoffs[1].action_subject.as_deref(),
            Some("handed off: conflicting_claims")
        );
    }

    #[test]
    fn handoffs_are_exactly_the_marked_rows_ctrl_t_lists_first() {
        let app = solver(
            vec![
                msg(id(1), "handed off: conflicting_claims", NOW - 300),
                msg(id(2), "mediating", NOW - 10),
                msg(id(3), "mediation could not start", NOW - 100),
            ],
            &[],
        );
        let relay = [
            relay(id(1), "in-progress"),
            relay(id(2), "in-progress"),
            relay(id(3), "in-progress"),
        ];

        let picker = solver_takeover_candidates(&app, &relay, NOW);
        let marked: Vec<TakeoverCandidate> = picker
            .iter()
            .take_while(|c| c.needs_action())
            .cloned()
            .collect();

        assert_eq!(handoff_candidates(&app, &relay, NOW), marked);
        assert_eq!(marked.len(), 2);
    }

    #[test]
    fn a_dispute_taken_locally_is_no_longer_offered() {
        let app = solver(
            vec![msg(id(1), "handed off: conflicting_claims", NOW - 300)],
            &[id(1)],
        );

        let relay = [relay(id(1), "in-progress")];

        assert!(solver_takeover_candidates(&app, &relay, NOW).is_empty());
        assert!(handoff_candidates(&app, &relay, NOW).is_empty());
    }

    #[test]
    fn the_cursor_stays_inside_the_list() {
        let mut cursor = 0;
        move_takeover_cursor(&mut cursor, 2, false);
        assert_eq!(cursor, 0);
        move_takeover_cursor(&mut cursor, 2, true);
        move_takeover_cursor(&mut cursor, 2, true);
        assert_eq!(cursor, 1);
        move_takeover_cursor(&mut cursor, 0, true);
        assert_eq!(cursor, 0);
    }

    #[test]
    fn the_picker_shows_ids_subjects_and_the_action_marker() {
        let candidates = vec![handed_off(
            id(7),
            "handed off: conflicting_claims",
            NOW - 25 * 60,
        )];
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();

        terminal
            .draw(|f| render_takeover_picker(f, &candidates, 0, NOW))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, &id(7).to_string()));
        assert!(buffer_contains(buf, "⚠ handed off: conflicting_claims"));
        assert!(buffer_contains(buf, "25 min ago"));
        assert!(buffer_contains(buf, TAKEOVER_PICKER_HINT));
    }

    #[test]
    fn a_mid_width_picker_keeps_the_whole_subject() {
        let candidates = vec![handed_off(
            id(7),
            "handed off: conflicting_claims",
            NOW - 60,
        )];
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();

        terminal
            .draw(|f| render_takeover_picker(f, &candidates, 0, NOW))
            .unwrap();

        assert!(buffer_contains(
            terminal.backend().buffer(),
            "handed off: conflicting_claims"
        ));
    }

    #[test]
    fn a_very_narrow_picker_still_shows_the_subject() {
        let candidates = vec![handed_off(id(7), "handed off: x", NOW - 60)];
        let mut terminal = Terminal::new(TestBackend::new(20, 12)).unwrap();

        terminal
            .draw(|f| render_takeover_picker(f, &candidates, 0, NOW))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "handed off"),
            "subject must stay visible"
        );
        assert!(buffer_contains(buf, "00000000"), "short id on its own line");
        for key in ["↑↓", "Enter", "Esc"] {
            assert!(buffer_contains(buf, key), "control {key} clipped");
        }
    }

    #[test]
    fn a_narrow_picker_keeps_the_short_id_and_subject() {
        let candidates = vec![handed_off(id(7), "handed off: x", NOW - 60)];
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();

        terminal
            .draw(|f| render_takeover_picker(f, &candidates, 0, NOW))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "00000000"));
        assert!(buffer_contains(buf, "handed off"));
    }
}
