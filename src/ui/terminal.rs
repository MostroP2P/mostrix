//! Crossterm / ratatui terminal lifecycle for the Mostrix TUI.
//!
//! Owns enter/leave of raw mode, alternate screen, mouse capture, bracketed
//! paste, focus reporting, the saved window title, and best-effort
//! keyboard-enhancement flags (Ctrl+I vs Tab).

use crate::ui::terminal_alert::{POP_TITLE, PUSH_TITLE};
use base64::{engine::general_purpose::STANDARD, Engine};
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::style::Print;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, stdin, stdout, IsTerminal, Stdout, Write};
use zeroize::Zeroizing;

const MAX_OSC52_BYTES: usize = 64 * 1024;

pub(crate) fn copy_with_osc52(text: &str) -> bool {
    let output = stdout();
    let interactive = output.is_terminal() && stdin().is_terminal();
    let term = std::env::var("TERM").ok();
    write_osc52(&mut output.lock(), text, interactive, term.as_deref()).is_ok()
}

fn write_osc52(
    writer: &mut impl Write,
    text: &str,
    interactive: bool,
    term: Option<&str>,
) -> io::Result<()> {
    if !interactive || term.is_some_and(|term| term.eq_ignore_ascii_case("dumb")) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Terminal clipboard unavailable",
        ));
    }
    if text.len() > MAX_OSC52_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Terminal clipboard payload too large",
        ));
    }
    let mut sequence = Zeroizing::new(String::from("\x1b]52;c;"));
    STANDARD.encode_string(text, &mut sequence);
    sequence.push('\x07');
    writer.write_all(sequence.as_bytes())?;
    writer.flush()
}

/// Crossterm-backed ratatui terminal used by the main event loop.
pub type MostrixTerminal = Terminal<CrosstermBackend<Stdout>>;

/// Best-effort Kitty/WezTerm/Ghostty keyboard-enhancement push; always pops on drop.
///
/// Enables `DISAMBIGUATE_ESCAPE_CODES` so Ctrl+I is not reported as Tab when the
/// terminal supports it. Unsupported terminals keep the classic collision;
/// My Trades COMMAND still has bare `i` / Insert to enter INSERT.
pub struct KeyboardEnhancementGuard {
    active: bool,
}

impl KeyboardEnhancementGuard {
    /// Push enhancement flags on `out`. Returns a guard that pops on drop when
    /// the push succeeded (push failures are ignored — terminals may not support it).
    pub fn try_enable(out: &mut impl Write) -> Self {
        let active = execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok();
        Self { active }
    }
}

impl Drop for KeyboardEnhancementGuard {
    fn drop(&mut self) {
        if self.active {
            // Fresh stdout handle — CrosstermBackend may already own the original
            // writer, or startup may have failed before Terminal::new.
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
            self.active = false;
        }
    }
}

/// Enter raw mode + alternate screen + mouse + bracketed paste + focus reporting,
/// save the window title (for the unread badge), then try to enable keyboard
/// enhancement. Returns the ratatui terminal and a guard that pops enhancement
/// flags on every exit path (including early `?`).
pub fn enter() -> io::Result<(MostrixTerminal, KeyboardEnhancementGuard)> {
    enable_raw_mode()?;
    let mut out = stdout();
    if let Err(e) = execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        EnableFocusChange,
        Print(PUSH_TITLE)
    ) {
        // Best-effort undo of whichever of the above actually applied.
        let _ = restore_modes(&mut out);
        let _ = disable_raw_mode();
        return Err(e);
    }
    let keyboard_enhancement = KeyboardEnhancementGuard::try_enable(&mut out);
    let backend = CrosstermBackend::new(out);
    match Terminal::new(backend) {
        Ok(terminal) => Ok((terminal, keyboard_enhancement)),
        Err(e) => {
            drop(keyboard_enhancement);
            let _ = restore_modes(&mut stdout());
            let _ = disable_raw_mode();
            Err(e)
        }
    }
}

