// Direct message utilities for Nostr
// Re-export from dm_manager module for backward compatibility
pub use crate::util::dm_manager::{
    handle_message_notification, handle_order_result, listen_for_order_messages, parse_dm_events,
    send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT,
};
