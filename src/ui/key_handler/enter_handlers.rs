use crate::models::{Order, ORDER_HISTORY_BULK_DELETE_STATUSES};
use crate::ui::helpers::{build_active_order_chat_list, save_order_chat_message};
use crate::ui::key_handler::chat_helpers::{
    handle_enter_finalize_popup, message_counter, FinalizeDisputePopupButton,
};
use crate::ui::key_handler::input_helpers::{
    prepare_admin_chat_message, send_admin_chat_message_via_shared_key,
};
use crate::ui::orders::{
    invoice_popup_allowed_for_order_status, strip_new_order_messages_and_clamp_selected,
};
use crate::ui::{
    order_message_to_notification, AdminMode, AdminTab, AppState, ChatParty, InvoiceInputState,
    InvoiceNotificationActionSelection, MessageViewState, OperationResult, RatingOrderState, Tab,
    TakeOrderState, ThreeState, UiMode, UserMode, UserRole, UserTab,
    ViewingMessageButtonSelection,
};
// User handlers moved to user_handlers.rs
use crate::ui::key_handler::async_tasks::{
    spawn_key_rotation_task, spawn_load_seed_words_task,
    spawn_refresh_mostro_info_from_settings_task, spawn_refresh_mostro_info_task,
    spawn_send_new_order_task,
};
use crate::ui::key_handler::user_handlers::{
    handle_enter_creating_order, handle_enter_taking_order,
};
use bip39::Mnemonic;
use mostro_core::prelude::*;
use nostr_sdk::nips::nip06::FromMnemonic;
use nostr_sdk::prelude::{Keys, PublicKey, SecretKey};
use nostr_sdk::ToBech32;
use std::collections::HashSet;
use std::str::FromStr;

use crate::ui::key_handler::confirmation::{
    create_key_input_state, handle_confirmation_enter, handle_input_to_confirmation,
};
use crate::ui::key_handler::settings::{
    clear_currency_filters, handle_mode_switch, save_currency_to_settings,
    save_mostro_pubkey_to_settings, save_relay_to_settings,
};

fn invoice_popup_action_for_message_action(action: &Action) -> Option<Action> {
    match action {
        Action::AddInvoice | Action::WaitingBuyerInvoice => Some(Action::AddInvoice),
        Action::PayInvoice | Action::WaitingSellerToPay => Some(Action::PayInvoice),
        _ => None,
    }
}
use crate::ui::key_handler::validation::{
    validate_currency, validate_mostro_pubkey, validate_relay,
};

fn generate_mnemonic_12_words() -> std::result::Result<String, String> {
    Mnemonic::generate(12)
        .map(|m| m.to_string())
        .map_err(|e| e.to_string())
}

fn derive_identity_nsec_from_mnemonic(mnemonic: &str) -> std::result::Result<String, String> {
    let account: u32 = mostro_core::prelude::NOSTR_ORDER_EVENT_KIND as u32;
    let identity_keys =
        Keys::from_mnemonic_advanced(mnemonic, None, Some(account), Some(0), Some(0))
            .map_err(|e| e.to_string())?;

    identity_keys
        .secret_key()
        .to_bech32()
        .map_err(|e| e.to_string())
}

#[derive(Clone)]
struct DisputeChatTarget {
    dispute_id_key: String,
    shared_key_hex: Option<String>,
}

#[derive(Clone)]
struct OrderChatTarget {
    order_id: String,
}

struct EnterChatSendConfig {
    mode_after_send: UiMode,
    input_enabled: bool,
    content: String,
}

/// Generic Enter-to-send pipeline used by dispute and user order chat.
fn run_enter_chat_send_flow<T, ResolveTarget, ApplyLocal, SpawnRemote, ResetInput>(
    app: &mut AppState,
    config: EnterChatSendConfig,
    resolve_target: ResolveTarget,
    apply_local: ApplyLocal,
    spawn_remote: SpawnRemote,
    reset_input: ResetInput,
) where
    ResolveTarget: FnOnce(&mut AppState) -> Option<T>,
    ApplyLocal: FnOnce(&mut AppState, &T, &str),
    SpawnRemote: FnOnce(T, String),
    ResetInput: FnOnce(&mut AppState),
{
    let EnterChatSendConfig {
        mode_after_send,
        input_enabled,
        content,
    } = config;

    app.mode = mode_after_send.clone();
    if content.is_empty() || !input_enabled {
        return;
    }

    let Some(target) = resolve_target(app) else {
        return;
    };

    apply_local(app, &target, &content);
    reset_input(app);
    app.mode = mode_after_send;
    spawn_remote(target, content);
}

