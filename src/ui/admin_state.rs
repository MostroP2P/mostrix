use crate::shared::permissions::SolverPermission;
use crate::ui::KeyInputState;
use crate::util::order_utils::BondSlashChoice;

#[derive(Clone, Debug)]
pub struct AddSolverState {
    pub key_input: KeyInputState,
    pub permission: SolverPermission,
}

#[derive(Clone, Debug)]
pub enum AdminMode {
    Normal,
    AddSolver(AddSolverState),
    ConfirmAddSolver {
        solver_pubkey: String,
        permission: SolverPermission,
        selected_button: bool, // true=Yes, false=No
    },
    WaitingAddSolver, // Waiting for Mostro response after admin-add-solver
    SetupAdminKey(KeyInputState),
    ConfirmAdminKey(String, bool), // (key_string, selected_button: true=Yes, false=No)
    ConfirmTakeDispute(uuid::Uuid, bool), // (dispute_id, selected_button: true=Yes, false=No)
    WaitingTakeDispute(uuid::Uuid), // (dispute_id)
    ManagingDispute,               // Mode for "Disputes in Progress" tab
    ReviewingDisputeForFinalization {
        dispute_id: uuid::Uuid,
        /// Index of the selected button: 0=Pay Buyer, 1=Refund Seller, 2=Bond slash
        selected_button_index: usize,
        /// Anti-abuse bond slash choice (default: no slash).
        bond: BondSlashChoice,
        /// When true, show the bond slash overlay submenu.
        slash_submenu_open: bool,
        /// Highlighted index in [`BondSlashChoice::ALL`] while submenu is open.
        slash_submenu_index: usize,
    },
    ConfirmFinalizeDispute {
        dispute_id: uuid::Uuid,
        /// true=Pay Buyer, false=Refund Seller
        is_settle: bool,
        bond: BondSlashChoice,
        /// true=Yes, false=No
        selected_button: bool,
    },
    WaitingDisputeFinalization(uuid::Uuid), // (dispute_id)
}
