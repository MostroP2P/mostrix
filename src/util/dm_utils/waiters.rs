//! In-flight [`super::wait_for_dm`] waiters that survive DM-listener abort/reconnect.
//!
//! The trade DM listener task is aborted on connectivity restore, key reload, and
//! supervised respawn. Waiters used to live in that task's local `Vec`, so dropping
//! it canceled oneshots and surfaced a command failure while Mostro may already have
//! processed the action. This registry is process-wide: `wait_for_dm` inserts before
//! sending the protocol DM, and a rebuilt listener re-subscribes plus catch-up fetches
//! from each waiter's `since` timestamp.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use nostr_sdk::prelude::{Event, Keys, PublicKey, Timestamp};
use tokio::sync::oneshot;

use crate::util::request_fatal_restart;

pub(crate) const PENDING_WAITER_GC_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const MAX_PENDING_WAITERS: usize = 32;

/// Oneshot was dropped (listener/registry loss) — not a Mostro `CantDo` / protocol rejection.
pub const WAIT_FOR_DM_CANCELED_MSG: &str = "DM waiter canceled before receiving an event";

/// Cap reached before the protocol DM was sent; safe to retry.
pub const WAIT_FOR_DM_BUSY_MSG: &str = "Too many in-flight Mostro requests; please retry shortly";

/// Subtract from wall-clock at register so relay `since` / event `created_at` skew
/// does not drop a same-second Mostro reply.
const WAITER_SINCE_SKEW_SECS: u64 = 2;

pub(crate) struct PendingDmWaiter {
    pub(crate) id: u64,
    pub(crate) trade_keys: Keys,
    pub(crate) response_tx: oneshot::Sender<Event>,
    pub(crate) since: Timestamp,
    pub(crate) expected_request_id: Option<u64>,
}

/// Decrypt probe data only — oneshots stay in the registry until a match is taken by id.
#[derive(Clone)]
pub(crate) struct PendingWaiterSnapshot {
    pub(crate) id: u64,
    pub(crate) trade_keys: Keys,
    pub(crate) since: Timestamp,
    pub(crate) expected_request_id: Option<u64>,
}

pub(crate) struct PendingWaiterRegistry {
    waiters: Vec<PendingDmWaiter>,
    next_id: u64,
}

impl PendingWaiterRegistry {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self {
            waiters: Vec::new(),
            next_id: 1,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.waiters.len()
    }

    pub(crate) fn prune_closed(&mut self) -> usize {
        let before = self.waiters.len();
        self.waiters.retain(|w| !w.response_tx.is_closed());
        before.saturating_sub(self.waiters.len())
    }

    pub(crate) fn register(
        &mut self,
        trade_keys: Keys,
        response_tx: oneshot::Sender<Event>,
        since: Timestamp,
        expected_request_id: Option<u64>,
    ) -> Result<(), &'static str> {
        self.prune_closed();
        if self.waiters.len() >= MAX_PENDING_WAITERS {
            return Err(WAIT_FOR_DM_BUSY_MSG);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.waiters.push(PendingDmWaiter {
            id,
            trade_keys,
            response_tx,
            since,
            expected_request_id,
        });
        Ok(())
    }

    pub(crate) fn snapshot_candidates(&self) -> Vec<PendingWaiterSnapshot> {
        self.waiters
            .iter()
            .filter(|w| !w.response_tx.is_closed())
            .map(|w| PendingWaiterSnapshot {
                id: w.id,
                trade_keys: w.trade_keys.clone(),
                since: w.since,
                expected_request_id: w.expected_request_id,
            })
            .collect()
    }

    /// Remove and send on a matched waiter. Holds the registry lock for the send.
    pub(crate) fn take_and_send(&mut self, id: u64, event: Event) -> bool {
        let Some(pos) = self.waiters.iter().position(|w| w.id == id) else {
            return false;
        };
        let waiter = self.waiters.remove(pos);
        waiter.response_tx.send(event).is_ok()
    }

    #[cfg(test)]
    pub(crate) fn take_all(&mut self) -> Vec<PendingDmWaiter> {
        std::mem::take(&mut self.waiters)
    }

