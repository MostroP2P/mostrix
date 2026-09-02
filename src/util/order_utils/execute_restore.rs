// Session restore: recover orders and disputes for this identity from Mostro.
use anyhow::Result;
use mostro_core::prelude::*;
use nostr_sdk::prelude::*;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::str::FromStr;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::models::{Order, User};
use crate::ui::helpers::user_dispute_chat_since_from_file;
use crate::util::chat_listener::track_user_dispute_chat;
use crate::util::chat_utils::derive_shared_key_hex;
use crate::util::dm_utils::{
    parse_dm_events, send_dm, wait_for_dm, OrderDmSubscriptionCmd, FETCH_EVENTS_TIMEOUT,
};
use crate::util::mostro_info::MostroInstanceInfo;
use crate::util::sync_trade_index::{
    effective_last_trade_index, fetch_last_trade_index_from_mostro,
};
use crate::util::types::get_cant_do_description;

use super::helper::{
    fetch_small_order_by_id_from_relay, handle_mostro_response, is_terminal_trade_status,
};

/// Outcome of a session restore, for the result popup.
#[derive(Debug, Default)]
pub struct RestoreSummary {
    /// Orders inserted into the local database.
    pub restored: usize,
    /// Orders that already existed locally (only their status was refreshed).
    pub already_known: usize,
    /// Restored orders whose details could not be found on the relays
    /// (persisted with what Mostro returned: id, trade index and status).
    pub missing_details: usize,
    /// Restored orders whose maker/taker role could not be determined
    /// (persisted as taker).
    pub role_unknown: usize,
    /// Orders that could not be persisted at all.
    pub failed: usize,
    /// Disputes reported by Mostro for this identity.
    pub disputes: usize,
    /// Disputes whose local order could not be moved to `Dispute` (row missing
    /// or write failed) — the dispute is still real on Mostro's side.
    pub dispute_status_failed: usize,
}

impl RestoreSummary {
    pub fn to_user_message(&self) -> String {
        let mut msg = format!(
            "Session restored: {} order(s) recovered, {} already known, {} dispute(s).",
            self.restored, self.already_known, self.disputes
        );
        if self.missing_details > 0 {
            msg.push_str(&format!(
                " {} order(s) had no relay details and were saved with minimal info.",
                self.missing_details
            ));
        }
        if self.role_unknown > 0 {
            msg.push_str(&format!(
                " {} order(s) restored with unknown maker/taker role (shown as taker).",
                self.role_unknown
            ));
        }
        if self.failed > 0 {
            msg.push_str(&format!(
                " {} order(s) could not be saved — see log.",
                self.failed
            ));
        }
        if self.dispute_status_failed > 0 {
            msg.push_str(&format!(
                " {} dispute(s) could not be marked locally — see log.",
                self.dispute_status_failed
            ));
        }
        msg
    }
}

/// Map the outcome of [`execute_restore_session`] to the operation result the
/// restore task must emit. `Ok` MUST become [`crate::ui::OperationResult::SessionRestored`]
/// — not a plain `Info` — because only that variant makes `apply_order_result`
/// clear stale chat projection, re-run the DB-to-UI sync, and spawn post-restore
/// relay hydrates; with `Info` the restored rows stay invisible until a later sync
/// or restart.
pub fn restore_completion_result(outcome: &Result<RestoreSummary>) -> crate::ui::OperationResult {
    match outcome {
        Ok(summary) => crate::ui::OperationResult::SessionRestored {
            message: summary.to_user_message(),
        },
        Err(e) => crate::ui::OperationResult::Error(format!("Restore failed: {e}")),
    }
}

