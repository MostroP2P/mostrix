//! Mostro protocol DMs addressed to the admin (solver) identity key.
//!
//! Mostro notifies an assigned solver when users close a dispute themselves
//! (cooperative cancel or seller release) with a kind-14 message that has no
//! `request_id` and a [`Payload::Dispute`]. The trade-DM router only tracks
//! mnemonic trade keys, so this listener keeps a durable Mostro→admin
//! subscription and routes those closures into the admin dispute UI.

use std::str::FromStr;
use std::time::Duration;

use futures::StreamExt;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::models::AdminDispute;
use crate::ui::{AppState, OperationResult};
use crate::util::chat_listener::untrack_dispute_chat_parties;
use crate::util::dm_utils::{handle_operation_result, FETCH_EVENTS_TIMEOUT};
use crate::util::filters::filter_protocol_dm_from_mostro;

/// How far back the first fetch looks when nothing is stored yet.
pub const HISTORY_WINDOW_SECS: i64 = 7 * 24 * 3600;
/// Re-read this much before "now" on each listen cycle for late relay delivery.
const RESUME_OVERLAP_SECS: i64 = 3600;
/// Pause before resubscribing after the notification stream ends or fails.
const RETRY_DELAY: Duration = Duration::from_secs(5);
/// Fixed relay subscription id, so a respawn replaces the previous one and
/// [`unsubscribe`] can drop it after an abort.
const SUBSCRIPTION_ID: &str = "mostrix-admin-protocol-dms";

fn subscription_id() -> SubscriptionId {
    SubscriptionId::new(SUBSCRIPTION_ID)
}

/// How the users closed the dispute (maps to Mostro's notify action).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserResolutionKind {
    /// Cooperative cancel → [`DisputeStatus::CooperativelyCanceled`].
    CoopCancel,
    /// Seller release → [`DisputeStatus::Released`].
    Released,
}

/// A Mostro DM telling the assigned solver that users resolved the dispute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserResolvedDispute {
    pub dispute_id: Uuid,
    pub order_id: Uuid,
    pub kind: UserResolutionKind,
}

impl UserResolvedDispute {
    /// Local status to persist for this resolution.
    pub fn dispute_status(&self) -> DisputeStatus {
        match self.kind {
            UserResolutionKind::CoopCancel => DisputeStatus::CooperativelyCanceled,
            UserResolutionKind::Released => DisputeStatus::Released,
        }
    }

    /// Short info popup for the admin.
    pub fn info_message(&self) -> String {
        user_closed_dispute_info_message(self.dispute_id, &self.dispute_status())
            .expect("UserResolvedDispute maps only to user-closed statuses")
    }
}

/// Info popup text when users closed a taken dispute (DM path or relay fallback).
///
/// Returns `None` for admin-attributed terminals (`Settled` / `SellerRefunded`).
pub fn user_closed_dispute_info_message(
    dispute_id: Uuid,
    status: &DisputeStatus,
) -> Option<String> {
    let short: String = dispute_id.to_string().chars().take(8).collect();
    match status {
        DisputeStatus::CooperativelyCanceled => Some(format!(
            "Dispute {short} was closed by the users: cooperative cancel (seller refunded). No action needed."
        )),
        DisputeStatus::Released => Some(format!(
            "Dispute {short} was closed by the users: seller released. No action needed."
        )),
        _ => None,
    }
}

/// When kind-38386 advances a taken row to a user-closed terminal status and the
/// Mostro→solver DM was missed, show the same once-per-dispute Info popup.
pub fn notify_admin_if_users_closed_dispute(app: &mut AppState, dispute_id: &str) {
    let Some(local) = app
        .admin_disputes_in_progress
        .iter()
        .find(|d| d.dispute_id == dispute_id)
    else {
        return;
    };
    if !local.closed_by_users() {
        return;
    }
    if app.notified_user_closed_dispute_ids.contains(dispute_id) {
        return;
    }
    let Ok(dispute_uuid) = Uuid::parse_str(dispute_id) else {
        return;
    };
    let Some(status_str) = local.status.as_deref() else {
        return;
    };
    let Ok(status) = DisputeStatus::from_str(status_str) else {
        return;
    };
    let Some(message) = user_closed_dispute_info_message(dispute_uuid, &status) else {
        return;
    };
    handle_operation_result(
        OperationResult::DisputeClosedByUsers {
            dispute_id: dispute_uuid,
            status,
            message,
        },
        app,
    );
}

