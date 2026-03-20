use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tokio::time::{interval_at, Duration, Instant};

use crate::models::AdminDispute;
use crate::ui::{AdminChatLastSeen, AdminChatUpdate, ChatParty};
use crate::util::chat_utils::fetch_admin_chat_updates;

use super::{get_disputes, get_orders};

/// Result of starting the fetch scheduler
/// Contains shared state for orders and disputes that are periodically updated
pub struct FetchSchedulerResult {
    pub orders: Arc<Mutex<Vec<SmallOrder>>>,
    pub disputes: Arc<Mutex<Vec<Dispute>>>,
    /// Background task for periodic order fetches; abort and call [`spawn_fetch_scheduler_loops`]
    /// after a soft client reload so polls use the new session.
    pub order_task: JoinHandle<()>,
    /// Background task for periodic dispute fetches; same as [`FetchSchedulerResult::order_task`].
    pub dispute_task: JoinHandle<()>,
}

// Semaphore to prevent multiple chat messages from being processed at the same time
pub static CHAT_MESSAGES_SEMAPHORE: AtomicBool = AtomicBool::new(false);

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
pub fn start_fetch_scheduler(
    client: Client,
    current_mostro_pubkey: Arc<Mutex<PublicKey>>,
) -> FetchSchedulerResult {
    let orders: Arc<Mutex<Vec<SmallOrder>>> = Arc::new(Mutex::new(Vec::new()));
    let disputes: Arc<Mutex<Vec<Dispute>>> = Arc::new(Mutex::new(Vec::new()));

    let (order_task, dispute_task) = spawn_fetch_scheduler_loops(
        client,
        Arc::clone(&current_mostro_pubkey),
        Arc::clone(&orders),
        Arc::clone(&disputes),
    );

    FetchSchedulerResult {
        orders,
        disputes,
        order_task,
        dispute_task,
    }
}

/// Spawns order/dispute polling loops using the given client and shared list state.
///
/// Callers must **abort** any previous handles returned for the same `orders`/`disputes` Arcs
/// before calling again (e.g. after a soft key reload replaces the [`Client`]).
pub fn spawn_fetch_scheduler_loops(
    client: Client,
    current_mostro_pubkey: Arc<Mutex<PublicKey>>,
    orders: Arc<Mutex<Vec<SmallOrder>>>,
    disputes: Arc<Mutex<Vec<Dispute>>>,
) -> (JoinHandle<()>, JoinHandle<()>) {
    // Spawn task to periodically fetch orders
    let orders_clone = Arc::clone(&orders);
    let client_for_orders = client.clone();
    let current_mostro_pubkey_for_orders = Arc::clone(&current_mostro_pubkey);
    let order_task = tokio::spawn(async move {
        // Periodically refresh orders list (immediate first fetch, then every 5 seconds)
        let mut refresh_interval = interval_at(Instant::now(), Duration::from_secs(5));
        loop {
            refresh_interval.tick().await;
            // Reload currency filters from settings on each fetch.
            // An empty list means "no filter" (show all currencies).
            let currencies = crate::settings::load_settings_from_disk()
                .ok()
                .map(|s| s.currencies_filter)
                .filter(|list| !list.is_empty());

            let mostro_pubkey_for_orders = match current_mostro_pubkey_for_orders.lock() {
                Ok(pk) => *pk,
                Err(e) => {
                    log::warn!(
                        "Failed to lock current_mostro_pubkey for orders fetch: {}",
                        e
                    );
                    continue;
                }
            };

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
    let current_mostro_pubkey_for_disputes = Arc::clone(&current_mostro_pubkey);
    let dispute_task = tokio::spawn(async move {
        // Periodically refresh disputes list (immediate first fetch, then every 5 seconds)
        let mut refresh_interval = interval_at(Instant::now(), Duration::from_secs(5));
        loop {
            refresh_interval.tick().await;
            let mostro_pubkey_for_disputes = match current_mostro_pubkey_for_disputes.lock() {
                Ok(pk) => *pk,
                Err(e) => {
                    log::warn!(
                        "Failed to lock current_mostro_pubkey for disputes fetch: {}",
                        e
                    );
                    continue;
                }
            };
            if let Ok(fetched_disputes) =
                get_disputes(&client_for_disputes, mostro_pubkey_for_disputes).await
            {
                let mut disputes_lock = disputes_clone.lock().unwrap();
                disputes_lock.clear();
                disputes_lock.extend(fetched_disputes);
            }
        }
    });

    (order_task, dispute_task)
}

/// Spawns a one-off background task to fetch admin chat updates and send the result on the given channel.
pub fn spawn_admin_chat_fetch(
    client: Client,
    disputes: Vec<AdminDispute>,
    admin_chat_last_seen: HashMap<(String, ChatParty), AdminChatLastSeen>,
    tx: UnboundedSender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
) {
    // If the semaphore is already true, return
    if CHAT_MESSAGES_SEMAPHORE
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    tokio::spawn(async move {
        let result = fetch_admin_chat_updates(&client, &disputes, &admin_chat_last_seen).await;
        CHAT_MESSAGES_SEMAPHORE.store(false, Ordering::Relaxed);
        let _ = tx.send(result);
    });
}