fn resolve_selected_order_chat_target(app: &AppState) -> Option<OrderChatTarget> {
    let messages_snapshot = match app.messages.lock() {
        Ok(g) => g.clone(),
        Err(e) => {
            crate::util::request_fatal_restart(format!(
                "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
            ));
            return None;
        }
    };

    build_active_order_chat_list(&messages_snapshot)
        .get(app.selected_order_chat_idx)
        .map(|row| OrderChatTarget {
            order_id: row.order_id.clone(),
        })
}

fn spawn_user_order_chat_send_task(
    ctx: &super::EnterKeyContext<'_>,
    order_id: String,
    content: String,
) {
    let client = ctx.client.clone();
    let pool = ctx.pool.clone();
    let mostro_info = ctx.mostro_info.clone();
    tokio::spawn(async move {
        let order = match crate::models::Order::get_by_id(&pool, &order_id).await {
            Ok(o) => o,
            Err(e) => {
                log::warn!("order chat send skipped (order not found): {}", e);
                return;
            }
        };
        let trade_sk = match order
            .trade_keys
            .as_deref()
            .and_then(|h| SecretKey::from_str(h).ok())
        {
            Some(sk) => sk,
            None => return,
        };
        let trade_keys = Keys::new(trade_sk);
        let shared_keys = order
            .order_chat_shared_key_hex
            .as_deref()
            .and_then(crate::util::chat_utils::keys_from_shared_hex)
            .or_else(|| {
                let cp = order.counterparty_pubkey.as_deref()?;
                let pk = PublicKey::parse(cp).ok()?;
                crate::util::chat_utils::derive_shared_keys(Some(&trade_keys), Some(&pk))
            });
        let Some(shared_keys) = shared_keys else {
            return;
        };
        if let Err(e) = crate::util::chat_utils::send_user_order_chat_message_via_shared_key(
            &client,
            &trade_keys,
            &shared_keys,
            &content,
            mostro_info.as_ref(),
        )
        .await
        {
            log::warn!("Failed to send user order chat: {}", e);
        }
    });
}

fn spawn_delete_single_terminal_order_task(
    pool: sqlx::SqlitePool,
    order_id: uuid::Uuid,
    order_result_tx: tokio::sync::mpsc::UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        match Order::delete_terminal_order_by_id(&pool, &order_id.to_string()).await {
            Ok(affected) if affected > 0 => {
                let _ = order_result_tx.send(OperationResult::OrderHistoryDeleted {
                    deleted_order_ids: vec![order_id],
                    message: format!("Deleted order {} from local history.", order_id),
                });
            }
            Ok(_) => {
                let _ = order_result_tx.send(OperationResult::Error(
                    "Selected order is not terminal or no longer exists in local database."
                        .to_string(),
                ));
            }
            Err(e) => {
                let _ = order_result_tx.send(OperationResult::Error(format!(
                    "Failed to delete selected order from local history: {}",
                    e
                )));
            }
        }
    });
}

fn spawn_bulk_history_cleanup_task(
    pool: sqlx::SqlitePool,
    order_result_tx: tokio::sync::mpsc::UnboundedSender<OperationResult>,
) {
    tokio::spawn(async move {
        let mut ids_to_remove = Vec::new();
        let allowed: HashSet<&str> = ORDER_HISTORY_BULK_DELETE_STATUSES.iter().copied().collect();
        if let Ok(rows) = Order::get_user_history_orders(&pool).await {
            ids_to_remove = rows
                .iter()
                .filter(|row| {
                    row.status
                        .as_deref()
                        .map(|status| allowed.contains(&status.to_lowercase().as_str()))
                        .unwrap_or(false)
                })
                .filter_map(|row| {
                    row.id
                        .as_deref()
                        .and_then(|id| uuid::Uuid::parse_str(id).ok())
                })
                .collect();
        }

        match Order::delete_bulk_history_cleanup_orders(&pool).await {
            Ok(affected) => {
                let message = format!(
                    "Removed {} success/canceled orders from local history.",
                    affected
                );
                let _ = order_result_tx.send(OperationResult::OrderHistoryDeleted {
                    deleted_order_ids: ids_to_remove,
                    message,
                });
            }
            Err(e) => {
                let _ = order_result_tx.send(OperationResult::Error(format!(
                    "Failed to clean up order history: {}",
                    e
                )));
            }
        }
    });
}

