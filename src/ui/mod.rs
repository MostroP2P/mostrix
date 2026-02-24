use ratatui::style::Color;

pub const PRIMARY_COLOR: Color = Color::Rgb(177, 204, 51); // #b1cc33
pub const BACKGROUND_COLOR: Color = Color::Rgb(29, 33, 44); // #1D212C

pub mod admin_key_confirm;
pub mod admin_state;
pub(crate) mod app_state;
pub(crate) mod chat;
pub mod dispute_finalization_confirm;
pub mod dispute_finalization_popup;
pub mod draw;
pub mod exit_confirm;
pub mod helpers;
pub mod key_handler;
pub mod key_input_popup;
pub mod message_notification;
pub(crate) mod navigation;
pub mod operation_result;
pub mod order_confirm;
pub mod order_form;
pub mod order_take;
pub(crate) mod orders;
pub mod state;
pub mod status;
pub mod tabs;
pub mod user_state;
pub mod waiting;

pub use admin_state::AdminMode;
pub use draw::ui_draw;
pub use state::{
    apply_kind_color, order_message_to_notification, AdminChatLastSeen, AdminChatUpdate, AdminTab,
    AppState, ChatAttachment, ChatAttachmentType, ChatParty, ChatSender, DisputeChatMessage,
    DisputeFilter, FormState, InvoiceInputState, KeyInputState, MessageNotification,
    MessageViewState, OperationResult, OrderMessage, Tab, TakeOrderState, UiMode, UserRole,
    UserTab,
};
pub use user_state::UserMode;
