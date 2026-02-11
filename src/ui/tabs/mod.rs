pub mod disputes_in_progress_tab;
pub mod disputes_tab;
pub mod orders_tab;
pub mod settings_tab;
pub mod tab_bar;
pub mod tab_content;

// Re-export for convenience
pub use settings_tab::{ADMIN_SETTINGS_OPTIONS_COUNT, USER_SETTINGS_OPTIONS_COUNT};
pub use tab_bar::render_tabs;
