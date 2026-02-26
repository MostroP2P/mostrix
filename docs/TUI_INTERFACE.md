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
    pub selected_order_idx: usize,
    pub selected_dispute_idx: usize,
    pub selected_in_progress_idx: usize,
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
3. **Footer (1 line)**: Renders the status bar with connection details.

## Roles and Navigation

Mostrix supports two distinct roles, each with its own set of tabs and workflows.

### User Role

Focused on trading and order management.

- **Orders**: View the global order book.
- **My Trades**: Manage active trades.
- **Messages**: Direct messages for trade coordination.
- **Settings**: Local configuration.
- **Create New Order**: Form for publishing new orders.

### Admin Role

Focused on dispute resolution and protocol management.

- **Disputes Pending**: List of disputes waiting to be taken. Only displays disputes with `Initiated` status (filtering implemented in `disputes_tab.rs`). Admins can select and take ownership of these disputes.
- **Disputes in Progress**: Complete workspace for managing taken disputes (state: `InProgress`), featuring:
  - Integrated chat system with buyer and seller
  - Comprehensive dispute information header
  - Dynamic message input with text wrapping
  - Chat history with scrolling (PageUp/PageDown)
  - Finalization popup for resolution actions
  - **Empty state**: When no disputes are available, displays helpful key hints footer (filter + `↑↓: Select Dispute | Ctrl+H: Help`); footer is width-aware (narrow terminals show only Ctrl+H).
- **Observer**: Read-only workspace for inspecting user-to-user encrypted chats that have been uploaded as files:
  - File-path input (relative to `~/.mostrix/downloads` or absolute)
  - Shared-key input (64-char hex secret used by both users)
  - Decrypts Blossom-encrypted blobs locally using ChaCha20-Poly1305 and renders the plaintext as a scrollable chat preview
  - Keyboard hints: `Tab/Shift+Tab` to switch between file and key fields, `Enter` to load & decrypt, `Ctrl+C` to clear inputs and preview, `Ctrl+H` for help
- **Settings**: Role-specific configuration including:
  - Add Dispute Solver
  - Change Admin Key
  - Manage relays and currency filters

For detailed information about admin dispute resolution workflows, see [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) and [FINALIZE_DISPUTES.md](FINALIZE_DISPUTES.md).

## UI Modes & State Machine

The interface uses a nested state machine defined by `UiMode`, `UserMode`, and `AdminMode`.

**Source**: `src/ui/app_state.rs`

```rust
pub enum UiMode {
    // Shared modes (available to both user and admin)
    Normal,
    ViewingMessage(MessageViewState),
    NewMessageNotification(MessageNotification, Action, InvoiceInputState),
    OperationResult(OperationResult), // Generic operation result popup (success/info/error)
    HelpPopup(Tab),                   // Context-aware keyboard shortcuts (Ctrl+H)

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
- Settings validation errors (invalid pubkey, relay, currency, etc.)
- Admin actions (add solver, finalize disputes)
- Blossom attachment downloads and Observer-mode file/key errors

**Example**: Rendering the `OperationResult` popup.

**Source**: `src/ui/operation_result.rs` (rendering), `src/ui/orders.rs` (`OperationResult` enum).

```rust
// Operation result popup overlay (shared)
if let UiMode::OperationResult(result) = &app.mode {
    operation_result::render_operation_result(f, result);
}
```

**Help popup (Ctrl+H)**:

- **Open**: Press **Ctrl+H** in normal or managing-dispute mode to show a context-aware shortcuts overlay for the current tab (Disputes in Progress, Observer, Settings, Orders, etc.).
- **Content**: The popup lists all relevant key bindings for that tab; e.g. in Disputes in Progress it shows filter toggle, Tab/Enter/Shift+I/Shift+F, scroll keys, and Ctrl+S for attachments when applicable.
- **Close**: **Esc**, **Enter**, or **Ctrl+H** close the popup; other keys are absorbed while it is open.
- **Source**: `src/ui/help_popup.rs` (rendering), `src/ui/key_handler/mod.rs` (Ctrl+H and close handling).

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

**Example**: Handling the `Enter` key (dispatched from `key_handler/mod.rs` to `enter_handlers::handle_enter_key`, which uses `order_result_tx` for operation-result feedback).

### Specialized Input

- **Forms**: Character input and Backspace are handled by `handle_char_input` and `handle_backspace` for fields in `FormState`.
- **Invoices**: `handle_invoice_input` handles text entry for Lightning invoices, including support for bracketed paste mode.
- **Admin Chat**: `handle_admin_chat_input` handles direct text input in the "Disputes in Progress" tab:
  - Takes priority over other input handling (except invoice and key input)
  - Supports direct character input and backspace
  - Dynamic input box that grows from 1 to 10 lines
  - Text wrapping with word boundary detection
  - **Input toggle**: Press **Shift+I** to enable/disable chat input (prevents accidental typing)
  - **Visual feedback**: Input title shows enabled/disabled state
- **Copy to Clipboard**: Pressing `C` in a `PayInvoice` notification uses the `arboard` crate to copy the invoice. On Linux, it uses the `SetExtLinux::wait()` method to properly wait until the clipboard is overwritten, ensuring reliable clipboard handling without arbitrary delays.
- **Exit Confirmation**: Pressing `Q` or selecting the Exit tab shows a confirmation popup before exiting the application. Use Left/Right to select Yes/No, Enter to confirm, or Esc to cancel.
- **Help popup**: Press **Ctrl+H** (in normal or managing-dispute mode) to open a centered overlay with all keyboard shortcuts for the current tab. Press Esc, Enter, or Ctrl+H to close.

## UI Components

### 1. Orders Tab

Renders a table of pending orders from the Mostro network. Status and order kinds are color-coded for readability.

**Source**: `src/ui/orders_tab.rs`

### 2. Messages Tab

Displays a list of direct messages related to the user's trades. Messages are tracked as `read` or `unread`.

**Source**: `src/ui/tab_content.rs:render_messages_tab`

### 3. Order Form

A stateful form for creating new orders. It supports both fixed amounts and fiat ranges.

**Source**: `src/ui/order_form.rs`

### 4. Color Coding

Mostrix uses a consistent color palette defined in `src/ui/mod.rs`:

- **`PRIMARY_COLOR`**: `#b1cc33` (Mostro green).
- **`BACKGROUND_COLOR`**: `#1D212C`.
- **Status Colors**: Yellow for pending, Green for active/success, Red for disputes/cancellation.
- **Chat Colors**:
  - Cyan for Admin messages
  - Green for Buyer messages
  - Red for Seller messages

