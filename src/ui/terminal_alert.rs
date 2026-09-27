//! Out-of-focus alerts: terminal bell, notification sound request, and
//! window-title unread badge.
//!
//! Events (trade DMs, chat messages) and focus/input changes only mutate
//! [`TerminalAlertState`]. Bytes reach the terminal exclusively through
//! [`TerminalAlertState::flush`], which the main loop calls right after
//! `terminal.draw` so escape sequences never interleave with a frame.

use crate::settings::NotificationSettings;
use std::io::{self, Write};
use std::time::{Duration, Instant};

pub const APP_TITLE: &str = "Mostrix";
/// xterm window op: save the current window title on the terminal's title stack.
/// Terminals without a title stack ignore it.
pub const PUSH_TITLE: &str = "\x1b[22;0t";
/// xterm window op: restore the window title saved by [`PUSH_TITLE`].
pub const POP_TITLE: &str = "\x1b[23;0t";
const BELL: &str = "\x07";
/// Bursts (e.g. several DMs in one relay batch) ring / play once.
const ATTENTION_COOLDOWN: Duration = Duration::from_secs(3);
/// Without focus reporting, recent keyboard/mouse input is the only presence signal.
const IDLE_BEFORE_ALERT_WHEN_FOCUS_UNKNOWN: Duration = Duration::from_secs(30);

/// Terminal focus as reported by `FocusGained` / `FocusLost`.
/// Stays `Unknown` until the first report (not every terminal/multiplexer sends them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Unknown,
    Focused,
    Unfocused,
}

#[derive(Debug)]
pub struct TerminalAlertState {
    settings: NotificationSettings,
    /// Unix seconds at launch; older events are startup hydration, not news.
    launch_ts: i64,
    focus: Focus,
    last_input: Option<Instant>,
    unread: usize,
    /// Bell and/or sound owed for events since the last flush.
    attention_pending: bool,
    last_attention: Option<Instant>,
    /// Set by [`Self::flush`]; the main loop plays the sound (needs the async runtime).
    sound_requested: bool,
    /// Count currently rendered in the window title (0 = title untouched / restored).
    shown_badge: usize,
}

impl TerminalAlertState {
    pub fn new(launch_ts: i64) -> Self {
        Self {
            settings: NotificationSettings::default(),
            launch_ts,
            focus: Focus::Unknown,
            last_input: None,
            unread: 0,
            attention_pending: false,
            last_attention: None,
            sound_requested: false,
            shown_badge: 0,
        }
    }

