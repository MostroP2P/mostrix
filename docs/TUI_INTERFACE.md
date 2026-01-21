# TUI Interface Guide

This guide explains the architecture and implementation of the Mostrix Text User Interface (TUI).

## Core Technologies

Mostrix is built using:

- **[Ratatui](https://github.com/ratatui-org/ratatui)**: For terminal rendering, layouts, and widgets.
- **[Crossterm](https://github.com/crossterm-rs/crossterm)**: For terminal manipulation (raw mode, alternate screen) and input event handling.

## UI Architecture

### 1. AppState: The Single Source of Truth

The `AppState` struct in `src/ui/mod.rs` manages the global state of the interface.

**Source**: `src/ui/mod.rs:415`

```415:424:src/ui/mod.rs
pub struct AppState {
    pub user_role: UserRole,
    pub active_tab: Tab,
    pub selected_order_idx: usize,
    pub mode: UiMode,
    pub messages: Arc<Mutex<Vec<OrderMessage>>>, // Messages related to orders
    pub active_order_trade_indices: Arc<Mutex<HashMap<uuid::Uuid, i64>>>, // Map order_id -> trade_index
    pub selected_message_idx: usize, // Selected message in Messages tab
    pub pending_notifications: Arc<Mutex<usize>>, // Count of pending notifications (non-critical)
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

- **Disputes Pending**: List of disputes waiting to be taken (state: `Initiated`). Admins can select and take ownership.
- **Disputes in Progress**: Complete workspace for managing taken disputes (state: `InProgress`), featuring:
  - Integrated chat system with buyer and seller
  - Comprehensive dispute information header
  - Dynamic message input with text wrapping
  - Chat history with scrolling (PageUp/PageDown)
  - Finalization popup for resolution actions
- **Settings**: Role-specific configuration including:
  - Add Dispute Solver
  - Change Admin Key
  - Manage relays and currency filters

For detailed information about admin dispute resolution workflows, see [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) and [FINALIZE_DISPUTES.md](FINALIZE_DISPUTES.md).

## UI Modes & State Machine

The interface uses a nested state machine defined by `UiMode`, `UserMode`, and `AdminMode`.

**Source**: `src/ui/mod.rs:306`

```306:318:src/ui/mod.rs
pub enum UiMode {
    // Shared modes (available to both user and admin)
    Normal,
    ViewingMessage(MessageViewState), // Simple message popup with yes/no options
    NewMessageNotification(MessageNotification, Action, InvoiceInputState), // Popup for new message with invoice input state
    OrderResult(OrderResult), // Show order result (success or error)

    // User-specific modes
    UserMode(UserMode),

    // Admin-specific modes
    AdminMode(AdminMode),
}
```

### Overlays (Popups)

Popups are implemented by rendering additional widgets on top of the main layout when the `UiMode` is not `Normal`. They are drawn at the end of the `ui_draw` function to ensure they appear as overlays.

**Example**: Rendering the `OrderResult` popup.

```600:603:src/ui/mod.rs
    // Order result popup overlay (shared)
    if let UiMode::OrderResult(result) = &app.mode {
        order_result::render_order_result(f, result);
    }
```

## Navigation & Input Handling

Input handling is centralized in `src/ui/key_handler.rs`.

### Tab Navigation

Users can switch between roles (User/Admin) and tabs using arrow keys.

- **Left/Right**: Switch tabs.
- **Up/Down**: Navigate within lists (Order book, Messages).

### Mode-Specific Dispatch

The `handle_key_event` function dispatches keys based on the current `UiMode`.

**Example**: Handling the `Enter` key.

```934:944:src/ui/key_handler.rs
        KeyCode::Enter => {
            handle_enter_key(
                app,
                orders,
                pool,
                client,
                mostro_pubkey,
                order_result_tx,
            );
            Some(true)
        }
```

### Specialized Input

- **Forms**: Character input and Backspace are handled by `handle_char_input` and `handle_backspace` for fields in `FormState`.
- **Invoices**: `handle_invoice_input` handles text entry for Lightning invoices, including support for bracketed paste mode.
- **Admin Chat**: `handle_admin_chat_input` handles direct text input in the "Disputes in Progress" tab:
  - Takes priority over other input handling (except invoice and key input)
  - Supports direct character input and backspace
  - Dynamic input box that grows from 1 to 10 lines
  - Text wrapping with word boundary detection
- **Copy to Clipboard**: Pressing `C` in a `PayInvoice` notification uses the `arboard` crate to copy the invoice. On Linux, it uses the `SetExtLinux::wait()` method to properly wait until the clipboard is overwritten, ensuring reliable clipboard handling without arbitrary delays.

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

**Source**: `src/ui/mod.rs:482` (`apply_status_color`), `src/ui/disputes_in_progress_tab.rs` (chat colors)

### 5. Admin Chat System

**Status**: ✅ **Fully Implemented**

The admin chat system in the "Disputes in Progress" tab provides real-time communication:

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
}
```

**Storage**: Messages are stored in `AppState.admin_dispute_chats: HashMap<String, Vec<DisputeChatMessage>>` keyed by dispute ID.

#### Features

- **Direct input**: Type immediately without mode switching
- **Dynamic sizing**: Input box grows from 1 to 10 lines based on content
- **Text wrapping**: Intelligent word-boundary wrapping with trim behavior
- **Scrolling**: PageUp/PageDown to navigate message history
- **Party filtering**: Only shows messages from active party (Buyer or Seller)
- **Visual feedback**: Focus indicators, color-coded messages, alignment prefixes

#### Input Handling Priority

The key handler processes input in this order:

1. Invoice input (highest priority, when in invoice mode)
2. Key input (for settings popups)
3. **Admin chat input** (takes priority in Disputes in Progress tab)
4. Other character/form input

**Source**: `src/ui/key_handler/mod.rs:153-156` (`handle_admin_chat_input`)
