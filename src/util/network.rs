use std::collections::BTreeSet;
use std::time::Duration;

use futures::stream::{FuturesUnordered, StreamExt};
use futures::FutureExt;
use nostr_sdk::prelude::{Client, Event, Filter, RelayUrl};
use tokio::net::TcpStream;
use tokio::time::{sleep_until, timeout, Instant};

const RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Bounded wait for at least one relay to reach `Connected` (reconnect / key reload).
const RELAY_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);

fn relay_host_port(relay: &str) -> Option<(String, u16)> {
    let relay = relay.trim();
    let rest = relay
        .strip_prefix("wss://")
        .or_else(|| relay.strip_prefix("ws://"))?;

    let host_port = rest.split('/').next()?.trim();
    if host_port.is_empty() {
        return None;
    }

    if let Some((host, port_str)) = host_port.rsplit_once(':') {
        let host = host.trim();
        let port_str = port_str.trim();
        if host.is_empty() || port_str.is_empty() {
            return None;
        }
        let port: u16 = port_str.parse().ok()?;
        return Some((host.to_string(), port));
    }

    let default_port = if relay.starts_with("ws://") { 80 } else { 443 };
    Some((host_port.to_string(), default_port))
}

/// Best-effort "offline" detection.
///
/// Returns `true` if at least one configured relay host:port accepts a TCP connection
/// within a short timeout. This avoids calling `nostr-sdk` connect paths that may panic
/// when the machine has no network.
///
/// TCP success is not a live Nostr session. Callers that rebuild the DM listener
/// must [`connect_and_wait_for_relay`] before aborting in-flight `wait_for_dm` waiters.
pub async fn any_relay_reachable(relays: &[String]) -> bool {
    for relay in relays {
        let Some((host, port)) = relay_host_port(relay) else {
            continue;
        };
        let addr = format!("{host}:{port}");
        let attempt = timeout(RELAY_CONNECT_TIMEOUT, TcpStream::connect(addr)).await;
        if matches!(attempt, Ok(Ok(_))) {
            return true;
        }
    }
    false
}

/// True when at least one pool relay is in [`nostr_sdk::prelude::RelayStatus::Connected`].
pub async fn any_nostr_relay_connected(client: &Client) -> bool {
    client
        .relays()
        .await
        .values()
        .any(|relay| relay.status().is_connected())
}

/// Relay URLs currently in the `Connected` state.
///
/// A dead/unreachable relay is simply absent here; nostr-sdk's background task flips
/// it back to `Connected` on its own once it recovers, so no manual re-probe is needed.
pub async fn connected_relay_urls(client: &Client) -> Vec<RelayUrl> {
    client
        .relays()
        .await
        .into_iter()
        .filter_map(|(url, relay)| relay.status().is_connected().then_some(url))
        .collect()
}

/// Grace window after the first relay returns data+EOSE before dropping still-pending
/// relays. A connected-but-silent relay (never sends EOSE) is cancelled once this elapses.
const CONNECTED_FETCH_GRACE: Duration = Duration::from_secs(2);

/// `fetch_events` that returns as soon as one connected relay is responsive.
///
/// `RelayStatus::Connected` is not a responsiveness signal — a relay can keep its socket
/// open while ignoring `REQ` / never sending EOSE. nostr-sdk's default `ExitOnEOSE`
/// aggregation waits for **every** targeted relay, so one silent relay would otherwise
/// hold the call until `timeout` even after a healthy relay already returned data+EOSE.
///
/// This races an independent single-relay `fetch_events` per connected relay and returns
/// the union of everything that completed by (first relay with data + [`CONNECTED_FETCH_GRACE`]),
/// bounded by `timeout`. Falls back to a pool-wide fetch only when **no** relay is connected.
pub async fn fetch_events_connected_only(
    client: &Client,
    filter: Filter,
    timeout: Duration,
) -> anyhow::Result<BTreeSet<Event>> {
    let connected = connected_relay_urls(client).await;
    // Pool-wide (auto-target) fetch only when nothing is connected: its automatic target is
    // every readable relay, which would re-include a disconnected/reconnecting peer and
    // reintroduce the hard-timeout stall. With 1+ connected we stay on the scoped single-relay
    // path (the collector handles a single relay fine).
    if connected.is_empty() {
        return Ok(client.fetch_events(filter).timeout(timeout).await?);
    }
    let fetches = connected.into_iter().map(|url| {
        let client = client.clone();
        let filter = filter.clone();
        async move {
            client
                .fetch_events(nostr_sdk::client::ReqTarget::single(url, vec![filter]))
                .timeout(timeout)
                .await
                .map_err(anyhow::Error::from)
        }
    });
    Ok(collect_first_ready_then_grace(fetches, CONNECTED_FETCH_GRACE, timeout).await)
}

