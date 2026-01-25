use crate::ui::{AppState, ChatParty, ChatSender};

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
            current
                .saturating_sub(10)
                .min(visible_count.saturating_sub(1))
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
