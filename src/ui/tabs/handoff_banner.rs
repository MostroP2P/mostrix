//! Serbero handoffs on Disputes Pending: a highlighted line above the table
//! and a `🙋 N` badge on the tab label, so a dispute Serbero handed to a
//! person is seen without opening the Ctrl+T picker. Both show
//! [`handoff_candidates`](crate::ui::takeover_picker::handoff_candidates),
//! the `⚠` rows the picker lists first, so the three always agree.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use crate::ui::constants::{HANDOFF_MARKER, HANDOFF_TAKE_OVER_HINT, HANDOFF_TAKE_OVER_KEY};
use crate::ui::takeover_picker::TakeoverCandidate;
use crate::util::solver_dms::handoff_reason;

/// The table under the banner keeps its borders and one dispute row.
const MIN_TABLE_HEIGHT: u16 = 3;

/// Blank column before the banner text.
const BANNER_MARGIN: &str = " ";

/// The yellow that marks handoffs in the SERBERO pane and the picker,
/// inverted so the line stands out above the green table.
const BANNER_STYLE: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Yellow)
    .add_modifier(Modifier::BOLD);

/// The yellow the SERBERO pane uses for handoff subjects.
pub const BADGE_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);

/// Splits `area` into the banner row and the rest. The banner shows only
/// with handoffs and when the table below still fits a dispute row.
pub fn split_handoff_banner(area: Rect, handoffs: usize) -> (Option<Rect>, Rect) {
    if handoffs == 0 || area.height <= MIN_TABLE_HEIGHT {
        return (None, area);
    }
    let banner = Rect { height: 1, ..area };
    let rest = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    (Some(banner), rest)
}

/// The most detailed banner text for `handoffs` that fits `width` after the
/// margin; the last resort keeps only the marker and Ctrl+T. `None` without
/// handoffs.
pub fn handoff_banner_text(handoffs: &[TakeoverCandidate], width: u16) -> Option<String> {
    let room = usize::from(width).saturating_sub(BANNER_MARGIN.len());
    let texts = banner_texts(handoffs);
    texts
        .iter()
        .find(|text| Span::raw(text.as_str()).width() <= room)
        .or(texts.last())
        .cloned()
}

/// Banner texts from the most to the least detailed.
fn banner_texts(handoffs: &[TakeoverCandidate]) -> Vec<String> {
    let (full, shorter) = match handoffs {
        [] => return Vec::new(),
        [one] => {
            let id = short_dispute_id(one);
            let subject = one.action_subject.as_deref().unwrap_or(&one.subject);
            (
                format!(
                    "Serbero handed off dispute {id} ({})",
                    handoff_reason(subject)
                ),
                vec![format!("Serbero handed off {id}"), "Handed off".to_string()],
            )
        }
        many => (
            format!("Serbero handed off {} disputes", many.len()),
            vec![format!("{} handed off", many.len())],
        ),
    };
    let line = |text: &str, hint: &str| format!("{HANDOFF_MARKER} {text} · {hint}");
    let mut texts = vec![
        line(&full, HANDOFF_TAKE_OVER_HINT),
        line(&full, HANDOFF_TAKE_OVER_KEY),
    ];
    texts.extend(shorter.iter().map(|text| line(text, HANDOFF_TAKE_OVER_KEY)));
    texts.push(format!("{HANDOFF_MARKER} {HANDOFF_TAKE_OVER_KEY}"));
    texts
}

fn short_dispute_id(handoff: &TakeoverCandidate) -> String {
    handoff.dispute_id.to_string().chars().take(8).collect()
}

/// Draws the banner for `handoffs` across the one-row `area`.
pub fn render_handoff_banner(f: &mut ratatui::Frame, area: Rect, handoffs: &[TakeoverCandidate]) {
    let Some(text) = handoff_banner_text(handoffs, area.width) else {
        return;
    };
    f.render_widget(
        Paragraph::new(format!("{BANNER_MARGIN}{text}")).style(BANNER_STYLE),
        area,
    );
}

