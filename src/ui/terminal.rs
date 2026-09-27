//! Crossterm / ratatui terminal lifecycle for the Mostrix TUI.
//!
//! Owns enter/leave of raw mode, alternate screen, mouse capture, bracketed
//! paste, focus reporting, the saved window title, and best-effort
//! keyboard-enhancement flags (Ctrl+I vs Tab).

use crate::ui::terminal_alert::{POP_TITLE, PUSH_TITLE};
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
use std::io::{self, stdout, Stdout, Write};

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