    /// Unique trade pubkeys with the oldest `since` per key (reconnect catch-up window).
    pub(crate) fn snapshot_targets(&self) -> Vec<(PublicKey, Timestamp)> {
        let mut by_pubkey: HashMap<PublicKey, Timestamp> = HashMap::new();
        for waiter in &self.waiters {
            if waiter.response_tx.is_closed() {
                continue;
            }
            let pubkey = waiter.trade_keys.public_key();
            by_pubkey
                .entry(pubkey)
                .and_modify(|existing| {
                    if waiter.since.as_secs() < existing.as_secs() {
                        *existing = waiter.since;
                    }
                })
                .or_insert(waiter.since);
        }
        by_pubkey.into_iter().collect()
    }
}

static PENDING_WAITERS: Mutex<PendingWaiterRegistry> = Mutex::new(PendingWaiterRegistry {
    waiters: Vec::new(),
    next_id: 1,
});

fn poisoned_registry() -> &'static str {
    request_fatal_restart(
        "Mostrix encountered an internal error (poisoned pending DM waiter lock). Please restart the app."
            .to_string(),
    );
    "pending waiter registry poisoned"
}

pub(crate) fn waiter_since_now() -> Timestamp {
    Timestamp::from(
        Timestamp::now()
            .as_secs()
            .saturating_sub(WAITER_SINCE_SKEW_SECS),
    )
}

pub(crate) fn event_created_at_meets_waiter_since(
    event_created_at: Timestamp,
    waiter_since: Timestamp,
) -> bool {
    event_created_at.as_secs() >= waiter_since.as_secs()
}

/// `None` expected (restore / unsolicited) matches any decoded id. `Some` requires an exact echo.
pub(crate) fn waiter_correlates_request_id(expected: Option<u64>, decoded: Option<u64>) -> bool {
    match expected {
        None => true,
        Some(want) => decoded == Some(want),
    }
}

pub(crate) fn protocol_dm_is_from_mostro(event: &Event, mostro_pubkey: PublicKey) -> bool {
    event.pubkey == mostro_pubkey
}

pub(crate) fn register_pending_waiter(
    trade_keys: Keys,
    response_tx: oneshot::Sender<Event>,
    expected_request_id: Option<u64>,
) -> Result<usize, &'static str> {
    let mut guard = PENDING_WAITERS.lock().map_err(|_| poisoned_registry())?;
    guard.register(
        trade_keys,
        response_tx,
        waiter_since_now(),
        expected_request_id,
    )?;
    Ok(guard.len())
}

pub(crate) fn prune_closed_pending_waiters() {
    match PENDING_WAITERS.lock() {
        Ok(mut guard) => {
            let pruned = guard.prune_closed();
            if pruned > 0 {
                log::debug!(
                    "[dm_listener] pruned {} closed waiter(s); pending_waiters={}",
                    pruned,
                    guard.len()
                );
            }
        }
        Err(_) => {
            let _ = poisoned_registry();
        }
    }
}

pub(crate) fn snapshot_pending_waiter_targets() -> Vec<(PublicKey, Timestamp)> {
    match PENDING_WAITERS.lock() {
        Ok(mut guard) => {
            guard.prune_closed();
            guard.snapshot_targets()
        }
        Err(_) => {
            let _ = poisoned_registry();
            Vec::new()
        }
    }
}

pub(crate) fn snapshot_pending_waiter_candidates() -> Vec<PendingWaiterSnapshot> {
    match PENDING_WAITERS.lock() {
        Ok(mut guard) => {
            guard.prune_closed();
            guard.snapshot_candidates()
        }
        Err(_) => {
            let _ = poisoned_registry();
            Vec::new()
        }
    }
}

pub(crate) fn take_and_send_pending_waiter(id: u64, event: Event) -> bool {
    match PENDING_WAITERS.lock() {
        Ok(mut guard) => guard.take_and_send(id, event),
        Err(_) => {
            let _ = poisoned_registry();
            false
        }
    }
}

#[cfg(test)]
pub(crate) fn reset_pending_waiters_for_tests() {
    if let Ok(mut guard) = PENDING_WAITERS.lock() {
        guard.waiters.clear();
    }
}

