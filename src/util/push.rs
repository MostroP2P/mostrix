//! Sender-triggered wake through mostro-push-server (`POST /api/notify`).
//!
//! Kind-14 chat is `p`-tagged to `pub(K_conv)`, which the push server's relay
//! listener cannot match, so the sender asks it to wake the recipient's trade
//! pubkey. Content-free, fire-and-forget, never a reason for a send to fail.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const NOTIFY_TIMEOUT: Duration = Duration::from_secs(10);
/// A burst of messages to one recipient costs one wake.
const NOTIFY_DEBOUNCE: Duration = Duration::from_secs(10);

fn last_notify() -> &'static Mutex<HashMap<String, Instant>> {
    static LAST: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    LAST.get_or_init(Default::default)
}

/// Pure, unit-testable: may we ring this pubkey now?
pub fn notify_allowed(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|t| now.duration_since(t) >= NOTIFY_DEBOUNCE)
}

/// Spawn a wake for `recipient_trade_pubkey` (64 hex). Never awaited by the send.
pub fn wake_recipient(server_url: &str, recipient_trade_pubkey: &str) {
    let pubkey = recipient_trade_pubkey.to_ascii_lowercase();
    if server_url.is_empty()
        || pubkey.len() != 64
        || !pubkey.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return;
    }
    // Record before the request, so messages sent while one is in flight don't each ring.
    {
        let mut last = last_notify().lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if !notify_allowed(last.get(&pubkey).copied(), now) {
            return;
        }
        last.insert(pubkey.clone(), now);
    }
    let url = format!("{}/api/notify", server_url.trim_end_matches('/'));
    tokio::spawn(async move {
        let client = match reqwest::Client::builder().timeout(NOTIFY_TIMEOUT).build() {
            Ok(c) => c,
            Err(e) => return log::warn!("[push] notify client: {e}"),
        };
        match client
            .post(&url)
            .json(&serde_json::json!({ "trade_pubkey": pubkey }))
            .send()
            .await
        {
            Ok(r) if r.status() == reqwest::StatusCode::BAD_REQUEST => {
                log::warn!("[push] notify rejected as malformed (client bug)")
            }
            Ok(r) => log::debug!("[push] notify: {}", r.status()),
            Err(e) => log::debug!("[push] notify failed: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_allowed_when_never_rung() {
        assert!(notify_allowed(None, Instant::now()));
    }

    #[test]
    fn notify_blocked_within_debounce_window() {
        let now = Instant::now();
        let last = now - (NOTIFY_DEBOUNCE / 2);
        assert!(!notify_allowed(Some(last), now));
    }

    #[test]
    fn notify_allowed_once_debounce_elapsed() {
        let now = Instant::now();
        let last = now - NOTIFY_DEBOUNCE;
        assert!(notify_allowed(Some(last), now));
    }
}
