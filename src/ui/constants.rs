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
pub const HELP_TITLE_MY_TRADES: &str = "Order Chat — Shortcuts";
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
pub const HELP_OBS_ENTER_LOAD: &str = "Enter: Load chat for shared key";
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
pub const HELP_OBS_PASTE_SHARED_KEY: &str = "Ctrl+Shift+V: Paste shared key";
#[cfg(target_os = "windows")]
pub const HELP_OBS_PASTE_SHARED_KEY: &str = "Ctrl+V: Paste shared key";
#[cfg(target_os = "macos")]
pub const HELP_OBS_PASTE_SHARED_KEY: &str = "Cmd+V: Paste shared key";
#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "windows",
    target_os = "macos"
)))]
pub const HELP_OBS_PASTE_SHARED_KEY: &str = "Ctrl+V: Paste shared key";
pub const HELP_OBS_SCROLL_LINE: &str = "↑↓: Scroll messages";
pub const HELP_OBS_SCROLL_PAGE: &str = "PgUp/PgDn: Scroll page";
pub const HELP_OBS_ESC_CLEAR_ERR: &str = "Esc: Clear error";
pub const HELP_OBS_CTRL_C_CLEAR: &str = "Ctrl+C: Clear all";
pub const HELP_OBS_CTRL_S_ATTACH: &str = "Ctrl+S: Save attachment";

// Help popup lines (Settings)
pub const HELP_SETTINGS_SWITCH_FROM_MENU: &str =
    "Enter on \"Switch Mode\": Toggle User/Admin (saved to settings.toml)";
pub const HELP_SETTINGS_SHIFT_H_FULL: &str = "Shift+H: Explain every settings option";
pub const HELP_SETTINGS_SELECT_OPTION: &str = "↑↓: Select option";
pub const HELP_SETTINGS_ENTER_OPEN: &str = "Enter: Open selected option";

/// Footer for the Settings instructions overlay (Shift+H).
pub const SETTINGS_INSTRUCTIONS_CLOSE_HINT: &str = "Esc, Enter, Shift+H or Ctrl+H to close";

// Help popup lines (Exit)
pub const HELP_EXIT_ENTER_CONFIRM: &str = "Enter: Confirm exit (then Yes/No)";

// Help popup lines (Orders)
pub const HELP_ORDERS_ENTER_TAKE: &str = "Enter: Take selected order";
pub const HELP_ORDERS_SELECT: &str = "↑↓: Select order";

// Help popup lines (My Trades)
pub const HELP_MY_TRADES_NAV: &str = "↑↓: Select order";
pub const HELP_MY_TRADES_ENTER_SEND: &str = "Enter: Send message (when input enabled)";
pub const HELP_MY_TRADES_SHIFT_I: &str = "Shift+I: Enable/disable message input";
pub const HELP_MY_TRADES_SHIFT_C_CANCEL: &str = "Shift+C: Cancel order (cooperative cancel)";
pub const HELP_MY_TRADES_SHIFT_F_FIAT_SENT: &str = "Shift+F: Mark fiat as sent (FiatSent message)";
pub const HELP_MY_TRADES_SHIFT_R_RELEASE: &str = "Shift+R: Release sats (Release message)";
pub const HELP_MY_TRADES_SHIFT_V_RATE: &str = "Shift+V: Rate counterparty (open rating popup)";
pub const HELP_MY_TRADES_SHIFT_H_HELP: &str = "Shift+H: Show shortcuts help";

// Confirmation messages for My Trades actions
pub const HELP_MY_TRADES_CANCEL_MSG: &str =
    "Cancel this order? This sends a cooperative Cancel request.";
pub const HELP_MY_TRADES_FIAT_SENT_MSG: &str =
    "Confirm fiat sent for this order? This sends a FiatSent message.";
pub const HELP_MY_TRADES_RELEASE_MSG: &str =
    "Release sats for this order? This sends a Release message.";

/// Multi-line body for Messages-tab confirmation when Mostro reports hold invoice paid (`HoldInvoicePaymentAccepted`).
/// Last line matches [`HELP_MY_TRADES_CANCEL_MSG`] (cooperative cancel).
pub const VIEW_MESSAGE_HOLD_INVOICE_PREVIEW: &str = concat!(
    "Hold invoice payment accepted — confirm fiat was sent?\n",
    "\n",
    "YES — Send FiatSent (you sent fiat).\n",
    "NO — Close without sending.\n",
    "CANCEL — Start cooperative cancel (both sides must agree; same as My Trades Shift+C).\n",
    "\n",
    "Cancel path: ",
    "Cancel this order? This sends a cooperative Cancel request.",
);

