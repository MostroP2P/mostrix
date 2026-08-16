use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{interval, Duration};

use crate::util::{any_relay_reachable, catch_unwind_request_fatal_restart};

#[derive(Debug, Clone)]
pub enum NetworkStatus {
    Offline(String),
    Online(String),
}

/// Spawn a background task that periodically checks relay reachability and
/// sends `NetworkStatus` transitions over the provided channel.
///
/// `initial_reachable` must match the startup check that may have set the offline
/// overlay. Seeding avoids a spurious `Online` on the first tick (tokio intervals
/// fire immediately) which would force a reconnect while already healthy.
pub fn spawn_network_status_monitor(
    initial_relays: Vec<String>,
    network_status_tx: UnboundedSender<NetworkStatus>,
    initial_reachable: bool,
) {
    tokio::spawn(async move {
        catch_unwind_request_fatal_restart("network status monitor", async move {
            let mut last_reachable = Some(initial_reachable);
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
        })
        .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_status_variants_carry_message() {
        let online = NetworkStatus::Online("Internet restored".to_string());
        let offline = NetworkStatus::Offline("No internet / relays unreachable".to_string());

        match &online {
            NetworkStatus::Online(msg) => assert_eq!(msg, "Internet restored"),
            NetworkStatus::Offline(_) => panic!("expected Online"),
        }
        match &offline {
            NetworkStatus::Offline(msg) => {
                assert_eq!(msg, "No internet / relays unreachable");
            }
            NetworkStatus::Online(_) => panic!("expected Offline"),
        }

        let debug_online = format!("{online:?}");
        let debug_offline = format!("{offline:?}");
        assert!(debug_online.contains("Online"));
        assert!(debug_online.contains("Internet restored"));
        assert!(debug_offline.contains("Offline"));
        assert!(debug_offline.contains("No internet"));
    }

    #[test]
    fn network_status_clone_preserves_payload() {
        let status = NetworkStatus::Offline("down".to_string());
        let cloned = status.clone();
        assert!(matches!(cloned, NetworkStatus::Offline(ref m) if m == "down"));
    }
}
