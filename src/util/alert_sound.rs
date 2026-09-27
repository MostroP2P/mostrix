//! Local notification sound for out-of-focus alerts.
//!
//! Many terminals (e.g. on Pop!_OS / GNOME) turn the bell into a silent taskbar
//! hint, so Mostrix can also play a short desktop sound itself. Players are
//! external commands tried in order; stdio is detached so nothing reaches the TUI.

use crate::settings::NotificationSettings;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

const FREEDESKTOP_SOUNDS: &[&str] = &[
    "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
    "/usr/share/sounds/freedesktop/stereo/message.oga",
    "/usr/share/sounds/freedesktop/stereo/bell.oga",
];
const MACOS_SOUND: &str = "/System/Library/Sounds/Glass.aiff";

/// One external player invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl SoundCommand {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }
}

/// Players to try, in order. A custom `sound_command` replaces the built-in chain.
pub fn sound_candidates(
    settings: &NotificationSettings,
    remote_session: bool,
    file_exists: impl Fn(&str) -> bool,
) -> Vec<SoundCommand> {
    if !settings.sound {
        return Vec::new();
    }
    let custom = settings.sound_command.trim();
    if !custom.is_empty() {
        return vec![if cfg!(windows) {
            SoundCommand::new("cmd", &["/C", custom])
        } else {
            SoundCommand::new("sh", &["-c", custom])
        }];
    }
    // Over SSH the built-in players would sound on the remote host, not at the user.
    if remote_session {
        return Vec::new();
    }
    if cfg!(target_os = "macos") {
        return vec![SoundCommand::new("afplay", &[MACOS_SOUND])];
    }
    if cfg!(windows) {
        return vec![SoundCommand::new(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "[System.Media.SystemSounds]::Asterisk.Play()",
            ],
        )];
    }
    let mut out = Vec::new();
    if let Some(file) = FREEDESKTOP_SOUNDS.iter().copied().find(|f| file_exists(f)) {
        out.push(SoundCommand::new("paplay", &[file]));
        out.push(SoundCommand::new("pw-play", &[file]));
    }
    out.push(SoundCommand::new(
        "canberra-gtk-play",
        &["--id", "message-new-instant"],
    ));
    out
}

fn is_remote_session() -> bool {
    std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some()
}

/// Fire-and-forget: try each candidate until one exits successfully.
/// Must be called from within the Tokio runtime.
pub fn play_alert_sound(settings: &NotificationSettings) {
    let candidates = sound_candidates(settings, is_remote_session(), |p| Path::new(p).exists());
    if candidates.is_empty() {
        return;
    }
    tokio::spawn(async move {
        for candidate in candidates {
            let status = Command::new(&candidate.program)
                .args(&candidate.args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            match status {
                Ok(s) if s.success() => return,
                Ok(s) => log::debug!("alert sound: {} exited with {s}", candidate.program),
                Err(e) => log::debug!("alert sound: {} unavailable: {e}", candidate.program),
            }
        }
        log::debug!("alert sound: no player succeeded");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> NotificationSettings {
        NotificationSettings::default()
    }

    #[test]
    fn sound_disabled_yields_no_candidates() {
        let s = NotificationSettings {
            sound: false,
            sound_command: "echo hi".into(),
            ..settings()
        };
        assert!(sound_candidates(&s, false, |_| true).is_empty());
    }

    #[test]
    fn custom_command_wins_and_runs_even_over_ssh() {
        let s = NotificationSettings {
            sound_command: "  paplay ~/ding.oga ".into(),
            ..settings()
        };
        let c = sound_candidates(&s, true, |_| true);
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0].args.last().map(String::as_str),
            Some("paplay ~/ding.oga")
        );
    }

    #[test]
    fn remote_session_skips_builtin_players() {
        assert!(sound_candidates(&settings(), true, |_| true).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_prefers_first_existing_freedesktop_sound() {
        let c = sound_candidates(&settings(), false, |p| p.ends_with("message.oga"));
        assert_eq!(
            c[0],
            SoundCommand::new(
                "paplay",
                &["/usr/share/sounds/freedesktop/stereo/message.oga"]
            )
        );
        assert_eq!(c[1].program, "pw-play");
        assert_eq!(c[2].program, "canberra-gtk-play");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_without_sound_files_falls_back_to_canberra() {
        let c = sound_candidates(&settings(), false, |_| false);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].program, "canberra-gtk-play");
    }

    #[tokio::test]
    async fn play_with_sound_disabled_spawns_nothing() {
        play_alert_sound(&NotificationSettings {
            sound: false,
            ..settings()
        });
    }
}