/// Drive per-relay fetches concurrently; return the union of results that arrive within
/// `grace` of the first result **that carries data**, or by `hard_timeout`, whichever comes
/// first.
///
/// The grace is armed only by a non-empty `Ok`: nostr-sdk suppresses per-relay stream errors
/// and returns `Ok(empty)` for a fast CLOSED/auth-failing/empty relay, so an empty result must
/// not cut off a slower relay that holds the only data. A future that never resolves (silent
/// relay) is dropped when the grace/timeout fires, so it cannot extend latency.
async fn collect_first_ready_then_grace<F>(
    fetches: impl IntoIterator<Item = F>,
    grace: Duration,
    hard_timeout: Duration,
) -> BTreeSet<Event>
where
    F: std::future::Future<Output = anyhow::Result<BTreeSet<Event>>>,
{
    let mut pending: FuturesUnordered<F> = fetches.into_iter().collect();
    let mut union: BTreeSet<Event> = BTreeSet::new();
    let mut grace_deadline: Option<Instant> = None;
    let overall = tokio::time::sleep(hard_timeout);
    tokio::pin!(overall);

    loop {
        tokio::select! {
            // Hard cap first, then drain ready results, then the post-first-data grace window.
            biased;
            _ = &mut overall => break,
            next = pending.next() => match next {
                None => break,
                Some(Ok(events)) => {
                    let carried_data = !events.is_empty();
                    union.extend(events);
                    // Empty Ok (possibly a suppressed CLOSED/auth-fail) must not arm the grace,
                    // or a fast empty relay could drop a slower relay holding the only data.
                    if carried_data && grace_deadline.is_none() {
                        grace_deadline = Some(Instant::now() + grace);
                    }
                }
                Some(Err(_)) => {}
            },
            _ = async {
                match grace_deadline {
                    Some(deadline) => sleep_until(deadline).await,
                    None => std::future::pending::<()>().await,
                }
            } => break,
        }
    }
    union
}

/// Connect the `nostr-sdk` client, but never let a panic crash the app.
///
/// Some `nostr-sdk` connect paths have historically panicked in "no network" environments.
/// This wrapper turns that into an error so the UI can keep running (offline overlay / retry).
///
/// `Client::connect().await` only *starts* connection attempts (`and_wait(None)`). It is not
/// proof that a relay can receive DMs. Use [`connect_and_wait_for_relay`] before tearing down
/// a live listener.
pub async fn connect_client_safely(client: &Client) -> Result<(), String> {
    let result = std::panic::AssertUnwindSafe(async {
        client.connect().await;
    })
    .catch_unwind()
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(_) => Err("nostr client connect panicked".to_string()),
    }
}

/// Start relay connections and wait until at least one is `Connected`, or `timeout`.
pub(crate) async fn connect_and_wait_for_relay_within(
    client: &Client,
    handshake_timeout: Duration,
) -> Result<(), String> {
    let result = std::panic::AssertUnwindSafe(async {
        client.connect().and_wait(handshake_timeout).await;
    })
    .catch_unwind()
    .await;
    match result {
        Ok(()) => {}
        Err(_) => return Err("nostr client connect panicked".to_string()),
    }
    if any_nostr_relay_connected(client).await {
        Ok(())
    } else {
        Err("No Nostr relay reached Connected".to_string())
    }
}

