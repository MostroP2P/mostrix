use crate::ui::KeyInputState;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SolverPermission {
    Read,
    ReadWrite,
}

impl SolverPermission {
    pub const fn toggle(self) -> Self {
        match self {
            Self::Read => Self::ReadWrite,
            Self::ReadWrite => Self::Read,
        }
    }

    pub const fn as_label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::ReadWrite => "read-write",
        }
    }
}

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
