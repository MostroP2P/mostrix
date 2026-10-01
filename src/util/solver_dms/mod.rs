//! Messages that trusted assistants (e.g. Serbero) send to the solver.
//!
//! Serbero writes to solvers with Mostro protocol v2 `send-dm` messages: a
//! `kind 14` event authored by its own key, whose `MessageKind.id` is the
//! dispute id and whose text starts with `Dispute <id> · <subject>`
//! (serbero `docs/messages.md` §3). They are plain text for a human and are
//! never interpreted as protocol actions.

pub mod listener;
pub mod store;

use std::collections::HashMap;

use mostro_core::prelude::*;
use mostro_core::transport::unwrap_message_nip44;
use nostr_sdk::prelude::*;
use uuid::Uuid;

use crate::ui::AppState;

/// Separates `Dispute <id>` from the subject on a solver message's first line.
const HEADER_SEPARATOR: &str = " · ";
/// Starts a handoff subject, followed by its reason (`handed off: flood`).
const HANDOFF_PREFIX: &str = "handed off:";

/// One `send-dm` text from a trusted sender, as persisted and shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolverDm {
    pub event_id: String,
    pub sender_pubkey: String,
    /// Admin key the message was written to; a key change starts a new inbox.
    pub recipient_pubkey: String,
    /// Dispute the message is about, when it names one.
    pub dispute_id: Option<String>,
    /// First-line subject, e.g. `handed off: conflicting_claims`.
    pub subject: String,
    pub text: String,
    pub created_at: i64,
}

impl SolverDm {
    /// Handoffs and failed openings mean a person has to act on the dispute.
    pub fn needs_action(&self) -> bool {
        needs_action(&self.subject)
    }
}

/// Stored messages younger than this are loaded into memory at startup.
pub const LOAD_WINDOW_SECS: i64 = 30 * 24 * 3600;

/// Stored messages to `recipient` from the last [`LOAD_WINDOW_SECS`] written
/// by a currently `trusted` sender (a removed sender's rows stay hidden),
/// indexed by dispute.
pub async fn load_recent_solver_dms(
    pool: &sqlx::SqlitePool,
    recipient: &PublicKey,
    trusted: &[PublicKey],
) -> SolverDmsByDispute {
    let since = chrono::Utc::now().timestamp() - LOAD_WINDOW_SECS;
    let trusted: Vec<String> = trusted.iter().map(PublicKey::to_hex).collect();
    match store::load_since(pool, &recipient.to_hex(), since).await {
        Ok(dms) => index_by_dispute(
            dms.into_iter()
                .filter(|dm| trusted.contains(&dm.sender_pubkey))
                .collect(),
        ),
        Err(e) => {
            log::warn!("Failed to load stored solver DMs: {e}");
            SolverDmsByDispute::new()
        }
    }
}

/// Solver DMs grouped by dispute id, each list oldest first.
pub type SolverDmsByDispute = HashMap<String, Vec<SolverDm>>;

/// Groups `dms` by dispute; messages that name no dispute are left out.
pub fn index_by_dispute(dms: Vec<SolverDm>) -> SolverDmsByDispute {
    let mut index = SolverDmsByDispute::new();
    for dm in dms {
        add_to_index(&mut index, dm);
    }
    index
}

/// Adds `dm` to its dispute's list, keeping it sorted by time. Returns
/// `false` when it names no dispute or is already there.
pub fn add_to_index(index: &mut SolverDmsByDispute, dm: SolverDm) -> bool {
    let Some(dispute_id) = dm.dispute_id.clone() else {
        return false;
    };
    let list = index.entry(dispute_id).or_default();
    if list.iter().any(|d| d.event_id == dm.event_id) {
        return false;
    }
    let at = list.partition_point(|d| d.created_at <= dm.created_at);
    list.insert(at, dm);
    true
}

/// Which messages the current inbox shows: written to this admin key (hex)
/// by one of these senders (hex). Set whenever the listener is respawned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxScope {
    pub recipient: String,
    pub senders: Vec<String>,
}

impl InboxScope {
    pub fn accepts(&self, dm: &SolverDm) -> bool {
        dm.recipient_pubkey == self.recipient && self.senders.contains(&dm.sender_pubkey)
    }
}