/// [`connect_and_wait_for_relay_within`] with the reconnect/key-reload handshake budget.
pub async fn connect_and_wait_for_relay(client: &Client) -> Result<(), String> {
    connect_and_wait_for_relay_within(client, RELAY_HANDSHAKE_TIMEOUT).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr_sdk::prelude::Client;

    #[tokio::test]
    async fn unconnected_client_reports_no_connected_relay() {
        let client = Client::default();
        assert!(!any_nostr_relay_connected(&client).await);
    }

    #[tokio::test]
    async fn handshake_wait_fails_when_no_relay_becomes_connected() {
        let client = Client::default();
        client
            .add_relay("ws://127.0.0.1:1")
            .await
            .expect("add closed-port relay");
        let err = connect_and_wait_for_relay_within(&client, Duration::from_millis(200))
            .await
            .expect_err("closed port must not reach Connected");
        assert!(
            err.contains("Connected") || err.contains("panicked"),
            "unexpected handshake error: {err}"
        );
        assert!(!any_nostr_relay_connected(&client).await);
    }

    #[tokio::test]
    async fn connected_relay_urls_empty_without_connection() {
        let client = Client::default();
        assert!(connected_relay_urls(&client).await.is_empty());
    }

    #[tokio::test]
    async fn connected_relay_urls_excludes_unconnected_relay() {
        let client = Client::default();
        client
            .add_relay("ws://127.0.0.1:1")
            .await
            .expect("add closed-port relay");
        // Added but never reaches Connected: must be excluded from the healthy set.
        assert!(connected_relay_urls(&client).await.is_empty());
    }

    fn sample_event(note: &str) -> Event {
        use nostr_sdk::prelude::{EventBuilder, FinalizeEvent, Keys, Kind};
        let keys = Keys::generate();
        EventBuilder::new(Kind::TextNote, note)
            .finalize(&keys)
            .expect("sign text note")
    }

    // P1 regression: a connected-but-silent relay (never sends EOSE) must not hold the
    // aggregate until the hard timeout once a healthy relay returned data+EOSE.
    #[tokio::test]
    async fn first_healthy_relay_returns_without_waiting_for_silent_relay() {
        use futures::future::{BoxFuture, FutureExt};

        let healthy_event = sample_event("healthy");
        let expected = healthy_event.id;
        let healthy: BoxFuture<'static, anyhow::Result<BTreeSet<Event>>> =
            async move { Ok(BTreeSet::from([healthy_event])) }.boxed();
        let silent: BoxFuture<'static, anyhow::Result<BTreeSet<Event>>> =
            std::future::pending().boxed();

        let start = Instant::now();
        let union = timeout(
            Duration::from_secs(5),
            collect_first_ready_then_grace(
                vec![healthy, silent],
                Duration::from_millis(20),
                Duration::from_secs(30),
            ),
        )
        .await
        .expect("must not wait for the silent relay or the 30s hard timeout");

        assert_eq!(union.len(), 1);
        assert!(union.iter().any(|e| e.id == expected));
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "returned in {:?}, expected well under the hard timeout",
            start.elapsed()
        );
    }

    // The grace window still merges a second healthy relay that lands shortly after the first.
    #[tokio::test]
    async fn grace_window_merges_second_healthy_relay() {
        use futures::future::{BoxFuture, FutureExt};

        let first = sample_event("first");
        let second = sample_event("second");
        let (id_a, id_b) = (first.id, second.id);
        let fast: BoxFuture<'static, anyhow::Result<BTreeSet<Event>>> =
            async move { Ok(BTreeSet::from([first])) }.boxed();
        let slightly_slower: BoxFuture<'static, anyhow::Result<BTreeSet<Event>>> = async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(BTreeSet::from([second]))
        }
        .boxed();

        let union = collect_first_ready_then_grace(
            vec![fast, slightly_slower],
            Duration::from_millis(200),
            Duration::from_secs(30),
        )
        .await;

        assert_eq!(union.len(), 2);
        assert!(union.iter().any(|e| e.id == id_a));
        assert!(union.iter().any(|e| e.id == id_b));
    }

    // Blocker-2 regression: a fast Ok(empty) (e.g. CLOSED/auth-fail suppressed to empty) must
    // not arm the grace and drop a slower relay that carries the only data.
    #[tokio::test]
    async fn empty_first_result_does_not_drop_late_data_relay() {
        use futures::future::{BoxFuture, FutureExt};

        let data_event = sample_event("late-data");
        let expected = data_event.id;
        let empty_fast: BoxFuture<'static, anyhow::Result<BTreeSet<Event>>> =
            async { Ok(BTreeSet::new()) }.boxed();
        let data_slow: BoxFuture<'static, anyhow::Result<BTreeSet<Event>>> = async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Ok(BTreeSet::from([data_event]))
        }
        .boxed();

        // Grace (20ms) is shorter than the data delay (50ms): if the empty result armed the
        // grace, the data relay would be dropped and the union would be empty.
        let union = timeout(
            Duration::from_secs(5),
            collect_first_ready_then_grace(
                vec![empty_fast, data_slow],
                Duration::from_millis(20),
                Duration::from_secs(30),
            ),
        )
        .await
        .expect("must not hang");

        assert_eq!(union.len(), 1);
        assert!(union.iter().any(|e| e.id == expected));
    }
}
