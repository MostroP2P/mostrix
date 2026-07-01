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
    ConfirmLeaveOrder {
        // Guard shown when navigating away from a non-empty New Order form.
        form: FormState,
        to_prev: bool,         // direction the user tried to navigate (Left = prev tab)
        selected_button: bool, // true = Keep editing (left), false = Leave (right)
    },
    TakingOrder(TakeOrderState),      // Taking an order from the list
    WaitingForMostro(FormState),      // Waiting for Mostro response (order creation)
    WaitingTakeOrder(TakeOrderState), // Waiting for Mostro response (taking order)
    WaitingAddInvoice,                // Waiting for Mostro response (adding invoice)
}
