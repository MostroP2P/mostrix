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

impl BondSlashChoice {
    /// All choices in UI display order.
    pub const ALL: [Self; 4] = [
        Self::None,
        Self::SlashBuyer,
        Self::SlashSeller,
        Self::SlashBoth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No bond slash",
            Self::SlashBuyer => "Slash buyer bond",
            Self::SlashSeller => "Slash seller bond",
            Self::SlashBoth => "Slash both bonds",
        }
    }

    pub fn slash_seller(self) -> bool {
        matches!(self, Self::SlashSeller | Self::SlashBoth)
    }

    pub fn slash_buyer(self) -> bool {
        matches!(self, Self::SlashBuyer | Self::SlashBoth)
    }

    pub fn to_bond_resolution(self) -> BondResolution {
        BondResolution {
            slash_seller: self.slash_seller(),
            slash_buyer: self.slash_buyer(),
        }
    }

    /// Wire payload for `admin-settle` / `admin-cancel`.
    ///
    /// Always emits an explicit [`Payload::BondResolution`]; `{false, false}` means
    /// no slash (same semantics as legacy `payload: null` on the server).
    pub fn to_payload(self) -> Payload {
        Payload::BondResolution(self.to_bond_resolution())
    }

    pub fn to_optional_payload(self) -> Option<Payload> {
        Some(self.to_payload())
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
    fn bond_resolution_none_is_explicit_false_false() {
        let msg = dispute_message(Action::AdminSettle, BondSlashChoice::None);
        let json = msg.as_json().expect("serialize");
        assert!(json.contains("\"slash_seller\":false"));
        assert!(json.contains("\"slash_buyer\":false"));
    }

    #[test]
    fn bond_resolution_slash_both_roundtrip() {
        let msg = dispute_message(Action::AdminCancel, BondSlashChoice::SlashBoth);
        let json = msg.as_json().expect("serialize");
        let decoded = Message::from_json(&json).expect("deserialize");
        assert!(decoded.verify());
        let b = bond_resolution_from_message(&decoded);
        assert!(b.slash_seller);
        assert!(b.slash_buyer);
    }
}