/// Ask Mostro for this identity's session state (`Action::RestoreSession`) and
/// rebuild the local database from the answer.
///
/// Restore is account-scoped: Mostro indexes users by identity pubkey, so the
/// whole exchange (send, wait, decrypt) runs on the identity keys — a trade key
/// would look like an unknown user and recovery would return nothing. The
/// request carries no request id (`Message::new_restore`), so the response is
/// validated by action instead of by id.
///
/// For every order Mostro reports, the trade keys are re-derived from the
/// user's mnemonic at the reported trade index, full details are fetched from
/// the relays when available, and the row is inserted locally. Non-terminal
/// orders are handed to the DM router (`TrackOrder`) so their messages route
/// live without a restart. Disputes restore their id and, when a solver is
/// already assigned, the user↔solver chat secret (re-derived from the restored
/// trade keys) so an in-flight dispute stays readable. Stage 3 fetches
/// `Action::LastTradeIndex` from Mostro; `last_trade_index` advances to
/// `max(restore indices, mostro_last)` so future trades never reuse a key.
pub async fn execute_restore_session(
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    mostro_instance: Option<&MostroInstanceInfo>,
    dm_subscription_tx: UnboundedSender<OrderDmSubscriptionCmd>,
) -> Result<RestoreSummary> {
    let user = User::get(pool).await?;
    let identity_keys = User::get_identity_keys(pool).await?;

    let message = Message::new_restore(None);
    let message_json = message
        .as_json()
        .map_err(|e| anyhow::anyhow!("Failed to serialize message: {e}"))?;

    log::info!(
        "Restore: requesting session state from {mostro_pubkey} as {}",
        identity_keys.public_key()
    );

    let sent_message = send_dm(
        client,
        Some(&identity_keys),
        &identity_keys,
        &mostro_pubkey,
        message_json,
        None,
        mostro_instance,
    );

    let recv_event = wait_for_dm(&identity_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;
    let messages = parse_dm_events(recv_event, &identity_keys, None).await;

    let Some((response_message, _, sender)) = messages.first() else {
        return Err(anyhow::anyhow!("No response received from Mostro"));
    };
    // The restore request carries no request id, so unlike the order flows the
    // response cannot be tied back by a random id only Mostro could echo. The
    // sender check is the only thing standing between us and a forged
    // gift-wrapped RestoreData seeding attacker-controlled orders.
    if sender != &mostro_pubkey {
        return Err(anyhow::anyhow!(
            "Restore response signed by {sender}, expected the configured Mostro instance"
        ));
    }
    let inner = response_message.get_inner_message_kind();

    if let Some(Payload::CantDo(reason)) = &inner.payload {
        let error_msg = match reason {
            Some(r) => get_cant_do_description(r),
            None => "Unknown error - Mostro couldn't process your request".to_string(),
        };
        return Err(anyhow::anyhow!(error_msg));
    }
    if inner.action != Action::RestoreSession {
        return Err(anyhow::anyhow!(
            "Unexpected action in response: {:?}",
            inner.action
        ));
    }
    let Some(Payload::RestoreData(restore_data)) = &inner.payload else {
        return Err(anyhow::anyhow!("No restore data payload in response"));
    };

    log::info!(
        "Restore stage 1: received {} order(s), {} dispute(s) from Mostro",
        restore_data.restore_orders.len(),
        restore_data.restore_disputes.len()
    );

    // Stage 2 (mobile-aligned): batch-fetch authoritative order details from Mostro
    // so buyer/seller trade pubkeys are available for role + peer chat resolution.
    let order_ids: Vec<Uuid> = restore_data
        .restore_orders
        .iter()
        .map(|o| o.order_id)
        .collect();
    let mostro_order_details = fetch_order_details_from_mostro(
        client,
        &identity_keys,
        mostro_pubkey,
        &order_ids,
        mostro_instance,
    )
    .await
    .unwrap_or_else(|e| {
        log::warn!(
            "Restore stage 2: Action::Orders fetch failed ({e}); falling back to relay lookup per order"
        );
        HashMap::new()
    });
    if !mostro_order_details.is_empty() {
        log::info!(
            "Restore stage 2: loaded details for {} order(s) from Mostro",
            mostro_order_details.len()
        );
    }

    // Stage 3 (mobile-aligned): authoritative last-trade-index from Mostro.
    // Always run — even when restore_orders is empty (completed-only history).
    let mostro_last =
        fetch_last_trade_index_from_mostro(client, &identity_keys, mostro_pubkey, mostro_instance)
            .await?;
    log::info!(
        "Restore stage 3: Mostro last trade index is {} (no_history={})",
        mostro_last.last_used_index,
        mostro_last.no_history
    );

    let mut summary = RestoreSummary {
        disputes: restore_data.restore_disputes.len(),
        ..Default::default()
    };

    // Advance last_trade_index BEFORE writing any order row. Mostro's index is
    // authoritative, and this ordering guarantees the failure mode is always
    // "index bumped, some rows missing" (a re-run repairs it) and never "rows
    // with restored trade keys present, index stale" (a later order would reuse
    // a restored key). If this write fails nothing else has been touched.
    let restore_max = restore_data
        .restore_orders
        .iter()
        .map(|o| o.trade_index)
        .chain(restore_data.restore_disputes.iter().map(|d| d.trade_index))
        .max()
        .unwrap_or(0);
    let effective_last = effective_last_trade_index(restore_max, mostro_last.last_used_index);
    if effective_last > user.last_trade_index.unwrap_or(0) {
        User::update_last_trade_index(pool, effective_last).await?;
        log::info!(
            "Restore: advanced last_trade_index to {effective_last} (restore_max={restore_max}, mostro_last={})",
            mostro_last.last_used_index
        );
    }

    for info in &restore_data.restore_orders {
        let mostro_detail = mostro_order_details.get(&info.order_id).cloned();
        match restore_one_order(
            pool,
            client,
            mostro_pubkey,
            &user,
            info,
            mostro_detail.as_ref(),
        )
        .await
        {
            Ok(RestoredAs::Inserted {
                with_details,
                role_unknown,
            }) => {
                summary.restored += 1;
                if !with_details {
                    summary.missing_details += 1;
                }
                if role_unknown {
                    summary.role_unknown += 1;
                }
            }
            Ok(RestoredAs::AlreadyKnown) => summary.already_known += 1,
            Err(e) => {
                // Keep going: one bad order must not abort the recovery of the rest.
                log::error!("Restore failed for order {}: {e}", info.order_id);
                summary.failed += 1;
                continue;
            }
        }

        let terminal = Status::from_str(&info.status)
            .map(is_terminal_trade_status)
            .unwrap_or(false);
        if !terminal {
            let _ = dm_subscription_tx.send(OrderDmSubscriptionCmd::TrackOrder {
                order_id: info.order_id,
                trade_index: info.trade_index,
            });
        }
    }

    // Disputed orders come back in the orders list too, so their row already
    // exists here; this pass restores what makes a dispute usable again: the
    // status, the dispute id, and the solver chat the user would otherwise
    // lose exactly when they need it most.
    for dispute in &restore_data.restore_disputes {
        let id_str = dispute.order_id.to_string();
        // UPDATE on a missing row is a silent no-op, so check presence first:
        // a dispute whose order failed to restore must not be reported as applied.
        let applied = match Order::get_by_id(pool, &id_str).await {
            Ok(row) => Order::update_status(pool, &id_str, Status::Dispute)
                .await
                .map_err(|e| e.to_string())
                .map(|()| row),
            Err(e) => Err(format!("order row not present: {e}")),
        };
        let row = match applied {
            Ok(row) => row,
            Err(e) => {
                log::error!(
                    "Restore: could not mark order {} as disputed (dispute {}): {e}",
                    dispute.order_id,
                    dispute.dispute_id
                );
                summary.dispute_status_failed += 1;
                continue;
            }
        };

        if let Err(e) =
            Order::update_dispute_id(pool, &id_str, &dispute.dispute_id.to_string()).await
        {
            log::warn!("Restore: failed to persist dispute id for order {id_str}: {e}");
        }

        // Re-derive the user↔solver chat secret from the restored trade keys, so
        // an in-flight dispute stays readable after a reinstall instead of
        // silently losing its conversation.
        let Some(solver_pubkey_str) = dispute.solver_pubkey.as_deref() else {
            continue;
        };
        let restored_chat = row
            .trade_keys
            .as_deref()
            .and_then(|hex| Keys::parse(hex).ok())
            .and_then(|trade_keys| {
                derive_shared_key_hex(Some(&trade_keys), Some(solver_pubkey_str))
                    .map(|hex| (trade_keys, hex))
            });
        let Some((trade_keys, shared_hex)) = restored_chat else {
            log::warn!(
                "Restore: could not derive solver chat key for order {id_str} (dispute {})",
                dispute.dispute_id
            );
            continue;
        };
        match Order::update_solver_chat(pool, &id_str, solver_pubkey_str, &shared_hex).await {
            Ok(()) => match PublicKey::parse(solver_pubkey_str) {
                Ok(solver_pubkey) => track_user_dispute_chat(
                    id_str.clone(),
                    shared_hex,
                    trade_keys.public_key(),
                    solver_pubkey,
                    user_dispute_chat_since_from_file(&id_str),
                ),
                Err(e) => log::warn!("Restore: invalid solver pubkey for order {id_str}: {e}"),
            },
            Err(e) => log::warn!("Restore: failed to persist solver chat for {id_str}: {e}"),
        }
    }

    // A successful restore used to leave no trace at all in the log, which made
    // "did it run and find nothing?" indistinguishable from "did it run?".
    log::info!("Restore: {}", summary.to_user_message());

    Ok(summary)
}

/// Stage 2: `Action::Orders` with `Payload::Ids` — same batch detail fetch mobile uses
/// after `restore-session` returns order ids + trade indices.
async fn fetch_order_details_from_mostro(
    client: &Client,
    identity_keys: &Keys,
    mostro_pubkey: PublicKey,
    order_ids: &[Uuid],
    mostro_instance: Option<&MostroInstanceInfo>,
) -> Result<HashMap<Uuid, SmallOrder>> {
    if order_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let request_id = Uuid::new_v4().as_u128() as u64;
    let message = Message::new_order(
        None,
        Some(request_id),
        None,
        Action::Orders,
        Some(Payload::Ids(order_ids.to_vec())),
    );
    let message_json = message
        .as_json()
        .map_err(|e| anyhow::anyhow!("Failed to serialize orders request: {e}"))?;

    log::info!(
        "Restore stage 2: requesting details for {} order(s) from {mostro_pubkey}",
        order_ids.len()
    );

    let sent_message = send_dm(
        client,
        Some(identity_keys),
        identity_keys,
        &mostro_pubkey,
        message_json,
        None,
        mostro_instance,
    );

    let recv_event = wait_for_dm(identity_keys, FETCH_EVENTS_TIMEOUT, sent_message).await?;
    let messages = parse_dm_events(recv_event, identity_keys, None).await;

    let Some((response_message, _, sender)) = messages.first() else {
        return Err(anyhow::anyhow!("No response received for Action::Orders"));
    };
    if sender != &mostro_pubkey {
        return Err(anyhow::anyhow!(
            "Orders response signed by {sender}, expected the configured Mostro instance"
        ));
    }

    let inner = handle_mostro_response(response_message, request_id)?;
    if inner.action != Action::Orders {
        return Err(anyhow::anyhow!(
            "Unexpected action in orders response: {:?}",
            inner.action
        ));
    }

    let Some(Payload::Orders(orders)) = &inner.payload else {
        return Err(anyhow::anyhow!("Orders response missing Payload::Orders"));
    };

    let mut map = HashMap::with_capacity(orders.len());
    for order in orders {
        if let Some(id) = order.id {
            map.insert(id, order.clone());
        }
    }
    Ok(map)
}

enum RestoredAs {
    Inserted {
        with_details: bool,
        role_unknown: bool,
    },
    AlreadyKnown,
}

/// Maker/taker resolution for a restored order.
///
/// Neither the restore payload nor the public relay events carry the role
/// (kind-38383 tags stop at the order terms — no buyer/seller pubkeys), so it
/// has to be inferred where the protocol allows it:
/// - `Pending` / `WaitingMakerBond` orders exist only for their maker; any
///   taker interaction immediately moves the order out of those states.
/// - Anything else is genuinely ambiguous. Those rows fall back to taker and
///   are counted in the summary so the fallback is never silent.
#[derive(Debug, PartialEq, Eq)]
enum RestoredRole {
    Maker,
    UnknownAsTaker,
}

/// A row persisted without relay details: `Order::new` from a default
/// `SmallOrder` leaves `fiat_code` empty, which no real order has.
fn is_minimal_placeholder(order: &Order) -> bool {
    order.fiat_code.trim().is_empty()
}

fn restored_order_role(status: Option<Status>) -> RestoredRole {
    match status {
        Some(Status::Pending) | Some(Status::WaitingMakerBond) => RestoredRole::Maker,
        _ => RestoredRole::UnknownAsTaker,
    }
}

/// When Mostro returns full `SmallOrder` details, infer maker vs taker from trade pubkeys.
fn is_maker_from_mostro_details(order: &SmallOrder, trade_keys: &Keys) -> Option<bool> {
    let my_pk = trade_keys.public_key();
    let kind = order.kind?;
    let buyer_s = order.buyer_trade_pubkey.as_deref()?;
    let seller_s = order.seller_trade_pubkey.as_deref()?;
    if buyer_s.is_empty() || seller_s.is_empty() {
        return None;
    }
    let buyer_pk = PublicKey::parse(buyer_s).ok()?;
    let seller_pk = PublicKey::parse(seller_s).ok()?;
    if my_pk == buyer_pk {
        return Some(matches!(kind, mostro_core::order::Kind::Buy));
    }
    if my_pk == seller_pk {
        return Some(matches!(kind, mostro_core::order::Kind::Sell));
    }
    None
}

async fn restore_one_order(
    pool: &SqlitePool,
    client: &Client,
    mostro_pubkey: PublicKey,
    user: &User,
    info: &RestoredOrdersInfo,
    mostro_detail: Option<&SmallOrder>,
) -> Result<RestoredAs> {
    let id_str = info.order_id.to_string();

    // A row from an earlier restore that never got relay details (empty fiat
    // code is impossible for a real order) is rehydrated by this run instead of
    // staying a minimal placeholder forever.
    let placeholder = match Order::get_by_id(pool, &id_str).await {
        Ok(existing) => {
            if !is_minimal_placeholder(&existing) {
                if let Ok(status) = Status::from_str(&info.status) {
                    Order::update_status(pool, &id_str, status).await?;
                }
                return Ok(RestoredAs::AlreadyKnown);
            }
            true
        }
        Err(_) => false,
    };

    let trade_keys = user.derive_trade_keys(info.trade_index)?;

    // Prefer stage-2 Mostro details (trade pubkeys), then relay, then minimal row.
    let small_order = if let Some(detail) = mostro_detail {
        let mut o = detail.clone();
        o.id = Some(info.order_id);
        if let Ok(status) = Status::from_str(&info.status) {
            o.status = Some(status);
        }
        Some(o)
    } else {
        match fetch_small_order_by_id_from_relay(client, mostro_pubkey, info.order_id).await {
            Ok(found) => found,
            Err(e) => {
                log::warn!(
                    "Restore: relay lookup failed for order {} (saving minimal row): {e}",
                    info.order_id
                );
                None
            }
        }
    };
    let with_details = small_order.is_some();
    let mut small_order = small_order.unwrap_or_default();
    small_order.id = Some(info.order_id);
    // Mostro's database is authoritative for the status; relay events may lag.
    if let Ok(status) = Status::from_str(&info.status) {
        small_order.status = Some(status);
    }

    let (is_maker, role_unknown) = match is_maker_from_mostro_details(&small_order, &trade_keys) {
        Some(maker) => (maker, false),
        None => {
            let heuristic = restored_order_role(small_order.status);
            (
                matches!(heuristic, RestoredRole::Maker),
                matches!(heuristic, RestoredRole::UnknownAsTaker),
            )
        }
    };

    if placeholder {
        // Rehydrate through the upsert that merges onto the existing row.
        // `Order::new` would rebuild from a default `SmallOrder` and fall back
        // to `update_db`, which writes every column — erasing the peer chat
        // key, dispute id and solver chat that live DMs may have persisted on
        // this row since the previous restore. The role decided on the first
        // pass is preserved too, so it is not re-counted here.
        Order::upsert_from_small_order_dm(pool, info.order_id, small_order, &trade_keys, None)
            .await?;
        return Ok(RestoredAs::Inserted {
            with_details,
            role_unknown,
        });
    }

    Order::insert_from_restore(pool, small_order, &trade_keys, info.trade_index, is_maker).await?;
    Ok(RestoredAs::Inserted {
        with_details,
        role_unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        is_maker_from_mostro_details, is_minimal_placeholder, restore_completion_result,
        restored_order_role, RestoreSummary, RestoredRole,
    };
    use crate::ui::OperationResult;
    use mostro_core::prelude::{Kind, SmallOrder, Status};
    use nostr_sdk::prelude::Keys;
    use uuid::Uuid;

    fn sample_small_order(kind: Kind, buyer_hex: &str, seller_hex: &str) -> SmallOrder {
        SmallOrder {
            id: Some(Uuid::new_v4()),
            kind: Some(kind),
            buyer_trade_pubkey: Some(buyer_hex.to_string()),
            seller_trade_pubkey: Some(seller_hex.to_string()),
            fiat_code: "USD".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn is_maker_from_mostro_details_matches_buyer_on_buy_order() {
        let trade_keys = Keys::generate();
        let order = sample_small_order(
            Kind::Buy,
            &trade_keys.public_key().to_string(),
            &Keys::generate().public_key().to_string(),
        );
        assert_eq!(
            is_maker_from_mostro_details(&order, &trade_keys),
            Some(true)
        );
    }

    #[test]
    fn is_maker_from_mostro_details_matches_seller_on_sell_order() {
        let trade_keys = Keys::generate();
        let order = sample_small_order(
            Kind::Sell,
            &Keys::generate().public_key().to_string(),
            &trade_keys.public_key().to_string(),
        );
        assert_eq!(
            is_maker_from_mostro_details(&order, &trade_keys),
            Some(true)
        );
    }

    #[test]
    fn is_maker_from_mostro_details_taker_when_buyer_on_sell_order() {
        let trade_keys = Keys::generate();
        let order = sample_small_order(
            Kind::Sell,
            &trade_keys.public_key().to_string(),
            &Keys::generate().public_key().to_string(),
        );
        assert_eq!(
            is_maker_from_mostro_details(&order, &trade_keys),
            Some(false)
        );
    }

    #[test]
    fn successful_restore_emits_session_restored_not_info() {
        // Regression (#114 review, twice): only SessionRestored makes
        // apply_order_result re-run the DB-to-UI sync. A plain Info here means
        // the restored rows stay invisible until restart.
        let summary = RestoreSummary {
            restored: 2,
            ..Default::default()
        };
        let expected = summary.to_user_message();
        match restore_completion_result(&Ok(summary)) {
            OperationResult::SessionRestored { message } => assert_eq!(message, expected),
            other => panic!("expected SessionRestored, got {other:?}"),
        }
    }

    #[test]
    fn failed_restore_emits_an_error_result() {
        match restore_completion_result(&Err(anyhow::anyhow!("boom"))) {
            OperationResult::Error(message) => assert!(message.contains("boom")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn summary_message_covers_the_happy_path() {
        let s = RestoreSummary {
            restored: 3,
            already_known: 1,
            disputes: 1,
            ..Default::default()
        };
        assert_eq!(
            s.to_user_message(),
            "Session restored: 3 order(s) recovered, 1 already known, 1 dispute(s)."
        );
    }

    #[test]
    fn maker_is_inferred_only_from_maker_exclusive_statuses() {
        // A pending / waiting-maker-bond order can only exist for its maker.
        assert_eq!(
            restored_order_role(Some(Status::Pending)),
            RestoredRole::Maker
        );
        assert_eq!(
            restored_order_role(Some(Status::WaitingMakerBond)),
            RestoredRole::Maker
        );
        // Anything else is ambiguous: fall back to taker, but never silently.
        assert_eq!(
            restored_order_role(Some(Status::Active)),
            RestoredRole::UnknownAsTaker
        );
        assert_eq!(
            restored_order_role(Some(Status::FiatSent)),
            RestoredRole::UnknownAsTaker
        );
        assert_eq!(restored_order_role(None), RestoredRole::UnknownAsTaker);
    }

    #[test]
    fn placeholder_rows_are_detected_by_their_empty_fiat_code() {
        // Regression: a row saved without relay details must be re-hydrated on
        // the next restore instead of being frozen as AlreadyKnown forever.
        let mut order = crate::models::Order {
            id: Some("x".into()),
            kind: None,
            status: None,
            amount: 0,
            fiat_code: String::new(),
            min_amount: None,
            max_amount: None,
            fiat_amount: 0,
            payment_method: String::new(),
            premium: 0,
            trade_keys: None,
            counterparty_pubkey: None,
            order_chat_shared_key_hex: None,
            dispute_id: None,
            solver_pubkey: None,
            dispute_chat_shared_key_hex: None,
            is_mine: false,
            buyer_invoice: None,
            request_id: None,
            trade_index: Some(1),
            created_at: None,
            expires_at: None,
            last_seen_dm_ts: None,
        };
        assert!(is_minimal_placeholder(&order));
        order.fiat_code = "EUR".into();
        assert!(!is_minimal_placeholder(&order));
    }

    #[test]
    fn summary_message_reports_dispute_status_failures() {
        let s = RestoreSummary {
            disputes: 2,
            dispute_status_failed: 1,
            ..Default::default()
        }
        .to_user_message();
        assert!(s.contains("1 dispute(s) could not be marked locally"));
        assert!(!RestoreSummary::default()
            .to_user_message()
            .contains("could not be marked"));
    }

    #[test]
    fn summary_message_reports_unknown_roles() {
        let s = RestoreSummary {
            restored: 2,
            role_unknown: 2,
            ..Default::default()
        }
        .to_user_message();
        assert!(s.contains("2 order(s) restored with unknown maker/taker role"));
        assert!(!RestoreSummary::default()
            .to_user_message()
            .contains("maker/taker"));
    }

    #[test]
    fn summary_message_mentions_missing_details_and_failures_only_when_present() {
        let clean = RestoreSummary::default().to_user_message();
        assert!(!clean.contains("relay details"));
        assert!(!clean.contains("could not be saved"));

        let bumpy = RestoreSummary {
            restored: 2,
            missing_details: 1,
            failed: 1,
            ..Default::default()
        }
        .to_user_message();
        assert!(bumpy.contains("1 order(s) had no relay details"));
        assert!(bumpy.contains("1 order(s) could not be saved"));
    }
}