/// Badge after the Disputes Pending tab label (` 🙋 2`); `None` without
/// handoffs.
pub fn handoff_badge(count: usize) -> Option<String> {
    (count > 0).then(|| format!(" {HANDOFF_MARKER} {count}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use mostro_core::prelude::Dispute;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;
    use uuid::Uuid;

    use crate::models::AdminDispute;
    use crate::ui::{ui_draw, AdminTab, AppState, Tab, UserRole};
    use crate::util::solver_dms::{add_to_index, index_by_dispute, SolverDm};

    const HANDED_OFF: &str = "4f1c2a9e-0000-4000-8000-000000000001";
    const ALSO_HANDED_OFF: &str = "7a2b3c4d-0000-4000-8000-000000000002";
    const PENDING: &str = "11111111-1111-4111-8111-111111111111";

    /// Buffer text as a terminal shows it: the cell hidden behind a wide
    /// symbol such as an emoji is skipped, so `🙋 2` reads as written.
    fn rendered_text(buf: &Buffer) -> String {
        let mut text = String::new();
        for y in 0..buf.area.height {
            let mut x = 0;
            while x < buf.area.width {
                let symbol = buf[(x, y)].symbol();
                text.push_str(symbol);
                x += u16::try_from(Span::raw(symbol).width()).unwrap_or(1).max(1);
            }
            text.push('\n');
        }
        text
    }

    fn uuid(id: &str) -> Uuid {
        Uuid::parse_str(id).unwrap()
    }

    fn handoff(id: &str, subject: &str) -> TakeoverCandidate {
        TakeoverCandidate {
            dispute_id: uuid(id),
            subject: "transcript (18 messages, times UTC)".to_string(),
            last_message_at: 0,
            action_subject: Some(subject.to_string()),
        }
    }

    fn one() -> Vec<TakeoverCandidate> {
        vec![handoff(HANDED_OFF, "handed off: conflicting_claims")]
    }

    fn two() -> Vec<TakeoverCandidate> {
        vec![
            handoff(HANDED_OFF, "handed off: conflicting_claims"),
            handoff(ALSO_HANDED_OFF, "mediation could not start"),
        ]
    }

    #[test]
    fn one_handoff_names_the_dispute_and_the_reason() {
        assert_eq!(
            handoff_banner_text(&one(), 120).as_deref(),
            Some(
                "🙋 Serbero handed off dispute 4f1c2a9e (conflicting claims) · Ctrl+T to take over"
            )
        );
    }

    #[test]
    fn a_failed_opening_reads_as_its_subject() {
        let failed = [handoff(HANDED_OFF, "mediation could not start")];

        assert_eq!(
            handoff_banner_text(&failed, 120).as_deref(),
            Some(
                "🙋 Serbero handed off dispute 4f1c2a9e (mediation could not start) · Ctrl+T to take over"
            )
        );
    }

    #[test]
    fn several_handoffs_are_counted() {
        assert_eq!(
            handoff_banner_text(&two(), 120).as_deref(),
            Some("🙋 Serbero handed off 2 disputes · Ctrl+T to take over")
        );
    }

    #[test]
    fn nothing_to_show_without_handoffs() {
        assert_eq!(handoff_banner_text(&[], 120), None);
    }

    #[test]
    fn one_handoff_shortens_to_fit_the_width() {
        let cases = [
            (
                82,
                "🙋 Serbero handed off dispute 4f1c2a9e (conflicting claims) · Ctrl+T to take over",
            ),
            (
                81,
                "🙋 Serbero handed off dispute 4f1c2a9e (conflicting claims) · Ctrl+T",
            ),
            (68, "🙋 Serbero handed off 4f1c2a9e · Ctrl+T"),
            (39, "🙋 Handed off · Ctrl+T"),
            (22, "🙋 Ctrl+T"),
            (4, "🙋 Ctrl+T"),
        ];
        for (width, expected) in cases {
            assert_eq!(
                handoff_banner_text(&one(), width).as_deref(),
                Some(expected),
                "at width {width}"
            );
        }
    }

    #[test]
    fn several_handoffs_shorten_to_fit_the_width() {
        let cases = [
            (55, "🙋 Serbero handed off 2 disputes · Ctrl+T to take over"),
            (54, "🙋 Serbero handed off 2 disputes · Ctrl+T"),
            (41, "🙋 2 handed off · Ctrl+T"),
            (24, "🙋 Ctrl+T"),
        ];
        for (width, expected) in cases {
            assert_eq!(
                handoff_banner_text(&two(), width).as_deref(),
                Some(expected),
                "at width {width}"
            );
        }
    }

    #[test]
    fn every_width_keeps_ctrl_t_on_the_line() {
        for handoffs in [one(), two()] {
            for width in 10..=120 {
                let text = handoff_banner_text(&handoffs, width).unwrap();
                assert!(text.contains("Ctrl+T"), "{text:?} at {width}");
                assert!(
                    Span::raw(text.as_str()).width() < usize::from(width),
                    "{text:?} does not fit {width} with its margin"
                );
            }
        }
    }

    #[test]
    fn the_badge_counts_handoffs() {
        assert_eq!(handoff_badge(0), None);
        assert_eq!(handoff_badge(1).as_deref(), Some(" 🙋 1"));
        assert_eq!(handoff_badge(12).as_deref(), Some(" 🙋 12"));
    }

    #[test]
    fn the_banner_takes_a_row_only_when_a_dispute_row_still_fits() {
        let area = |height| Rect::new(0, 5, 80, height);

        assert_eq!(split_handoff_banner(area(10), 0), (None, area(10)));
        assert_eq!(split_handoff_banner(area(3), 2), (None, area(3)));
        assert_eq!(
            split_handoff_banner(area(4), 2),
            (Some(Rect::new(0, 5, 80, 1)), Rect::new(0, 6, 80, 3))
        );
    }

    #[test]
    fn the_banner_is_a_highlighted_row() {
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();

        terminal
            .draw(|f| render_handoff_banner(f, f.area(), &two()))
            .unwrap();

        let buf = terminal.backend().buffer();
        assert!(rendered_text(buf).contains(" 🙋 Serbero handed off 2 disputes · Ctrl+T"));
        for x in [0, 79] {
            assert_eq!(buf[(x, 0)].bg, Color::Yellow, "row filled at x={x}");
        }
        assert_eq!(buf[(5, 0)].fg, Color::Black);
    }

    // --- Whole screen (`ui_draw`): tab badge and banner from app state ---

    fn serbero_dm(dispute: &str, event_id: &str, subject: &str) -> SolverDm {
        SolverDm {
            event_id: event_id.to_string(),
            sender_pubkey: String::new(),
            recipient_pubkey: String::new(),
            dispute_id: Some(dispute.to_string()),
            subject: subject.to_string(),
            text: String::new(),
            created_at: chrono::Utc::now().timestamp() - 60,
        }
    }

    fn relay_dispute(id: &str, status: &str) -> Dispute {
        Dispute {
            id: uuid(id),
            status: status.to_string(),
            ..Default::default()
        }
    }

    /// Solver on Disputes Pending with one pending dispute; Serbero handed
    /// off `HANDED_OFF`, which the relay shows with `status`.
    fn solver_with_handoff(status: &str) -> (AppState, Arc<Mutex<Vec<Dispute>>>) {
        let mut app = AppState::new(UserRole::Admin);
        app.solver_dms = index_by_dispute(vec![
            serbero_dm(HANDED_OFF, "a", "handed off: conflicting_claims"),
            serbero_dm(HANDED_OFF, "b", "transcript (18 messages, times UTC)"),
        ]);
        let relay = vec![
            relay_dispute(PENDING, "initiated"),
            relay_dispute(HANDED_OFF, status),
        ];
        (app, Arc::new(Mutex::new(relay)))
    }

    fn draw(app: &mut AppState, disputes: &Arc<Mutex<Vec<Dispute>>>, w: u16, h: u16) -> String {
        let orders = Arc::new(Mutex::new(Vec::new()));
        let status = ["status".to_string()];
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| ui_draw(f, app, &orders, disputes, Some(&status)))
            .unwrap();
        rendered_text(terminal.backend().buffer())
    }

    #[test]
    fn a_handoff_shows_the_banner_and_the_tab_badge() {
        let (mut app, disputes) = solver_with_handoff("in-progress");

        let screen = draw(&mut app, &disputes, 120, 20);

        assert!(screen.contains("Disputes Pending 🙋 1"), "{screen}");
        assert!(
            screen.contains(
                "🙋 Serbero handed off dispute 4f1c2a9e (conflicting claims) · Ctrl+T to take over"
            ),
            "{screen}"
        );
        assert!(screen.contains("11111111"), "pending table still shown");
    }

    #[test]
    fn several_handoffs_show_the_count() {
        let (mut app, disputes) = solver_with_handoff("in-progress");
        add_to_index(
            &mut app.solver_dms,
            serbero_dm(ALSO_HANDED_OFF, "c", "mediation could not start"),
        );
        disputes
            .lock()
            .unwrap()
            .push(relay_dispute(ALSO_HANDED_OFF, "in-progress"));

        let screen = draw(&mut app, &disputes, 120, 20);

        assert!(screen.contains("Disputes Pending 🙋 2"), "{screen}");
        assert!(
            screen.contains("Serbero handed off 2 disputes · Ctrl+T to take over"),
            "{screen}"
        );
    }

    #[test]
    fn the_badge_stays_visible_on_other_tabs() {
        let (mut app, disputes) = solver_with_handoff("in-progress");
        app.active_tab = Tab::Admin(AdminTab::Observer);

        let screen = draw(&mut app, &disputes, 120, 20);

        assert!(screen.contains("Disputes Pending 🙋 1"), "{screen}");
        assert!(
            !screen.contains("Serbero handed off"),
            "banner only on Pending"
        );
    }

    #[test]
    fn a_dispute_taken_locally_clears_the_banner_and_badge() {
        let (mut app, disputes) = solver_with_handoff("in-progress");
        app.admin_disputes_in_progress = vec![AdminDispute {
            dispute_id: HANDED_OFF.to_string(),
            ..Default::default()
        }];

        let screen = draw(&mut app, &disputes, 120, 20);

        assert!(!screen.contains('🙋'), "{screen}");
        assert!(!screen.contains("handed off"), "{screen}");
    }

    #[test]
    fn a_dispute_no_longer_in_progress_clears_the_banner_and_badge() {
        let (mut app, disputes) = solver_with_handoff("settled");

        let screen = draw(&mut app, &disputes, 120, 20);

        assert!(!screen.contains('🙋'), "{screen}");
        assert!(!screen.contains("handed off"), "{screen}");
    }

    #[test]
    fn mediating_disputes_raise_no_banner() {
        let mut app = AppState::new(UserRole::Admin);
        app.solver_dms = index_by_dispute(vec![serbero_dm(HANDED_OFF, "a", "mediating")]);
        let disputes = Arc::new(Mutex::new(vec![relay_dispute(HANDED_OFF, "in-progress")]));

        let screen = draw(&mut app, &disputes, 120, 20);

        assert!(!screen.contains('🙋'), "{screen}");
    }

    /// 8 rows leave the tab 3: the pending row wins over the banner, and the
    /// tab badge still says Serbero needs a person.
    #[test]
    fn a_short_terminal_keeps_the_pending_row_and_the_badge() {
        let (mut app, disputes) = solver_with_handoff("in-progress");

        let screen = draw(&mut app, &disputes, 100, 8);

        assert!(screen.contains("11111111"), "{screen}");
        assert!(screen.contains("initiated"), "{screen}");
        assert!(screen.contains("Disputes Pending 🙋 1"), "{screen}");
        assert!(!screen.contains("Serbero handed off"), "{screen}");
    }

    #[test]
    fn a_narrow_terminal_keeps_ctrl_t_in_the_banner() {
        let (mut app, disputes) = solver_with_handoff("in-progress");

        let screen = draw(&mut app, &disputes, 50, 20);

        assert!(
            screen.contains("🙋 Serbero handed off 4f1c2a9e · Ctrl+T"),
            "{screen}"
        );
        assert!(screen.contains("11111111"), "{screen}");
    }
}
