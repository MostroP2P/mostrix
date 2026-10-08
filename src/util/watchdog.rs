//! Solver notifications through mostro-watchdog.
//!
//! A solver links the watchdog from Settings → Link Watchdog with the code
//! its Telegram bot gives on `/link`. From then on, admin mode tells the
//! watchdog which dispute chats to watch, so the solver gets a private
//! Telegram message when a party writes. Only public keys are shared: the
//! signing key `K_sign` of each conversation, and the id of every message the
//! solver sends, so the watchdog does not notify the solver's own messages.
//! No private key leaves Mostrix and the watchdog never reads the chat.
//!
//! The protocol is `SOLVER_NOTIFICATIONS.md` in the mostro-watchdog repo:
//! a Mostro v2 `send-dm` whose text is versioned JSON, wrapped with the
//! admin key as identity and a fresh trade key, so relays do not see the
//! solver writing to the watchdog.

use std::sync::RwLock;

use anyhow::{anyhow, Result};
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use serde_json::json;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::models::AdminDispute;
use crate::util::chat_utils::{chat_keys_from_ecdh, keys_from_shared_hex};

/// The protocol version the watchdog speaks.
const PROTOCOL_VERSION: u64 = 1;

/// Characters of a watchdog link code (no `0`/`O`, `1`/`I`/`L`).
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// Characters in a link code, shown as two groups of four.
const CODE_LEN: usize = 8;
const CODE_GROUP_LEN: usize = 4;

/// The party on the other side of one of the solver's conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Party {
    Buyer,
    Seller,
}

impl Party {
    fn as_str(self) -> &'static str {
        match self {
            Self::Buyer => "buyer",
            Self::Seller => "seller",
        }
    }
}

/// One conversation of a dispute, by the public key that signs its events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedConversation {
    pub party: Party,
    pub sign_pubkey: PublicKey,
}

/// A message to the watchdog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchdogMessage {
    /// Tie the admin key to the Telegram chat that asked for `code`.
    Link { code: String },
    /// Watch these conversations of the dispute.
    Watch {
        dispute_id: Uuid,
        conversations: Vec<WatchedConversation>,
    },
    /// The solver wrote chat event `event_id`: do not notify it.
    Sent { event_id: EventId },
    /// Stop watching the dispute.
    Unwatch { dispute_id: Uuid },
}

impl WatchdogMessage {
    /// The message text, as the watchdog parses it.
    pub fn to_json(&self) -> String {
        let value = match self {
            Self::Link { code } => json!({"v": PROTOCOL_VERSION, "type": "link", "code": code}),
            Self::Watch {
                dispute_id,
                conversations,
            } => json!({
                "v": PROTOCOL_VERSION,
                "type": "watch",
                "dispute_id": dispute_id.to_string(),
                "conversations": conversations
                    .iter()
                    .map(|c| json!({"party": c.party.as_str(), "sign_pubkey": c.sign_pubkey.to_hex()}))
                    .collect::<Vec<_>>(),
            }),
            Self::Sent { event_id } => {
                json!({"v": PROTOCOL_VERSION, "type": "sent", "event_id": event_id.to_hex()})
            }
            Self::Unwatch { dispute_id } => json!({
                "v": PROTOCOL_VERSION,
                "type": "unwatch",
                "dispute_id": dispute_id.to_string(),
            }),
        };
        value.to_string()
    }

    /// The dispute the message is about, used as the envelope's message id.
    fn dispute_id(&self) -> Option<Uuid> {
        match self {
            Self::Watch { dispute_id, .. } | Self::Unwatch { dispute_id } => Some(*dispute_id),
            Self::Link { .. } | Self::Sent { .. } => None,
        }
    }
}

/// `input` as a link code in its canonical form (`K7QM-2XPA`): any case,
/// with or without the dash and surrounding spaces.
pub fn normalize_code(input: &str) -> Option<String> {
    let chars: Vec<u8> = input
        .trim()
        .bytes()
        .filter(|&b| b != b'-')
        .map(|b| b.to_ascii_uppercase())
        .collect();
    if chars.len() != CODE_LEN || !chars.iter().all(|b| CODE_ALPHABET.contains(b)) {
        return None;
    }
    let (head, tail) = chars.split_at(CODE_GROUP_LEN);
    Some(format!(
        "{}-{}",
        String::from_utf8_lossy(head),
        String::from_utf8_lossy(tail)
    ))
}