/// Adds a message the live listener forwarded, unless it falls outside
/// `scope`: a listener replaced on a reload may still have queued messages
/// for a previous admin key or a sender that is no longer trusted.
pub fn add_live_dm(
    index: &mut SolverDmsByDispute,
    dm: SolverDm,
    scope: Option<&InboxScope>,
) -> bool {
    if !scope.is_some_and(|s| s.accepts(&dm)) {
        return false;
    }
    add_to_index(index, dm)
}

/// Opens `event` as a solver DM when it is a `send-dm` text written by one of
/// `trusted` to `receiver`. Anything else (untrusted author, other action or
/// payload, not decryptable, bad signature) yields `None`.
pub fn parse_solver_dm(event: &Event, receiver: &Keys, trusted: &[PublicKey]) -> Option<SolverDm> {
    if event.kind != nostr_sdk::prelude::Kind::PrivateDirectMessage
        || !trusted.contains(&event.pubkey)
    {
        return None;
    }
    let unwrapped = match unwrap_message_nip44(event, receiver) {
        Ok(Some(u)) => u,
        Ok(None) => return None,
        Err(e) => {
            log::warn!("Could not open solver DM {}: {}", event.id, e);
            return None;
        }
    };
    let kind = unwrapped.message.get_inner_message_kind();
    let (Action::SendDm, Some(Payload::TextMessage(text))) = (&kind.action, &kind.payload) else {
        return None;
    };
    let dispute_id = kind.id.or_else(|| dispute_id_from_text(text));
    Some(SolverDm {
        event_id: event.id.to_hex(),
        sender_pubkey: event.pubkey.to_hex(),
        recipient_pubkey: receiver.public_key().to_hex(),
        dispute_id: dispute_id.map(|id| id.to_string()),
        subject: subject_from_text(text),
        text: text.clone(),
        created_at: capped_created_at(
            unwrapped.created_at.as_secs() as i64,
            Timestamp::now().as_secs() as i64,
        ),
    })
}

/// The sender sets `created_at`; a future value would push the backfill
/// resume point past real messages, so it is capped at `now`.
pub fn capped_created_at(created_at: i64, now: i64) -> i64 {
    created_at.min(now)
}

/// Dispute id named in the first two lines of `text` (covers the
/// `Dispute <id> · …` header and the older `dispute: <id>` second line).
pub fn dispute_id_from_text(text: &str) -> Option<Uuid> {
    text.lines()
        .take(2)
        .flat_map(|line| line.split(|c: char| !(c.is_ascii_hexdigit() || c == '-')))
        .filter(|token| token.len() == 36)
        .find_map(|token| Uuid::parse_str(token).ok())
}

/// Subject of a solver message: what follows `Dispute <id> · ` on the first
/// line, or the whole first line for messages without that header.
pub fn subject_from_text(text: &str) -> String {
    let first = text.lines().next().unwrap_or_default().trim();
    match first.split_once(HEADER_SEPARATOR) {
        Some((head, subject)) if head.starts_with("Dispute ") => subject.trim().to_string(),
        _ => first.to_string(),
    }
}

/// True for subjects that ask a person to take the dispute over.
pub fn needs_action(subject: &str) -> bool {
    subject.starts_with("handed off") || subject.starts_with("mediation could not start")
}

/// Human-readable reason of a subject that needs action:
/// `handed off: conflicting_claims` reads `conflicting claims`; a subject
/// without a reason (`mediation could not start`) reads as written.
pub fn handoff_reason(subject: &str) -> String {
    subject
        .strip_prefix(HANDOFF_PREFIX)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or(subject)
        .replace('_', " ")
}

/// Handles a message from the live listener: adds it to the inbox (see
/// [`add_live_dm`]) and, when it is new and asks a person to act, records an
/// out-of-focus alert (bell, sound, title badge) as a new chat message does.
/// The Disputes Pending banner and tab badge show it on the next frame.
pub fn apply_live_dm(app: &mut AppState, dm: SolverDm) {
    let needs_action = dm.needs_action();
    let created_at = dm.created_at;
    if add_live_dm(&mut app.solver_dms, dm, app.solver_dm_scope.as_ref()) && needs_action {
        app.terminal_alert.record_event(created_at);
    }
}