fn handle_enter_admin_managing_dispute_chat(app: &mut AppState, ctx: &super::EnterKeyContext<'_>) {
    let mode_after_send = UiMode::AdminMode(AdminMode::ManagingDispute);
    if !matches!(app.active_tab, Tab::Admin(AdminTab::DisputesInProgress)) {
        app.mode = mode_after_send;
        return;
    }

    let content = app.admin_chat_input.trim().to_string();
    let input_enabled = app.admin_chat_input_enabled;
    run_enter_chat_send_flow(
        app,
        EnterChatSendConfig {
            mode_after_send,
            input_enabled,
            content,
        },
        |app| {
            app.admin_disputes_in_progress
                .get(app.selected_in_progress_idx)
                .map(|selected_dispute| {
                    let shared_key_hex = match app.active_chat_party {
                        ChatParty::Buyer => selected_dispute.buyer_shared_key_hex.clone(),
                        ChatParty::Seller => selected_dispute.seller_shared_key_hex.clone(),
                    };
                    DisputeChatTarget {
                        dispute_id_key: selected_dispute.dispute_id.clone(),
                        shared_key_hex,
                    }
                })
        },
        |app, target, content| {
            prepare_admin_chat_message(&target.dispute_id_key, content, app);
            message_counter(app, &target.dispute_id_key);
        },
        |target, content| {
            send_admin_chat_message_via_shared_key(
                &target.dispute_id_key,
                target.shared_key_hex.as_deref(),
                &content,
                ctx.client,
                ctx.admin_chat_keys,
                ctx.mostro_info.clone(),
            );
        },
        |app| {
            app.admin_chat_input.clear();
            app.admin_chat_input_enabled = true;
        },
    );
}

fn handle_enter_user_order_chat(app: &mut AppState, ctx: &super::EnterKeyContext<'_>) {
    let mode_after_send = app.mode.clone();
    let content = app.order_chat_input.trim().to_string();
    let input_enabled = app.order_chat_input_enabled;
    run_enter_chat_send_flow(
        app,
        EnterChatSendConfig {
            mode_after_send,
            input_enabled,
            content,
        },
        |app| resolve_selected_order_chat_target(app),
        |app, target, content| {
            let local_msg = crate::ui::UserOrderChatMessage {
                sender: crate::ui::UserChatSender::You,
                content: content.to_string(),
                timestamp: chrono::Utc::now().timestamp(),
                attachment: None,
            };
            app.order_chats
                .entry(target.order_id.clone())
                .or_default()
                .push(local_msg.clone());
            save_order_chat_message(&target.order_id, &local_msg);
        },
        |target, content| {
            spawn_user_order_chat_send_task(ctx, target.order_id, content);
        },
        |app| {
            app.order_chat_input.clear();
            app.order_chat_input_enabled = true;
        },
    );
}

// Admin handlers moved to admin_handlers.rs
use crate::ui::key_handler::admin_handlers::{
    execute_finalize_dispute_action, execute_take_dispute_action, handle_enter_admin_mode,
};

// Message handlers moved to message_handlers.rs
use crate::ui::key_handler::message_handlers::{
    handle_enter_message_notification, handle_enter_rating_order, handle_enter_viewing_message,
};

