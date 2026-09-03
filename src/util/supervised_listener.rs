//! Supervised spawns for long-lived listeners whose command channels must be recreated on respawn.
//!
//! Unlike [`crate::util::supervise_critical_task`], these loops own an `mpsc` command
//! receiver. On panic or unexpected exit the supervisor **immediately** recreates the
//! channel and re-publishes the global sender (before backoff) so `TrackOrder` /
//! `TrackChatKey` buffer instead of hitting a closed channel. Main is notified via
//! [`crate::util::FatalNotify::DmRouterSender`] / [`crate::util::FatalNotify::ChatRouterSender`].
//! Chat recovery also replays [`crate::ui::helpers::track_startup_chats`] from the main loop.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::FutureExt;
use mostro_core::prelude::Transport;
use nostr_sdk::prelude::{Client, PublicKey};
use sqlx::SqlitePool;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::Duration;
use uuid::Uuid;

use crate::ui::{AdminChatUpdate, MessageNotification, OrderChatUpdate, OrderMessage};
use crate::util::chat_listener::{listen_for_chat_messages, set_chat_router_cmd_tx, ChatRouterCmd};
use crate::util::dm_utils::{
    hydrate_startup_active_order_dm_state, listen_for_order_messages, set_dm_router_cmd_tx,
    OrderDmSubscriptionCmd,
};
use crate::util::fatal::{fatal_requested, next_backoff_secs, send_fatal_notify, FatalNotify};

const TRADE_DM_LISTENER_LABEL: &str = "trade DM listener";
const CHAT_ROUTER_LABEL: &str = "chat subscription router";
const INITIAL_BACKOFF_SECS: u64 = 1;

fn publish_fresh_cmd_channel<T>(publish: impl FnOnce(UnboundedSender<T>)) -> UnboundedReceiver<T> {
    let (new_tx, new_rx) = mpsc::unbounded_channel();
    publish(new_tx);
    new_rx
}

async fn supervise_replaceable_rx_loop<T, R, Fut, P>(
    label: &'static str,
    mut rx: UnboundedReceiver<T>,
    mut publish: P,
    mut run: R,
    initial_backoff_secs: u64,
    stop_on_fatal: bool,
) where
    T: Send + 'static,
    R: FnMut(UnboundedReceiver<T>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send,
    P: FnMut(UnboundedSender<T>),
{
    let mut backoff_secs = initial_backoff_secs;
    loop {
        if stop_on_fatal && fatal_requested() {
            break;
        }
        let started = Instant::now();
        send_fatal_notify(FatalNotify::TaskResumed(label.to_string()));
        let result = std::panic::AssertUnwindSafe(run(rx)).catch_unwind().await;
        if stop_on_fatal && fatal_requested() {
            break;
        }
        match result {
            Ok(()) => {
                log::warn!(
                    "critical task {:?} exited unexpectedly; respawning after {}s backoff",
                    label,
                    backoff_secs
                );
            }
            Err(_) => {
                log::error!(
                    "[panic] critical task {:?} unwound; respawning after {}s backoff",
                    label,
                    backoff_secs
                );
            }
        }
        // Rotate the command channel *before* backoff so helpers keep a live sender
        // and commands issued while the worker is down are queued for the next run.
        rx = publish_fresh_cmd_channel(&mut publish);
        send_fatal_notify(FatalNotify::TaskAlarm(format!(
            "Background task \"{label}\" stopped unexpectedly and is restarting (retry in {backoff_secs}s).\n\
Other protocol channels remain active."
        )));
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = next_backoff_secs(backoff_secs, started.elapsed());
    }
}

async fn merge_durable_dm_tracks(
    pool: &SqlitePool,
    active_order_trade_indices: &Arc<Mutex<HashMap<Uuid, i64>>>,
    order_last_seen_dm_ts: &mut HashMap<Uuid, i64>,
) {
    let Ok(hydration) = hydrate_startup_active_order_dm_state(pool).await else {
        return;
    };
    if let Ok(mut indices) = active_order_trade_indices.lock() {
        for (order_id, trade_index) in hydration.active_order_trade_indices {
            indices.entry(order_id).or_insert(trade_index);
        }
    }
    for (order_id, ts) in hydration.order_last_seen_dm_ts {
        order_last_seen_dm_ts.entry(order_id).or_insert(ts);
    }
}

/// Spawn the trade DM listener with per-task panic/exit recovery and command-channel refresh.
#[allow(clippy::too_many_arguments)]
pub fn spawn_supervised_trade_dm_listener(
    client: Client,
    mostro_pubkey: PublicKey,
    transport: Transport,
    pool: SqlitePool,
    active_order_trade_indices: Arc<Mutex<HashMap<Uuid, i64>>>,
    order_last_seen_dm_ts: HashMap<Uuid, i64>,
    messages: Arc<Mutex<Vec<OrderMessage>>>,
    message_notification_tx: UnboundedSender<MessageNotification>,
    pending_notifications: Arc<Mutex<usize>>,
    dropped_user_history_order_ids: Arc<Mutex<HashSet<Uuid>>>,
    initial_dm_rx: UnboundedReceiver<OrderDmSubscriptionCmd>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let last_seen_seed = order_last_seen_dm_ts;
        supervise_replaceable_rx_loop(
            TRADE_DM_LISTENER_LABEL,
            initial_dm_rx,
            |tx| {
                if set_dm_router_cmd_tx(tx.clone()).is_ok() {
                    send_fatal_notify(FatalNotify::DmRouterSender(tx));
                } else {
                    log::error!("[dm_listener] failed to register router sender after respawn");
                }
            },
            {
                move |rx| {
                    let client = client.clone();
                    let pool = pool.clone();
                    let active_order_trade_indices = Arc::clone(&active_order_trade_indices);
                    let messages = Arc::clone(&messages);
                    let message_notification_tx = message_notification_tx.clone();
                    let pending_notifications = Arc::clone(&pending_notifications);
                    let dropped_user_history_order_ids =
                        Arc::clone(&dropped_user_history_order_ids);
                    let mut last_seen = last_seen_seed.clone();
                    async move {
                        merge_durable_dm_tracks(&pool, &active_order_trade_indices, &mut last_seen)
                            .await;
                        listen_for_order_messages(
                            client,
                            mostro_pubkey,
                            transport,
                            pool,
                            active_order_trade_indices,
                            last_seen,
                            messages,
                            message_notification_tx,
                            pending_notifications,
                            dropped_user_history_order_ids,
                            rx,
                        )
                        .await;
                    }
                }
            },
            INITIAL_BACKOFF_SECS,
            true,
        )
        .await;
    })
}