/// The watchdog key typed in Settings (npub or hex).
pub fn parse_watchdog_pubkey(input: &str) -> Result<PublicKey, String> {
    PublicKey::parse(input.trim())
        .map_err(|_| "Invalid watchdog key: expected an npub or 64 hex characters".to_string())
}

/// `pub(K_sign)` of the conversation whose ECDH secret is `shared_key_hex`
/// (as stored in `admin_disputes`).
pub fn sign_pubkey(shared_key_hex: &str) -> Option<PublicKey> {
    let shared = keys_from_shared_hex(shared_key_hex)?;
    let (_conv, sign) = chat_keys_from_ecdh(&shared)?;
    Some(sign.public_key())
}

/// The `watch` for a dispute from its stored per-party shared keys. `None`
/// when no conversation has a key yet.
pub fn watch_message(
    dispute_id: Uuid,
    buyer_shared_key_hex: Option<&str>,
    seller_shared_key_hex: Option<&str>,
) -> Option<WatchdogMessage> {
    let conversations: Vec<WatchedConversation> = [
        (Party::Buyer, buyer_shared_key_hex),
        (Party::Seller, seller_shared_key_hex),
    ]
    .into_iter()
    .filter_map(|(party, hex)| {
        Some(WatchedConversation {
            party,
            sign_pubkey: sign_pubkey(hex?)?,
        })
    })
    .collect();
    (!conversations.is_empty()).then_some(WatchdogMessage::Watch {
        dispute_id,
        conversations,
    })
}

/// The `watch` for a stored dispute; `None` for a dispute id that is not a
/// UUID or without shared keys.
pub fn watch_message_for(dispute: &AdminDispute) -> Option<WatchdogMessage> {
    let dispute_id = Uuid::parse_str(&dispute.dispute_id).ok()?;
    watch_message(
        dispute_id,
        dispute.buyer_shared_key_hex.as_deref(),
        dispute.seller_shared_key_hex.as_deref(),
    )
}

/// `message` wrapped for `watchdog`: the admin key proves the identity
/// inside the ciphertext, and a fresh trade key signs the event.
pub fn build_event(
    admin_keys: &Keys,
    watchdog: PublicKey,
    message: &WatchdogMessage,
) -> Result<Event> {
    let dm = Message::new_dm(
        message.dispute_id(),
        None,
        Action::SendDm,
        Some(Payload::TextMessage(message.to_json())),
    );
    let trade_keys = Keys::generate();
    wrap_message_nip44(
        &dm,
        admin_keys,
        &trade_keys,
        watchdog,
        WrapOptions::default(),
    )
    .map_err(|e| anyhow!("Failed to wrap watchdog message: {e}"))
}

/// The watchdog linked during this run, overriding `settings.toml` (which
/// is loaded once at startup).
static LINKED_WATCHDOG: RwLock<Option<PublicKey>> = RwLock::new(None);

/// The linked watchdog, if any.
pub fn linked_watchdog() -> Option<PublicKey> {
    let linked = *LINKED_WATCHDOG.read().unwrap_or_else(|e| e.into_inner());
    linked.or_else(|| {
        crate::SETTINGS
            .get()
            .and_then(|s| parse_watchdog_pubkey(&s.watchdog_pubkey).ok())
    })
}

fn set_linked_watchdog(watchdog: PublicKey) {
    *LINKED_WATCHDOG.write().unwrap_or_else(|e| e.into_inner()) = Some(watchdog);
}

/// Sends `message` to `watchdog`. Fails when no relay accepted it.
pub async fn send(
    client: &Client,
    admin_keys: &Keys,
    watchdog: PublicKey,
    message: &WatchdogMessage,
) -> Result<()> {
    let event = build_event(admin_keys, watchdog, message)?;
    let output = client
        .send_event(&event)
        .await
        .map_err(|e| anyhow!("Failed to send watchdog message: {e}"))?;
    if output.success.is_empty() {
        return Err(anyhow!("No relay accepted the watchdog message"));
    }
    Ok(())
}

/// Sends `message` to the linked watchdog, if any. Failures are logged:
/// the watchdog only notifies, so the dispute action goes on regardless.
pub async fn notify_linked(client: &Client, admin_keys: &Keys, message: &WatchdogMessage) {
    let Some(watchdog) = linked_watchdog() else {
        return;
    };
    if let Err(e) = send(client, admin_keys, watchdog, message).await {
        log::warn!("[watchdog] {e}");
    }
}

