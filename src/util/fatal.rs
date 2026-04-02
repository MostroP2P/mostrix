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

/// Route background task panics to the fatal UI path, then run the previous panic hook.
///
/// Call **after** [`set_fatal_error_tx`] so panics can notify the main loop.
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

        request_fatal_restart(format!(
            "A background task panicked ({payload}) at {location}.\n\
This can happen when no network is available.\n\
Please restart Mostrix after restoring internet connectivity."
        ));
        previous_hook(info);
    }));
}