/// Multi-line body for `Action::BuyerTookOrder` in the Messages tab (waiting for buyer fiat; optional cooperative cancel).
pub const VIEW_MESSAGE_BUYER_TOOK_ORDER_PREVIEW: &str = concat!(
    "Buyer took order — do you want to start a cooperative cancel?\n",
    "\n",
    "CANCEL — Start cooperative cancel (both sides must agree; same as My Trades Shift+C).\n",
    "NO — Close without canceling.\n",
);

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

// --- Footer (My Trades / Order Chat) ---

pub const FOOTER_MYTRADES_SELECT_ORDER: &str = "↑↓: Select order";
pub const FOOTER_MYTRADES_ENTER_SEND: &str = "Enter: Send";
pub const FOOTER_MYTRADES_SHIFT_I_DISABLE: &str = "Shift+I: Disable input";
pub const FOOTER_MYTRADES_SHIFT_I_ENABLE: &str = "Shift+I: Enable input";
pub const FOOTER_MYTRADES_SHIFT_C_CANCEL: &str = "Shift+C: Cancel order";
pub const FOOTER_MYTRADES_SHIFT_F_FIAT_SENT: &str = "Shift+F: Mark fiat sent";
pub const FOOTER_MYTRADES_SHIFT_R_RELEASE: &str = "Shift+R: Release sats";
pub const FOOTER_MYTRADES_SHIFT_V_RATE: &str = "Shift+V: Rate counterparty";
pub const FOOTER_MYTRADES_PGUP_PGDN_SCROLL_CHAT: &str = "PgUp/PgDn: Scroll chat";
pub const FOOTER_MYTRADES_END_BOTTOM: &str = "End: Bottom";

/// Step label for the buy order flow
///
/// Describes the top and bottom of the step label of the orders flow in UI
#[derive(Copy, Clone, Debug)]
pub struct StepLabel {
    pub top: &'static str,
    pub bottom: &'static str,
}

impl StepLabel {
    #[must_use]
    pub fn as_single_line(self) -> String {
        format!("{} {}", self.top, self.bottom).trim().to_string()
    }
}

pub const BUY_ORDER_FLOW_STEPS_MAKER: [StepLabel; 6] = [
    StepLabel {
        top: "Wait for",
        bottom: "Seller",
    },
    StepLabel {
        top: "Paste",
        bottom: "Invoice",
    },
    StepLabel {
        top: "Order",
        bottom: "Active",
    },
    StepLabel {
        top: "Send",
        bottom: "Fiat",
    },
    StepLabel {
        top: "Wait for",
        bottom: "Sats",
    },
    StepLabel {
        top: "Rate",
        bottom: "Counterparty",
    },
];

pub const BUY_ORDER_FLOW_STEPS_TAKER: [StepLabel; 6] = [
    StepLabel {
        top: "Pay Hold",
        bottom: "Invoice",
    },
    StepLabel {
        top: "Wait for",
        bottom: "Buyer Invoice",
    },
    StepLabel {
        top: "Order",
        bottom: "Active",
    },
    StepLabel {
        top: "Wait for",
        bottom: "Fiat",
    },
    StepLabel {
        top: "Release",
        bottom: "Sats",
    },
    StepLabel {
        top: "Rate",
        bottom: "Counterparty",
    },
];

pub const SELL_ORDER_FLOW_STEPS_MAKER: [StepLabel; 6] = [
    StepLabel {
        top: "Wait for",
        bottom: "Buyer",
    },
    StepLabel {
        top: "Pay Hold",
        bottom: "Invoice",
    },
    StepLabel {
        top: "Order",
        bottom: "Active",
    },
    StepLabel {
        top: "Wait for",
        bottom: "Fiat",
    },
    StepLabel {
        top: "Release",
        bottom: "Sats",
    },
    StepLabel {
        top: "Rate",
        bottom: "Counterparty",
    },
];

pub const SELL_ORDER_FLOW_STEPS_TAKER: [StepLabel; 6] = [
    StepLabel {
        top: "Add",
        bottom: "Invoice",
    },
    StepLabel {
        top: "Wait for",
        bottom: "Seller",
    },
    StepLabel {
        top: "Order",
        bottom: "Active",
    },
    StepLabel {
        top: "Send",
        bottom: "Fiat",
    },
    StepLabel {
        top: "Wait for",
        bottom: "Sats",
    },
    StepLabel {
        top: "Rate",
        bottom: "Counterparty",
    },
];

pub const GENERIC_ORDER_FLOW_STEPS_TAKER: [StepLabel; 6] = [
    StepLabel {
        top: "Payment",
        bottom: "/ Wait",
    },
    StepLabel {
        top: "",
        bottom: "Invoice",
    },
    StepLabel {
        top: "Order",
        bottom: "Active",
    },
    StepLabel {
        top: "",
        bottom: "Fiat",
    },
    StepLabel {
        top: "",
        bottom: "Sats",
    },
    StepLabel {
        top: "",
        bottom: "Rate",
    },
];
