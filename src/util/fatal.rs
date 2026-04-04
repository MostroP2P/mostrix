use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use futures::FutureExt;
use tokio::sync::mpsc::UnboundedSender;

static FATAL_ERROR_TX: OnceLock<UnboundedSender<String>> = OnceLock::new();
static FATAL_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Register a global sender for fatal (restart-required) errors.
///
/// This is intended for cross-cutting failures in background tasks where we need
/// to notify the UI loop and exit cleanly.
pub fn set_fatal_error_tx(tx: UnboundedSender<String>) -> Result<(), &'static str> {
    FATAL_ERROR_TX
        .set(tx)
        .map_err(|_| "fatal error sender already registered")
}

pub fn fatal_requested() -> bool {
    FATAL_REQUESTED.load(Ordering::Relaxed)
}

/// Request a user-facing fatal error + restart prompt.
///
/// Safe to call from any thread/task. First call wins; subsequent calls are ignored.
pub fn request_fatal_restart(message: impl Into<String>) {
    if FATAL_REQUESTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let msg = message.into();
    log::error!("[fatal] {}", msg);
    if let Some(tx) = FATAL_ERROR_TX.get() {
        let _ = tx.send(msg);
    }
}

/// Log panics (payload + location) and chain the previous hook. Does **not** call
/// [`request_fatal_restart`]; long-lived tasks should use [`catch_unwind_request_fatal_restart`]
/// at spawn boundaries when a panic should prompt restart.
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
            "[panic] unwound ({payload}) at {location} — see task-boundary `catch_unwind_request_fatal_restart` for restart prompts on critical workers"
        );
        previous_hook(info);
    }));
}

/// Run a `Future` that must not silently die: on unwind, log and call [`request_fatal_restart`].
/// Use inside `tokio::spawn` for critical background loops (DM router, fetch schedulers, etc.).
pub async fn catch_unwind_request_fatal_restart<F>(label: &str, future: F)
where
    F: std::future::Future<Output = ()> + Send,
{
    let result = std::panic::AssertUnwindSafe(future).catch_unwind().await;
    if result.is_err() {
        log::error!(
            "[panic] critical task {:?} unwound; requesting user-facing fatal restart",
            label
        );
        request_fatal_restart(format!(
            "A background task panicked ({label}).\n\
Please restart Mostrix after restoring connectivity if the issue persists."
        ));
    }
}
