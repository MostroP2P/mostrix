use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tokio::time::{interval_at, Duration, Instant};

use crate::models::AdminDispute;
use crate::settings::Settings;
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
const RECONCILIATION_INTERVAL_SECS: u64 = 30;

fn apply_live_order_update(orders: &Arc<Mutex<Vec<SmallOrder>>>, order: SmallOrder) {
    let Some(order_id) = order.id else {
        return;
    };
    let mut orders_lock = match orders.lock() {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!(
                "[orders_live] Failed to lock orders Arc<Mutex<Vec<SmallOrder>>> (poisoned): {} order_id={}",
                e,
                order_id
            );
            return;
        }
    };
    if order.status != Some(Status::Pending) {
        log::debug!(
            "[orders_live] removing non-pending order_id={} status={:?}",
            order_id,
            order.status
        );
        orders_lock.retain(|existing| existing.id != Some(order_id));
        return;
    }

    if let Some(existing) = orders_lock
        .iter_mut()
        .find(|existing| existing.id == Some(order_id))
    {
        let existing_ts = existing.created_at.unwrap_or(0);
        let new_ts = order.created_at.unwrap_or(0);
        if new_ts >= existing_ts {
            *existing = order;
        }
    } else {
        orders_lock.push(order);
    }
    orders_lock.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    log::debug!(
        "[orders_live] upserted pending order_id={}, total_pending={}",
        order_id,
        orders_lock.len()
    );
}

fn apply_live_dispute_update(disputes: &Arc<Mutex<Vec<Dispute>>>, dispute: Dispute) {
    let dispute_id = dispute.id;
    let dispute_status = dispute.status.clone();
    let mut disputes_lock = match disputes.lock() {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!(
                "[disputes_live] Failed to lock disputes Arc<Mutex<Vec<Dispute>>> (poisoned): {} dispute_id={}",
                e,
                dispute_id
            );
            return;
        }
    };
    if let Some(existing) = disputes_lock
        .iter_mut()
        .find(|existing| existing.id == dispute.id)
    {
        if dispute.created_at >= existing.created_at {
            *existing = dispute;
        }
    } else {
        disputes_lock.push(dispute);
    }
    disputes_lock.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    log::debug!(
        "[disputes_live] upserted dispute_id={} status={} total_disputes={}",
        dispute_id,
        dispute_status,
        disputes_lock.len()
    );
}

