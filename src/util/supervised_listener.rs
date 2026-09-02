//! Supervised spawns for long-lived listeners whose command channels must be recreated on respawn.

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
    listen_for_order_messages, set_dm_router_cmd_tx, OrderDmSubscriptionCmd,
};
use crate::util::fatal::{fatal_requested, next_backoff_secs, send_fatal_notify, FatalNotify};

const TRADE_DM_LISTENER_LABEL: &str = "trade DM listener";
const CHAT_ROUTER_LABEL: &str = "chat subscription router";
const INITIAL_BACKOFF_SECS: u64 = 1;

async fn after_listener_failure(label: &str, started: Instant, backoff_secs: u64) -> u64 {
    send_fatal_notify(FatalNotify::TaskAlarm(format!(
        "Background task \"{label}\" stopped unexpectedly and is restarting (retry in {backoff_secs}s).\n\
Other protocol channels remain active."
    )));
    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
    next_backoff_secs(backoff_secs, started.elapsed())
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
        let mut dm_rx = initial_dm_rx;
        let mut backoff_secs = INITIAL_BACKOFF_SECS;
        loop {
            if fatal_requested() {
                break;
            }
            let started = Instant::now();
            send_fatal_notify(FatalNotify::TaskResumed(
                TRADE_DM_LISTENER_LABEL.to_string(),
            ));
            let rx = dm_rx;
            let result = std::panic::AssertUnwindSafe(async {
                listen_for_order_messages(
                    client.clone(),
                    mostro_pubkey,
                    transport,
                    pool.clone(),
                    Arc::clone(&active_order_trade_indices),
                    order_last_seen_dm_ts.clone(),
                    Arc::clone(&messages),
                    message_notification_tx.clone(),
                    Arc::clone(&pending_notifications),
                    Arc::clone(&dropped_user_history_order_ids),
                    rx,
                )
                .await;
            })
            .catch_unwind()
            .await;
            if fatal_requested() {
                break;
            }
            match result {
                Ok(()) => {
                    log::warn!(
                        "critical task {:?} exited unexpectedly; respawning after {}s backoff",
                        TRADE_DM_LISTENER_LABEL,
                        backoff_secs
                    );
                }
                Err(_) => {
                    log::error!(
                        "[panic] critical task {:?} unwound; respawning after {}s backoff",
                        TRADE_DM_LISTENER_LABEL,
                        backoff_secs
                    );
                }
            }
            backoff_secs =
                after_listener_failure(TRADE_DM_LISTENER_LABEL, started, backoff_secs).await;
            let (new_tx, new_rx) = mpsc::unbounded_channel();
            dm_rx = new_rx;
            if set_dm_router_cmd_tx(new_tx.clone()).is_ok() {
                send_fatal_notify(FatalNotify::DmRouterSender(new_tx));
            } else {
                log::error!("[dm_listener] failed to register router sender after respawn");
            }
        }
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
        let mut chat_rx = initial_chat_rx;
        let mut backoff_secs = INITIAL_BACKOFF_SECS;
        loop {
            if fatal_requested() {
                break;
            }
            let started = Instant::now();
            send_fatal_notify(FatalNotify::TaskResumed(CHAT_ROUTER_LABEL.to_string()));
            let rx = chat_rx;
            let result = std::panic::AssertUnwindSafe(async {
                listen_for_chat_messages(
                    client.clone(),
                    admin_chat_updates_tx.clone(),
                    user_order_chat_updates_tx.clone(),
                    rx,
                )
                .await;
            })
            .catch_unwind()
            .await;
            if fatal_requested() {
                break;
            }
            match result {
                Ok(()) => {
                    log::warn!(
                        "critical task {:?} exited unexpectedly; respawning after {}s backoff",
                        CHAT_ROUTER_LABEL,
                        backoff_secs
                    );
                }
                Err(_) => {
                    log::error!(
                        "[panic] critical task {:?} unwound; respawning after {}s backoff",
                        CHAT_ROUTER_LABEL,
                        backoff_secs
                    );
                }
            }
            backoff_secs = after_listener_failure(CHAT_ROUTER_LABEL, started, backoff_secs).await;
            let (new_tx, new_rx) = mpsc::unbounded_channel();
            chat_rx = new_rx;
            if set_chat_router_cmd_tx(new_tx.clone()).is_ok() {
                send_fatal_notify(FatalNotify::ChatRouterSender(new_tx));
            } else {
                log::error!("[chat_listener] failed to register router sender after respawn");
            }
        }
    })
}
