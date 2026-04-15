mod attachments;
mod chat_render;
mod chat_storage;
mod chat_visibility;
mod formatting;
mod layout;
mod startup;

pub use attachments::expire_attachment_toast;
pub(crate) use attachments::try_parse_attachment_message;
pub use chat_render::{
    build_chat_list_items, build_chat_scrollview_content, build_observer_scrollview_content,
    ChatScrollViewContent,
};
pub use chat_storage::{
    load_chat_from_file, load_order_chat_from_file, save_chat_message, save_order_chat_message,
};
pub use chat_visibility::{
    count_visible_attachments, get_selected_chat_message, get_visible_attachment_messages,
    message_visible_for_party,
};
pub use formatting::{format_order_id, format_user_rating, is_dispute_finalized};
pub use layout::{create_centered_popup, render_help_text, render_yes_no_buttons};
pub use startup::{
    admin_chat_keys_clone_for_role, apply_admin_chat_updates, apply_user_order_chat_updates,
    hydrate_app_admin_keys_from_privkey, load_admin_disputes_at_startup,
    load_user_order_chats_at_startup, recover_admin_chat_from_files,
};
