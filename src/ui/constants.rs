//! UI constants: help text, footer hints, and other shared strings.
//! Centralizes copy to avoid duplication and keep the UI consistent.

// --- Help popup (Ctrl+H) ---

pub const HELP_CLOSE_HINT: &str = "Esc, Enter or Ctrl+H to close";

/// Footer hint shown in help and disputes footer
pub const HELP_KEY: &str = "Ctrl+H: Help";

// Filter toggle (Disputes in Progress)
pub const FILTER_VIEW_FINALIZED: &str = "Shift+C: View Finalized";
pub const FILTER_VIEW_IN_PROGRESS: &str = "Shift+C: View In Progress";

// Help popup titles (per tab)
pub const HELP_TITLE_DISPUTES_IN_PROGRESS: &str = "Disputes in Progress — Shortcuts";
pub const HELP_TITLE_DISPUTES_PENDING: &str = "Disputes Pending — Shortcuts";
pub const HELP_TITLE_OBSERVER: &str = "Observer — Shortcuts";
pub const HELP_TITLE_SETTINGS_ADMIN: &str = "Settings (Admin) — Shortcuts";
pub const HELP_TITLE_SETTINGS_USER: &str = "Settings (User) — Shortcuts";
pub const HELP_TITLE_EXIT: &str = "Exit — Shortcuts";
pub const HELP_TITLE_ORDERS: &str = "Orders — Shortcuts";
pub const HELP_TITLE_MY_TRADES: &str = "My Trades — Shortcuts";
pub const HELP_TITLE_MESSAGES: &str = "Messages — Shortcuts";
pub const HELP_TITLE_CREATE_NEW_ORDER: &str = "Create New Order — Shortcuts";

// Help popup lines (Disputes in Progress)
pub const HELP_DIP_TAB_PARTY: &str = "Tab: Switch Party (Buyer/Seller)";
pub const HELP_DIP_SELECT_DISPUTE: &str = "↑↓: Select dispute (sidebar)";
pub const HELP_DIP_SCROLL_CHAT: &str = "PgUp/PgDn: Scroll chat";
pub const HELP_DIP_END_BOTTOM: &str = "End: Jump to bottom of chat";
pub const HELP_DIP_SHIFT_F_RESOLVE: &str = "Shift+F: Resolve (finalize) dispute";
pub const HELP_DIP_SHIFT_I_INPUT: &str = "Shift+I: Enable/disable message input";
pub const HELP_DIP_ENTER_SEND: &str = "Enter: Send message (when input enabled)";
pub const HELP_DIP_CTRL_S_ATTACH: &str = "Ctrl+S: Save attachment (choose from list)";

// Help popup lines (Disputes Pending)
pub const HELP_DP_ENTER_TAKE: &str = "Enter: Take selected dispute";
pub const HELP_DP_SELECT_DISPUTE: &str = "↑↓: Select dispute";

// Help popup lines (Observer)
pub const HELP_OBS_TAB_FIELD: &str = "Tab / Shift+Tab: Switch field (path / key)";
pub const HELP_OBS_ENTER_LOAD: &str = "Enter: Load file and decrypt";
pub const HELP_OBS_ESC_CLEAR_ERR: &str = "Esc: Clear error";
pub const HELP_OBS_CTRL_C_CLEAR: &str = "Ctrl+C: Clear all inputs and preview";

// Help popup lines (Settings)
pub const HELP_SETTINGS_M_MODE: &str = "M: Switch User/Admin mode";
pub const HELP_SETTINGS_SELECT_OPTION: &str = "↑↓: Select option";
pub const HELP_SETTINGS_ENTER_OPEN: &str = "Enter: Open selected option";

// Help popup lines (Exit)
pub const HELP_EXIT_ENTER_CONFIRM: &str = "Enter: Confirm exit (then Yes/No)";

// Help popup lines (Orders)
pub const HELP_ORDERS_ENTER_TAKE: &str = "Enter: Take selected order";
pub const HELP_ORDERS_SELECT: &str = "↑↓: Select order";

// Help popup lines (My Trades)
pub const HELP_MY_TRADES_NAV: &str = "↑↓: Navigate (when available)";

// Help popup lines (Messages)
pub const HELP_MSG_ENTER_OPEN: &str = "Enter: Open selected message";
pub const HELP_MSG_SELECT: &str = "↑↓: Select message";

// Help popup lines (Create New Order)
pub const HELP_CNO_CHANGE_FIELD: &str = "↑↓: Change field";
pub const HELP_CNO_TAB_NEXT: &str = "Tab: Next field";
pub const HELP_CNO_ENTER_CONFIRM: &str = "Enter: Confirm order (from form)";

// --- Footer (Disputes in Progress) ---

/// Hint shown in the Save Attachment popup footer (↑↓ Select, Enter Save, Esc Cancel).
pub const SAVE_ATTACHMENT_POPUP_HINT: &str = "↑↓ Select, Enter Save, Esc Cancel";

pub const FOOTER_CTRL_S_SAVE_FILE: &str = " | Ctrl+S: Save file";
pub const FOOTER_UP_DOWN_SELECT: &str = "↑↓: Select";
pub const FOOTER_UP_DOWN_SELECT_DISPUTE: &str = "↑↓: Select Dispute";
pub const FOOTER_TAB_PARTY: &str = "Tab: Party";
pub const FOOTER_TAB_SWITCH_PARTY: &str = "Tab: Switch Party";
pub const FOOTER_ENTER_SEND: &str = "Enter: Send";
pub const FOOTER_SHIFT_F_RESOLVE: &str = "Shift+F: Resolve";
pub const FOOTER_SHIFT_I_DISABLE: &str = "Shift+I: Disable";
pub const FOOTER_SHIFT_I_ENABLE: &str = "Shift+I: Enable";
pub const FOOTER_PGUP_PGDN_SCROLL: &str = "PgUp/PgDn: Scroll";
pub const FOOTER_END_BOTTOM: &str = "End: Bottom";
pub const FOOTER_NAV_CHAT: &str = "↑↓: Navigate Chat";
pub const FOOTER_PGUP_PGDN_SCROLL_CHAT: &str = "PgUp/PgDn: Scroll Chat";
