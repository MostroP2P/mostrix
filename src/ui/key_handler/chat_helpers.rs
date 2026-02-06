use crate::ui::{AdminMode, AppState, ChatParty, ChatSender, OrderResult, UiMode};
use tokio::sync::mpsc::UnboundedSender;

/// Filter messages by active chat party and return the count of visible messages.
///
/// This helper eliminates code duplication across multiple key handlers.
/// Admin messages are only shown in the chat party they were sent to.
pub fn get_visible_message_count(
    messages: &[crate::ui::DisputeChatMessage],
    active_chat_party: ChatParty,
) -> usize {
    messages
        .iter()
        .filter(|msg| match msg.sender {
            ChatSender::Admin => {
                // Admin messages should only show in the chat party they were sent to
                msg.target_party == Some(active_chat_party)
            }
            ChatSender::Buyer => active_chat_party == ChatParty::Buyer,
            ChatSender::Seller => active_chat_party == ChatParty::Seller,
        })
        .count()
}

/// Get the visible message count for a dispute's chat.
///
/// Returns 0 if the dispute or its messages are not found.
pub fn get_dispute_visible_message_count(app: &AppState, dispute_id_key: &str) -> usize {
    app.admin_dispute_chats
        .get(dispute_id_key)
        .map(|messages| get_visible_message_count(messages, app.active_chat_party))
        .unwrap_or(0)
}

/// Count visible messages for a dispute and auto-scroll to the bottom (e.g. after sending a new message).
pub fn message_counter(app: &mut AppState, dispute_id_key: &str) {
    let visible_count = get_dispute_visible_message_count(app, dispute_id_key);
    if visible_count > 0 {
        app.admin_chat_list_state
            .select(Some(visible_count.saturating_sub(1)));
    }
}

/// Navigate chat messages (Up/Down).
///
/// Returns true if navigation occurred, false otherwise.
pub fn navigate_chat_messages(
    app: &mut AppState,
    dispute_id_key: &str,
    direction: crossterm::event::KeyCode,
) -> bool {
    let visible_count = get_dispute_visible_message_count(app, dispute_id_key);
    if visible_count == 0 {
        return false;
    }

    let current = app
        .admin_chat_list_state
        .selected()
        .unwrap_or(visible_count.saturating_sub(1));

    let new_selection = match direction {
        crossterm::event::KeyCode::Up => {
            // Move up (older messages)
            if current > 0 {
                current - 1
            } else {
                0
            }
        }
        crossterm::event::KeyCode::Down => {
            // Move down (newer messages)
            (current + 1).min(visible_count.saturating_sub(1))
        }
        _ => return false,
    };

    app.admin_chat_list_state.select(Some(new_selection));
    true
}

/// Scroll chat messages (PageUp/PageDown).
///
/// Returns true if scrolling occurred, false otherwise.
pub fn scroll_chat_messages(
    app: &mut AppState,
    dispute_id_key: &str,
    direction: crossterm::event::KeyCode,
) -> bool {
    let visible_count = get_dispute_visible_message_count(app, dispute_id_key);
    if visible_count == 0 {
        return false;
    }

    let current = app
        .admin_chat_list_state
        .selected()
        .unwrap_or(visible_count.saturating_sub(1));

    let new_selection = match direction {
        crossterm::event::KeyCode::PageUp => {
            // Scroll up (show older messages) - move selection up by ~10 items
            current.saturating_sub(10)
        }
        crossterm::event::KeyCode::PageDown => {
            // Scroll down (show newer messages) - move selection down by ~10 items
            (current + 10).min(visible_count.saturating_sub(1))
        }
        _ => return false,
    };

    app.admin_chat_list_state.select(Some(new_selection));
    true
}

/// Jump to the bottom of the chat (latest messages).
///
/// Returns true if the jump occurred, false otherwise.
pub fn jump_to_chat_bottom(app: &mut AppState, dispute_id_key: &str) -> bool {
    let visible_count = get_dispute_visible_message_count(app, dispute_id_key);
    if visible_count == 0 {
        return false;
    }

    // Jump to last message (bottom)
    app.admin_chat_list_state
        .select(Some(visible_count.saturating_sub(1)));
    true
}

/// Finalization popup button index: 0 = Pay Buyer, 1 = Refund Seller, 2 = Exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalizeDisputePopupButton {
    PayBuyer,
    RefundSeller,
    Exit,
}

impl FinalizeDisputePopupButton {
    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(Self::PayBuyer),
            1 => Some(Self::RefundSeller),
            2 => Some(Self::Exit),
            _ => None,
        }
    }
}

/// Handle Enter in the finalization popup: transition to confirmation or back to managing dispute.
pub fn handle_enter_finalize_popup(
    app: &mut AppState,
    button: FinalizeDisputePopupButton,
    dispute_id: uuid::Uuid,
    dispute_is_finalized: bool,
    order_result_tx: &UnboundedSender<OrderResult>,
) -> bool {
    match button {
        FinalizeDisputePopupButton::PayBuyer => {
            if dispute_is_finalized {
                let _ = order_result_tx.send(OrderResult::Error(
                    "Cannot finalize: dispute is already finalized".to_string(),
                ));
                app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
            } else {
                app.mode = UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute {
                    dispute_id,
                    // is_settle: true = Pay Buyer
                    is_settle: true,
                    // selected_button: true = Yes
                    selected_button: true,
                });
            }
            true
        }
        FinalizeDisputePopupButton::RefundSeller => {
            if dispute_is_finalized {
                let _ = order_result_tx.send(OrderResult::Error(
                    "Cannot finalize: dispute is already finalized".to_string(),
                ));
                app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
            } else {
                app.mode = UiMode::AdminMode(AdminMode::ConfirmFinalizeDispute {
                    dispute_id,
                    // is_settle: false = Refund Seller
                    is_settle: false,
                    // selected_button: true = Yes
                    selected_button: true,
                });
            }
            true
        }
        FinalizeDisputePopupButton::Exit => {
            app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
            true
        }
    }
}
