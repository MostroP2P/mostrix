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