#[cfg(test)]
pub(crate) async fn lock_pending_waiters_for_tests() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::{set_dm_router_cmd_tx, wait_for_dm, WAIT_FOR_DM_TIMEOUT_MSG};
    use nostr_sdk::prelude::{EventBuilder, FinalizeEvent, Keys, Kind, Tag};
    use tokio::sync::oneshot;

    fn dummy_event(keys: &Keys) -> Event {
        EventBuilder::new(Kind::PrivateDirectMessage, "ciphertext")
            .tags([Tag::public_key(keys.public_key())])
            .finalize(keys)
            .expect("sign kind-14")
    }

    async fn async_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
        lock_pending_waiters_for_tests().await
    }

    #[test]
    fn listener_local_vec_drop_does_not_cancel_registry_owned_waiter() {
        let mut registry = PendingWaiterRegistry::new();
        let (tx, mut rx) = oneshot::channel::<Event>();
        registry
            .register(Keys::generate(), tx, waiter_since_now(), None)
            .expect("register");

        // New design: the listener does not own waiters. Aborting it drops only a
        // local vec — the registry still holds the oneshot.
        let listener_local: Vec<PendingDmWaiter> = Vec::new();
        drop(listener_local);

        let closed = rx.try_recv();
        assert!(
            matches!(closed, Err(oneshot::error::TryRecvError::Empty)),
            "waiter must stay pending after a listener-local drop; got {closed:?}"
        );
    }

    #[test]
    fn dropping_taken_waiters_cancels_oneshot_the_old_abort_bug() {
        let mut registry = PendingWaiterRegistry::new();
        let (tx, mut rx) = oneshot::channel::<Event>();
        registry
            .register(Keys::generate(), tx, waiter_since_now(), None)
            .expect("register");

        // Old design: waiters lived in the listener task. Abort => drop vec => cancel.
        let taken = registry.take_all();
        drop(taken);

        assert!(
            matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Closed)),
            "document MOSTRO-80: dropping listener-owned waiters cancels wait_for_dm"
        );
    }

    #[test]
    fn cap_rejects_before_protocol_send() {
        let mut registry = PendingWaiterRegistry::new();
        let mut keep_alive = Vec::new();
        for _ in 0..MAX_PENDING_WAITERS {
            let (tx, rx) = oneshot::channel::<Event>();
            keep_alive.push(rx);
            registry
                .register(Keys::generate(), tx, waiter_since_now(), None)
                .expect("under cap");
        }
        let (tx, _rx) = oneshot::channel::<Event>();
        assert_eq!(
            registry.register(Keys::generate(), tx, waiter_since_now(), None),
            Err(WAIT_FOR_DM_BUSY_MSG)
        );
    }

    #[test]
    fn prune_closed_removes_timed_out_waiters() {
        let mut registry = PendingWaiterRegistry::new();
        let (tx, rx) = oneshot::channel::<Event>();
        registry
            .register(Keys::generate(), tx, waiter_since_now(), None)
            .expect("register");
        drop(rx);
        assert_eq!(registry.prune_closed(), 1);
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn snapshot_targets_uses_oldest_since_per_pubkey() {
        let mut registry = PendingWaiterRegistry::new();
        let keys = Keys::generate();
        let older = Timestamp::from(1_000);
        let newer = Timestamp::from(2_000);
        let (tx_a, _rx_a) = oneshot::channel::<Event>();
        let (tx_b, _rx_b) = oneshot::channel::<Event>();
        registry
            .register(keys.clone(), tx_a, newer, None)
            .expect("register a");
        registry
            .register(keys.clone(), tx_b, older, None)
            .expect("register b");
        let targets = registry.snapshot_targets();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].0, keys.public_key());
        assert_eq!(targets[0].1, older);
    }

    #[test]
    fn snapshot_drop_does_not_cancel_oneshot() {
        let mut registry = PendingWaiterRegistry::new();
        let (tx, mut rx) = oneshot::channel::<Event>();
        registry
            .register(Keys::generate(), tx, waiter_since_now(), None)
            .expect("register");

        let snapshot = registry.snapshot_candidates();
        assert_eq!(snapshot.len(), 1);
        drop(snapshot);

        assert!(
            matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)),
            "probe snapshot must not own the oneshot"
        );
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn take_and_send_removes_only_matched_waiter() {
        let mut registry = PendingWaiterRegistry::new();
        let keys_a = Keys::generate();
        let keys_b = Keys::generate();
        let (tx_a, mut rx_a) = oneshot::channel::<Event>();
        let (tx_b, mut rx_b) = oneshot::channel::<Event>();
        registry
            .register(keys_a.clone(), tx_a, waiter_since_now(), None)
            .expect("a");
        registry
            .register(keys_b, tx_b, waiter_since_now(), None)
            .expect("b");
        let snap = registry.snapshot_candidates();
        let id_a = snap
            .iter()
            .find(|s| s.trade_keys.public_key() == keys_a.public_key())
            .expect("waiter a")
            .id;
        let event = dummy_event(&keys_a);

        assert!(registry.take_and_send(id_a, event.clone()));
        assert_eq!(registry.len(), 1);
        assert!(rx_a.try_recv().is_ok());
        assert!(matches!(
            rx_b.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn event_before_waiter_since_is_not_eligible() {
        let since = Timestamp::from(50);
        assert!(!event_created_at_meets_waiter_since(
            Timestamp::from(49),
            since
        ));
        assert!(event_created_at_meets_waiter_since(
            Timestamp::from(50),
            since
        ));
        assert!(event_created_at_meets_waiter_since(
            Timestamp::from(51),
            since
        ));
    }

    #[test]
    fn waiter_correlates_request_id_requires_exact_echo_when_expected() {
        assert!(waiter_correlates_request_id(None, None));
        assert!(waiter_correlates_request_id(None, Some(7)));
        assert!(waiter_correlates_request_id(Some(7), Some(7)));
        assert!(!waiter_correlates_request_id(Some(7), Some(8)));
        assert!(!waiter_correlates_request_id(Some(7), None));
    }

    #[test]
    fn protocol_dm_is_from_mostro_rejects_other_authors() {
        let mostro = Keys::generate();
        let attacker = Keys::generate();
        let from_mostro = dummy_event(&mostro);
        let from_attacker = dummy_event(&attacker);
        assert!(protocol_dm_is_from_mostro(
            &from_mostro,
            mostro.public_key()
        ));
        assert!(!protocol_dm_is_from_mostro(
            &from_attacker,
            mostro.public_key()
        ));
    }

    #[tokio::test]
    async fn global_register_survives_take_from_empty_listener_vec() {
        let _lock = async_test_lock().await;
        reset_pending_waiters_for_tests();
        let (tx, mut rx) = oneshot::channel::<Event>();
        let len = register_pending_waiter(Keys::generate(), tx, None).expect("register");
        assert_eq!(len, 1);
        drop(Vec::<PendingDmWaiter>::new());
        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        reset_pending_waiters_for_tests();
    }

    #[tokio::test]
    async fn wait_for_dm_times_out_when_router_channel_closed_after_register() {
        let _lock = async_test_lock().await;
        reset_pending_waiters_for_tests();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        set_dm_router_cmd_tx(tx).expect("router sender");
        drop(rx);

        let result = wait_for_dm(&Keys::generate(), Duration::from_millis(80), None, async {
            Ok(())
        })
        .await;
        let err = result.expect_err("expected timeout, not cancel");
        assert_eq!(
            err.to_string(),
            WAIT_FOR_DM_TIMEOUT_MSG,
            "closed router must not abort the waiter; reconnect rebuilds subscriptions"
        );
        reset_pending_waiters_for_tests();
    }

    #[tokio::test]
    async fn wait_for_dm_does_not_send_protocol_dm_when_registry_is_full() {
        let _lock = async_test_lock().await;
        reset_pending_waiters_for_tests();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        set_dm_router_cmd_tx(tx).expect("router sender");

        let mut keep_alive = Vec::new();
        for _ in 0..MAX_PENDING_WAITERS {
            let (wtx, wrx) = oneshot::channel();
            keep_alive.push(wrx);
            register_pending_waiter(Keys::generate(), wtx, None).expect("fill cap");
        }

        let sent = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sent_flag = std::sync::Arc::clone(&sent);
        let result = wait_for_dm(
            &Keys::generate(),
            Duration::from_millis(50),
            None,
            async move {
                sent_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), WAIT_FOR_DM_BUSY_MSG);
        assert!(
            !sent.load(std::sync::atomic::Ordering::SeqCst),
            "busy rejection must happen before the protocol DM is sent"
        );
        reset_pending_waiters_for_tests();
    }
}