/// Handle Enter key - dispatches to mode-specific handlers
pub fn handle_enter_key(app: &mut AppState, ctx: &super::EnterKeyContext<'_>) -> bool {
    let default_mode = match app.user_role {
        UserRole::User => UiMode::UserMode(UserMode::Normal),
        UserRole::Admin => UiMode::AdminMode(AdminMode::Normal),
    };
    let current_mode = std::mem::replace(&mut app.mode, default_mode.clone());
    match current_mode {
        UiMode::Normal
        | UiMode::UserMode(UserMode::Normal)
        | UiMode::AdminMode(AdminMode::Normal) => {
            handle_enter_normal_mode(app, ctx);
            true
        }
        UiMode::UserMode(UserMode::CreatingOrder(ref form)) => {
            handle_enter_creating_order(app, form);
            true
        }
        UiMode::UserMode(UserMode::ConfirmingOrder {
            form,
            selected_button,
        }) => {
            if selected_button {
                // YES selected - send the order (similar to handle_confirm_key)
                let form_clone = form.clone();
                app.mode = UiMode::UserMode(UserMode::WaitingForMostro(form_clone.clone()));
                spawn_send_new_order_task(ctx, form_clone);
            } else {
                // NO selected - go back to form
                app.mode = UiMode::UserMode(UserMode::CreatingOrder(form.clone()));
            }
            true
        }
        UiMode::UserMode(UserMode::TakingOrder(take_state)) => {
            handle_enter_taking_order(app, take_state, ctx);
            true
        }
        UiMode::UserMode(UserMode::WaitingForMostro(_))
        | UiMode::UserMode(UserMode::WaitingTakeOrder(_))
        | UiMode::UserMode(UserMode::WaitingAddInvoice) => {
            // No action while waiting
            app.mode = default_mode;
            true
        }
        UiMode::HelpPopup(..) | UiMode::SettingsInstructionsPopup(..) => {
            // Close help / settings reference (mode restored in key_handler/mod.rs)
            true
        }
        UiMode::SaveAttachmentPopup(_) => {
            // Up/Down/Enter/Esc handled in key_handler/mod.rs
            app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
            true
        }
        UiMode::ObserverSaveAttachmentPopup(_) => {
            // Handled in key_handler/mod.rs
            app.mode = default_mode;
            true
        }
        UiMode::OperationResult(_) => {
            if app.fatal_exit_on_close {
                return false;
            }
            // Close result popup. If on Disputes in Progress, stay there and return to ManagingDispute.
            if matches!(app.active_tab, Tab::Admin(AdminTab::DisputesInProgress)) {
                app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
            } else if !matches!(
                app.active_tab,
                Tab::Admin(AdminTab::Settings)
                    | Tab::User(UserTab::Settings)
                    | Tab::Admin(AdminTab::Observer)
                    | Tab::User(UserTab::MyTrades)
                    | Tab::User(UserTab::MostroInfo)
                    | Tab::Admin(AdminTab::MostroInfo)
                    | Tab::User(UserTab::Messages)
            ) {
                app.active_tab = Tab::first(app.user_role);
            }
            true
        }
        UiMode::NewMessageNotification(notification, action, mut invoice_state) => {
            handle_enter_message_notification(
                app,
                ctx,
                &action,
                &mut invoice_state,
                notification.order_id,
            );
            // Mode is updated inside handle_enter_message_notification
            true
        }
        UiMode::ViewingMessage(view_state) => {
            // Enter confirms the selected button (YES or NO)
            handle_enter_viewing_message(app, &view_state, ctx);
            // Mode is updated inside handle_enter_viewing_message
            true
        }
        UiMode::RatingOrder(state) => {
            handle_enter_rating_order(app, &state, ctx);
            true
        }
        UiMode::AdminMode(AdminMode::AddSolver(_))
        | UiMode::AdminMode(AdminMode::ConfirmAddSolver(_, _))
        | UiMode::AdminMode(AdminMode::SetupAdminKey(_))
        | UiMode::AdminMode(AdminMode::ConfirmAdminKey(_, _)) => {
            handle_enter_admin_mode(app, current_mode, default_mode, ctx);
            true
        }
        UiMode::AddMostroPubkey(_)
        | UiMode::ConfirmMostroPubkey(_, _)
        | UiMode::AddRelay(_)
        | UiMode::ConfirmRelay(_, _)
        | UiMode::AddCurrency(_)
        | UiMode::ConfirmCurrency(_, _)
        | UiMode::ConfirmClearCurrencies(_)
        | UiMode::ConfirmExit(_) => {
            let should_continue = handle_enter_settings_mode(app, current_mode, default_mode, ctx);
            if !should_continue {
                return false; // Exit application
            }
            true
        }
        UiMode::ConfirmDeleteHistoryOrder(order_id, selected_button) => {
            if selected_button {
                app.mode = UiMode::OperationResult(OperationResult::Info(
                    "Deleting selected terminal order from local history...".to_string(),
                ));
                spawn_delete_single_terminal_order_task(
                    ctx.pool.clone(),
                    order_id,
                    ctx.order_result_tx.clone(),
                );
            } else {
                app.mode = default_mode;
            }
            true
        }
        UiMode::ConfirmBulkDeleteHistory(selected_button) => {
            if selected_button {
                app.mode = UiMode::OperationResult(OperationResult::Info(
                    "Cleaning up success/canceled orders from local history...".to_string(),
                ));
                spawn_bulk_history_cleanup_task(ctx.pool.clone(), ctx.order_result_tx.clone());
            } else {
                app.mode = default_mode;
            }
            true
        }
        UiMode::ConfirmGenerateNewKeys(selected_button) => {
            if !selected_button {
                // NO: just close warning popup.
                app.mode = default_mode;
                return true;
            }

            // YES: generate mnemonic + derived key, persist in background, then show backup popup.
            let mnemonic = match generate_mnemonic_12_words() {
                Ok(m) => m,
                Err(e) => {
                    app.mode = UiMode::OperationResult(OperationResult::Error(format!(
                        "Failed to generate mnemonic: {}",
                        e
                    )));
                    return true;
                }
            };

            let derived_nsec = match derive_identity_nsec_from_mnemonic(&mnemonic) {
                Ok(nsec) => nsec,
                Err(e) => {
                    app.mode = UiMode::OperationResult(OperationResult::Error(format!(
                        "Failed to derive nsec from mnemonic: {}",
                        e
                    )));
                    return true;
                }
            };

            let is_user_mode = matches!(app.user_role, UserRole::User);

            // Persist rotation asynchronously to avoid UI blocking; backup popup
            // will be shown only after successful commit via key_rotation_rx in main.
            spawn_key_rotation_task(
                ctx.pool.clone(),
                is_user_mode,
                mnemonic.clone(),
                derived_nsec,
                ctx.key_rotation_tx.clone(),
            );

            app.mode =
                UiMode::OperationResult(OperationResult::Info("Saving new keys...".to_string()));
            true
        }
        UiMode::BackupNewKeys(_) => {
            if app.backup_requires_restart {
                // Trigger in-process runtime reload handled by main loop.
                app.pending_key_reload = true;
                app.mode = UiMode::OperationResult(OperationResult::Info(
                    "Reloading keys and resetting active session...".to_string(),
                ));
                true
            } else {
                // First-launch backup flow: no runtime key swap happened.
                app.mode = default_mode;
                true
            }
        }
        UiMode::AdminMode(AdminMode::ConfirmTakeDispute(dispute_id, selected_button)) => {
            if selected_button {
                // YES selected - take the dispute
                execute_take_dispute_action(app, dispute_id, ctx);
            } else {
                // NO selected - go back to normal mode
                app.mode = default_mode;
            }
            true
        }
        UiMode::AdminMode(AdminMode::WaitingTakeDispute(_)) => {
            // No action while waiting
            app.mode = default_mode;
            true
        }
        UiMode::AdminMode(AdminMode::ManagingDispute) => {
            handle_enter_admin_managing_dispute_chat(app, ctx);
            true
        }
        UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization {
            dispute_id,
            selected_button_index,
        }) => {
            // Check if dispute is finalized
            use std::str::FromStr;
            let dispute_is_finalized = app
                .admin_disputes_in_progress
                .iter()
                .find(|d| d.dispute_id == dispute_id.to_string() || d.id == dispute_id.to_string())
                .and_then(|d| d.status.as_deref())
                .and_then(|s| DisputeStatus::from_str(s).ok())
                .map(|s| {
                    matches!(
                        s,
                        DisputeStatus::Settled
                            | DisputeStatus::SellerRefunded
                            | DisputeStatus::Released
                    )
                })
                .unwrap_or(false);

            // Handle Enter in finalization popup
            match FinalizeDisputePopupButton::from_index(selected_button_index) {
                Some(button) => handle_enter_finalize_popup(
                    app,
                    button,
                    dispute_id,
                    dispute_is_finalized,
                    ctx.order_result_tx,
                ),
                None => {
                    app.mode = default_mode;
                    true
                }
            }
        }
        UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute {
            dispute_id,
            is_settle,
            selected_button,
        }) => {
            if selected_button {
                // YES selected - execute the finalization action
                execute_finalize_dispute_action(app, dispute_id, ctx, is_settle);
            } else {
                // NO selected - go back to finalization popup
                app.mode = UiMode::AdminMode(AdminMode::ReviewingDisputeForFinalization {
                    dispute_id,
                    // Restore the button that was selected: 0=Pay Buyer, 1=Refund Seller
                    selected_button_index: if is_settle { 0 } else { 1 },
                });
            }
            true
        }
        UiMode::AdminMode(AdminMode::WaitingDisputeFinalization(_)) => {
            // No action while waiting
            app.mode = default_mode;
            true
        }
    }
}

