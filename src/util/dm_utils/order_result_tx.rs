use std::sync::Mutex;

use nostr_sdk::prelude::{Client, PublicKey};
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;

use crate::ui::OperationResult;
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::sync_user_info::spawn_fetch_user_info;

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

/// Ask the main loop to refetch own reputation with its live Mostro instance info.
pub fn try_request_own_reputation_refresh(delayed: bool) {
    let Some(tx) = cloned_order_result_tx() else {
        log::debug!("Own reputation refresh skipped: order_result_tx not registered");
        return;
    };
    let _ = tx.send(OperationResult::OwnReputationRefreshRequested { delayed });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Listener must not carry its own instance snapshot (stale `pow_first_contact`);
    /// it only asks the main loop, which reads live `app.mostro_info`.
    #[test]
    fn refresh_request_carries_no_instance_snapshot() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        set_order_result_tx(tx).expect("register channel");

        try_request_own_reputation_refresh(true);

        let mut found = false;
        while let Ok(result) = rx.try_recv() {
            if let OperationResult::OwnReputationRefreshRequested { delayed } = result {
                assert!(delayed);
                found = true;
            }
        }
        assert!(found, "refresh request must reach the main-loop channel");
    }
}