/// Parses configured senders (npub or hex), skipping and logging bad entries.
pub fn parse_trusted_senders(raw: &[String]) -> Vec<PublicKey> {
    raw.iter()
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| match PublicKey::parse(entry) {
            Ok(pk) => Some(pk),
            Err(e) => {
                log::warn!("Ignoring invalid trusted DM sender {entry:?}: {e}");
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UserRole;
    use mostro_core::nip59::WrapOptions;
    use mostro_core::transport::wrap_message_nip44;

    const DISPUTE: &str = "58511141-6e3f-4b87-9c4a-1f2e3d4c5b6a";

    fn dm_event(
        author: &Keys,
        to: &Keys,
        id: Option<Uuid>,
        action: Action,
        payload: Option<Payload>,
    ) -> Event {
        let message = Message::new_dm(id, None, action, payload);
        wrap_message_nip44(
            &message,
            author,
            author,
            to.public_key(),
            WrapOptions::default(),
        )
        .expect("wrap")
    }

    fn text(t: &str) -> Option<Payload> {
        Some(Payload::TextMessage(t.to_string()))
    }

    #[test]
    fn a_send_dm_from_a_trusted_sender_is_read_and_linked_by_message_id() {
        let serbero = Keys::generate();
        let solver = Keys::generate();
        let id = Uuid::parse_str(DISPUTE).unwrap();
        let body = format!("Dispute {DISPUTE} · handed off: conflicting_claims\nTopic: …");
        let event = dm_event(&serbero, &solver, Some(id), Action::SendDm, text(&body));

        let dm = parse_solver_dm(&event, &solver, &[serbero.public_key()]).expect("accepted");

        assert_eq!(dm.dispute_id.as_deref(), Some(DISPUTE));
        assert_eq!(dm.subject, "handed off: conflicting_claims");
        assert_eq!(dm.text, body);
        assert_eq!(dm.sender_pubkey, serbero.public_key().to_hex());
        assert_eq!(dm.event_id, event.id.to_hex());
        assert_eq!(dm.recipient_pubkey, solver.public_key().to_hex());
        assert!(dm.needs_action());
    }

    #[test]
    fn a_message_without_id_is_linked_from_its_text() {
        let serbero = Keys::generate();
        let solver = Keys::generate();
        let body = format!("New Mostro dispute\ndispute: {DISPUTE}\nopened by: seller");
        let event = dm_event(&serbero, &solver, None, Action::SendDm, text(&body));

        let dm = parse_solver_dm(&event, &solver, &[serbero.public_key()]).expect("accepted");

        assert_eq!(dm.dispute_id.as_deref(), Some(DISPUTE));
        assert_eq!(dm.subject, "New Mostro dispute");
    }

    #[test]
    fn a_send_dm_from_an_untrusted_author_is_ignored() {
        let stranger = Keys::generate();
        let solver = Keys::generate();
        let event = dm_event(&stranger, &solver, None, Action::SendDm, text("hi"));

        assert_eq!(
            parse_solver_dm(&event, &solver, &[Keys::generate().public_key()]),
            None
        );
    }

    #[test]
    fn a_protocol_action_from_a_trusted_sender_is_ignored() {
        let serbero = Keys::generate();
        let solver = Keys::generate();
        let id = Uuid::parse_str(DISPUTE).unwrap();
        let event = dm_event(&serbero, &solver, Some(id), Action::AdminSettle, None);

        assert_eq!(
            parse_solver_dm(&event, &solver, &[serbero.public_key()]),
            None
        );
    }

    #[test]
    fn a_send_dm_without_text_is_ignored() {
        let serbero = Keys::generate();
        let solver = Keys::generate();
        let event = dm_event(&serbero, &solver, None, Action::SendDm, None);

        assert_eq!(
            parse_solver_dm(&event, &solver, &[serbero.public_key()]),
            None
        );
    }

    #[test]
    fn a_dm_for_another_solver_is_ignored() {
        let serbero = Keys::generate();
        let other = Keys::generate();
        let event = dm_event(&serbero, &other, None, Action::SendDm, text("hi"));

        assert_eq!(
            parse_solver_dm(&event, &Keys::generate(), &[serbero.public_key()]),
            None
        );
    }

    #[test]
    fn a_future_timestamp_is_capped_at_now() {
        assert_eq!(capped_created_at(5_000, 1_000), 1_000);
        assert_eq!(capped_created_at(900, 1_000), 900);
    }

    #[test]
    fn subjects_come_from_the_header() {
        assert_eq!(
            subject_from_text(&format!("Dispute {DISPUTE} · taken\ntaken by: Serbero")),
            "taken"
        );
        assert_eq!(
            subject_from_text(&format!("Dispute {DISPUTE} · resolved: settled")),
            "resolved: settled"
        );
        assert_eq!(
            subject_from_text("Serbero is mediating dispute x.\nmore"),
            "Serbero is mediating dispute x."
        );
        assert_eq!(subject_from_text(""), "");
    }

    #[test]
    fn handoffs_and_failed_openings_need_action() {
        assert!(needs_action("handed off: fraud_signal"));
        assert!(needs_action("mediation could not start"));
        assert!(!needs_action("mediating"));
        assert!(!needs_action("transcript (3 messages, times UTC)"));
        assert!(!needs_action("resolved: settled"));
    }

    #[test]
    fn handoff_reasons_read_as_words() {
        assert_eq!(
            handoff_reason("handed off: conflicting_claims"),
            "conflicting claims"
        );
        assert_eq!(
            handoff_reason("handed off: self_resolution_stalled"),
            "self resolution stalled"
        );
        assert_eq!(handoff_reason("handed off: flood"), "flood");
        assert_eq!(
            handoff_reason("mediation could not start"),
            "mediation could not start"
        );
        assert_eq!(handoff_reason("handed off:"), "handed off:");
    }

    #[test]
    fn dispute_ids_are_found_in_the_first_two_lines_only() {
        let id = Uuid::parse_str(DISPUTE).unwrap();
        assert_eq!(
            dispute_id_from_text(&format!("Dispute {DISPUTE} · taken")),
            Some(id)
        );
        assert_eq!(
            dispute_id_from_text(&format!("Serbero is mediating dispute {DISPUTE}.")),
            Some(id)
        );
        assert_eq!(dispute_id_from_text(&format!("a\nb\n{DISPUTE}")), None);
        assert_eq!(dispute_id_from_text("Dispute d1 · taken"), None);
    }

    fn stored(event_id: &str, dispute_id: Option<&str>, created_at: i64) -> SolverDm {
        SolverDm {
            event_id: event_id.into(),
            sender_pubkey: String::new(),
            recipient_pubkey: String::new(),
            dispute_id: dispute_id.map(Into::into),
            subject: String::new(),
            text: String::new(),
            created_at,
        }
    }

    #[test]
    fn the_index_groups_by_dispute_in_time_order_and_skips_unlinked() {
        let index = index_by_dispute(vec![
            stored("b", Some("d1"), 20),
            stored("x", None, 5),
            stored("a", Some("d1"), 10),
            stored("c", Some("d2"), 30),
        ]);

        let d1: Vec<&str> = index["d1"].iter().map(|d| d.event_id.as_str()).collect();
        assert_eq!(d1, ["a", "b"]);
        assert_eq!(index["d2"].len(), 1);
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn adding_a_known_event_again_changes_nothing() {
        let mut index = index_by_dispute(vec![stored("a", Some("d1"), 10)]);

        assert!(!add_to_index(&mut index, stored("a", Some("d1"), 10)));
        assert!(add_to_index(&mut index, stored("b", Some("d1"), 5)));

        let d1: Vec<&str> = index["d1"].iter().map(|d| d.event_id.as_str()).collect();
        assert_eq!(d1, ["b", "a"]);
    }

    #[test]
    fn a_live_message_outside_the_current_inbox_is_dropped() {
        let scope = InboxScope {
            recipient: "admin".into(),
            senders: vec!["serbero".into()],
        };
        let msg = |event_id: &str, recipient: &str, sender: &str| SolverDm {
            recipient_pubkey: recipient.into(),
            sender_pubkey: sender.into(),
            ..stored(event_id, Some("d1"), 10)
        };
        let mut index = SolverDmsByDispute::new();

        assert!(!add_live_dm(
            &mut index,
            msg("a", "old-admin", "serbero"),
            Some(&scope)
        ));
        assert!(!add_live_dm(
            &mut index,
            msg("b", "admin", "revoked"),
            Some(&scope)
        ));
        assert!(!add_live_dm(&mut index, msg("c", "admin", "serbero"), None));
        assert!(add_live_dm(
            &mut index,
            msg("d", "admin", "serbero"),
            Some(&scope)
        ));

        assert_eq!(index["d1"].len(), 1);
    }

    /// Admin away from the terminal, inbox scoped to `admin` / `serbero`.
    fn unfocused_admin() -> AppState {
        let mut app = AppState::new(UserRole::Admin);
        app.solver_dm_scope = Some(InboxScope {
            recipient: "admin".into(),
            senders: vec!["serbero".into()],
        });
        app.terminal_alert.set_focus(false);
        app
    }

    fn live(event_id: &str, recipient: &str, subject: &str) -> SolverDm {
        SolverDm {
            recipient_pubkey: recipient.into(),
            sender_pubkey: "serbero".into(),
            subject: subject.into(),
            ..stored(event_id, Some("d1"), chrono::Utc::now().timestamp())
        }
    }

    #[test]
    fn a_new_live_handoff_alerts_once() {
        let mut app = unfocused_admin();

        apply_live_dm(&mut app, live("a", "admin", "handed off: fraud_signal"));
        apply_live_dm(&mut app, live("a", "admin", "handed off: fraud_signal"));

        assert_eq!(app.solver_dms["d1"].len(), 1);
        assert_eq!(app.terminal_alert.unread(), 1);
    }

    #[test]
    fn a_failed_opening_alerts_like_a_handoff() {
        let mut app = unfocused_admin();

        apply_live_dm(&mut app, live("a", "admin", "mediation could not start"));

        assert_eq!(app.terminal_alert.unread(), 1);
    }

    #[test]
    fn live_messages_that_need_nobody_do_not_alert() {
        let mut app = unfocused_admin();

        apply_live_dm(&mut app, live("a", "admin", "mediating"));
        apply_live_dm(
            &mut app,
            live("b", "admin", "transcript (18 messages, times UTC)"),
        );

        assert_eq!(app.solver_dms["d1"].len(), 2);
        assert_eq!(app.terminal_alert.unread(), 0);
    }

    #[test]
    fn a_live_handoff_outside_the_inbox_does_not_alert() {
        let mut app = unfocused_admin();

        apply_live_dm(
            &mut app,
            live("a", "old-admin", "handed off: conflicting_claims"),
        );

        assert!(app.solver_dms.is_empty());
        assert_eq!(app.terminal_alert.unread(), 0);
    }

    #[tokio::test]
    async fn stored_messages_from_revoked_senders_are_not_loaded() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        store::ensure_table(&pool).await.unwrap();
        let admin = Keys::generate().public_key();
        let kept = Keys::generate().public_key();
        let revoked = Keys::generate().public_key();
        let now = chrono::Utc::now().timestamp();
        for (event_id, sender) in [("a", kept), ("b", revoked)] {
            let mut dm = stored(event_id, Some("d1"), now);
            dm.sender_pubkey = sender.to_hex();
            dm.recipient_pubkey = admin.to_hex();
            store::insert(&pool, &dm).await.unwrap();
        }

        let index = load_recent_solver_dms(&pool, &admin, &[kept]).await;

        let ids: Vec<&str> = index["d1"].iter().map(|d| d.event_id.as_str()).collect();
        assert_eq!(ids, ["a"]);
        assert!(load_recent_solver_dms(&pool, &admin, &[]).await.is_empty());
    }

    #[test]
    fn trusted_senders_accept_npub_and_hex_and_skip_garbage() {
        let a = Keys::generate().public_key();
        let b = Keys::generate().public_key();
        let raw = vec![
            a.to_bech32().unwrap(),
            b.to_hex(),
            "nope".to_string(),
            " ".to_string(),
        ];

        assert_eq!(parse_trusted_senders(&raw), vec![a, b]);
    }
}