/// Longest wait for a `sent` receipt before the solver's message is
/// published anyway: a slow watchdog relay must not hold the chat back.
pub const RECEIPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Sends the `sent` receipt for the solver's chat event `event_id`, waiting
/// at most [`RECEIPT_TIMEOUT`].
pub async fn send_receipt(client: &Client, admin_keys: &Keys, event_id: EventId) {
    let receipt = WatchdogMessage::Sent { event_id };
    if tokio::time::timeout(RECEIPT_TIMEOUT, notify_linked(client, admin_keys, &receipt))
        .await
        .is_err()
    {
        log::warn!("[watchdog] receipt timed out; the watchdog may notify this message");
    }
}

/// [`notify_linked`] in the background.
pub fn spawn_notify_linked(client: &Client, admin_keys: &Keys, message: WatchdogMessage) {
    if linked_watchdog().is_none() {
        return;
    }
    let client = client.clone();
    let admin_keys = admin_keys.clone();
    tokio::spawn(async move { notify_linked(&client, &admin_keys, &message).await });
}

/// What linking did after the `link` itself was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkOutcome {
    /// Held disputes whose `watch` was sent.
    pub watched: usize,
    /// Held disputes whose `watch` could not be sent.
    pub failed: usize,
}

/// Links `watchdog` with `code`: sends `link`, remembers the watchdog, then
/// sends a `watch` for every dispute this admin holds. Only a failed `link`
/// is an error; a failed `watch` is counted and logged.
pub async fn link(
    client: &Client,
    pool: &SqlitePool,
    admin_keys: &Keys,
    watchdog: PublicKey,
    code: &str,
) -> Result<LinkOutcome> {
    let code = normalize_code(code).ok_or_else(|| anyhow!("Invalid link code"))?;
    send(
        client,
        admin_keys,
        watchdog,
        &WatchdogMessage::Link { code },
    )
    .await?;
    set_linked_watchdog(watchdog);
    let mut outcome = LinkOutcome {
        watched: 0,
        failed: 0,
    };
    let held = match AdminDispute::get_all(pool).await {
        Ok(held) => held,
        Err(e) => {
            log::warn!("[watchdog] could not read held disputes to watch: {e}");
            return Ok(outcome);
        }
    };
    let in_progress = DisputeStatus::InProgress.to_string();
    for message in held
        .iter()
        .filter(|d| d.status.as_deref() == Some(in_progress.as_str()))
        .filter_map(watch_message_for)
    {
        match send(client, admin_keys, watchdog, &message).await {
            Ok(()) => outcome.watched += 1,
            Err(e) => {
                log::warn!("[watchdog] {e}");
                outcome.failed += 1;
            }
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::chat_utils::derive_shared_key_hex;
    use mostro_core::chat::derive_chat_keys;
    use mostro_core::transport::unwrap_message_nip44;

    const DISPUTE: &str = "58511141-6e3f-4b87-9c4a-1f2e3d4c5b6a";

    fn dispute() -> Uuid {
        Uuid::parse_str(DISPUTE).unwrap()
    }

    fn json(message: &WatchdogMessage) -> serde_json::Value {
        serde_json::from_str(&message.to_json()).unwrap()
    }

    #[test]
    fn messages_match_the_watchdog_protocol() {
        let sign = Keys::generate().public_key();
        let id = EventId::from_byte_array([7; 32]);

        assert_eq!(
            json(&WatchdogMessage::Link {
                code: "K7QM-2XPA".into()
            }),
            json!({"v": 1, "type": "link", "code": "K7QM-2XPA"})
        );
        assert_eq!(
            json(&WatchdogMessage::Watch {
                dispute_id: dispute(),
                conversations: vec![WatchedConversation {
                    party: Party::Seller,
                    sign_pubkey: sign,
                }],
            }),
            json!({
                "v": 1,
                "type": "watch",
                "dispute_id": DISPUTE,
                "conversations": [{"party": "seller", "sign_pubkey": sign.to_hex()}],
            })
        );
        assert_eq!(
            json(&WatchdogMessage::Sent { event_id: id }),
            json!({"v": 1, "type": "sent", "event_id": id.to_hex()})
        );
        assert_eq!(
            json(&WatchdogMessage::Unwatch {
                dispute_id: dispute()
            }),
            json!({"v": 1, "type": "unwatch", "dispute_id": DISPUTE})
        );
    }

    #[test]
    fn codes_are_read_in_any_case_with_or_without_the_dash() {
        for input in ["K7QM-2XPA", "k7qm2xpa", "  K7QM2XPA\n"] {
            assert_eq!(
                normalize_code(input).as_deref(),
                Some("K7QM-2XPA"),
                "{input:?}"
            );
        }
        for input in ["", "K7QM-2XP", "K7QM-2XP0", "K7QM-2XPAA"] {
            assert_eq!(normalize_code(input), None, "{input:?}");
        }
    }

    #[test]
    fn the_watchdog_key_is_an_npub_or_hex() {
        let key = Keys::generate().public_key();

        assert_eq!(parse_watchdog_pubkey(&key.to_hex()), Ok(key));
        assert_eq!(parse_watchdog_pubkey(&key.to_bech32().unwrap()), Ok(key));
        assert!(parse_watchdog_pubkey("npub1nope").is_err());
    }

    #[test]
    fn the_sign_key_is_the_one_both_sides_derive() {
        let admin = Keys::generate();
        let buyer = Keys::generate();
        let hex = derive_shared_key_hex(Some(&admin), Some(&buyer.public_key().to_hex())).unwrap();

        let (_conv, party_sign) = derive_chat_keys(&buyer, &admin.public_key()).unwrap();

        assert_eq!(sign_pubkey(&hex), Some(party_sign.public_key()));
    }

    #[test]
    fn a_watch_names_each_conversation_with_a_key() {
        let admin = Keys::generate();
        let (buyer, seller) = (Keys::generate(), Keys::generate());
        let hex = |party: &Keys| {
            derive_shared_key_hex(Some(&admin), Some(&party.public_key().to_hex())).unwrap()
        };
        let (b, s) = (hex(&buyer), hex(&seller));

        let both = watch_message(dispute(), Some(&b), Some(&s)).unwrap();
        let seller_only = watch_message(dispute(), None, Some(&s)).unwrap();

        let WatchdogMessage::Watch { conversations, .. } = both else {
            panic!("a watch");
        };
        assert_eq!(conversations.len(), 2);
        assert_eq!(conversations[0].party, Party::Buyer);
        assert_eq!(conversations[0].sign_pubkey, sign_pubkey(&b).unwrap());
        assert!(matches!(
            seller_only,
            WatchdogMessage::Watch { ref conversations, .. } if conversations.len() == 1
        ));
        assert_eq!(watch_message(dispute(), None, Some("zz")), None);
    }

    #[test]
    fn a_stored_dispute_without_a_uuid_or_keys_is_not_watched() {
        let admin = Keys::generate();
        let buyer = Keys::generate();
        let hex = derive_shared_key_hex(Some(&admin), Some(&buyer.public_key().to_hex()));
        let stored = |dispute_id: &str, buyer_hex: Option<String>| AdminDispute {
            dispute_id: dispute_id.into(),
            buyer_shared_key_hex: buyer_hex,
            ..Default::default()
        };

        assert!(watch_message_for(&stored(DISPUTE, hex.clone())).is_some());
        assert_eq!(watch_message_for(&stored("not-a-uuid", hex)), None);
        assert_eq!(watch_message_for(&stored(DISPUTE, None)), None);
    }

    #[test]
    fn the_envelope_proves_the_admin_key_behind_a_fresh_trade_key() {
        let admin = Keys::generate();
        let watchdog = Keys::generate();
        let message = WatchdogMessage::Unwatch {
            dispute_id: dispute(),
        };

        let event = build_event(&admin, watchdog.public_key(), &message).unwrap();
        let opened = unwrap_message_nip44(&event, &watchdog)
            .unwrap()
            .expect("for the watchdog");

        assert_eq!(event.kind, nostr_sdk::prelude::Kind::PrivateDirectMessage);
        assert_ne!(event.pubkey, admin.public_key());
        assert_eq!(opened.identity, admin.public_key());
        let kind = opened.message.get_inner_message_kind();
        assert_eq!(kind.action, Action::SendDm);
        assert_eq!(kind.id, Some(dispute()));
        assert!(
            matches!(&kind.payload, Some(Payload::TextMessage(text)) if *text == message.to_json()),
            "{:?}",
            kind.payload
        );
    }
}
