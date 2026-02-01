use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{interval_at, Duration, Instant};

use crate::models::AdminDispute;
use crate::ui::{AdminChatSharedKey, AdminChatUpdate, ChatParty};
use crate::util::chat_utils::fetch_admin_chat_updates;

use super::{get_disputes, get_orders};

/// Result of starting the fetch scheduler
/// Contains shared state for orders and disputes that are periodically updated
pub struct FetchSchedulerResult {
    pub orders: Arc<Mutex<Vec<SmallOrder>>>,
    pub disputes: Arc<Mutex<Vec<Dispute>>>,
}

/// Start background tasks to periodically fetch orders and disputes
///
/// This function spawns two async tasks:
/// - Orders fetcher: Fetches pending orders every 10 seconds
/// - Disputes fetcher: Fetches all disputes every 10 seconds
///
/// Both tasks start immediately and then refresh at the specified interval.
///
/// # Arguments
///
/// * `client` - Nostr client for fetching events
/// * `mostro_pubkey` - Public key of the Mostro daemon
///
/// # Returns
///
/// Returns `FetchSchedulerResult` containing shared state for orders and disputes
pub fn start_fetch_scheduler(client: Client, mostro_pubkey: PublicKey) -> FetchSchedulerResult {
    let orders: Arc<Mutex<Vec<SmallOrder>>> = Arc::new(Mutex::new(Vec::new()));
    let disputes: Arc<Mutex<Vec<Dispute>>> = Arc::new(Mutex::new(Vec::new()));

    // Spawn task to periodically fetch orders
    let orders_clone = Arc::clone(&orders);
    let client_for_orders = client.clone();
    let mostro_pubkey_for_orders = mostro_pubkey;
    tokio::spawn(async move {
        // Periodically refresh orders list (immediate first fetch, then every 10 seconds)
        let mut refresh_interval = interval_at(Instant::now(), Duration::from_secs(10));
        loop {
            refresh_interval.tick().await;
            // Reload currencies from settings dynamically on each fetch
            let currencies = crate::settings::load_settings_from_disk()
                .ok()
                .and_then(|s| {
                    if s.currencies.is_empty() {
                        None
                    } else {
                        Some(s.currencies)
                    }
                });

            if let Ok(fetched_orders) = get_orders(
                &client_for_orders,
                mostro_pubkey_for_orders,
                Some(Status::Pending),
                currencies,
            )
            .await
            {
                let mut orders_lock = orders_clone.lock().unwrap();
                orders_lock.clear();
                orders_lock.extend(fetched_orders);
            }
        }
    });

    // Spawn task to periodically fetch disputes
    let disputes_clone = Arc::clone(&disputes);
    let client_for_disputes = client.clone();
    let mostro_pubkey_for_disputes = mostro_pubkey;
    tokio::spawn(async move {
        // Periodically refresh disputes list (immediate first fetch, then every 10 seconds)
        let mut refresh_interval = interval_at(Instant::now(), Duration::from_secs(10));
        loop {
            refresh_interval.tick().await;
            if let Ok(fetched_disputes) =
                get_disputes(&client_for_disputes, mostro_pubkey_for_disputes).await
            {
                let mut disputes_lock = disputes_clone.lock().unwrap();
                disputes_lock.clear();
                disputes_lock.extend(fetched_disputes);
            }
        }
    });

    FetchSchedulerResult { orders, disputes }
}

/// Spawns a one-off background task to fetch admin chat updates and send the result on the given channel.
pub fn spawn_admin_chat_fetch(
    client: Client,
    admin_keys: Keys,
    disputes: Vec<AdminDispute>,
    shared_keys: HashMap<(String, ChatParty), AdminChatSharedKey>,
    tx: UnboundedSender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
) {
    tokio::spawn(async move {
        let result = fetch_admin_chat_updates(&client, &admin_keys, &disputes, &shared_keys).await;
        let _ = tx.send(result);
    });
}