/// Spawn the shared-key chat router with per-task panic/exit recovery and command-channel refresh.
pub fn spawn_supervised_chat_listener(
    client: Client,
    admin_chat_updates_tx: tokio::sync::mpsc::Sender<Result<Vec<AdminChatUpdate>, anyhow::Error>>,
    user_order_chat_updates_tx: tokio::sync::mpsc::Sender<
        Result<Vec<OrderChatUpdate>, anyhow::Error>,
    >,
    initial_chat_rx: UnboundedReceiver<ChatRouterCmd>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        supervise_replaceable_rx_loop(
            CHAT_ROUTER_LABEL,
            initial_chat_rx,
            |tx| {
                if set_chat_router_cmd_tx(tx.clone()).is_ok() {
                    send_fatal_notify(FatalNotify::ChatRouterSender(tx));
                } else {
                    log::error!("[chat_listener] failed to register router sender after respawn");
                }
            },
            |rx| {
                let client = client.clone();
                let admin_chat_updates_tx = admin_chat_updates_tx.clone();
                let user_order_chat_updates_tx = user_order_chat_updates_tx.clone();
                async move {
                    listen_for_chat_messages(
                        client,
                        admin_chat_updates_tx,
                        user_order_chat_updates_tx,
                        rx,
                    )
                    .await;
                }
            },
            INITIAL_BACKOFF_SECS,
            true,
        )
        .await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex as AsyncMutex;

    #[tokio::test]
    async fn commands_sent_while_listener_down_are_delivered_after_respawn() {
        crate::util::fatal::reset_fatal_requested_for_tests();

        let received = Arc::new(AsyncMutex::new(None));
        let (initial_tx, initial_rx) = mpsc::unbounded_channel::<u8>();
        let live_tx = Arc::new(Mutex::new(initial_tx));
        let rotated = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let runs = Arc::new(AtomicUsize::new(0));

        let live_tx_for_publish = Arc::clone(&live_tx);
        let rotated_for_publish = Arc::clone(&rotated);
        let received_for_run = Arc::clone(&received);
        let runs_for_run = Arc::clone(&runs);

        let handle = tokio::spawn(async move {
            supervise_replaceable_rx_loop(
                "test listener",
                initial_rx,
                |tx| {
                    *live_tx_for_publish.lock().expect("live tx") = tx;
                    rotated_for_publish.store(true, Ordering::SeqCst);
                },
                move |mut rx| {
                    let received = Arc::clone(&received_for_run);
                    let run_idx = runs_for_run.fetch_add(1, Ordering::SeqCst);
                    async move {
                        if run_idx == 0 {
                            panic!("simulated intake panic");
                        }
                        if let Some(cmd) = rx.recv().await {
                            *received.lock().await = Some(cmd);
                        }
                        std::future::pending::<()>().await;
                    }
                },
                0,
                false,
            )
            .await;
        });

        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while !rotated.load(Ordering::SeqCst) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "listener should panic and rotate the command channel"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        live_tx
            .lock()
            .expect("live tx")
            .send(7)
            .expect("send during backoff after immediate channel rotate");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            *received.lock().await,
            Some(7),
            "TrackOrder-equivalent command issued while the listener was down must be received after respawn"
        );
        handle.abort();
    }
}
