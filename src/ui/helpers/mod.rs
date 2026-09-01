mod ascii_art;
mod attachments;
mod chat_render;
mod chat_storage;
mod chat_visibility;
mod dispute_selection;
mod formatting;
mod layout;
mod order_chat_projection;
mod order_selection;
mod startup;

pub use ascii_art::{render_centered_lines, MAILBOX_EMPTY_ART};

pub(crate) use attachments::try_parse_attachment_message;
pub use attachments::{
    build_attachment_toast, build_file_encrypted_json, build_image_encrypted_json,
    expire_attachment_toast, OutboundAttachmentPayload, PreparedOrderChatAttachment,
};
pub(crate) use chat_render::wrap_text_to_lines;
pub use chat_render::{
    build_chat_list_items, build_chat_scrollview_content, build_observer_scrollview_content,
    ChatScrollViewContent,
};
pub use chat_storage::{
    dispute_chat_inner_id_known, dispute_chat_since_from_file, load_chat_from_file,
    load_dispute_chat_inner_ids, load_order_chat_from_file, load_order_chat_inner_ids,
    order_chat_inner_id_known, order_chat_since_from_file, remember_dispute_chat_inner_id,
    remember_order_chat_inner_id, rewrite_dispute_chat_messages, rewrite_order_chat_messages,
    save_chat_message, save_order_chat_message,
};
pub use chat_storage::{
    load_user_dispute_chat_from_file, load_user_dispute_chat_inner_ids,
    remember_user_dispute_chat_inner_id, save_user_dispute_chat_message,
    user_dispute_chat_inner_id_known, user_dispute_chat_since_from_file,
};
pub use chat_visibility::{
    count_order_attachments, count_visible_attachments, get_order_attachment_messages,
    get_selected_chat_message, get_visible_attachment_messages, message_visible_for_party,
};
pub use dispute_selection::{
    clamp_pending_dispute_selection, get_filtered_disputes, get_initiated_disputes,
    move_dispute_selection, move_pending_dispute_selection, retain_closed_displayed_dispute,
    selected_display_idx, selected_filtered_dispute, selected_pending_display_idx,
    selected_pending_dispute,
};
pub use formatting::{
    dispute_status_color, format_local_timestamp, format_order_id, format_premium,
    format_user_rating, is_dispute_finalized, relative_time_compact, short_order_id,
};
pub use layout::{
    create_centered_popup, render_help_text, render_table_list_scrollbar, render_yes_no_buttons,
    render_yes_no_cancel_buttons,
};
pub use order_chat_projection::{
    active_order_chat_list_len, active_order_chat_list_snapshot, build_active_order_chat_list,
    order_chat_list_item_from_db_order, OrderChatListItem,
};
pub use order_selection::{
    get_filtered_book_orders, move_book_order_selection, order_passes_currency_filter,
    selected_book_display_idx, selected_filtered_book_order,
};
pub use startup::{
    admin_chat_keys_clone_for_role, apply_admin_chat_updates, apply_user_order_chat_updates,
    hydrate_app_admin_keys_from_privkey, load_admin_disputes_at_startup,
    load_user_order_chats_at_startup, prepare_post_restore_trade_dm_replay,
    recover_admin_chat_from_files, refresh_my_trades_maker_book_cache,
    spawn_post_restore_trade_dm_replay, sync_user_order_history_messages_from_db,
    track_startup_chats,
};
