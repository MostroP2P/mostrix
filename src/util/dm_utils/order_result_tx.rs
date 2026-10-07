use std::sync::Mutex;
use std::time::Duration;

use nostr_sdk::prelude::{Client, PublicKey};
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;

use crate::ui::OperationResult;
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::sync_user_info::{
    spawn_fetch_user_info, spawn_fetch_user_info_delayed,
    OWN_REPUTATION_REFRESH_AFTER_SUCCESS_DELAY,
};

static ORDER_RESULT_TX: Mutex<Option<UnboundedSender<OperationResult>>> = Mutex::new(None);

/// Registers the main-loop channel used to refresh UI state after background trade events.
pub fn set_order_result_tx(tx: UnboundedSender<OperationResult>) -> Result<(), &'static str> {
    match ORDER_RESULT_TX.lock() {
        Ok(mut guard) => {
            *guard = Some(tx);
            Ok(())
        }
        Err(_) => Err("ORDER_RESULT_TX mutex poisoned"),
    }
}

fn cloned_order_result_tx() -> Option<UnboundedSender<OperationResult>> {
    ORDER_RESULT_TX
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().cloned())
}

/// Notify the UI thread to rebuild the My Trades maker-on-book sidebar cache from SQLite.
pub fn try_notify_my_trades_maker_book_changed() {
    let Some(tx) = cloned_order_result_tx() else {
        return;
    };
    let _ = tx.send(OperationResult::MyTradesMakerBookChanged);
}

/// Spawn an immediate own-reputation fetch when the order-result channel is registered.
pub fn try_spawn_fetch_own_reputation(
    pool: SqlitePool,
    client: Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<MostroInstanceInfo>,
) {
    let Some(tx) = cloned_order_result_tx() else {
        log::debug!("Own reputation fetch skipped: order_result_tx not registered");
        return;
    };
    spawn_fetch_user_info(pool, client, mostro_pubkey, mostro_instance, tx);
}

/// Spawn a delayed own-reputation fetch (after trade success).
pub fn try_spawn_fetch_own_reputation_delayed(
    pool: SqlitePool,
    client: Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<MostroInstanceInfo>,
    delay: Duration,
) {
    let Some(tx) = cloned_order_result_tx() else {
        log::debug!("Delayed own reputation fetch skipped: order_result_tx not registered");
        return;
    };
    spawn_fetch_user_info_delayed(pool, client, mostro_pubkey, mostro_instance, tx, delay);
}

/// Immediate + delayed refresh after `PurchaseCompleted` (protocol freshness).
pub fn try_spawn_own_reputation_refresh_after_success(
    pool: SqlitePool,
    client: Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<MostroInstanceInfo>,
) {
    try_spawn_fetch_own_reputation(
        pool.clone(),
        client.clone(),
        mostro_pubkey,
        mostro_instance.clone(),
    );
    try_spawn_fetch_own_reputation_delayed(
        pool,
        client,
        mostro_pubkey,
        mostro_instance,
        OWN_REPUTATION_REFRESH_AFTER_SUCCESS_DELAY,
    );
}