/// Handle Enter key for settings-related modes (Mostro pubkey, relay, currency, etc.)
fn handle_enter_settings_mode(
    app: &mut AppState,
    mode: UiMode,
    default_mode: UiMode,
    ctx: &super::EnterKeyContext<'_>,
) -> bool {
    match mode {
        UiMode::AddMostroPubkey(key_state) => {
            // Validate Mostro pubkey (hex format) before proceeding to confirmation
            match validate_mostro_pubkey(&key_state.key_input) {
                Ok(_) => {
                    app.mode =
                        handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                            UiMode::ConfirmMostroPubkey(input, true)
                        });
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::OperationResult(OperationResult::Error(e));
                }
            }
        }
        UiMode::ConfirmMostroPubkey(key_string, selected_button) => {
            app.mode = handle_confirmation_enter(
                selected_button,
                &key_string,
                default_mode,
                save_mostro_pubkey_to_settings,
                |input| UiMode::AddMostroPubkey(create_key_input_state(input)),
            );

            // If the selected button is YES, spawn a task to refresh Mostro instance info
            // using the new pubkey (no disk round-trip); UI stays responsive.
            if selected_button {
                let new_pubkey = match PublicKey::from_str(&key_string) {
                    Ok(pk) => pk,
                    Err(e) => {
                        log::error!("Invalid pubkey after confirmation: {}", e);
                        return false;
                    }
                };
                match ctx.current_mostro_pubkey.lock() {
                    Ok(mut active_pubkey) => {
                        *active_pubkey = new_pubkey;
                    }
                    Err(e) => {
                        crate::util::request_fatal_restart(format!(
                            "Mostrix encountered an internal error (poisoned Mostro pubkey lock: {e}). Please restart the app."
                        ));
                        return false;
                    }
                }
                app.pending_fetch_scheduler_reload = true;
                spawn_refresh_mostro_info_task(
                    ctx.client.clone(),
                    new_pubkey,
                    ctx.mostro_info_tx.clone(),
                    true,
                );
                app.mode = UiMode::OperationResult(OperationResult::Info(
                    "Fetching Mostro instance info...".to_string(),
                ));
            }
        }
        UiMode::AddRelay(key_state) => {
            // Validate relay URL format before proceeding to confirmation
            match validate_relay(&key_state.key_input) {
                Ok(_) => {
                    app.mode =
                        handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                            UiMode::ConfirmRelay(input, true)
                        });
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::OperationResult(OperationResult::Error(e));
                }
            }
        }
        UiMode::ConfirmRelay(relay_string, selected_button) => {
            app.mode = handle_confirmation_enter(
                selected_button,
                &relay_string,
                default_mode,
                save_relay_to_settings,
                |input| UiMode::AddRelay(create_key_input_state(input)),
            );

            // If YES was selected, also add the relay to the running Nostr client
            if selected_button {
                let relay_to_add = relay_string.clone();
                let client_clone = ctx.client.clone();
                tokio::spawn(async move {
                    if let Err(e) = client_clone.add_relay(relay_to_add.trim()).await {
                        log::error!("Failed to add relay at runtime: {}", e);
                    }
                });
            }
        }
        UiMode::AddCurrency(key_state) => {
            // Validate currency code before proceeding to confirmation
            match validate_currency(&key_state.key_input) {
                Ok(_) => {
                    app.mode =
                        handle_input_to_confirmation(&key_state.key_input, default_mode, |input| {
                            UiMode::ConfirmCurrency(input, true)
                        });
                }
                Err(e) => {
                    // Show error popup
                    app.mode = UiMode::OperationResult(OperationResult::Error(e));
                }
            }
        }
        UiMode::ConfirmCurrency(currency_string, selected_button) => {
            if selected_button {
                // Persist to settings and update in-memory cache.
                save_currency_to_settings(&currency_string);
                app.pending_fetch_scheduler_reload = true;
                let upper = currency_string.trim().to_uppercase();
                if !upper.is_empty() && !app.currencies_filter.contains(&upper) {
                    app.currencies_filter.push(upper);
                }
            }
            app.mode = default_mode;
        }
        UiMode::ConfirmClearCurrencies(selected_button) => {
            if selected_button {
                // YES selected - clear currency filters (both on disk and in cache)
                app.pending_fetch_scheduler_reload = true;
                clear_currency_filters();
                app.currencies_filter.clear();
            }
            app.mode = default_mode;
        }
        UiMode::ConfirmExit(selected_button) => {
            if selected_button {
                // YES selected - exit the application
                // Return false to break the main loop
                return false;
            } else {
                // NO selected - cancel and return to normal mode
                app.mode = default_mode;
                return true;
            }
        }
        _ => {
            // This should not happen, but handle gracefully
            app.mode = default_mode;
        }
    }
    true // Continue the loop by default
}