/// Undo the terminal modes set by [`enter`] (everything except raw mode).
fn restore_modes(out: &mut impl Write) -> io::Result<()> {
    execute!(
        out,
        Print(POP_TITLE),
        DisableFocusChange,
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )
}

/// Restore the terminal to its pre-[`enter`] state.
///
/// Keyboard enhancement is popped by [`KeyboardEnhancementGuard`]'s `Drop`
/// (even if `disable_raw_mode` fails), so keep that guard alive until after
/// this returns or until the process unwinds.
pub fn leave(terminal: &mut MostrixTerminal) -> io::Result<()> {
    disable_raw_mode()?;
    restore_modes(terminal.backend_mut())?;
    terminal.show_cursor()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct ClipboardWriter {
        bytes: Vec<u8>,
        fail_write: bool,
        fail_flush: bool,
        flushed: bool,
    }

    impl Write for ClipboardWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.fail_write {
                return Err(io::Error::other("write failed"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushed = true;
            if self.fail_flush {
                Err(io::Error::other("flush failed"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn osc52_encodes_exact_text_and_flushes_without_control_injection() {
        let text = "  message\r\n\t\u{00e9}\u{754c}\x1b]52;c;untrusted\x07  ";
        let mut writer = ClipboardWriter::default();
        write_osc52(&mut writer, text, true, Some("xterm-256color")).unwrap();
        assert!(writer.flushed);
        assert!(writer.bytes.starts_with(b"\x1b]52;c;"));
        assert!(writer.bytes.ends_with(b"\x07"));
        let encoded = &writer.bytes[7..writer.bytes.len() - 1];
        assert!(encoded
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+/=".contains(byte)));
        assert_eq!(STANDARD.decode(encoded).unwrap(), text.as_bytes());
    }

    #[test]
    fn osc52_requires_interactive_output_and_rejects_dumb_terminals() {
        for (interactive, term) in [
            (false, Some("xterm")),
            (true, Some("dumb")),
            (true, Some("DUMB")),
        ] {
            let mut writer = ClipboardWriter::default();
            assert!(write_osc52(&mut writer, "private text", interactive, term).is_err());
            assert!(writer.bytes.is_empty());
            assert!(!writer.flushed);
        }
    }

    #[test]
    fn osc52_caps_raw_utf8_bytes_without_truncating() {
        let text = "\u{00e9}".repeat(MAX_OSC52_BYTES / 2);
        let mut writer = ClipboardWriter::default();
        write_osc52(&mut writer, &text, true, None).unwrap();
        assert_eq!(
            STANDARD
                .decode(&writer.bytes[7..writer.bytes.len() - 1])
                .unwrap(),
            text.as_bytes()
        );
        let mut writer = ClipboardWriter::default();
        assert!(write_osc52(&mut writer, &(text + "x"), true, None).is_err());
        assert!(writer.bytes.is_empty());
    }

    #[test]
    fn osc52_reports_write_and_flush_failures() {
        for fail_write in [true, false] {
            let mut writer = ClipboardWriter {
                fail_write,
                fail_flush: !fail_write,
                ..Default::default()
            };
            assert!(write_osc52(&mut writer, "text", true, Some("xterm")).is_err());
            assert_eq!(writer.flushed, !fail_write);
        }
    }

    #[test]
    fn keyboard_enhancement_guard_drop_does_not_panic() {
        let mut buf = Vec::new();
        let guard = KeyboardEnhancementGuard::try_enable(&mut buf);
        // Whether push succeeded depends on the writer; Drop must stay safe either way.
        drop(guard);
    }

    // Windows crossterm may route mode commands through WinAPI instead of the writer.
    #[cfg(not(windows))]
    #[test]
    fn restore_modes_restores_title_and_disables_focus_reporting() {
        let mut buf = Vec::new();
        restore_modes(&mut buf).expect("write to Vec");
        let out = String::from_utf8(buf).expect("utf8");
        assert!(out.starts_with(POP_TITLE));
        assert!(out.contains("\x1b[?1004l"), "focus reporting off");
    }
}
