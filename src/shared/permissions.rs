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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_switches_between_read_and_read_write() {
        assert_eq!(SolverPermission::Read.toggle(), SolverPermission::ReadWrite);
        assert_eq!(SolverPermission::ReadWrite.toggle(), SolverPermission::Read);
        assert_eq!(
            SolverPermission::Read.toggle().toggle(),
            SolverPermission::Read
        );
    }

    #[test]
    fn as_label_returns_expected_strings() {
        assert_eq!(SolverPermission::Read.as_label(), "read");
        assert_eq!(SolverPermission::ReadWrite.as_label(), "read-write");
    }
}
