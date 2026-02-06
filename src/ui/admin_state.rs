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
    ReviewingDisputeForFinalization(uuid::Uuid, usize), // (dispute_id, selected_button: 0=Pay Buyer, 1=Refund Seller, 2=Exit)
    ConfirmFinalizeDispute(uuid::Uuid, bool, bool), // (dispute_id, is_settle: true=Pay Buyer, false=Refund Seller, selected_button: true=Yes, false=No)
    WaitingDisputeFinalization(uuid::Uuid),         // (dispute_id)
}
