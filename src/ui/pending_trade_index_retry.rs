//! Stashed command to re-run after an on-the-fly trade-index sync.

use crate::ui::{FormState, TakeOrderState};

#[derive(Clone, Debug)]
pub enum PendingTradeIndexRetry {
    NewOrder {
        form: FormState,
    },
    TakeOrder {
        take_state: TakeOrderState,
        amount: Option<i64>,
        invoice: Option<String>,
    },
}
