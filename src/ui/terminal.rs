//! Crossterm / ratatui terminal lifecycle for the Mostrix TUI.
//!
//! Owns enter/leave of raw mode, alternate screen, mouse capture, bracketed
//! paste, and best-effort keyboard-enhancement flags (Ctrl+I vs Tab).

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
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

/// Enter raw mode + alternate screen + mouse + bracketed paste, then try to
/// enable keyboard enhancement. Returns the ratatui terminal and a guard that
/// pops enhancement flags on every exit path (including early `?`).
pub fn enter() -> io::Result<(MostrixTerminal, KeyboardEnhancementGuard)> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let keyboard_enhancement = KeyboardEnhancementGuard::try_enable(&mut out);
    let backend = CrosstermBackend::new(out);
    let terminal = Terminal::new(backend)?;
    Ok((terminal, keyboard_enhancement))
}

/// Restore the terminal to its pre-[`enter`] state.
///
/// Keyboard enhancement is popped by [`KeyboardEnhancementGuard`]'s `Drop`
/// (even if `disable_raw_mode` fails), so keep that guard alive until after
/// this returns or until the process unwinds.
pub fn leave(terminal: &mut MostrixTerminal) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
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
}
