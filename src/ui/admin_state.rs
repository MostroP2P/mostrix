use crate::ui::KeyInputState;

#[derive(Clone, Debug)]
pub enum AdminMode {
    Normal,
    AddSolver(KeyInputState),
    ConfirmAddSolver(String, bool), // (solver_pubkey, selected_button: true=Yes, false=No)
    SetupAdminKey(KeyInputState),
    ConfirmAdminKey(String, bool), // (key_string, selected_button: true=Yes, false=No)
    ConfirmTakeDispute(uuid::Uuid, bool), // (dispute_id, selected_button: true=Yes, false=No)
    WaitingTakeDispute(uuid::Uuid), // (dispute_id)
    ManagingDispute,               // Mode for "Disputes in Progress" tab
    ReviewingDisputeForFinalization {
        dispute_id: uuid::Uuid,
        /// Index of the selected button: 0=Pay Buyer, 1=Refund Seller, 2=Exit
        selected_button_index: usize,
    },
    ConfirmFinalizeDispute {
        dispute_id: uuid::Uuid,
        /// true=Pay Buyer, false=Refund Seller
        is_settle: bool,
        /// true=Yes, false=No
        selected_button: bool,
    },
    WaitingDisputeFinalization(uuid::Uuid), // (dispute_id)
}
