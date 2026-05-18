use std::sync::Mutex;

use tokio::sync::mpsc::UnboundedSender;

use crate::ui::OperationResult;

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

/// Notify the UI thread to rebuild the My Trades maker-on-book sidebar cache from SQLite.
pub fn try_notify_my_trades_maker_book_changed() {
    let Ok(guard) = ORDER_RESULT_TX.lock() else {
        return;
    };
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(OperationResult::MyTradesMakerBookChanged);
    }
}
