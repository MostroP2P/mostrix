use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

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
