use std::time::Duration;

use futures::FutureExt;
use nostr_sdk::prelude::Client;
use tokio::net::TcpStream;
use tokio::time::timeout;

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
}
