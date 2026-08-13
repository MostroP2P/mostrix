//! Caller-owned chat security controls from the Mostro P2P chat spec.
//!
//! mostro-core's [`unwrap_chat_message`](mostro_core::chat::unwrap_chat_message)
//! performs the cheap cryptographic/structural checks. Clients must still:
//! * drop duplicate **outer** event ids (bounded LRU, pre-decrypt)
//! * enforce a per-conversation **token bucket** (~30/min, burst 60) before decrypt
//! * durably dedupe **inner** event ids (see [`crate::ui::helpers::chat_storage`])
//! * isolate chat flood from the UI / DM path (bounded update channels)

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::time::Instant;

use nostr_sdk::prelude::EventId;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::Sender;

/// Sustained chat event rate per conversation (spec recommendation).
pub const CHAT_RATE_PER_MINUTE: f64 = 30.0;
/// Burst capacity for the per-conversation token bucket.
pub const CHAT_RATE_BURST: f64 = 60.0;
/// Bounded outer-id LRU capacity (duplicate relay deliveries only).
pub const CHAT_SEEN_OUTER_CAP: usize = 4096;
/// Max queued chat-update batches toward the UI; excess is dropped.
pub const CHAT_UPDATE_CAPACITY: usize = 128;

/// Token bucket that bounds how much decrypt work a counterparty can force.
#[derive(Debug)]
pub struct ChatRateLimiter {
    tokens: f64,
    last: Instant,
}

impl ChatRateLimiter {
    pub fn new() -> Self {
        Self {
            tokens: CHAT_RATE_BURST,
            last: Instant::now(),
        }
    }

    /// Returns `true` when one event may proceed to decrypt.
    pub fn allow(&mut self) -> bool {
        self.allow_at(Instant::now())
    }

    /// Testable variant of [`Self::allow`].
    pub fn allow_at(&mut self, now: Instant) -> bool {
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * (CHAT_RATE_PER_MINUTE / 60.0)).min(CHAT_RATE_BURST);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl Default for ChatRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-conversation rate limiters keyed by chat identity.
pub struct ChatRateLimiters<K: Eq + Hash> {
    buckets: HashMap<K, ChatRateLimiter>,
}

impl<K: Eq + Hash> Default for ChatRateLimiters<K> {
    fn default() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Clone> ChatRateLimiters<K> {
    pub fn allow(&mut self, key: &K) -> bool {
        self.buckets.entry(key.clone()).or_default().allow()
    }

    pub fn remove(&mut self, key: &K) {
        self.buckets.remove(key);
    }
}

/// Fixed-capacity set of recently seen outer event ids (oldest evicted first).
///
/// Spec: cheap pre-decryption filter against duplicate relay deliveries — not a
/// security boundary. Dropping old entries is safe.
#[derive(Debug)]
pub struct OuterIdLru {
    set: HashSet<EventId>,
    order: VecDeque<EventId>,
    cap: usize,
}

impl OuterIdLru {
    pub fn new(cap: usize) -> Self {
        Self {
            set: HashSet::new(),
            order: VecDeque::with_capacity(cap.min(64)),
            cap: cap.max(1),
        }
    }

    /// Records `id`, returning `true` when it had not been seen (or was evicted).
    pub fn insert(&mut self, id: EventId) -> bool {
        if !self.set.insert(id) {
            return false;
        }
        self.order.push_back(id);
        while self.order.len() > self.cap {
            if let Some(evicted) = self.order.pop_front() {
                self.set.remove(&evicted);
            }
        }
        true
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

impl Default for OuterIdLru {
    fn default() -> Self {
        Self::new(CHAT_SEEN_OUTER_CAP)
    }
}

/// Try to enqueue a chat update; on a full bounded channel, drop and warn.
///
/// Keeps a chat flood from growing unbounded memory or stalling the UI loop.
pub fn try_emit_chat_update<T: std::fmt::Debug>(
    tx: &Sender<Result<T, anyhow::Error>>,
    update: T,
    label: &str,
) -> bool {
    match tx.try_send(Ok(update)) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            log::warn!("[chat] dropping {label} update: UI queue full (capacity under pressure)");
            false
        }
        Err(TrySendError::Closed(_)) => {
            log::debug!("[chat] {label} update channel closed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::prelude::EventId;
    use std::time::Duration;
    use tokio::sync::mpsc;

    fn fake_event_id(byte: u8) -> EventId {
        EventId::from_slice(&[byte; 32]).expect("event id")
    }

    #[test]
    fn rate_limiter_allows_burst_then_rejects() {
        let mut lim = ChatRateLimiter::new();
        let t0 = Instant::now();
        for _ in 0..(CHAT_RATE_BURST as u32) {
            assert!(lim.allow_at(t0), "burst should be allowed");
        }
        assert!(!lim.allow_at(t0), "over burst must be rejected");
        // After ~2 seconds at 30/min we regain one token.
        let later = t0 + Duration::from_secs_f64(2.1);
        assert!(lim.allow_at(later), "tokens refill over time");
    }

    #[test]
    fn outer_lru_rejects_duplicates_and_evicts() {
        let mut lru = OuterIdLru::new(2);
        let a = fake_event_id(1);
        let b = fake_event_id(2);
        let c = fake_event_id(3);
        assert!(lru.insert(a));
        assert!(!lru.insert(a));
        assert!(lru.insert(b));
        assert!(lru.insert(c)); // evicts a
        assert_eq!(lru.len(), 2);
        assert!(lru.insert(a)); // a was evicted, accepted again
    }

    #[tokio::test]
    async fn try_emit_drops_when_queue_full_without_blocking() {
        let (tx, mut rx) = mpsc::channel::<Result<&'static str, anyhow::Error>>(1);
        assert!(try_emit_chat_update(&tx, "first", "test"));
        assert!(!try_emit_chat_update(&tx, "second", "test"));
        // First item still receivable; flood did not block the caller.
        assert_eq!(rx.recv().await.unwrap().unwrap(), "first");
        assert!(try_emit_chat_update(&tx, "third", "test"));
    }
}