/// Match a solver-bound user-resolution DM from Mostro.
///
/// Requires `request_id == None` (admin finalize acks echo a request id),
/// `Payload::Dispute(dispute_id, _)`, order `id`, and action
/// [`Action::CooperativeCancelAccepted`] or [`Action::Released`].
pub fn classify_user_resolution(message: &Message) -> Option<UserResolvedDispute> {
    let inner = message.get_inner_message_kind();
    if inner.request_id.is_some() {
        return None;
    }
    let order_id = inner.id?;
    let Payload::Dispute(dispute_id, _) = inner.payload.as_ref()? else {
        return None;
    };
    let kind = match inner.action {
        Action::CooperativeCancelAccepted => UserResolutionKind::CoopCancel,
        Action::Released => UserResolutionKind::Released,
        _ => return None,
    };
    Some(UserResolvedDispute {
        dispute_id: *dispute_id,
        order_id,
        kind,
    })
}

/// Own outbound v2 protocol DMs are signed kind-14 events authored by the admin key.
///
/// When admin == Mostro, the broader self-addressed filter also matches those
/// outbound events; skip them so a take/finalize request is not treated as a
/// user-resolution notice.
fn is_own_signed_v2_outbound(
    event: &Event,
    admin_keys: &Keys,
    unwrapped: &UnwrappedMessage,
) -> bool {
    event.kind == nostr_sdk::prelude::Kind::PrivateDirectMessage
        && event.pubkey == admin_keys.public_key()
        && unwrapped.signature.is_some()
}

/// Drops the listener's relay subscription (after aborting its task).
pub async fn unsubscribe(client: &Client) {
    if let Err(e) = client.unsubscribe(&subscription_id()).await {
        log::debug!("[admin_protocol_dms] unsubscribe failed: {e}");
    }
}

/// Start of the backfill window: [`HISTORY_WINDOW_SECS`] ago, with overlap.
pub fn history_since(now: i64) -> i64 {
    now.saturating_sub(HISTORY_WINDOW_SECS)
        .saturating_sub(RESUME_OVERLAP_SECS)
}

/// Runs the listener until aborted, resubscribing after failures.
pub fn spawn_admin_protocol_dm_listener(
    client: Client,
    admin_keys: Keys,
    mostro_pubkey: PublicKey,
    transport: Transport,
    pool: SqlitePool,
    order_result_tx: UnboundedSender<OperationResult>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(e) = listen_once(
                &client,
                &admin_keys,
                mostro_pubkey,
                transport,
                &pool,
                &order_result_tx,
            )
            .await
            {
                log::warn!("[admin_protocol_dms] listener stopped: {e}; retrying");
            }
            if order_result_tx.is_closed() {
                return;
            }
            tokio::time::sleep(RETRY_DELAY).await;
        }
    })
}

async fn listen_once(
    client: &Client,
    admin_keys: &Keys,
    mostro_pubkey: PublicKey,
    transport: Transport,
    pool: &SqlitePool,
    order_result_tx: &UnboundedSender<OperationResult>,
) -> anyhow::Result<()> {
    let filter = filter_protocol_dm_from_mostro(transport, mostro_pubkey, admin_keys.public_key());
    let notifications = client.notifications();
    client
        .subscribe(filter.clone().limit(0))
        .with_id(subscription_id())
        .await?;
    let result = backfill_then_follow(
        client,
        filter,
        notifications,
        admin_keys,
        mostro_pubkey,
        pool,
        order_result_tx,
    )
    .await;
    unsubscribe(client).await;
    result
}

