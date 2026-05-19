//! Bond slash choices for admin dispute finalization (`admin-settle` / `admin-cancel`).
//!
//! See [admin settle](https://mostro.network/protocol/admin_settle_order.html) and
//! [admin cancel](https://mostro.network/protocol/admin_cancel_order.html).

use mostro_core::prelude::*;

/// How to resolve anti-abuse bonds when an admin finalizes a dispute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BondSlashChoice {
    /// Release bonds; do not slash either party.
    None,
    /// Slash the buyer's bond (if posted).
    SlashBuyer,
    /// Slash the seller's bond (if posted).
    SlashSeller,
    /// Slash both parties' bonds (if posted).
    SlashBoth,
}

// Default for bond slash choice is None
impl Default for BondSlashChoice {
    fn default() -> Self {
        Self::None
    }
}

// BondSlashChoice implementation
impl BondSlashChoice {
    /// All choices in UI display order.
    pub const ALL: [Self; 4] = [
        Self::None,
        Self::SlashBuyer,
        Self::SlashSeller,
        Self::SlashBoth,
    ];

    /// Index in [`Self::ALL`] for TUI list selection.
    pub fn choice_index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    /// Choice at `index` in [`Self::ALL`], or [`Self::None`] if out of range.
    pub fn from_choice_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::None)
    }

    /// Human-readable label for TUI display (includes choice emoji).
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "🔓 No bond slash",
            Self::SlashBuyer => "⚔️ Slash buyer bond",
            Self::SlashSeller => "⚔️ Slash seller bond",
            Self::SlashBoth => "⚔️ Slash both bonds",
        }
    }

    /// Returns `true` if the seller's bond should be slashed.
    pub fn slash_seller(self) -> bool {
        matches!(self, Self::SlashSeller | Self::SlashBoth)
    }

    /// Returns `true` if the buyer's bond should be slashed.
    pub fn slash_buyer(self) -> bool {
        matches!(self, Self::SlashBuyer | Self::SlashBoth)
    }

    pub(crate) fn to_bond_resolution(self) -> BondResolution {
        BondResolution {
            slash_seller: self.slash_seller(),
            slash_buyer: self.slash_buyer(),
        }
    }

    /// Explicit [`Payload::BondResolution`] (including `{ slash_seller: false, slash_buyer: false }`
    /// for [`Self::None`]). Prefer [`Self::to_optional_payload`] for wire messages.
    pub fn to_payload(self) -> Payload {
        Payload::BondResolution(self.to_bond_resolution())
    }

    /// Wire payload for `admin-settle` / `admin-cancel`.
    ///
    /// [`Self::None`] → `None` (`payload: null`, legacy no-slash). Slash variants →
    /// `Some(Payload::BondResolution(..))`.
    pub fn to_optional_payload(self) -> Option<Payload> {
        match self {
            Self::None => None,
            _ => Some(self.to_payload()),
        }
    }

    /// Short phrase for admin finalization log lines.
    pub fn log_context(self) -> &'static str {
        match self {
            Self::None => "no bond slash",
            Self::SlashBuyer => "slash buyer bond",
            Self::SlashSeller => "slash seller bond",
            Self::SlashBoth => "slash both bonds",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::uuid;

    fn dispute_message(action: Action, bond: BondSlashChoice) -> Message {
        Message::new_dispute(
            Some(uuid!("308e1272-d5f4-47e6-bd97-3504baea9c23")),
            None,
            None,
            action,
            bond.to_optional_payload(),
        )
    }

    fn bond_resolution_from_message(msg: &Message) -> BondResolution {
        match msg {
            Message::Dispute(kind) => match &kind.payload {
                Some(Payload::BondResolution(b)) => b.clone(),
                other => panic!("expected BondResolution payload, got {other:?}"),
            },
            other => panic!("expected Dispute message, got {other:?}"),
        }
    }

    #[test]
    fn choice_index_roundtrip() {
        for (i, choice) in BondSlashChoice::ALL.iter().enumerate() {
            assert_eq!(choice.choice_index(), i);
            assert_eq!(BondSlashChoice::from_choice_index(i), *choice);
        }
    }

    #[test]
    fn slash_flags_match_choice() {
        assert!(!BondSlashChoice::None.slash_seller());
        assert!(!BondSlashChoice::None.slash_buyer());

        assert!(!BondSlashChoice::SlashBuyer.slash_seller());
        assert!(BondSlashChoice::SlashBuyer.slash_buyer());

        assert!(BondSlashChoice::SlashSeller.slash_seller());
        assert!(!BondSlashChoice::SlashSeller.slash_buyer());

        assert!(BondSlashChoice::SlashBoth.slash_seller());
        assert!(BondSlashChoice::SlashBoth.slash_buyer());
    }

    #[test]
    fn admin_settle_and_cancel_verify_with_bond_resolution() {
        for action in [Action::AdminSettle, Action::AdminCancel] {
            for bond in BondSlashChoice::ALL {
                let msg = dispute_message(action.clone(), bond);
                assert!(msg.verify(), "{action:?} + {bond:?} should verify");
            }
        }
    }

    #[test]
    fn bond_resolution_wire_format_admin_settle() {
        let msg = dispute_message(Action::AdminSettle, BondSlashChoice::SlashBuyer);
        let json = msg.as_json().expect("serialize");
        assert!(
            json.contains("\"bond_resolution\""),
            "expected snake_case discriminator, got: {json}"
        );
        assert!(json.contains("\"slash_seller\":false"));
        assert!(json.contains("\"slash_buyer\":true"));
        assert!(json.contains("\"action\":\"admin-settle\""));

        let decoded = Message::from_json(&json).expect("deserialize");
        assert!(decoded.verify());
        let b = bond_resolution_from_message(&decoded);
        assert!(!b.slash_seller);
        assert!(b.slash_buyer);
    }

    #[test]
    fn bond_resolution_wire_format_admin_cancel_slash_seller() {
        let msg = dispute_message(Action::AdminCancel, BondSlashChoice::SlashSeller);
        let json = msg.as_json().expect("serialize");
        assert!(json.contains("\"action\":\"admin-cancel\""));
        assert!(json.contains("\"slash_seller\":true"));
        assert!(json.contains("\"slash_buyer\":false"));

        let decoded = Message::from_json(&json).expect("deserialize");
        assert!(decoded.verify());
    }

    #[test]
    fn bond_resolution_none_omits_payload() {
        let msg = dispute_message(Action::AdminSettle, BondSlashChoice::None);
        assert!(msg.verify());
        let json = msg.as_json().expect("serialize");
        assert!(
            json.contains("\"payload\":null"),
            "None should serialize as legacy null payload, got: {json}"
        );
        assert!(
            !json.contains("bond_resolution"),
            "None must not emit bond_resolution, got: {json}"
        );
    }

    #[test]
    fn to_payload_none_is_explicit_false_false() {
        let payload = BondSlashChoice::None.to_payload();
        let BondResolution {
            slash_seller,
            slash_buyer,
        } = match payload {
            Payload::BondResolution(b) => b,
            other => panic!("expected BondResolution, got {other:?}"),
        };
        assert!(!slash_seller);
        assert!(!slash_buyer);
    }

    #[test]
    fn bond_resolution_wire_format_slash_both() {
        let msg = dispute_message(Action::AdminSettle, BondSlashChoice::SlashBoth);
        let json = msg.as_json().expect("serialize");
        assert!(
            json.contains("\"bond_resolution\""),
            "expected snake_case discriminator, got: {json}"
        );
        assert!(json.contains("\"slash_seller\":true"));
        assert!(json.contains("\"slash_buyer\":true"));
        assert!(json.contains("\"action\":\"admin-settle\""));

        let decoded = Message::from_json(&json).expect("deserialize");
        assert!(decoded.verify());
        let b = bond_resolution_from_message(&decoded);
        assert!(b.slash_seller);
        assert!(b.slash_buyer);
    }
}
