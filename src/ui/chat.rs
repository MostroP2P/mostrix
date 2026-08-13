use std::fmt::{self, Display};

use nostr_sdk::prelude::{EventId, PublicKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatParty {
    Buyer,
    Seller,
}

/// Filter for viewing disputes in the Disputes in Progress tab
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisputeFilter {
    InProgress, // Show only InProgress disputes
    Finalized,  // Show only finalized disputes (Settled, SellerRefunded, Released)
}

impl Display for ChatParty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatParty::Buyer => write!(f, "Buyer"),
            ChatParty::Seller => write!(f, "Seller"),
        }
    }
}

/// Represents the sender of a chat message in dispute resolution
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatSender {
    Admin,
    Buyer,
    Seller,
}

/// Sender role in user-to-user order chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserChatSender {
    You,
    Peer,
}

/// User-facing chat channel selected in My Trades.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UserChatChannel {
    /// Chat with the order counterparty.
    #[default]
    Peer,
    /// Chat with the solver assigned to the dispute.
    Solver,
}

impl Display for UserChatChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Peer => write!(f, "Peer"),
            Self::Solver => write!(f, "Solver"),
        }
    }
}

/// Type of file attachment (Mostro Mobile image_encrypted / file_encrypted).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatAttachmentType {
    Image,
    File,
}

/// Attachment metadata for a dispute chat message (Blossom URL + decryption key).
/// File bytes are fetched from Blossom when the admin saves (Ctrl+S).
#[derive(Clone, Debug)]
pub struct ChatAttachment {
    pub blossom_url: String,
    pub filename: String,
    pub mime_type: Option<String>,
    pub file_type: ChatAttachmentType,
    /// When provided by the sender, used to decrypt the blob when saving.
    pub decryption_key: Option<Vec<u8>>,
}

/// A chat message in the dispute resolution interface
#[derive(Clone, Debug)]
pub struct DisputeChatMessage {
    pub sender: ChatSender,
    pub content: String,
    pub timestamp: i64,                  // Unix timestamp
    pub target_party: Option<ChatParty>, // For Admin messages: which party this was sent to
    /// When set, this message is an attachment (image or file); content is display-only (e.g. "📎 File: name").
    pub attachment: Option<ChatAttachment>,
}

/// A chat message in the user order-in-progress chat.
#[derive(Clone, Debug)]
pub struct UserOrderChatMessage {
    pub sender: UserChatSender,
    pub content: String,
    pub timestamp: i64, // Unix timestamp
    pub attachment: Option<ChatAttachment>,
}

/// Per-(dispute, party) last-seen timestamp for admin chat.
/// Used to filter incoming buyer/seller messages so we only process new ones.
#[derive(Clone, Debug)]
pub struct AdminChatLastSeen {
    /// Last seen timestamp (inner/canonical unix seconds) for messages from this party.
    pub last_seen_timestamp: Option<i64>,
}

/// A decrypted peer/admin chat message ready for UI merge.
#[derive(Clone, Debug)]
pub struct DecodedChatMessage {
    pub content: String,
    pub timestamp: i64,
    pub sender: PublicKey,
    /// Verified inner kind-1 event id — durable replay protection.
    pub inner_event_id: EventId,
}

/// Result of polling for admin chat messages for a single dispute/party.
#[derive(Clone, Debug)]
pub struct AdminChatUpdate {
    pub dispute_id: String,
    pub party: ChatParty,
    pub messages: Vec<DecodedChatMessage>,
}

/// Per-order last-seen timestamp for user order chat.
#[derive(Clone, Debug)]
pub struct OrderChatLastSeen {
    pub last_seen_timestamp: Option<i64>,
}

/// Result of polling for user order chat messages.
#[derive(Clone, Debug)]
pub struct OrderChatUpdate {
    pub order_id: String,
    /// Conversation that should receive this batch.
    pub channel: UserChatChannel,
    /// Local trade public key for this order; used to skip relay echoes of our own sends.
    pub local_trade_pubkey: PublicKey,
    pub messages: Vec<DecodedChatMessage>,
}