    pub fn set_settings(&mut self, settings: &NotificationSettings) {
        if self.settings != *settings {
            self.settings = settings.clone();
            if !self.wants_attention() {
                self.attention_pending = false;
            }
        }
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn unread(&self) -> usize {
        self.unread
    }

    pub fn set_focus(&mut self, focused: bool) {
        if focused {
            self.focus = Focus::Focused;
            self.clear_unread();
        } else {
            self.focus = Focus::Unfocused;
        }
    }

    /// Keyboard / mouse / paste input: the user is looking at Mostrix.
    pub fn note_input(&mut self, now: Instant) {
        self.last_input = Some(now);
        self.clear_unread();
    }

    /// Record a new event with rumor timestamp `event_ts`. Returns whether it alerted.
    pub fn record_event(&mut self, event_ts: i64) -> bool {
        self.record_event_at(event_ts, Instant::now())
    }

    pub fn record_event_at(&mut self, event_ts: i64, now: Instant) -> bool {
        if event_ts < self.launch_ts {
            return false;
        }
        if !self.wants_attention() && !self.settings.title_badge {
            return false;
        }
        if self.settings.only_when_unfocused && self.user_is_present(now) {
            return false;
        }
        self.unread = self.unread.saturating_add(1);
        if self.wants_attention() {
            self.attention_pending = true;
        }
        true
    }

    fn wants_attention(&self) -> bool {
        self.settings.bell || self.settings.sound
    }

    /// True once per flushed alert when `sound` is enabled; resets on read.
    pub fn take_sound_request(&mut self) -> bool {
        std::mem::take(&mut self.sound_requested)
    }

    fn user_is_present(&self, now: Instant) -> bool {
        match self.focus {
            Focus::Focused => true,
            Focus::Unfocused => false,
            Focus::Unknown => self.last_input.is_some_and(|t| {
                now.saturating_duration_since(t) < IDLE_BEFORE_ALERT_WHEN_FOCUS_UNKNOWN
            }),
        }
    }

    fn clear_unread(&mut self) {
        self.unread = 0;
        self.attention_pending = false;
    }

    fn desired_badge(&self) -> usize {
        if self.settings.title_badge {
            self.unread
        } else {
            0
        }
    }

    /// Write pending bell / title changes to `out` and arm [`Self::take_sound_request`].
    /// No-op when nothing changed.
    pub fn flush(&mut self, out: &mut impl Write, now: Instant) -> io::Result<()> {
        let mut wrote = false;
        if std::mem::take(&mut self.attention_pending)
            && self
                .last_attention
                .is_none_or(|t| now.saturating_duration_since(t) >= ATTENTION_COOLDOWN)
        {
            self.last_attention = Some(now);
            self.sound_requested = self.settings.sound;
            if self.settings.bell {
                out.write_all(BELL.as_bytes())?;
                wrote = true;
            }
        }
        let desired = self.desired_badge();
        if desired != self.shown_badge {
            // Raw OSC 0 rather than crossterm `SetTitle`, which may bypass `out` via WinAPI.
            write!(out, "\x1b]0;{}\x07", title_text(desired))?;
            if desired == 0 {
                // Title-stack terminals get their original title back; others keep "Mostrix".
                out.write_all(POP_TITLE.as_bytes())?;
                out.write_all(PUSH_TITLE.as_bytes())?;
            }
            self.shown_badge = desired;
            wrote = true;
        }
        if wrote {
            out.flush()?;
        }
        Ok(())
    }
}

pub fn title_text(unread: usize) -> String {
    if unread == 0 {
        APP_TITLE.to_string()
    } else {
        format!("({unread}) {APP_TITLE}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAUNCH: i64 = 1_000;

    fn state() -> TerminalAlertState {
        TerminalAlertState::new(LAUNCH)
    }

    fn flushed(s: &mut TerminalAlertState, now: Instant) -> String {
        let mut out = Vec::new();
        s.flush(&mut out, now).expect("flush");
        String::from_utf8(out).expect("utf8")
    }

    #[test]
    fn title_text_shows_count_only_when_unread() {
        assert_eq!(title_text(0), "Mostrix");
        assert_eq!(title_text(3), "(3) Mostrix");
    }

    #[test]
    fn events_before_launch_are_historical() {
        let mut s = state();
        s.set_focus(false);
        assert!(!s.record_event_at(LAUNCH - 1, Instant::now()));
        assert_eq!(s.unread(), 0);
        assert!(s.record_event_at(LAUNCH, Instant::now()));
    }

    #[test]
    fn unfocused_event_rings_bell_and_sets_badge() {
        let mut s = state();
        s.set_focus(false);
        let now = Instant::now();
        assert!(s.record_event_at(LAUNCH + 1, now));
        assert!(s.record_event_at(LAUNCH + 2, now));
        let out = flushed(&mut s, now);
        assert_eq!(out.matches(BELL).count(), 2, "bell + OSC title terminator");
        assert!(out.contains("(2) Mostrix"));
        assert!(!out.contains(POP_TITLE));
    }

    #[test]
    fn focused_user_gets_no_alert() {
        let mut s = state();
        s.set_focus(true);
        assert!(!s.record_event_at(LAUNCH + 1, Instant::now()));
        assert_eq!(flushed(&mut s, Instant::now()), "");
    }

    #[test]
    fn focused_user_alerted_when_only_when_unfocused_disabled() {
        let mut s = state();
        s.set_settings(&NotificationSettings {
            only_when_unfocused: false,
            ..NotificationSettings::default()
        });
        s.set_focus(true);
        assert!(s.record_event_at(LAUNCH + 1, Instant::now()));
    }

    #[test]
    fn unknown_focus_alerts_only_after_idle() {
        let mut s = state();
        let t0 = Instant::now();
        assert_eq!(s.focus(), Focus::Unknown);
        s.note_input(t0);
        assert!(!s.record_event_at(LAUNCH + 1, t0 + Duration::from_secs(5)));
        assert!(s.record_event_at(LAUNCH + 1, t0 + Duration::from_secs(31)));
    }

    #[test]
    fn unknown_focus_without_any_input_alerts() {
        let mut s = state();
        assert!(s.record_event_at(LAUNCH + 1, Instant::now()));
    }

    #[test]
    fn regaining_focus_clears_badge_and_restores_title() {
        let mut s = state();
        s.set_focus(false);
        let now = Instant::now();
        s.record_event_at(LAUNCH + 1, now);
        flushed(&mut s, now);

        s.set_focus(true);
        assert_eq!(s.unread(), 0);
        let out = flushed(&mut s, now);
        assert!(out.contains("\x1b]0;Mostrix\x07"));
        assert!(out.ends_with(&format!("{POP_TITLE}{PUSH_TITLE}")));
        assert_eq!(flushed(&mut s, now), "", "restore is written once");
    }

    #[test]
    fn input_clears_unread_and_pending_bell() {
        let mut s = state();
        s.set_focus(false);
        let now = Instant::now();
        s.record_event_at(LAUNCH + 1, now);
        s.note_input(now);
        assert_eq!(s.unread(), 0);
        assert_eq!(flushed(&mut s, now), "");
    }

    #[test]
    fn bell_respects_cooldown() {
        let mut s = state();
        s.set_settings(&NotificationSettings {
            title_badge: false,
            ..NotificationSettings::default()
        });
        s.set_focus(false);
        let t0 = Instant::now();
        s.record_event_at(LAUNCH + 1, t0);
        assert_eq!(flushed(&mut s, t0), BELL);
        s.record_event_at(LAUNCH + 2, t0 + Duration::from_secs(1));
        assert_eq!(flushed(&mut s, t0 + Duration::from_secs(1)), "");
        s.record_event_at(LAUNCH + 3, t0 + Duration::from_secs(4));
        assert_eq!(flushed(&mut s, t0 + Duration::from_secs(4)), BELL);
    }

    #[test]
    fn bell_disabled_only_updates_title() {
        let mut s = state();
        s.set_settings(&NotificationSettings {
            bell: false,
            ..NotificationSettings::default()
        });
        s.set_focus(false);
        let now = Instant::now();
        assert!(s.record_event_at(LAUNCH + 1, now));
        assert_eq!(flushed(&mut s, now), "\x1b]0;(1) Mostrix\x07");
    }

    #[test]
    fn all_channels_disabled_records_nothing() {
        let mut s = state();
        s.set_settings(&NotificationSettings {
            bell: false,
            title_badge: false,
            sound: false,
            only_when_unfocused: false,
            ..NotificationSettings::default()
        });
        assert!(!s.record_event_at(LAUNCH + 1, Instant::now()));
        assert_eq!(s.unread(), 0);
    }

    #[test]
    fn flushed_alert_requests_sound_once() {
        let mut s = state();
        s.set_focus(false);
        let now = Instant::now();
        s.record_event_at(LAUNCH + 1, now);
        assert!(!s.take_sound_request(), "only armed by flush");
        flushed(&mut s, now);
        assert!(s.take_sound_request());
        assert!(!s.take_sound_request());
    }

    #[test]
    fn sound_shares_bell_cooldown() {
        let mut s = state();
        s.set_focus(false);
        let t0 = Instant::now();
        s.record_event_at(LAUNCH + 1, t0);
        flushed(&mut s, t0);
        assert!(s.take_sound_request());
        let t1 = t0 + Duration::from_secs(1);
        s.record_event_at(LAUNCH + 2, t1);
        flushed(&mut s, t1);
        assert!(!s.take_sound_request());
    }

    #[test]
    fn sound_only_plays_without_writing_bell() {
        let mut s = state();
        s.set_settings(&NotificationSettings {
            bell: false,
            title_badge: false,
            ..NotificationSettings::default()
        });
        s.set_focus(false);
        let now = Instant::now();
        assert!(s.record_event_at(LAUNCH + 1, now));
        assert_eq!(flushed(&mut s, now), "");
        assert!(s.take_sound_request());
    }

    #[test]
    fn sound_disabled_never_requests() {
        let mut s = state();
        s.set_settings(&NotificationSettings {
            sound: false,
            ..NotificationSettings::default()
        });
        s.set_focus(false);
        let now = Instant::now();
        s.record_event_at(LAUNCH + 1, now);
        assert_eq!(flushed(&mut s, now).matches(BELL).count(), 2);
        assert!(!s.take_sound_request());
    }

    #[test]
    fn disabling_badge_while_shown_restores_title() {
        let mut s = state();
        s.set_focus(false);
        let now = Instant::now();
        s.record_event_at(LAUNCH + 1, now);
        flushed(&mut s, now);
        s.set_settings(&NotificationSettings {
            title_badge: false,
            ..NotificationSettings::default()
        });
        let out = flushed(&mut s, now);
        assert!(out.contains("\x1b]0;Mostrix\x07"));
    }
}