**Source**: `src/ui/mod.rs` (color constants), `src/ui/orders_tab.rs` and `src/ui/disputes_in_progress_tab.rs` (status colors), `src/ui/disputes_in_progress_tab.rs` (chat colors)

### 5. Admin Chat System

**Status**: ✅ **Fully Implemented (NIP‑59 + Shared Keys)**

The admin chat system in the "Disputes in Progress" tab provides real-time, Nostr-based communication using NIP‑59 gift-wrap events and per‑dispute shared keys derived between the admin key and each party’s trade pubkey.

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

- **Direct input**: Type immediately without mode switching (when input enabled).
- **Input toggle**: Press **Shift+I** to enable/disable chat input.
- **Dynamic sizing**: Input box grows from 1 to 10 lines based on content.
- **Text wrapping**: Intelligent word-boundary wrapping with trim behavior.
- **Scrolling**:
  - **PageUp/PageDown**: Navigate through message history.
  - **End**: Jump to bottom of chat (latest messages).
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

#### NIP‑59 Chat Internals (Shared Key Model)

- **Shared key derivation**:
  - When a dispute is taken (`AdminDispute::new`), per-party shared keys are eagerly derived using ECDH: `nostr_sdk::util::generate_shared_key(admin_secret, counterparty_pubkey)`.
  - Two shared keys are stored (as hex) in the `admin_disputes` table: `buyer_shared_key_hex` and `seller_shared_key_hex`.
  - The same derivation is used by `mostro-chat` so both the admin and the counterparty can independently derive the same shared key and subscribe to the same events.

- **Message addressing**:
  - Admin chat messages are addressed to the **shared key's public key** (not the counterparty's trade pubkey directly).
  - The admin reads `admin_privkey` from `settings.toml` to sign the inner rumor; the gift wrap `p` tag targets the shared key pubkey.

- **Sending messages**:
  - Admin messages are sent via `send_admin_chat_message_via_shared_key` (spawned as an async task to avoid blocking the UI):
    - Rumor content: Mostro protocol format `(Message::Dm(SendDm, TextMessage(...)), None)`.
    - The gift wrap is built using `EventBuilder::gift_wrap` with the admin keys and the shared key public key as the recipient.
    - Published to relays without blocking the main UI thread.

- **Receiving messages**:
  - The main loop (every 5 seconds when in Admin mode) calls `spawn_admin_chat_fetch`, which runs `fetch_admin_chat_updates` in a one-off task. A single-flight guard ensures only one fetch runs at a time; overlapping interval ticks skip spawning until the current fetch completes.
  - For each in-progress dispute, the fetch:
    - Rebuilds buyer/seller shared `Keys` from the stored hex.
    - Fetches `GiftWrap` events addressed to each shared key's public key (7-day rolling window).
    - Decrypts each event using the shared key (standard NIP-59 or simplified mostro-chat format).
    - Uses `last_seen_timestamp` to skip already-processed events.
    - Skips events signed by the admin identity to avoid duplicating locally-sent messages.

- **Behavior on restart (Chat Restore at Startup)**:
  - Admin chat uses a **hybrid persistence model** to provide instant UI restore and incremental sync:
    - For each in‑progress dispute, chat transcripts are stored as human‑readable files under:

      ```text
      ~/.mostrix/<dispute_id>.txt
      ```

    - On startup, `recover_admin_chat_from_files`:
      - Reads each existing transcript file.
      - Rebuilds `AppState.admin_dispute_chats` so the Disputes in Progress tab immediately shows previous messages.
      - Computes the latest timestamps per party and updates `AppState.admin_chat_last_seen`.
    - The latest buyer/seller timestamps are also persisted in the `admin_disputes` table (`buyer_chat_last_seen`, `seller_chat_last_seen`) via `update_chat_last_seen_by_dispute_id` so that:
      - Background NIP‑59 fetches only request **newer** events (7-day rolling window).
      - Chat resumes from where it left off without replaying the full history.

#### Exit Confirmation

Mostrix includes a safety feature to prevent accidental exits:

- **Trigger**: Navigate to the Exit tab (User or Admin) and confirm
- **Popup**: Shows confirmation dialog with "Are you sure you want to exit Mostrix?"
- **Navigation**: Use Left/Right arrows to select Yes/No buttons
- **Confirmation**: Press Enter to confirm exit, or Esc to cancel
- **Visual**: Green "✓ YES" button and red "✗ NO" button with clear styling

**Source**: `src/ui/exit_confirm.rs`, `src/ui/key_handler/enter_handlers.rs`