fn handle_enter_normal_mode(app: &mut AppState, ctx: &super::EnterKeyContext<'_>) {
    // Refresh Mostro instance info when Enter is pressed in the Mostro Info tab (spawn, no UI freeze)
    if matches!(
        app.active_tab,
        Tab::User(UserTab::MostroInfo) | Tab::Admin(AdminTab::MostroInfo)
    ) {
        spawn_refresh_mostro_info_from_settings_task(
            ctx.client.clone(),
            ctx.mostro_info_tx.clone(),
        );
        app.mode = UiMode::OperationResult(OperationResult::Info(
            "Fetching Mostro instance info...".to_string(),
        ));
    } else if let Tab::User(UserTab::MyTrades) = app.active_tab {
        handle_enter_user_order_chat(app, ctx);
    } else if let Tab::User(UserTab::Orders) = app.active_tab {
        // Show take order popup when Enter is pressed in Orders tab (user mode only)
        let orders_lock = match ctx.orders.lock() {
            Ok(g) => g,
            Err(e) => {
                crate::util::request_fatal_restart(format!(
                    "Mostrix encountered an internal error (poisoned orders lock: {e}). Please restart the app."
                ));
                return;
            }
        };
        if let Some(order) = orders_lock.get(app.selected_order_idx) {
            let is_range_order = order.min_amount.is_some() || order.max_amount.is_some();
            let take_state = TakeOrderState {
                order: order.clone(),
                amount_input: String::new(),
                is_range_order,
                validation_error: None,
                selected_button: true, // Default to YES
            };
            app.mode = UiMode::UserMode(UserMode::TakingOrder(take_state));
        }
    } else if let Tab::Admin(AdminTab::DisputesPending) = app.active_tab {
        // Show take dispute confirmation popup when Enter is pressed in Disputes tab (admin mode only)
        let disputes_lock = match ctx.disputes.lock() {
            Ok(g) => g,
            Err(e) => {
                crate::util::request_fatal_restart(format!(
                    "Mostrix encountered an internal error (poisoned disputes lock: {e}). Please restart the app."
                ));
                return;
            }
        };
        // Filter to only get "initiated" disputes
        let initiated_disputes: Vec<(usize, &Dispute)> = disputes_lock
            .iter()
            .enumerate()
            .filter(|(_, dispute)| {
                DisputeStatus::from_str(dispute.status.as_str())
                    .map(|s| s == DisputeStatus::Initiated)
                    .unwrap_or(false)
            })
            .collect();

        if let Some((_original_idx, dispute)) = initiated_disputes.get(app.selected_dispute_idx) {
            // Only allow taking disputes with "Initiated" status
            // (We already filtered, so this should always be true)
            app.mode = UiMode::AdminMode(AdminMode::ConfirmTakeDispute(dispute.id, true));
            // Default to YES
        }
    } else if let Tab::User(UserTab::Messages) = app.active_tab {
        let mut messages_lock = match app.messages.lock() {
            Ok(g) => g,
            Err(e) => {
                crate::util::request_fatal_restart(format!(
                    "Mostrix encountered an internal error (poisoned messages lock: {e}). Please restart the app."
                ));
                return;
            }
        };
        strip_new_order_messages_and_clamp_selected(
            &mut messages_lock,
            &mut app.selected_message_idx,
        );
        if let Some(msg) = messages_lock.get(app.selected_message_idx) {
            let inner_message_kind = msg.message.get_inner_message_kind();
            let action = inner_message_kind.action.clone();
            if let Some(invoice_popup_action) = invoice_popup_action_for_message_action(&action) {
                if invoice_popup_allowed_for_order_status(&invoice_popup_action, msg.order_status) {
                    // Show invoice/payment popup only when the phase still requires it.
                    let notification = order_message_to_notification(msg);
                    let invoice_state = InvoiceInputState {
                        invoice_input: String::new(),
                        focused: matches!(invoice_popup_action, Action::AddInvoice),
                        just_pasted: false,
                        copied_to_clipboard: false,
                        scroll_y: 0,
                        action_selection: InvoiceNotificationActionSelection::Primary,
                    };

                    app.mode = UiMode::NewMessageNotification(
                        notification,
                        invoice_popup_action,
                        invoice_state,
                    );
                } else if matches!(
                    action,
                    Action::AddInvoice
                        | Action::PayInvoice
                        | Action::WaitingBuyerInvoice
                        | Action::WaitingSellerToPay
                ) {
                    // Stale replayed invoice/payment DMs after the trade moved on.
                    let info = if matches!(action, Action::PayInvoice)
                        && matches!(
                            msg.order_status,
                            Some(mostro_core::order::Status::WaitingBuyerInvoice)
                        ) {
                        "Waiting for the buyer to add their invoice. Hold invoice is already paid."
                            .to_string()
                    } else {
                        "Trade already advanced; invoice action no longer required.".to_string()
                    };
                    app.mode = UiMode::OperationResult(OperationResult::Info(info));
                }
            } else if matches!(
                action,
                Action::HoldInvoicePaymentAccepted
                    | Action::FiatSentOk
                    | Action::CooperativeCancelInitiatedByPeer
                    | Action::BuyerTookOrder
            ) {
                // Only these message types are actionable (send a follow-up message to Mostro).
                let notification = order_message_to_notification(msg);
                let button_selection = if matches!(action, Action::HoldInvoicePaymentAccepted) {
                    ViewingMessageButtonSelection::Three(ThreeState::Yes)
                } else if matches!(action, Action::CooperativeCancelInitiatedByPeer) {
                    // Safer default: Enter should not accept cooperative cancel implicitly.
                    ViewingMessageButtonSelection::Two {
                        yes_selected: false,
                    }
                } else {
                    ViewingMessageButtonSelection::Two { yes_selected: true }
                };
                let view_state = MessageViewState {
                    message_content: notification.message_preview,
                    order_id: notification.order_id,
                    action: notification.action,
                    button_selection,
                };
                app.mode = UiMode::ViewingMessage(view_state);
            } else if matches!(action, Action::Rate) {
                if let Some(oid) = msg.order_id {
                    app.mode = UiMode::RatingOrder(RatingOrderState {
                        order_id: oid,
                        selected_rating: 3,
                    });
                } else {
                    app.mode = UiMode::OperationResult(OperationResult::Error(
                        "No order ID for rating".to_string(),
                    ));
                }
            } else {
                // Non-actionable messages: show info popup (no "send" semantics).
                let notification = order_message_to_notification(msg);
                app.mode =
                    UiMode::OperationResult(OperationResult::Info(notification.message_preview));
            }
        }
    } else if let Tab::Admin(AdminTab::Observer) = app.active_tab {
        // Validate and trigger async fetch for observer chat via shared key
        let key_str = app.observer_shared_key_input.trim().to_string();
        if key_str.is_empty() {
            let msg = "Shared key is required".to_string();
            app.observer_error = Some(msg.clone());
            app.mode = UiMode::OperationResult(OperationResult::Error(msg));
            return;
        }

        if crate::util::chat_utils::keys_from_shared_hex(&key_str).is_none() {
            let msg = "Shared key must be a valid 64-char hex secret (32 bytes)".to_string();
            app.observer_error = Some(msg.clone());
            app.mode = UiMode::OperationResult(OperationResult::Error(msg));
            return;
        }

        // Clear previous results and set loading state
        for msg in &mut app.observer_messages {
            zeroize::Zeroize::zeroize(&mut msg.content);
        }
        app.observer_messages.clear();
        app.observer_error = None;
        app.observer_loading = true;

        // Spawn async fetch via the order_result channel
        let client = ctx.client.clone();
        let admin_pubkey = ctx.admin_chat_keys.map(|k| k.public_key());
        let tx = ctx.order_result_tx.clone();

        tokio::spawn(async move {
            match crate::util::chat_utils::fetch_observer_chat(
                &client,
                &key_str,
                admin_pubkey.as_ref(),
            )
            .await
            {
                Ok(messages) => {
                    let _ = tx.send(OperationResult::ObserverChatLoaded(messages));
                }
                Err(e) => {
                    let _ = tx.send(OperationResult::ObserverChatError(e.to_string()));
                }
            }
        });
    } else if matches!(
        app.active_tab,
        Tab::Admin(AdminTab::Exit) | Tab::User(UserTab::Exit)
    ) {
        // Show exit confirmation popup
        app.mode = UiMode::ConfirmExit(true); // true = Yes button selected by default
    } else if matches!(
        app.active_tab,
        Tab::Admin(AdminTab::Settings) | Tab::User(UserTab::Settings)
    ) {
        // Open key input popup based on selected option
        let key_state = create_key_input_state("");

        match app.selected_settings_option {
            0 => {
                handle_mode_switch(app);
            }
            1 => {
                // Add Mostro Pubkey (Common for both roles)
                app.mode = UiMode::AddMostroPubkey(key_state);
            }
            2 => {
                // Add Relay (Common for both roles)
                app.mode = UiMode::AddRelay(key_state);
            }
            3 => {
                // Add Currency Filter (Common for both roles)
                app.mode = UiMode::AddCurrency(key_state);
            }
            4 => {
                // Clear Currency Filters (Common for both roles) - show confirmation
                app.mode = UiMode::ConfirmClearCurrencies(true);
            }
            5 if app.user_role == UserRole::User => {
                // View current seed words (User)
                spawn_load_seed_words_task(ctx.pool.clone(), ctx.seed_words_tx.clone());
                app.mode = UiMode::OperationResult(OperationResult::Info(
                    "Loading seed words...".to_string(),
                ));
            }
            6 if app.user_role == UserRole::User => {
                // Generate new keys for current role (user)
                app.mode = UiMode::ConfirmGenerateNewKeys(true);
            }
            5 if app.user_role == UserRole::Admin => {
                // View current seed words (Admin mode still uses user identity seed)
                spawn_load_seed_words_task(ctx.pool.clone(), ctx.seed_words_tx.clone());
                app.mode = UiMode::OperationResult(OperationResult::Info(
                    "Loading seed words...".to_string(),
                ));
            }
            6 if app.user_role == UserRole::Admin => {
                // Add Solver (Admin only)
                app.mode = UiMode::AdminMode(AdminMode::AddSolver(key_state));
            }
            7 if app.user_role == UserRole::Admin => {
                // Setup Admin Key (Admin only)
                app.mode = UiMode::AdminMode(AdminMode::SetupAdminKey(key_state));
            }
            8 if app.user_role == UserRole::Admin => {
                // Generate new keys for current role (admin)
                app.mode = UiMode::ConfirmGenerateNewKeys(true);
            }
            _ => {}
        }
    }
}
