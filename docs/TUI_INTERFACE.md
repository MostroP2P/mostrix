# TUI Interface Guide

This guide explains the architecture and implementation of the Mostrix Text User Interface (TUI).

## Core Technologies

Mostrix is built using:

- **[Ratatui](https://github.com/ratatui-org/ratatui)**: For terminal rendering, layouts, and widgets.
- **[Crossterm](https://github.com/crossterm-rs/crossterm)**: For terminal manipulation (raw mode, alternate screen) and input event handling.

## UI Architecture

### 1. AppState: The Single Source of Truth

The `AppState` struct in `src/ui/app_state.rs` manages the global state of the interface.

**Source**: `src/ui/app_state.rs`

```rust
pub struct AppState {
    pub user_role: UserRole,
    pub active_tab: Tab,
    /// Orders tab selection by order UUID (currency-filtered; see helpers/order_selection.rs).
    pub selected_order_id: Option<Uuid>,
    /// Persistent scroll state for the Orders tab table.
    pub orders_table_state: TableState,
    /// Disputes Pending selection by dispute UUID (initiated projection; see dispute_selection.rs).
    pub selected_pending_dispute_id: Option<Uuid>,
    /// Persistent scroll state for the Disputes Pending table.
    pub disputes_table_state: TableState,
    /// Disputes In Progress / Finalized selection is by **dispute id**, not a raw
    /// index into `admin_disputes_in_progress` (see `helpers/dispute_selection.rs`).
    pub selected_dispute_id: Option<String>,
    pub active_chat_party: ChatParty,
    pub admin_chat_input: String,
    pub admin_chat_input_enabled: bool,
    pub admin_dispute_chats: HashMap<String, Vec<DisputeChatMessage>>,
    pub admin_chat_scrollview_state: tui_scrollview::ScrollViewState,
    pub admin_chat_selected_message_idx: Option<usize>,
    pub admin_chat_line_starts: Vec<usize>,
    pub admin_chat_scroll_tracker: Option<(String, ChatParty, usize)>,
    pub admin_chat_last_seen: HashMap<(String, ChatParty), AdminChatLastSeen>,
    pub selected_settings_option: usize,
    pub mode: UiMode,
    pub messages: Arc<Mutex<Vec<OrderMessage>>>,
    pub active_order_trade_indices: Arc<Mutex<HashMap<uuid::Uuid, i64>>>,
    pub selected_message_idx: usize,
    pub pending_notifications: Arc<Mutex<usize>>,
    pub order_form_draft: Option<FormState>, // preserved New Order draft when leaving tab (see Create New Order)
    // …see source for observer and attachment fields…
}
```

### 2. UI Layout

The screen is divided into three horizontal chunks using `ratatui` layouts. The available tabs and layout change dynamically based on the active `UserRole`.

**Source**: `src/ui/mod.rs:523`

```523:531:src/ui/mod.rs
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ],
    )
    .split(f.area());
```

1. **Header (3 lines)**: Renders the navigation tabs. The tab list is determined by the `UserRole` (User vs Admin).
2. **Body (remaining space)**: Renders the active tab content or forms.
3. **Footer / Status bar (3 lines)**: Renders the multi-line status bar with settings + connection details.

## Roles and Navigation

Mostrix supports two distinct roles, each with its own set of tabs and workflows.

### User Role

Focused on trading and order management.

- **Orders**: View the global order book (persistent `TableState` scrolls with ↑↓; shared vertical scrollbar confined to data rows).
- **My Trades**: Manage active trades.
- **Messages**: Direct messages for trade coordination.
- **Settings**: Local configuration. **User mode**: key rotation via **Generate New Keys** and mnemonic backup prompts; **Set Lightning Address (buyer)** / **Clear Lightning Address** — optional `user@domain.com` stored in `settings.toml`; confirm-save fetches LNURL metadata (`payRequest`) before persisting (see `src/util/ln_address.rs`, `spawn_verify_and_save_ln_address_task`). **Admin mode**: **Change Admin Key** / **Add Dispute Solver** (no Generate New Keys — admin must use the Mostro daemon nsec). The visible menu and **Enter** routing share **`ADMIN_SETTINGS`** / **`USER_SETTINGS`** in `src/ui/tabs/settings_tab.rs` (`SettingsMenuAction` + label per row; **`settings_action_for_index`**).
- **Create New Order**: Sectioned order form with live preview, searchable currency picker (instance `fiat_currencies_accepted` or bundled ISO list), and silent draft persistence when switching tabs.

### Admin Role

Focused on dispute resolution and protocol management.

- **Disputes Pending**: List of disputes waiting to be taken (`Initiated` only via `get_initiated_disputes`). Selection by dispute UUID (`selected_pending_dispute_id` + `dispute_selection.rs`); persistent `disputes_table_state` + shared scrollbar (same pattern as Orders). Admins take ownership with Enter.
- **Disputes in Progress**: Complete workspace for managing taken disputes (state: `InProgress`), featuring:
  - Integrated chat system with buyer and seller
  - Comprehensive dispute information header
  - Dynamic message input with text wrapping
  - Chat history with scrolling (PageUp/PageDown)
  - **Sidebar selection by dispute id** (`selected_dispute_id` + `selected_filtered_dispute`) so chat/finalize/attachments always target the highlighted dispute under the InProgress/Finalized filter — not a hidden closed row above it in `taken_at DESC` order
  - **Scrollable sidebar list** (`List` + `ListState`): ↑↓ keeps the selected dispute in view when many disputes overflow the sidebar; scrollbar when the list is taller than the panel
  - Finalization popup for resolution actions
  - **Empty state**: When no disputes are available, displays helpful key hints footer (filter + `↑↓: Select Dispute | Ctrl+H: Help`); footer is width-aware (narrow terminals show only Ctrl+H).
- **Observer**: Read-only workspace for inspecting user-to-user encrypted chats via a disclosed **Shared key** (protocol `K_conv`):
  - **Shared key** input (64-char hex secret, paste-friendly). There is currently no UI field for the optional Signer pubkey (`pub(K_sign)`) locator, so `fetch_observer_chat` is always called with `sign_pubkey: None`.
  - Fetches kind-14 chat events from relays for the last 7 days (`#p = pub(K_conv)`)
  - Decrypts messages and maps sender pubkeys to Buyer/Seller/Admin roles automatically
  - Displays chat using the same formatting as the dispute chat (color-coded, right-aligned Buyer/Seller, left-aligned Admin)
  - Supports file/image attachments with `Ctrl+S` to save (same popup as dispute chat)
  - Keyboard hints: `Enter` to fetch chat, `Ctrl+C` to clear all, `Ctrl+S` to save attachment, `Ctrl+H` for help
- **Settings**: Role-specific configuration including:
  - Add Dispute Solver
  - Change Admin Key (set `admin_privkey` to the Mostro daemon nsec)
  - Manage relays and currency filters
  - (**User mode**) Generate New Keys / Lightning address options

For detailed information about admin dispute resolution workflows, see [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) and [FINALIZE_DISPUTES.md](FINALIZE_DISPUTES.md).

## UI Modes & State Machine

The interface uses a nested state machine defined by `UiMode`, `UserMode`, and `AdminMode`.

**Source**: `src/ui/app_state.rs`

```rust
pub enum UiMode {
    // Shared modes (available to both user and admin)
    Normal,
    ViewingMessage(MessageViewState),
    RatingOrder(RatingOrderState), // 1–5 stars → RateUser DM (Messages tab, action `rate`)
    NewMessageNotification(MessageNotification, Action, InvoiceInputState),
    OperationResult(OperationResult), // Generic operation result popup (success/info/error)
    HelpPopup(Tab, Box<UiMode>),      // Context-aware keyboard shortcuts (Ctrl+H); Box<UiMode> = mode to restore on close
    SaveAttachmentPopup(usize),              // Dispute chat: list index of selected attachment (Ctrl+S opens, ↑↓/Enter/Esc in popup)
    ObserverSaveAttachmentPopup(usize),      // Observer tab: list index of selected attachment (Ctrl+S opens, ↑↓/Enter/Esc in popup)
    UserSaveAttachmentPopup(String, usize),  // My Trades: pinned order_id + list index (Ctrl+S; order_id pinned so sidebar changes do not retarget save)
    UserSendAttachmentPicker(String),        // My Trades: pinned order_id + ratatui-explorer (Ctrl+O; Enter on file enqueues send job)

    // User-specific modes
    UserMode(UserMode),

    // Admin-specific modes
    AdminMode(AdminMode),
}
```

### Overlays (Popups)

Popups are implemented by rendering additional widgets on top of the main layout when the `UiMode` is not `Normal`. They are drawn at the end of the `ui_draw` function to ensure they appear as overlays.

The primary shared popup is the **operation result** modal, used for:

- Order creation / take-order flows
- Settings validation errors (invalid pubkey, relay, currency, Lightning address format, LNURL verification failure, etc.)
- Admin actions (add solver, finalize disputes)
- Blossom attachment downloads and Observer-mode Shared key errors

When the popup is closed (**Esc** or **Enter**) from the **Disputes in Progress** tab, the app stays on that tab and returns to **ManagingDispute** mode (it does not switch to the first tab).

**Create New Order draft persistence**:

- **Leave**: **Left** / **Right** tab navigation from Create New Order silently saves the current `FormState` to `AppState.order_form_draft` and switches tabs — no confirmation popup.
- **Return**: Opening Create New Order again restores that draft (`navigation::restore_or_new_form`, also auto-init in `draw.rs` when the tab is active in `Normal` mode).
- **Discard**: **Esc** on the form (or a successful submit) clears `order_form_draft`.
- **Source**: `src/ui/key_handler/navigation.rs` (`save_and_leave_creating_order`), `src/ui/draw.rs`.

**Example**: Rendering the `OperationResult` popup.

**Source**: `src/ui/operation_result.rs` (rendering), `src/ui/orders.rs` (`OperationResult` enum). Notable variants handled in [`handle_operation_result`](../src/util/dm_utils/order_ch_mng.rs):

| Variant | Effect |
|---------|--------|
| `Success` | My Trades placeholder + `order_chat_static` |
| `PaymentRequestRequired` | Opens Pay / bond invoice popup (`NewMessageNotification`) |
| `OpenInvoicePopup` | Opens Add Invoice or waiting popup from synchronous execute (bond payout reply); does **not** show the operation-result modal |
| `InvoiceSubmitted` | Clears sticky replacement-invoice tracking for `order_id`; optional buyer LN-address preference; then normalized to `Info` toast |
| `TradeClosed` / `OrderHistoryDeleted` | Side effects on Messages list, then `Info` toast |
| `OrderChatAttachmentSent` | Appends **You** chat row + JSON transcript save; clears `pending_order_attachment_sends` and `sending_attachment_order_id` when `order_id` matches; then normalized to `Info` (`Attachment sent: …`) |
| `OrderChatAttachmentError` | Early send failure (validate / encrypt / upload before DM); clears `sending_attachment_order_id` when `order_id` matches; normalized to `Error` popup. Generic `OperationResult::Error` from other tasks does **not** clear the in-flight send guard. |
| `OrderChatAttachmentSendFailed` | Stores `PreparedOrderChatAttachment` in `AppState.pending_order_attachment_sends`; clears `sending_attachment_order_id` when `order_id` matches; normalized to `Error` (Blossom URL + **Ctrl+Shift+O** retry hint) |
| `ConversationDisclosure` | My Trades **Shift+K** result: shows the disclosed Shared key (`K_conv`). Not normalized to `Info` — rendered as its own popup so **C** can safely copy only the Shared key hex to the clipboard (never the signing key) |

**`OperationResult::Info` / `Error` text**: `render_operation_result` splits on newlines, wraps at word boundaries (avoids mid-word breaks on long UUIDs), and sizes the popup from line count. Admin dispute **finalization success** uses a structured multi-line body from `BondSlashChoice::finalize_success_message` (see [FINALIZE_DISPUTES.md](FINALIZE_DISPUTES.md)).

```rust
// Operation result popup overlay (shared)
if let UiMode::OperationResult(result) = &app.mode {
    operation_result::render_operation_result(f, result);
}
```

**Help popup (Ctrl+H)**:

- **Open**: Press **Ctrl+H** in normal or managing-dispute mode to show a context-aware shortcuts overlay for the current tab (Disputes in Progress, Observer, Settings, Orders, etc.).
- **Content**: The popup lists all relevant key bindings for that tab; e.g. in Disputes in Progress it shows filter toggle, Tab/Enter/Shift+I/Shift+F, scroll keys, and Ctrl+S to open the save-attachment list when applicable. On **My Trades** it includes PgUp/PgDn/End chat scroll, **Shift+K** (reveal Shared key for solvers), **Ctrl+S** (save attachment list), **Ctrl+O** (send file picker), and **Ctrl+Shift+O** (retry DM after upload ok / send failed). On **Observer**, it lists Enter to load chat, paste, scroll, clear, and save-attachment shortcuts (Left/Right still change tabs).
- **Close**: **Esc**, **Enter**, or **Ctrl+H** close the popup; other keys are absorbed while it is open.
- **Source**: `src/ui/help_popup.rs` (rendering), `src/ui/key_handler/mod.rs` (Ctrl+H and close handling).

**Save attachment popup (Ctrl+S in Disputes in Progress, Observer tab, or My Trades)**:

- **Open**: When managing a dispute, viewing an observer chat, or on **My Trades** with a selected active order, press **Ctrl+S** to open a centered popup listing all file/image attachments. In Disputes in Progress, attachments are scoped to the current dispute and active party (Buyer or Seller). In Observer mode, attachments are drawn from all observer messages. On My Trades, attachments come from the selected order’s chat transcript (`get_order_attachment_messages`). If there are no attachments, Ctrl+S does nothing.
- **In popup**: **↑/↓** change selection, **Enter** enqueues the selected attachment on `save_attachment_tx` and closes the popup; **Esc** cancels. Other keys are absorbed. Footer shows "↑↓ Select, Enter Save, Esc Cancel".
- **Async download + popup**: `spawn_save_attachment` runs on a Tokio task and sends `OperationResult::Info` ("Saved to …") or `Error` on `order_result_tx`. The main loop applies those results in `apply_order_result` and also **`try_recv`s attachment and result channels before every frame** (`drain_save_attachment_queue`, `drain_send_order_attachment_queue`, `drain_order_result_queue` in `src/main.rs`) so the operation-result modal appears without waiting for another key (My Trades may enqueue the job after a short async DB key lookup).
- **Saveable list**: only attachments with a non-empty Blossom URL appear in the popup (`attachment_is_saveable` / `get_order_attachment_messages` in `src/ui/helpers/chat_visibility.rs`).
- **My Trades decrypt**: when the sender did not embed a key in the attachment JSON, Mostrix derives the shared ChaCha20 key from `order_chat_shared_key_hex` or ECDH (`order_chat_decryption_key_bytes` in `src/util/chat_utils.rs`) before writing the file.
- **Source**: `src/ui/save_attachment_popup.rs` (dispute, observer, and user order popups), `src/ui/key_handler/mod.rs` (open and popup key handling), `src/main.rs` (queue drain), `src/ui/constants.rs` (`SAVE_ATTACHMENT_POPUP_HINT`, `FOOTER_CTRL_S_SAVE_FILE`).

**Send attachment picker (Ctrl+O on My Trades)**:

- **Open**: On **My Trades** with a selected active order, press **Ctrl+O** while `user_my_trades_interactive()` is true. Opens `UiMode::UserSendAttachmentPicker(order_id)` with a `ratatui-explorer` modal (`build_send_attachment_explorer` in `src/ui/send_attachment_picker.rs`). Starts in `dirs::document_dir()` or `$HOME`. Does nothing while `sending_attachment_order_id` is set (send already in flight). Build failures show `OperationResult::Error` ("Could not open file picker: …").
- **Filter**: Only directories and files whose extension passes `attachment_extension_allowed` (`jpg`/`jpeg`/`png`/`pdf`/`mp4`/`mov`/`avi`/`doc`/`docx` in `src/util/file_validation.rs`) appear in the list.
- **In picker**: **h/j/k/l** (and other explorer keys routed via `FileExplorer::handle`) navigate; **Enter** on a regular file (not `..` or a directory) enqueues `SendOrderAttachmentJob::FromPath { order_id, path }` on `send_order_attachment_tx`, sets `sending_attachment_order_id`, and closes the picker; **Esc** cancels. Footer hint: `SEND_ATTACHMENT_PICKER_HINT` ("Enter: Send file | Esc: Cancel | h/j/k/l: Navigate | …").
- **Retry**: **Ctrl+Shift+O** on the same selected order enqueues `SendOrderAttachmentJob::RetryPrepared` when `AppState.pending_order_attachment_sends` holds that order (upload succeeded but shared-key DM failed). Same in-flight guard as Ctrl+O.
- **Async pipeline + popup**: `spawn_send_order_chat_attachment` in `src/util/send_attachment.rs` validates (including PNG/JPEG dimensions for images), encrypts, uploads to Blossom, builds mobile-compatible wire JSON, and sends the DM. Results arrive on `order_result_tx` as `OrderChatAttachmentSent`, `OrderChatAttachmentError` (early failure), or `OrderChatAttachmentSendFailed` (upload ok / DM fail); `handle_operation_result` clears `sending_attachment_order_id` only for those attachment-specific variants (scoped by `order_id`). The main loop drains `send_order_attachment_rx` and `order_result_rx` before every draw (see [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md)).
- **Source**: `src/ui/send_attachment_picker.rs`, `src/ui/key_handler/mod.rs` (Ctrl+O / Ctrl+Shift+O and picker keys), `src/util/send_attachment.rs`, `src/ui/constants.rs` (`FOOTER_CTRL_O_SEND_FILE`, `FOOTER_CTRL_SHIFT_O_RETRY`, `FOOTER_SENDING_ATTACHMENT`, `HELP_MY_TRADES_CTRL_O_SEND`, `HELP_MY_TRADES_CTRL_SHIFT_O_RETRY`).

Backup New Keys popup (first launch + key rotation):

- **Purpose**: Displays the newly generated 12-word mnemonic so it can be backed up after **Generate New Keys**.
- **Where it appears**:
  - On key rotation: shown as an overlay popup.
  - On very first launch: shown as an overlay on the initial Orders/Disputes tab (Mostrix does not force switching to the Settings tab).
- **Reminder**: After saving the mnemonic, restart Mostrix so it uses the rotated keys everywhere.

### UI constants

Shared copy (help titles, footer hints, filter labels) lives in **`src/ui/constants.rs`** so strings are defined once and reused by the help popup and by width-aware footers (e.g. Disputes in Progress). Use these constants when adding or changing UI text to avoid duplication.

## Navigation & Input Handling

Input handling is centralized in `src/ui/key_handler/` (mod.rs and submodules: enter_handlers, esc_handlers, navigation, etc.).

### Tab Navigation

Users can switch between roles (User/Admin) and tabs using arrow keys.

- **Left/Right**: Switch tabs.
- **Up/Down**: Navigate within lists (Order book, Messages).

### Mode-Specific Dispatch

The `handle_key_event` function dispatches keys based on the current `UiMode`.

**Example**: Handling the `Enter` key (dispatched from `key_handler/mod.rs` to `enter_handlers::handle_enter_key`). Async feedback uses purpose-specific channels from **`EnterKeyContext`**: **`order_result_tx`** for orders, takes, disputes, bulk history cleanup, attachments, etc., and **`ln_address_result_tx`** for **Lightning address verify-and-save** only (`LnAddressVerifyResult` → main loop maps to `OperationResult` for the shared popup).

### Specialized Input

- **Forms**: Character input and Backspace are handled by `form_input::handle_char_input` and `form_input::handle_backspace` for fields in `FormState` while `UiMode::UserMode(UserMode::CreatingOrder(_))`.
  - **Create New Order** (`src/ui/order_form.rs`, `src/ui/orders.rs` — `FormState` / `FormField`, `src/ui/currencies.rs` — picker metadata):
    - **Layout**: Two-column body — **Order details** (left, sectioned **TRADE** / **PRICING** / **TERMS** with compact input strips) + **Live preview** receipt card (right); contextual **Field help** strip and footer key hints below.
    - **Focus**: **Tab** / **Shift+Tab** cycle fields; focused row shows a `▸` accent and green input strip; inline **✓** / **✗** per field; section headers turn green when all fields in that section validate.
    - **Toggles**: **Space** on **Order Type** toggles buy/sell (`⇄ Space` hint); **Space** on **Fiat Amount** toggles single/range; **Space** on **Payment Method** inserts a space (labels like `SEPA Instant` are allowed).
    - **Currency picker** (`CurrencyPicker` on `FormState`, `form_input::handle_currency_picker_key` — early interceptor in `key_handler/mod.rs`): on **Currency**, **Enter** / **Space** / typing opens a searchable dropdown anchored under the row. Options come from `MostroInstanceInfo.fiat_currencies_accepted` when non-empty, else the bundled ISO-4217 list in `currencies.rs` (code + human name; no emoji flags — terminal fonts rarely render them). **↑/↓** move, **Enter** selects, **Esc** closes; filter matches code prefix or name substring.
    - **Submit**: **Enter** on a complete form opens `ConfirmingOrder` (YES/NO); **Esc** cancels and clears `order_form_draft`.
    - **Draft persistence**: **Left** / **Right** tab navigation silently saves the form to `AppState.order_form_draft` and switches tabs. Returning to Create New Order restores the draft (`navigation::restore_or_new_form`, auto-init in `draw.rs` when tab is active in `Normal` mode).
  - **Global shortcut guard**: `c` / `C` (copy invoice / observer clear) is handled before the generic `Char(_)` arm in `key_handler/mod.rs`. When a **text** field is focused (`is_creating_order_text_input` in `form_input.rs` — any field except **Order Type**), that key is routed to form typing instead. On **Currency**, the picker interceptor runs first and consumes most keys while the dropdown is open. Outside the form, `c` still copies PayInvoice / PayBondInvoice invoices. Confirmation popups confirm with **Enter** on the focused button and cancel with **Esc** only (the `y` / `n` shortcuts were removed).
- **Invoices**: `handle_invoice_input` handles text entry for Lightning invoices, including support for bracketed paste mode.
- **Paste support**: The event loop now centralizes paste routing for active inputs and supports:
  - `Event::Paste(...)` (bracketed paste; enabled at startup via `EnableBracketedPaste`)
  - mouse right-click paste (`MouseEventKind::Down(MouseButton::Right)`) using clipboard read fallback
  - key fallbacks: **Ctrl+V** / **Ctrl+Shift+V** / **Cmd+V** / **Shift+Insert** where bracketed paste is unavailable
  This applies to invoice input, admin key/solver inputs, the Observer Shared key field, and the **Disputes in Progress** admin chatbox.
- **Admin Chat**: `handle_admin_chat_input` handles direct text input in the "Disputes in Progress" tab:
  - Takes priority over other input handling (except invoice and key input)
  - Supports direct character input, backspace, and paste (see above)
  - Ignores **Ctrl/Alt/Cmd** chords and **Shift+F/I/R/C** so those shortcuts are never typed into the box
  - Dynamic input box that grows from 1 to 10 lines
  - Text wrapping with word boundary detection
  - **Input toggle**: Press **Shift+I** to enable/disable chat input (prevents accidental typing)
  - **Visual feedback**: Input title shows enabled/disabled state
- **Copy to Clipboard**: Pressing `C` in a `PayInvoice` or `PayBondInvoice` notification, or in the My Trades **Shift+K** Shared key disclosure popup, uses the `arboard` crate (`handle_clipboard_copy` in `src/ui/key_handler/mod.rs`) to copy the invoice or the Shared key hex respectively. Only the Shared key is copyable from the disclosure popup — the signing key is never copied (and never displayed). The write runs synchronously and reports the real result: `copied_to_clipboard` (and the "✓ ... copied!" confirmation) is only set once `arboard::Clipboard::new()` and `set_text()` actually succeed; a failed write leaves the popup showing "Press C to copy" instead of a false success message. Persistence beyond that call is handled by the platform backend without blocking on it — the shared clipboard worker thread `arboard` starts on X11, or the background process `wl-clipboard-rs` detaches on Wayland.
- **Exit Confirmation**: Pressing `Q` or selecting the Exit tab shows a confirmation popup before exiting the application. Use Left/Right to select Yes/No, Enter to confirm, or Esc to cancel.
- **Help popup**: Press **Ctrl+H** (in normal or managing-dispute mode) to open a centered overlay with all keyboard shortcuts for the current tab. Press Esc, Enter, or Ctrl+H to close.

## UI Components

### 1. Orders Tab

Renders a table of pending orders from the Mostro network. Status and order kinds are color-coded for readability.

- **Scrolling**: persistent [`TableState`](https://docs.rs/ratatui) on `AppState.orders_table_state` so ↑↓ keeps the selected row in view without resetting the viewport each frame (aligned with Disputes Pending). A vertical scrollbar from `render_table_list_scrollbar` appears when row count exceeds the visible body; thumb tracks viewport **offset** and stays on the data-row track (does not overwrite borders/header).
- **Selection by order id** (`selected_order_id` + `helpers/order_selection.rs`): ↑↓ / highlight / Enter all resolve through the same currency-filtered book projection. If the stored id is hidden by `currencies_filter`, selection falls back to the first visible row so take/cancel never targets a filtered-out order. Survives book reorders better than a raw list index.
- **Narrow terminals** (`width < 100`): compact column set (Kind / Fiat Amt / Premium / Payment) — Premium stays visible.
- **Short terminals** (`height < 4`): header row is dropped so at least one data row remains visible.

**Source**: `src/ui/tabs/orders_tab.rs`, `src/ui/helpers/order_selection.rs`

### 2. Messages Tab

Displays a list of direct messages related to the user's trades. Messages are tracked as `read` or `unread`. The detail panel includes a **trade timeline stepper** (six columns): **`FlowStep`** from `src/ui/orders.rs` (`message_trade_timeline_step`), with per-column copy from **`src/ui/constants.rs`** (`listing_timeline_labels`). See [buy order flow.md](buy%20order%20flow.md) and [sell order flow.md](sell%20order%20flow.md).

- **Sidebar labels**: `message_action_compact_label_for_message` prefers **`order_status`** over raw **`action`** (e.g. **Pending order**, **Trade Completed**) so reboot replay does not show stale action text. Non-actionable rows opened with **Enter** use that compact label in an **`OperationResult::Info`** popup (`enter_handlers.rs`).
- **Pending / bond-pending timeline**: `Status::Pending`, `Status::WaitingTakerBond`, and `Status::WaitingMakerBond` map to **`StepPendingOrder`** (discriminant **0**). The stepper uses `step_number() == 0`, so **no column is highlighted** (all steps gray) until the trade advances — avoids falsely lighting step 1 while the order is still on the book or awaiting bond.
- **Post-success placeholder row**: when **`OperationResult::Success`** arrives before any DM row exists, `try_placeholder_order_message_from_success` (`orders.rs`) inserts one synthetic **`OrderMessage`** for My Trades / Messages (action from status: maker → `NewOrder` or `PayBondInvoice` when `WaitingMakerBond`; taker → `PayBondInvoice` / `WaitingSellerToPay` / etc.; never synthetic `take-buy` / `take-sell`). **`main.rs`** also resyncs My Trades from DB after **`Success`**, not only after history delete.

- **Enter** on a row: opens an invoice popup, a confirmation popup, the **rating** overlay (`RatingOrder`) when the daemon sent **`action: rate`**, or an info line for other actions (`src/ui/key_handler/enter_handlers.rs`).
- **Rating overlay**: `render_rating_order` in `src/ui/tabs/tab_content.rs`; keys **Left/Right** or **+/-** adjust stars, **Enter** submits, **Esc** closes.
- **Invoice popups (`NewMessageNotification`)**:
  - `AddInvoice` / `WaitingBuyerInvoice` map to invoice-submit popup mode. If **`settings.toml`** **`ln_address`** is non-empty, **`UiMode::ConfirmSavedLnAddressForInvoice`** may appear first (**YES** / **NO**), listing the saved address; **Left/Right** moves the highlight; **YES + Enter** auto-submits **`AddInvoice`** via **`submit_add_invoice`** (`message_handlers.rs`) without opening the invoice input popup (**`UseSavedLnAddress`** is saved only after the send succeeds — **`OperationResult::InvoiceSubmitted`** in **`order_ch_mng.rs`**). Choose **NO** and press **Enter** to open the manual invoice UI (**`ManualInvoice`**). **Esc** closes the confirm popup without committing YES or NO (**`handle_esc_key`** in `esc_handlers.rs`). **`BuyerInvoicePreference`** per **`order_id`** (`src/ui/app_state.rs`, `src/ui/orders.rs`) remembers the choice for that trade until **Cancel Order** from the popup removes it (`message_handlers.rs`) or the trade row is torn down (`order_ch_mng.rs`).
  - `PayInvoice` / `WaitingSellerToPay` map to payment popup mode.
  - **`PayBondInvoice`** (Mostro **Phase 1.5+** taker bond / **Phase 5+** maker bond) maps to a dedicated bond popup mode (`render_pay_bond_invoice` in `src/ui/message_notification.rs`). It mirrors the PayInvoice layout but uses a **🛡️** title (`Anti-abuse Bond Invoice`), a taker or maker amount label (via `MessageNotification.maker_bond_publish`: "Bond invoice to pay" vs "Pay bond to publish your order"), and a yellow "Locked, not spent — refunded on normal completion" disclaimer. Primary button is **Acknowledge** (closes the popup; payment happens in the user's wallet); **Cancel Order** is still wired to `Action::Cancel`. The popup is gated on `order_status ∈ {WaitingTakerBond, WaitingMakerBond, None}` and role (`local_user_must_act_on_invoice_popup` — status-based for `PayBondInvoice`, not listing kind). Sync paths: **`take_order`** (taker) and **`send_new_order`** (maker) return `PaymentRequestRequired`. Bonds are **configurable in mostrod** — when not enabled, create/take flows skip this popup.
  - All three popups provide two actions (`Primary` + `Cancel Order`) via Left/Right selection; Enter confirms the selected action.
  - `PayInvoice` and `PayBondInvoice` keep copy (`C`) and scroll (`Up/Down`, `PageUp/PageDown`) behavior while adding cancel selection.

**`ViewingMessage` (trade confirmations)** — `render_message_view` in `src/ui/tabs/tab_content.rs`:

- Used for actionable events that need a **YES/NO** before sending a follow-up to Mostro: **`HoldInvoicePaymentAccepted`**, **`FiatSentOk`**, **`CooperativeCancelInitiatedByPeer`**, plus explicit user-triggered actions from **My Trades** (**`Cancel`**, **`FiatSent`**, **`Release`**).
- Buttons are rendered with **`helpers::render_yes_no_buttons`** (same visual pattern as exit/settings confirms: **✓ YES** / **✗ NO**, **Left/Right** to move selection, **Enter** to confirm, **Esc** to dismiss).
- Handler: **`handle_enter_viewing_message`** in `src/ui/key_handler/message_handlers.rs` (only proceeds when **YES** is selected). Cooperative cancel completion surfaces **`OperationResult::TradeClosed`**, which **`handle_operation_result`** resolves after removing the row from the Messages list (see **MESSAGE_FLOW_AND_PROTOCOL.md**).

**Source**: `src/ui/tab_content.rs` (`render_messages_tab`, `render_message_view`, `render_rating_order`)

### 3. Order Form (Create New Order)

Stateful form for publishing buy/sell orders. Supports fixed amounts, market price (`0` sats), fiat ranges, and optional Lightning invoice / expiration.

**Source**: `src/ui/order_form.rs`, `src/ui/order_confirm.rs`, `src/ui/currencies.rs`, `src/ui/key_handler/form_input.rs`, `src/ui/key_handler/navigation.rs`

#### State

| Type | Location | Role |
|------|----------|------|
| `FormState` | `src/ui/orders.rs` | Field values, `focused`, `use_range`, embedded `CurrencyPicker` |
| `FormField` | `src/ui/orders.rs` | Order Type, Currency, Amount (sats), Fiat (+ max when range), Payment Method, Premium, Invoice, Expiration |
| `CurrencyPicker` | `src/ui/orders.rs` | `open`, `filter`, `selected` (index into filtered list) |
| `order_form_draft` | `AppState` | `Option<FormState>` — draft silently kept when leaving the tab via Left/Right |
| `UserMode::CreatingOrder` | `src/ui/user_state.rs` | Active editing |
| `UserMode::ConfirmingOrder` | `src/ui/user_state.rs` | Pre-submit YES/NO popup |
| `UserMode::WaitingForMostro` | `src/ui/user_state.rs` | Async `send_new_order` in flight |

#### Rendering (`render_order_form`)

- **Signature**: `render_order_form(f, area, form, info: Option<&MostroInstanceInfo>)` — instance info supplies `fiat_currencies_accepted`, `min_order_amount`, `max_order_amount` (limits hint under Amount when set).
- **Panels**:
  - **Order details** — three sections with vertically centered content inside the panel:
    - **TRADE**: Type (colored buy/sell dot), Currency (bold cyan code + dim name + `▾ pick`)
    - **PRICING**: Amount (`market` when 0/empty), Fiat min (+ Fiat max when range)
    - **TERMS**: Method, Premium, Invoice, Expiry
  - **Live preview** — rounded **Order** receipt card with implied `≈ …/BTC` when computable; status dot (`ready` / `fill: …` / `invalid: …`).
  - **Field help** — one-line contextual help for the focused field (`build_field_help`).
  - **Footer** — `Enter` submit / `Tab` focus / `Space` toggle / `Esc` cancel.
- **Currency dropdown** (`render_currency_dropdown`): modal overlay when `currency_picker.open`; list shows bold ISO code + dim name; title includes accepted count from instance or “common” fallback list.

#### Currency metadata (`src/ui/currencies.rs`)

- `CURRENCIES`: curated ISO-4217 table (`code`, `name`).
- `resolve_options(accepted)`: instance-advertised codes when non-empty, else full bundled list; unknown instance codes get code-only rows.
- `filter_options(options, query)`: case-insensitive code-prefix or name-substring filter.
- `lookup` / `name_for`: enrich display strings (confirmation popup, selected currency row).
- **No emoji flags** in the UI — regional-indicator emoji degrade to letter pairs in most terminal fonts.

#### Tab lifecycle (`src/ui/draw.rs`, `navigation.rs`)

1. User opens **Create New Order** → `CreatingOrder(restore_or_new_form())` (draft or default).
2. **Left** / **Right** → silently store `order_form_draft` and switch tab.
3. User returns to tab while `mode == Normal` → `draw.rs` auto-promotes to `CreatingOrder` (consumes draft or fresh default) so the tab is never stuck on an empty shell.
4. Successful submit (`ConfirmingOrder` YES) or explicit **Esc** cancel clears `order_form_draft`.

#### Key files for codegen

```text
src/ui/order_form.rs          # render_order_form, validation, preview
src/ui/currencies.rs          # ISO list + resolve/filter helpers
src/ui/orders.rs              # FormState, FormField, CurrencyPicker
src/ui/order_confirm.rs       # render_order_confirm
src/ui/draw.rs                # tab render + draft restore on entry
src/ui/key_handler/form_input.rs   # char/backspace + handle_currency_picker_key
src/ui/key_handler/navigation.rs   # silent draft save on leave, restore_or_new_form
src/ui/key_handler/enter_handlers.rs  # ConfirmingOrder Enter
src/ui/key_handler/esc_handlers.rs    # Esc clears draft
```

### My Trades interactive mode (`user_my_trades_interactive`)

**Source**: `src/ui/app_state.rs` — `UiMode::default_for_role`, `UiMode::user_my_trades_interactive`

- On **`AppState::new`** and **`switch_role`**, user mode starts in **`UiMode::UserMode(UserMode::Normal)`** (not bare `UiMode::Normal` alone).
- **Chat input**, **Ctrl+S**, **Ctrl+O**, **Ctrl+Shift+O**, **PgUp/PgDn/End** scroll, and related shortcuts are active only when `app.mode.user_my_trades_interactive()` is true: `UiMode::Normal` **or** `UiMode::UserMode(UserMode::Normal)`.
- Popups (save attachment, send attachment picker, operation result, viewing message, etc.) set other `UiMode` variants; My Trades shortcuts resume when the popup closes back to normal user mode.

### My Trades (Order In Progress) updates

The My Trades workspace (`src/ui/tabs/order_in_progress_tab.rs`) now shows richer order context and cleaner chat readability:

- **Header metadata (two layers)**:
  - **Stable (one-time)**: order id, kind, created-at, trade index, and initiator (role: Maker/Taker + truncated trade pubkey) come from `OrderChatStaticHeader` in `AppState.order_chat_static`. The map is filled when the user **creates** an order (maker) or **takes** an order (taker, including the `PaymentRequestRequired` path for both `PayInvoice` and `PayBondInvoice` responses — the variant carries the originating `Action`), and from `sync_user_order_history_messages_from_db` in `src/ui/helpers/startup.rs` (parses local `orders` rows so restarts do not lose the header). Entries are removed when a trade is closed or history cleanup deletes the row (`handle_operation_result` in `src/util/dm_utils/order_ch_mng.rs`).
  - **Live (from DMs)**: **status**, **amount / fiats**, **payment method**, **premium**, and **buyer/seller rating** (when present) still come from the message projection (see below).
- **Privacy / ratings**: there is no placeholder row for **`Privacy:`** / **`Buyer -`** / **`Seller -`** until trade privacy can be sourced from the same context as disputes (DM `SmallOrder` does not carry those flags). **Buyer Rating:** / **Seller Rating:** lines are shown only when reputation exists: `helpers::build_active_order_chat_list` merges `Payload::Peer` with `UserInfo` when `peer.pubkey` matches `buyer_trade_pubkey` / `seller_trade_pubkey` from `Payload::Order`, and the header uses `helpers::format_user_rating` for display.
- **Chat rendering**: user/peer messages are wrapped to fit pane width (including splitting overlong tokens by **Unicode character** count so lines do not overflow); peer messages are right-aligned for better sender separation.
- **Chat scrolling**: message history uses `tui_scrollview::ScrollView` with full content height (not viewport height) and an always-visible vertical scrollbar — same pattern as Disputes in Progress and Observer. **PgUp/PgDn** scroll the chat; **End** jumps to the latest messages. Auto-scroll-to-bottom runs when new messages arrive, when switching orders, or after sending (`order_chat_scroll_tracker`, `scroll_order_chat_messages` / `scroll_order_chat_after_send` in `src/ui/key_handler/chat_helpers.rs`).
- **Attachments (receive + transcript)**: encrypted file/image messages show as yellow lines with 🖼/📎 icons; the block title adds a file count; a yellow toast appears on new **peer** relay merges. Transcripts under `~/.mostrix/orders_chat/` persist attachment metadata as **JSON** so **Ctrl+S** works after restart (legacy placeholder lines hydrate from relay). **Ctrl+S** opens the save popup (see above).
- **Attachments (send)**: **Ctrl+O** opens `UserSendAttachmentPicker` (`ratatui-explorer` in `src/ui/send_attachment_picker.rs`); **Enter** on a file enqueues `SendOrderAttachmentJob::FromPath` on `send_order_attachment_tx`. Backend in `src/util/send_attachment.rs`: validate → encrypt (shared key) → Blossom upload (NIP-24242 auth signed with **order trade key**) → shared-key DM. **Ctrl+Shift+O** enqueues `RetryPrepared` when `AppState.pending_order_attachment_sends` has the selected order (upload ok / DM failed). `sending_attachment_order_id` blocks duplicate sends while in flight; cleared only by attachment-specific `OperationResult` variants in `order_ch_mng.rs`, not by unrelated errors on `order_result_tx`.
- **Empty states**: sidebar/main panel copy is clearer ("No active orders yet"), and the help hint remains visible in the footer.
- **Footer shortcuts (width-aware)**:
  - **Shift+I** toggles chat input.
  - **Shift+C** cooperative cancel (YES/NO popup).
  - **Shift+F** mark fiat sent (YES/NO popup).
  - **Shift+R** release sats (YES/NO popup).
  - **Shift+V** rate counterparty (opens 1–5 star rating picker).
  - **Shift+K** reveal Shared key (read-only grant for solvers; never the signing key). Opens a popup where **C** copies the Shared key to the clipboard.
  - **PgUp/PgDn** scroll chat history; **End** jump to bottom.
  - **Ctrl+S** save attachment (when the selected order has attachments).
  - **Ctrl+O** send attachment (file picker); **Ctrl+Shift+O** retry DM when a prepared send is pending.
  - **Sending attachment…** (`FOOTER_SENDING_ATTACHMENT`) while `sending_attachment_order_id` matches the selected order.
  - **Shift+H** opens the shortcuts popup for the current tab.
- **Projection vs static**: the DM-based list row (`OrderChatListItem` in `src/ui/helpers/order_chat_projection.rs`) holds **live** fields only: `status`, first-seen **economic** snapshot from `Payload::Order` (amount, fiat, payment, premium), `trade_index` (from any message in the order), and buyer/seller **trade pubkeys** plus **reputation** from `Payload::Peer`. It no longer carries kind, `created_at`, or initiator metadata (those are on `order_chat_static` above).
- **Selection correctness (shared projection)**: both the sidebar list and Enter/send handlers derive the selected order from the same projection (`helpers::build_active_order_chat_list`), with identical filtering and ordering. This prevents UI/action desync where `selected_order_chat_idx` could resolve a different trade than the highlighted row. **Trade ID** in the header still falls back to projection `trade_index` if a static entry is not yet in the map (e.g. race before the result handler runs).

**Source**: `src/ui/tabs/order_in_progress_tab.rs`

### 4. Color Coding

Mostrix uses a consistent color palette defined in `src/ui/mod.rs`:

- **`PRIMARY_COLOR`**: `#b1cc33` (Mostro green).
- **`BACKGROUND_COLOR`**: `#1D212C`.
- **Status Colors**: Yellow for pending, Green for active/success, Red for disputes/cancellation.
- **Chat Colors**:
  - Cyan for Admin messages
  - Green for Buyer messages
  - Red for Seller messages

**Source**: `src/ui/mod.rs` (color constants), `src/ui/tabs/orders_tab.rs` and `src/ui/tabs/disputes_in_progress_tab.rs` (status colors), `src/ui/tabs/disputes_in_progress_tab.rs` (chat colors)

### 5. Admin Chat System

**Status**: ✅ **Fully Implemented (kind 14 + ECDH shared keys)**

The admin chat system in the "Disputes in Progress" tab provides real-time, Nostr-based communication using kind-14 chat envelopes (`K_sign` / `K_conv`, with dual-read of legacy GiftWrap while `CHAT_ACCEPT_LEGACY_GIFTWRAP` is true) and per‑dispute ECDH shared keys derived between the admin key and each party’s trade pubkey.

#### Helper module organization (readability refactor)

The previous monolithic helper file was split into focused modules under `src/ui/helpers/`:

- `mod.rs`: compatibility re-export layer (`crate::ui::helpers::...`) for existing call sites.
- `layout.rs`: popup/help/button rendering helpers (`create_centered_popup`, `render_help_text`, `render_yes_no_buttons`, `render_yes_no_cancel_buttons`). **Unit-tested** with `TestBackend` (geometry + selection highlights) — reuse as the template for other popup tests.
- Thin overlays with the same TestBackend pattern: `exit_confirm`, `waiting`, `offline_overlay`, `key_input_popup`, `generate_keys_popup`, `dispute_bond_slash_popup`, `order_confirm`, `status` (and `network_status` enum smoke tests; monitor spawn is not unit-tested).
- `formatting.rs`: UI formatting helpers (ratings, order id labels, finalized status check).
- `chat_visibility.rs`: per-party visibility and selection helpers.
- `chat_render.rs`: wrapped line formatting plus list/scrollview builders.
- `chat_storage.rs`: transcript parse/load/save logic for disputes and user order chat.
- `attachments.rs`: attachment JSON parse/serialize, outbound `build_*_encrypted_json` helpers, placeholders, and toast expiration/building.
- `order_chat_projection.rs`: shared "My Trades" active-order projection (single source of truth for sidebar ordering and Enter/action resolution). Merges `Payload::Order` (economic first snapshot, buyer/seller trade pubkeys) and `Payload::Peer` (reputation when pubkeys match). Payload merge runs even when `order_kind` is missing on a message, so **Peer** reputation is not skipped for odd DM shapes.
- `startup.rs`: startup hydration/recovery, applying chat updates to app state, and seeding **`order_chat_static`** from `orders` when syncing user history.

#### Data Structures

```rust
pub enum ChatSender {
    Admin,
    Buyer,
    Seller,
}

pub struct DisputeChatMessage {
    pub sender: ChatSender,
    pub content: String,
    pub timestamp: i64,
    pub target_party: Option<ChatParty>, // For Admin messages: which party this was sent to
}

pub struct AdminChatLastSeen {
    pub last_seen_timestamp: Option<u64>, // Last seen message timestamp for incremental fetches
}
```

**Storage**:

- `AppState.admin_dispute_chats: HashMap<String, Vec<DisputeChatMessage>>` keyed by dispute ID.
- `AppState.admin_chat_last_seen: HashMap<(String, ChatParty), AdminChatLastSeen>` keyed by (dispute_id, party).

#### UI Features

- **Dispute selection**: `selected_dispute_id` + `selected_filtered_dispute` — send/finalize/attachments always target the sidebar highlight under the current filter.
- **Direct input**: Type immediately without mode switching (when input enabled).
- **Input toggle**: Press **Shift+I** to enable/disable chat input.
- **Dynamic sizing**: Input box grows from 1 to 10 lines based on content.
- **Text wrapping**: Intelligent word-boundary wrapping with trim behavior.
- **Scrolling**:
  - **PageUp/PageDown**: Navigate through message history.
  - **End**: Jump to bottom of chat (latest messages).
  - **Sidebar**: ↑↓ dispute list uses stateful `List` so selection stays visible when many disputes are open.
  - **Visual scrollbar**: Right-side scrollbar shows position (↑/↓/│/█ symbols).
- **Party filtering**:
  - Admin messages are only shown in the chat view of the party they were sent to (based on `target_party`).
  - Buyer/Seller messages are only shown in their respective chat views.
- **Visual feedback**: Focus indicators, color-coded messages, alignment prefixes, input state indicators.

#### Input Handling Priority

The key handler processes input in this order:

1. Invoice input (highest priority, when in invoice mode).
2. Key input (for settings popups).
3. **Shift+I toggle** (for enabling/disabling admin chat input).
4. **Admin chat input** (takes priority in Disputes in Progress tab, only when enabled).
5. Other character/form input.

**Source**: `src/ui/key_handler/mod.rs` (`handle_admin_chat_input`, Shift+I toggle).

#### Kind-14 Chat Internals (Shared Key Model)

Active admin-chat transport is kind 14 (`K_sign` / `K_conv`). GiftWrap is inbound-only during the `CHAT_ACCEPT_LEGACY_GIFTWRAP` dual-read window, not the send path.

- **Shared key derivation**:
  - When a dispute is taken (`AdminDispute::new`), per-party ECDH shared secrets are eagerly derived (`derive_shared_key_hex`) and stored as hex in `buyer_shared_key_hex` / `seller_shared_key_hex`. Chat wrap/unwrap derives `K_conv` / `K_sign` from that IKM at runtime.
  - Both admin and counterparty can independently derive the same ECDH secret (and thus the same chat keys), matching mostro-chat.

- **Message addressing**:
  - Outbound chat is kind 14 signed by `K_sign` with `p` = `pub(K_conv)` (derived from stored ECDH hex). The admin identity signs the inner rumor.

- **Sending messages**:
  - Admin messages are sent via `send_admin_chat_message_via_shared_key` (spawned as an async task to avoid blocking the UI):
    - Inner event is a kind-1 text note signed by the admin key.
    - Outer envelope is always kind 14 (`wrap_chat_message`); GiftWrap is receive-only during the dual-read window.
    - Published to relays without blocking the main UI thread.

- **Receiving messages**:
  - The shared-key chat subscription router (`listen_for_chat_messages`) delivers messages live over a batched subscription (kind 14 always; GiftWrap while `CHAT_ACCEPT_LEGACY_GIFTWRAP` is true). Disputes are tracked via `track_dispute_chat` when taken (with a party+admin inner-signer allow-list) and re-tracked by `track_startup_chats` at startup/reconnect. History is hydrated once per key on track.
  - For each in-progress dispute, the fetch:
    - Rebuilds buyer/seller shared `Keys` from the stored hex.
    - Fetches history with kind-14 `authors = [pub(K_sign)]`, plus a legacy `kind: 1059` `#p` query while dual-read is on (7-day rolling window; GiftWrap `created_at` is randomized so that query keeps the wide floor).
    - Decrypts each event using the shared key (`K_conv` / `K_sign`) and rejects inner signers outside the party+admin allow-list.
    - Uses `last_seen_timestamp` to skip already-processed events.
    - Skips events signed by the admin identity to avoid duplicating locally-sent messages.

- **Behavior on restart (Chat Restore at Startup)**:
  - Admin chat uses a **hybrid persistence model** to provide instant UI restore and incremental sync:
    - For each in‑progress dispute, chat transcripts are stored as human‑readable files under:

      ```text
      ~/.mostrix/disputes_chat/<dispute_id>.txt
      ```

    - On startup, `recover_admin_chat_from_files`:
      - Reads each existing transcript file.
      - Rebuilds `AppState.admin_dispute_chats` so the Disputes in Progress tab immediately shows previous messages.
      - Computes the latest timestamps per party and updates `AppState.admin_chat_last_seen`.
    - The latest buyer/seller timestamps are also persisted in the `admin_disputes` table (`buyer_chat_last_seen`, `seller_chat_last_seen`) via `update_chat_last_seen_by_dispute_id` so that:
      - Background chat fetches only request **newer** events (7-day rolling window).
      - Chat resumes from where it left off without replaying the full history.

#### Exit Confirmation

Mostrix includes a safety feature to prevent accidental exits:

- **Trigger**: Navigate to the Exit tab (User or Admin) and confirm
- **Popup**: Shows confirmation dialog with "Are you sure you want to exit Mostrix?"
- **Navigation**: Use Left/Right arrows to select Yes/No buttons
- **Confirmation**: Press Enter to confirm exit, or Esc to cancel
- **Visual**: Green "✓ YES" button and red "✗ NO" button with clear styling

**Source**: `src/ui/exit_confirm.rs`, `src/ui/key_handler/enter_handlers.rs`
