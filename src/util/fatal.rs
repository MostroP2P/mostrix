//! Fatal vs recoverable background-task notifications for the TUI main loop.
//!
//! [`request_fatal_restart`] is for **unrecoverable** state (e.g. poisoned mutexes):
//! the main loop aborts all workers and shows a sticky restart prompt.
//! Panic or unexpected exit of a single critical task uses [`supervise_critical_task`]
//! (or the DM/chat helpers in [`crate::util::supervised_listener`]): only that task
//! respawns with backoff, and the UI shows a non-blocking [`FatalNotify::TaskAlarm`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use futures::FutureExt;
use tokio::sync::mpsc::UnboundedSender;

use crate::util::chat_listener::ChatRouterCmd;
use crate::util::dm_utils::OrderDmSubscriptionCmd;

/// Notification from background tasks to the main UI loop.
#[derive(Debug, Clone)]
pub enum FatalNotify {
    /// A background task panicked or exited unexpectedly; it will respawn with backoff.
    TaskAlarm(String),
    /// The task recovered and is running again — clear any task alarm banner.
    TaskResumed(String),
    /// DM router sender replaced after listener respawn; main must update its clone.
    DmRouterSender(UnboundedSender<OrderDmSubscriptionCmd>),
    /// Chat router sender replaced after listener respawn; main must update its clone.
    ChatRouterSender(UnboundedSender<ChatRouterCmd>),
    /// Unrecoverable error — user must restart the process.
    RestartRequired(String),
}

static FATAL_NOTIFY_TX: OnceLock<UnboundedSender<FatalNotify>> = OnceLock::new();
static FATAL_REQUESTED: AtomicBool = AtomicBool::new(false);

const INITIAL_BACKOFF_SECS: u64 = 1;
const MAX_BACKOFF_SECS: u64 = 60;
const BACKOFF_RESET_AFTER: Duration = Duration::from_secs(30);

/// Register a global sender for fatal / task-alarm notifications.
///
/// This is intended for cross-cutting failures in background tasks where we need
/// to notify the UI loop without necessarily tearing down the whole runtime.
pub fn set_fatal_error_tx(tx: UnboundedSender<FatalNotify>) -> Result<(), &'static str> {
    FATAL_NOTIFY_TX
        .set(tx)
        .map_err(|_| "fatal error sender already registered")
}

pub fn fatal_requested() -> bool {
    FATAL_REQUESTED.load(Ordering::Relaxed)
}

fn send_notify(event: FatalNotify) {
    if let Some(tx) = FATAL_NOTIFY_TX.get() {
        let _ = tx.send(event);
    }
}

pub(crate) fn send_fatal_notify(event: FatalNotify) {
    send_notify(event);
}

/// Request a user-facing fatal error + restart prompt.
///
/// Safe to call from any thread/task. First call wins; subsequent calls are ignored.
/// Aborts all background work in the main loop — use only for unrecoverable state
/// (e.g. poisoned mutexes).
pub fn request_fatal_restart(message: impl Into<String>) {
    if FATAL_REQUESTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let msg = message.into();
    log::error!("[fatal] {}", msg);
    send_notify(FatalNotify::RestartRequired(msg));
}

fn request_task_alarm(message: impl Into<String>) {
    let msg = message.into();
    log::warn!("[task-alarm] {}", msg);
    send_notify(FatalNotify::TaskAlarm(msg));
}

fn request_task_resumed(label: &str) {
    log::info!("[task-resumed] {:?}", label);
    send_notify(FatalNotify::TaskResumed(label.to_string()));
}

/// Compute the next backoff delay after a failure, optionally resetting after a long healthy run.
pub fn next_backoff_secs(current: u64, healthy_run: Duration) -> u64 {
    if healthy_run >= BACKOFF_RESET_AFTER {
        INITIAL_BACKOFF_SECS
    } else {
        current.saturating_mul(2).min(MAX_BACKOFF_SECS)
    }
}

/// Run a critical background loop with panic/exit recovery: on failure, notify the UI,
/// back off, and respawn **only this task**. Other workers keep running.
///
/// Stops permanently when [`request_fatal_restart`] has been called (e.g. poisoned lock)
/// or when the enclosing [`JoinHandle`] is aborted (reload/shutdown).
pub async fn supervise_critical_task<F, Fut>(label: &'static str, mut run: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()> + Send,
{
    let mut backoff_secs = INITIAL_BACKOFF_SECS;
    loop {
        if fatal_requested() {
            break;
        }
        let started = Instant::now();
        request_task_resumed(label);
        let result = std::panic::AssertUnwindSafe(run()).catch_unwind().await;
        if fatal_requested() {
            break;
        }
        match result {
            Ok(()) => {
                log::warn!(
                    "critical task {:?} exited unexpectedly; respawning after {}s backoff",
                    label,
                    backoff_secs
                );
            }
            Err(_) => {
                log::error!(
                    "[panic] critical task {:?} unwound; respawning after {}s backoff",
                    label,
                    backoff_secs
                );
            }
        }
        request_task_alarm(format!(
            "Background task \"{label}\" stopped unexpectedly and is restarting (retry in {backoff_secs}s).\n\
Other protocol channels remain active."
        ));
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = next_backoff_secs(backoff_secs, started.elapsed());
    }
}

/// Log panics (payload + location) and chain the previous hook. Does **not** call
/// [`request_fatal_restart`]; long-lived tasks should use [`supervise_critical_task`]
/// at spawn boundaries when a panic should trigger per-task respawn.
///
/// Call **after** [`set_fatal_error_tx`] if other code paths still need the sender registered
/// before any task runs.
pub fn install_background_panic_hook() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("panic");
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        log::error!(
            "[panic] unwound ({payload}) at {location} — see task-boundary `supervise_critical_task` for per-task respawn on critical workers"
        );
        previous_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_backoff_doubles_until_cap() {
        assert_eq!(next_backoff_secs(1, Duration::from_secs(1)), 2);
        assert_eq!(next_backoff_secs(32, Duration::from_secs(1)), 60);
        assert_eq!(next_backoff_secs(60, Duration::from_secs(1)), 60);
    }

    #[test]
    fn next_backoff_resets_after_long_healthy_run() {
        assert_eq!(
            next_backoff_secs(60, BACKOFF_RESET_AFTER),
            INITIAL_BACKOFF_SECS
        );
    }

    #[test]
    fn fatal_notify_string_variants_differ() {
        assert_ne!(
            format!("{:?}", FatalNotify::TaskAlarm("a".into())),
            format!("{:?}", FatalNotify::TaskResumed("a".into()))
        );
    }
}
