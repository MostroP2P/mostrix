mod ascii_art;
mod attachments;
mod chat_render;
mod chat_storage;
mod chat_visibility;
mod formatting;
mod layout;
mod order_chat_projection;
mod startup;

pub use ascii_art::render_centered_lines;

pub(crate) use attachments::try_parse_attachment_message;
pub use attachments::{
    build_attachment_toast, build_file_encrypted_json, build_image_encrypted_json,
    expire_attachment_toast, OutboundAttachmentPayload, PreparedOrderChatAttachment,
};
pub use chat_render::{
    build_chat_list_items, build_chat_scrollview_content, build_observer_scrollview_content,
    ChatScrollViewContent,
};
pub use chat_storage::{
    load_chat_from_file, load_order_chat_from_file, save_chat_message, save_order_chat_message,
};
pub use chat_visibility::{
    count_order_attachments, count_visible_attachments, get_order_attachment_messages,
    get_selected_chat_message, get_visible_attachment_messages, message_visible_for_party,
};
pub use formatting::{format_order_id, format_user_rating, is_dispute_finalized};
pub use layout::{
    create_centered_popup, render_help_text, render_yes_no_buttons, render_yes_no_cancel_buttons,
};
pub use order_chat_projection::{
    active_order_chat_list_len, active_order_chat_list_snapshot, build_active_order_chat_list,
    order_chat_list_item_from_db_order, OrderChatListItem,
};
pub use startup::{
    admin_chat_keys_clone_for_role, apply_admin_chat_updates, apply_user_order_chat_updates,
    hydrate_app_admin_keys_from_privkey, load_admin_disputes_at_startup,
    load_user_order_chats_at_startup, recover_admin_chat_from_files,
    refresh_my_trades_maker_book_cache, sync_user_order_history_messages_from_db,
};
