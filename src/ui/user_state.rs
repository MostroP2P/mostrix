use crate::ui::{FormState, TakeOrderState};

#[derive(Clone, Debug)]
pub enum UserMode {
    Normal,
    CreatingOrder(FormState),
    ConfirmingOrder {
        // Confirmation popup with YES/NO selection
        form: FormState,
        selected_button: bool, // true = YES, false = NO
    },
    TakingOrder(TakeOrderState),      // Taking an order from the list
    WaitingForMostro(FormState),      // Waiting for Mostro response (order creation)
    WaitingTakeOrder(TakeOrderState), // Waiting for Mostro response (taking order)
    WaitingAddInvoice,                // Waiting for Mostro response (adding invoice)
}
