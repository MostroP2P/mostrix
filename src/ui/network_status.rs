use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{interval, Duration};

use crate::util::any_relay_reachable;

#[derive(Debug, Clone)]
pub enum NetworkStatus {
    Offline(String),
    Online(String),
}

/// Spawn a background task that periodically checks relay reachability and
/// sends `NetworkStatus` transitions over the provided channel.
pub fn spawn_network_status_monitor(
    initial_relays: Vec<String>,
    network_status_tx: UnboundedSender<NetworkStatus>,
) {
    tokio::spawn(async move {
        let mut last_reachable: Option<bool> = None;
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            let relays = crate::settings::load_settings_from_disk()
                .map(|s| s.relays)
                .unwrap_or_else(|_| initial_relays.clone());
            let reachable = any_relay_reachable(&relays).await;
            if last_reachable == Some(reachable) {
                continue;
            }
            last_reachable = Some(reachable);
            let _ = if reachable {
                network_status_tx.send(NetworkStatus::Online("Internet restored".to_string()))
            } else {
                network_status_tx.send(NetworkStatus::Offline(
                    "No internet / relays unreachable".to_string(),
                ))
            };
        }
    });
}