/// Start background tasks to periodically fetch orders and disputes
///
/// This function spawns two async tasks:
/// - Orders updater: Applies live subscription updates + reconciles pending orders every 30s
/// - Disputes updater: Applies live subscription updates + reconciles disputes every 30s
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
    settings: &Settings,
) -> FetchSchedulerResult {
    let orders: Arc<Mutex<Vec<SmallOrder>>> = Arc::new(Mutex::new(Vec::new()));
    let disputes: Arc<Mutex<Vec<Dispute>>> = Arc::new(Mutex::new(Vec::new()));

    let (order_task, dispute_task) = spawn_fetch_scheduler_loops(
        client,
        Arc::clone(&current_mostro_pubkey),
        Arc::clone(&orders),
        Arc::clone(&disputes),
        settings,
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
    settings: &Settings,
) -> (JoinHandle<()>, JoinHandle<()>) {
    // Spawn task to periodically fetch orders
    let orders_clone = Arc::clone(&orders);
    let client_for_orders = client.clone();
    let current_mostro_pubkey_for_orders = Arc::clone(&current_mostro_pubkey);
    let latest_settings = settings.clone();
    let order_task = tokio::spawn(async move {
        let mut notifications = client_for_orders.notifications();
        // Real-time order subscription + periodic reconciliation poll.
        let mostro_pubkey_for_order_subscribe = match current_mostro_pubkey_for_orders.lock() {
            Ok(pk) => *pk,
            Err(e) => {
                log::warn!(
                    "Failed to lock current_mostro_pubkey for live order subscription: {}",
                    e
                );
                return;
            }
        };
        let order_filter = Filter::new()
            .author(mostro_pubkey_for_order_subscribe)
            .kind(nostr_sdk::Kind::Custom(NOSTR_ORDER_EVENT_KIND))
            .limit(0);
        match client_for_orders.subscribe(order_filter, None).await {
            Ok(output) => {
                log::debug!(
                    "[orders_live] subscribed to order updates subscription_id={}",
                    output.val
                );
            }
            Err(e) => {
                log::warn!("Failed to subscribe live order updates: {}", e);
            }
        }

        // Reconcile from relay every 30s (immediate first poll, then periodic).
        let mut refresh_interval = interval_at(
            Instant::now(),
            Duration::from_secs(RECONCILIATION_INTERVAL_SECS),
        );
        loop {
            tokio::select! {
                _ = refresh_interval.tick() => {
                    // Reload currency filters from settings on each fetch.
                    // An empty list means "no filter" (show all currencies).
                    let currencies = latest_settings.currencies_filter.clone();

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
                        Some(currencies),
                    )
                    .await
                    {
                        let mut orders_lock = orders_clone.lock().unwrap();
                        orders_lock.clear();
                        orders_lock.extend(fetched_orders);
                        log::debug!(
                            "[orders_reconcile] refreshed pending orders count={}",
                            orders_lock.len()
                        );
                    }
                }
                notification = notifications.recv() => {
                    let Ok(RelayPoolNotification::Event { event, .. }) = notification else {
                        continue;
                    };
                    let event = *event;
                    if event.kind != nostr_sdk::Kind::Custom(NOSTR_ORDER_EVENT_KIND) {
                        continue;
                    }
                    let mut one = Events::default();
                    one.insert(event);
                    let currencies = latest_settings.currencies_filter.clone();
                    let mut parsed = super::parse_orders_events(
                        one,
                        Some(currencies),
                        None,
                        None,
                    );
                    log::debug!(
                        "[orders_live] received order event, parsed_candidates={}",
                        parsed.len()
                    );
                    if let Some(order) = parsed.pop() {
                        apply_live_order_update(&orders_clone, order);
                    }
                }
            }
        }
    });

    // Spawn task to periodically fetch disputes
    let disputes_clone = Arc::clone(&disputes);
    let client_for_disputes = client.clone();
    let current_mostro_pubkey_for_disputes = Arc::clone(&current_mostro_pubkey);
    let dispute_task = tokio::spawn(async move {
        let mut notifications = client_for_disputes.notifications();
        let mostro_pubkey_for_dispute_subscribe = match current_mostro_pubkey_for_disputes.lock() {
            Ok(pk) => *pk,
            Err(e) => {
                log::warn!(
                    "Failed to lock current_mostro_pubkey for live dispute subscription: {}",
                    e
                );
                return;
            }
        };
        let dispute_filter = Filter::new()
            .author(mostro_pubkey_for_dispute_subscribe)
            .kind(nostr_sdk::Kind::Custom(NOSTR_DISPUTE_EVENT_KIND))
            .limit(0);
        match client_for_disputes.subscribe(dispute_filter, None).await {
            Ok(output) => {
                log::debug!(
                    "[disputes_live] subscribed to dispute updates subscription_id={}",
                    output.val
                );
            }
            Err(e) => {
                log::warn!("Failed to subscribe live dispute updates: {}", e);
            }
        }

        // Reconcile from relay every 30s (immediate first poll, then periodic).
        let mut refresh_interval = interval_at(
            Instant::now(),
            Duration::from_secs(RECONCILIATION_INTERVAL_SECS),
        );
        loop {
            tokio::select! {
                _ = refresh_interval.tick() => {
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
                        log::debug!(
                            "[disputes_reconcile] refreshed disputes count={}",
                            disputes_lock.len()
                        );
                    }
                }
                notification = notifications.recv() => {
                    let Ok(RelayPoolNotification::Event { event, .. }) = notification else {
                        continue;
                    };
                    let event = *event;
                    if event.kind != nostr_sdk::Kind::Custom(NOSTR_DISPUTE_EVENT_KIND) {
                        continue;
                    }
                    let mut one = Events::default();
                    one.insert(event);
                    let mut parsed = super::parse_disputes_events(one);
                    log::debug!(
                        "[disputes_live] received dispute event, parsed_candidates={}",
                        parsed.len()
                    );
                    if let Some(dispute) = parsed.pop() {
                        apply_live_dispute_update(&disputes_clone, dispute);
                    }
                }
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