async fn backfill_then_follow(
    client: &Client,
    filter: Filter,
    mut notifications: impl futures::Stream<Item = ClientNotification> + Unpin,
    admin_keys: &Keys,
    mostro_pubkey: PublicKey,
    pool: &SqlitePool,
    order_result_tx: &UnboundedSender<OperationResult>,
) -> anyhow::Result<()> {
    let since = history_since(Timestamp::now().as_secs() as i64);
    let history = client
        .fetch_events(filter.since(Timestamp::from_secs(since.max(0) as u64)))
        .timeout(FETCH_EVENTS_TIMEOUT)
        .await;
    match history {
        Ok(events) => {
            for event in events.into_iter() {
                accept(
                    &event,
                    client,
                    admin_keys,
                    mostro_pubkey,
                    pool,
                    order_result_tx,
                )
                .await;
            }
        }
        Err(e) => log::warn!("[admin_protocol_dms] history fetch failed: {e}"),
    }

    while let Some(notification) = notifications.next().await {
        if let ClientNotification::Event { event, .. } = notification {
            accept(
                &event,
                client,
                admin_keys,
                mostro_pubkey,
                pool,
                order_result_tx,
            )
            .await;
        }
        if order_result_tx.is_closed() {
            break;
        }
    }
    Ok(())
}

/// Outer event author must be Mostro.
///
/// `client.notifications()` carries every client subscription, so a relay
/// filter is not enough. Keep this even when the admin key equals the Mostro
/// key — a real Mostro event still has `event.pubkey == mostro_pubkey`.
fn is_mostro_authored(event: &Event, mostro_pubkey: PublicKey) -> bool {
    event.pubkey == mostro_pubkey
}

