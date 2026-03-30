use std::time::Duration;

use futures::FutureExt;
use nostr_sdk::prelude::Client;
use tokio::net::TcpStream;
use tokio::time::timeout;

const RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

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

/// Connect the `nostr-sdk` client, but never let a panic crash the app.
///
/// Some `nostr-sdk` connect paths have historically panicked in "no network" environments.
/// This wrapper turns that into an error so the UI can keep running (offline overlay / retry).
pub async fn connect_client_safely(client: &Client) -> Result<(), String> {
    let result = std::panic::AssertUnwindSafe(client.connect())
        .catch_unwind()
        .await;
    match result {
        Ok(()) => Ok(()),
        Err(_) => Err("nostr client connect panicked".to_string()),
    }
}
