use crate::ui::{AppState, ChatParty, ChatSender, DisputeChatMessage};

/// Returns true if this message should be shown in the given party's chat view.
pub fn message_visible_for_party(msg: &DisputeChatMessage, active_chat_party: ChatParty) -> bool {
    match msg.sender {
        ChatSender::Admin => msg.target_party.is_none_or(|p| p == active_chat_party),
        ChatSender::Buyer => active_chat_party == ChatParty::Buyer,
        ChatSender::Seller => active_chat_party == ChatParty::Seller,
    }
}

/// Returns the number of messages in the given list that are visible for the given party and have an attachment.
pub fn count_visible_attachments(
    messages: &[DisputeChatMessage],
    active_chat_party: ChatParty,
) -> usize {
    messages
        .iter()
        .filter(|msg| message_visible_for_party(msg, active_chat_party) && msg.attachment.is_some())
        .count()
}

/// Returns visible messages that have an attachment, for the current dispute and party.
/// Used by the Save Attachment popup (Ctrl+S) to list saveable files.
pub fn get_visible_attachment_messages<'a>(
    app: &'a AppState,
    dispute_id_key: &str,
) -> Vec<&'a DisputeChatMessage> {
    let messages = match app.admin_dispute_chats.get(dispute_id_key) {
        Some(m) => m,
        None => return vec![],
    };
    messages
        .iter()
        .filter(|msg| {
            message_visible_for_party(msg, app.active_chat_party) && msg.attachment.is_some()
        })
        .collect()
}

/// Returns the currently selected chat message (by index) for the given dispute, or None.
pub fn get_selected_chat_message<'a>(
    app: &'a AppState,
    dispute_id_key: &str,
) -> Option<&'a DisputeChatMessage> {
    let messages = app.admin_dispute_chats.get(dispute_id_key)?;
    let selected_idx = app.admin_chat_selected_message_idx?;
    let visible: Vec<&DisputeChatMessage> = messages
        .iter()
        .filter(|msg| message_visible_for_party(msg, app.active_chat_party))
        .collect();
    visible.get(selected_idx).copied()
}