/// Decrypt, classify, advance SQLite, and notify the UI when the local row moves.
async fn accept(
    event: &Event,
    client: &Client,
    admin_keys: &Keys,
    mostro_pubkey: PublicKey,
    pool: &SqlitePool,
    order_result_tx: &UnboundedSender<OperationResult>,
) {
    if !is_mostro_authored(event, mostro_pubkey) {
        return;
    }

    let unwrapped = match unwrap_incoming(event, admin_keys).await {
        Ok(Some(u)) => u,
        Ok(None) => return,
        Err(e) => {
            log::debug!(
                "[admin_protocol_dms] unwrap failed (event {}): {e}",
                event.id
            );
            return;
        }
    };

    if is_own_signed_v2_outbound(event, admin_keys, &unwrapped) {
        return;
    }

    let Some(resolved) = classify_user_resolution(&unwrapped.message) else {
        return;
    };

    let dispute_id = resolved.dispute_id.to_string();
    let status = resolved.dispute_status();
    match AdminDispute::set_status_by_dispute_id(pool, &dispute_id, status.clone()).await {
        Ok(true) => {
            log::info!(
                "[admin_protocol_dms] dispute {} advanced to {} (order {})",
                dispute_id,
                status,
                resolved.order_id
            );
            untrack_dispute_chat_parties(&dispute_id);
            crate::util::watchdog::spawn_unwatch(client, admin_keys, &dispute_id);
            let _ = order_result_tx.send(OperationResult::DisputeClosedByUsers {
                dispute_id: resolved.dispute_id,
                status,
                message: resolved.info_message(),
            });
        }
        Ok(false) => {
            log::debug!(
                "[admin_protocol_dms] dispute {} already terminal or unknown; skip notify",
                dispute_id
            );
        }
        Err(e) => {
            log::warn!(
                "[admin_protocol_dms] failed to update dispute {} to {}: {e}",
                dispute_id,
                status
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mostro_core::transport::wrap_message_nip44;

    fn resolution_message(
        action: Action,
        order_id: Option<Uuid>,
        request_id: Option<u64>,
        payload: Option<Payload>,
    ) -> Message {
        Message::new_dispute(order_id, request_id, None, action, payload)
    }

    #[test]
    fn classify_accepts_cooperative_cancel_without_request_id() {
        let dispute_id = Uuid::new_v4();
        let order_id = Uuid::new_v4();
        let msg = resolution_message(
            Action::CooperativeCancelAccepted,
            Some(order_id),
            None,
            Some(Payload::Dispute(dispute_id, None)),
        );

        let resolved = classify_user_resolution(&msg).expect("match");
        assert_eq!(resolved.dispute_id, dispute_id);
        assert_eq!(resolved.order_id, order_id);
        assert_eq!(resolved.kind, UserResolutionKind::CoopCancel);
        assert_eq!(
            resolved.dispute_status(),
            DisputeStatus::CooperativelyCanceled
        );
        assert!(resolved.info_message().contains("cooperative cancel"));
    }

    #[test]
    fn classify_accepts_released_without_request_id() {
        let dispute_id = Uuid::new_v4();
        let order_id = Uuid::new_v4();
        let msg = resolution_message(
            Action::Released,
            Some(order_id),
            None,
            Some(Payload::Dispute(dispute_id, None)),
        );

        let resolved = classify_user_resolution(&msg).expect("match");
        assert_eq!(resolved.kind, UserResolutionKind::Released);
        assert_eq!(resolved.dispute_status(), DisputeStatus::Released);
        assert!(resolved.info_message().contains("seller released"));
    }

    #[test]
    fn classify_rejects_when_request_id_is_set() {
        let msg = resolution_message(
            Action::CooperativeCancelAccepted,
            Some(Uuid::new_v4()),
            Some(42),
            Some(Payload::Dispute(Uuid::new_v4(), None)),
        );
        assert!(classify_user_resolution(&msg).is_none());
    }

    #[test]
    fn classify_rejects_wrong_payload_or_action() {
        let order_id = Uuid::new_v4();
        let text = resolution_message(
            Action::CooperativeCancelAccepted,
            Some(order_id),
            None,
            Some(Payload::TextMessage("nope".into())),
        );
        assert!(classify_user_resolution(&text).is_none());

        let wrong_action = resolution_message(
            Action::AdminTookDispute,
            Some(order_id),
            None,
            Some(Payload::Dispute(Uuid::new_v4(), None)),
        );
        assert!(classify_user_resolution(&wrong_action).is_none());

        let missing_order = resolution_message(
            Action::Released,
            None,
            None,
            Some(Payload::Dispute(Uuid::new_v4(), None)),
        );
        assert!(classify_user_resolution(&missing_order).is_none());
    }

    #[test]
    fn history_since_looks_back_the_window_plus_overlap() {
        assert_eq!(
            history_since(1_000_000),
            1_000_000 - HISTORY_WINDOW_SECS - RESUME_OVERLAP_SECS
        );
    }

    #[test]
    fn info_message_helper_covers_user_closed_statuses_only() {
        let id = Uuid::from_u128(9);
        let coop = user_closed_dispute_info_message(id, &DisputeStatus::CooperativelyCanceled)
            .expect("coop");
        assert!(coop.contains("cooperative cancel"));
        let released =
            user_closed_dispute_info_message(id, &DisputeStatus::Released).expect("released");
        assert!(released.contains("seller released"));
        assert!(user_closed_dispute_info_message(id, &DisputeStatus::Settled).is_none());
        assert!(user_closed_dispute_info_message(id, &DisputeStatus::SellerRefunded).is_none());
    }

    async fn create_admin_disputes_table(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE admin_disputes (
                id TEXT PRIMARY KEY,
                dispute_id TEXT NOT NULL,
                kind TEXT,
                status TEXT,
                hash TEXT,
                preimage TEXT,
                order_previous_status TEXT,
                initiator_pubkey TEXT NOT NULL,
                buyer_pubkey TEXT,
                seller_pubkey TEXT,
                initiator_full_privacy INTEGER NOT NULL,
                counterpart_full_privacy INTEGER NOT NULL,
                initiator_info TEXT,
                counterpart_info TEXT,
                premium INTEGER NOT NULL,
                payment_method TEXT NOT NULL,
                amount INTEGER NOT NULL,
                fiat_amount INTEGER NOT NULL,
                fiat_code TEXT NOT NULL,
                fee INTEGER NOT NULL,
                routing_fee INTEGER NOT NULL,
                buyer_invoice TEXT,
                invoice_held_at INTEGER,
                taken_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                buyer_chat_last_seen INTEGER,
                seller_chat_last_seen INTEGER,
                buyer_shared_key_hex TEXT,
                seller_shared_key_hex TEXT
            );
            "#,
        )
        .execute(pool)
        .await
        .expect("admin_disputes table");
    }

    async fn insert_in_progress(pool: &SqlitePool, dispute_id: &str) {
        sqlx::query(
            r#"INSERT INTO admin_disputes (
                id, dispute_id, initiator_pubkey, initiator_full_privacy,
                counterpart_full_privacy, premium, payment_method, amount, fiat_amount,
                fiat_code, fee, routing_fee, taken_at, created_at, status
            ) VALUES (?, ?, 'npub1initiator', 0, 0, 0, 'sepa', 0, 0, 'USD', 0, 0, 1, 1, 'in-progress')"#,
        )
        .bind(format!("order-{dispute_id}"))
        .bind(dispute_id)
        .execute(pool)
        .await
        .expect("insert in-progress row");
    }

    #[test]
    fn foreign_author_is_rejected_even_when_admin_equals_mostro() {
        let mostro = Keys::generate();
        let attacker = Keys::generate();
        let message = resolution_message(
            Action::CooperativeCancelAccepted,
            Some(Uuid::new_v4()),
            None,
            Some(Payload::Dispute(Uuid::new_v4(), None)),
        );
        let mostro_event = wrap_message_nip44(
            &message,
            &mostro,
            &mostro,
            mostro.public_key(),
            WrapOptions::default(),
        )
        .expect("wrap mostro");
        let attacker_event = wrap_message_nip44(
            &message,
            &attacker,
            &attacker,
            mostro.public_key(),
            WrapOptions::default(),
        )
        .expect("wrap attacker");

        assert!(is_mostro_authored(&mostro_event, mostro.public_key()));
        assert!(!is_mostro_authored(&attacker_event, mostro.public_key()));
    }

    #[tokio::test]
    async fn accept_ignores_foreign_author_when_admin_equals_mostro() {
        let mostro = Keys::generate();
        let admin = mostro.clone();
        let attacker = Keys::generate();
        let dispute_id = Uuid::new_v4();
        let order_id = Uuid::new_v4();
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        create_admin_disputes_table(&pool).await;
        insert_in_progress(&pool, &dispute_id.to_string()).await;

        let message = resolution_message(
            Action::CooperativeCancelAccepted,
            Some(order_id),
            None,
            Some(Payload::Dispute(dispute_id, None)),
        );
        let event = wrap_message_nip44(
            &message,
            &attacker,
            &attacker,
            admin.public_key(),
            WrapOptions::default(),
        )
        .expect("wrap");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        accept(
            &event,
            &Client::default(),
            &admin,
            mostro.public_key(),
            &pool,
            &tx,
        )
        .await;

        assert!(rx.try_recv().is_err(), "foreign author must not notify");
        let row = AdminDispute::get_by_dispute_id(&pool, &dispute_id.to_string())
            .await
            .expect("query")
            .expect("row");
        assert_eq!(row.status.as_deref(), Some("in-progress"));
    }

    #[tokio::test]
    async fn classify_works_on_unwrapped_nip44_event() {
        let mostro = Keys::generate();
        let admin = Keys::generate();
        let dispute_id = Uuid::new_v4();
        let order_id = Uuid::new_v4();
        let message = resolution_message(
            Action::CooperativeCancelAccepted,
            Some(order_id),
            None,
            Some(Payload::Dispute(dispute_id, None)),
        );
        let event = wrap_message_nip44(
            &message,
            &mostro,
            &mostro,
            admin.public_key(),
            WrapOptions::default(),
        )
        .expect("wrap");

        let unwrapped = unwrap_incoming(&event, &admin)
            .await
            .expect("unwrap ok")
            .expect("decryptable");
        let resolved = classify_user_resolution(&unwrapped.message).expect("classified");
        assert_eq!(resolved.dispute_id, dispute_id);
        assert_eq!(resolved.order_id, order_id);
    }
}
