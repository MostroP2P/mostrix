# Admin Dispute Finalization

## Overview

This document describes how admins finalize disputes in Mostrix after reviewing the case and communicating with the buyer and seller.

## User Flow

1. **Navigate to Disputes**: Admin opens the "Disputes in Progress" tab
2. **Select Dispute**: Use Up/Down arrows to select a dispute from the left sidebar
3. **Review Details**: View dispute information in the header (parties, amounts, ratings, privacy)
4. **Chat with Parties**:
   - Use Tab to switch between buyer and seller chat views
   - Press Shift+I to enable/disable chat input (prevents accidental typing)
   - Type messages directly in the input box (when input enabled)
   - Press Enter to send messages
   - Use PageUp/PageDown to scroll through chat history
   - Press End to jump to bottom of chat (latest messages)
   - Visual scrollbar on the right shows position in chat history
5. **Open Finalization**: Press Enter when input is empty to open finalization popup
6. **Review Full Details**: Popup shows complete dispute information
7. **Choose Action**: Use Left/Right arrows to select action button
8. **Execute**: Press Enter to execute the selected action

## Finalization Actions

### Pay Buyer (AdminSettle)

- **Protocol Action**: `Action::AdminSettle`
- **Effect**: Settles the dispute in favor of the buyer
- **Result**: Buyer receives the full amount from escrow
- **Use Case**: When buyer's claim is valid (e.g., seller didn't deliver, scam attempt)

### Refund Seller (AdminCancel)

- **Protocol Action**: `Action::AdminCancel`
- **Effect**: Cancels the order and refunds the seller
- **Result**: Seller receives the full amount back from escrow
- **Use Case**: When seller's position is valid (e.g., buyer false claim, buyer unresponsive)

### Exit

- **Effect**: Returns to dispute management without taking action
- **Use Case**: Need more information, want to continue chatting with parties

## UI Components

### Finalization Popup

The popup displays comprehensive dispute information:

**Header Section**:

- Dispute ID (full UUID)
- Dispute type and status
- Creation timestamp

**Parties Section**:

- Buyer information: pubkey (truncated), role, privacy status (🟢 info available / 🔴 private), rating with operating days
- Seller information: pubkey (truncated), role, privacy status (🟢 info available / 🔴 private), rating with operating days
- Initiator indicator (shows who started the dispute)

**Financial Section**:

- Amount in satoshis
- Fiat amount and currency
- Premium percentage
- Fee amounts

**Action Buttons**:

- Pay Buyer (Full) - Green background when selected
- Refund Seller (Full) - Red background when selected
- Exit - Gray/default when selected

### Keyboard Navigation

**In Dispute List**:

- Up/Down: Select dispute in sidebar
- Tab: Switch between buyer/seller chat party
- Shift+I: Toggle chat input enabled/disabled
- Type: Start typing message in input box (when input enabled)
- Enter: Send message (if input has text) or Open finalization popup (if input is empty)
- PageUp/PageDown: Scroll through chat history
- End: Jump to bottom of chat (latest messages)
- Backspace: Delete characters from input (when input enabled)

**In Finalization Popup**:

- Left/Right: Navigate between action buttons (cycles through 3 buttons)
- Enter: Execute selected action
- Esc: Cancel and return to dispute list

## Protocol Details

### Message Structure

Both finalization actions use the same message structure:

```rust
Message::new_dispute(
    Some(dispute_id),  // UUID of the dispute
    None,              // No order_id needed
    None,              // No trade_index needed
    action,            // AdminSettle or AdminCancel
    None               // No payload needed
)
```

### Authentication

- Uses admin private key from settings
- Sent via encrypted DM to Mostro daemon
- Admin keys must be configured in `settings.toml`

### Expected Responses

After sending a finalization action, Mostro should respond with:

- Success confirmation
- Updated dispute status
- Transaction details

## Database Updates

After successful finalization:

1. Dispute status updated in local database
2. Dispute may be moved to "resolved" list
3. Local dispute cache refreshed

## Error Handling

Possible error scenarios:

- Mostro daemon unresponsive
- Invalid admin credentials
- Dispute already finalized
- Network/relay issues
- Dispute not found (e.g., dispute was removed or ID is invalid)

All errors are displayed in a result popup with appropriate error messages. The finalization popup includes robust error handling:

- **Dispute Not Found**: If a dispute ID is invalid or the dispute is no longer available, a clear error popup is displayed with the dispute ID and instructions to close it (Press ESC or ENTER).
- **User-Friendly Messages**: All error messages are descriptive and help users understand what went wrong.
- **Safe Display**: Dispute IDs and other data are safely truncated to prevent display issues with unexpected data lengths.

**Source**: `src/ui/dispute_finalization_popup.rs:22`

## Chat System

### Features

The chat interface provides real-time communication with dispute parties:

**Visual Design**:

- **Color-coded senders**:
  - Cyan with `▶` prefix: Admin messages
  - Green with `◀` prefix: Buyer messages
  - Red with `◀` prefix: Seller messages
- **Dynamic input box**: Automatically grows from 1 to 10 lines based on message length
- **Focus indicators**: Bold yellow border when typing, gray when inactive
- **Chat history**: Scrollable message history per dispute

**Message Management**:

- **Per-dispute storage**: Each dispute has its own chat history (stored in `admin_dispute_chats`)
- **Party filtering**: Only shows messages from the active chat party (Buyer or Seller)
- **Scroll control**: 
  - PageUp/PageDown to navigate history
  - End key to jump to bottom (latest messages)
  - Visual scrollbar on the right shows position (↑/↓/│/█ symbols)
  - Auto-scrolls to newest after sending
- **Empty state**: Shows "No messages yet" when starting a new conversation

**Input Handling**:

- **Input toggle**: Press Shift+I to enable/disable chat input
  - When disabled, prevents accidental typing while navigating
  - Visual indicator in input title shows enabled/disabled state
  - Input is enabled by default when entering dispute management
- **Text wrapping**: Input wraps at word boundaries, respects available width
- **Character limit**: Grows up to 10 lines, with visual feedback
- **Send behavior**: Enter sends message (or opens finalization if input is empty)
- **Clear on send**: Input automatically clears after sending

### Chat Footer

The footer shows context-sensitive shortcuts:

**When typing (input enabled)**:

```text
Tab: Switch Party | Enter: Send | Shift+I: Disable | PgUp/PgDn: Scroll | End: Bottom | ↑↓: Select Dispute
```

**When typing (input disabled)**:

```text
Tab: Switch Party | Shift+I: Enable | PgUp/PgDn: Scroll | ↑↓: Navigate Chat | End: Bottom | ↑↓: Select Dispute
```

**When not typing**:

```text
Tab: Switch Party | Enter: Finalize | ↑↓: Select Dispute | PgUp/PgDn: Scroll Chat | End: Bottom
```

## Best Practices

1. **Always chat first**: Communicate with both parties before finalizing
2. **Review all evidence**: Check chat history, payment proofs, timestamps
3. **Consider reputation**: Factor in user ratings and operating days (shown in header)
4. **Document reasoning**: All chat messages are stored per dispute for review
5. **Be impartial**: Base decisions on facts, not party behavior alone
6. **Check privacy**: Privacy icons (🟢 info available / 🔴 private) indicate data availability
7. **Switch parties**: Use Tab to alternate between buyer and seller chats
8. **Scroll history**: Use PageUp/PageDown to review full conversation history, or End to jump to latest
9. **Toggle input**: Use Shift+I to disable input when navigating to prevent accidental typing
10. **Monitor scrollbar**: Visual scrollbar on the right shows your position in the chat history

## Related Files

- `src/ui/dispute_finalization_popup.rs` - Popup rendering logic
- `src/util/order_utils/execute_admin_settle.rs` - AdminSettle implementation
- `src/util/order_utils/execute_admin_cancel.rs` - AdminCancel implementation
- `src/ui/disputes_in_progress_tab.rs` - Main disputes UI with chat interface
- `src/ui/key_handler/enter_handlers.rs` - Enter key handling and chat message sending
- `src/ui/key_handler/mod.rs` - Chat input handling and clipboard operations
- `src/ui/mod.rs` - AppState with chat storage (DisputeChatMessage, ChatSender)
- `src/models.rs` - AdminDispute data model

## See Also

- [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) - Admin dispute management overview
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) - Mostro protocol details
- [TUI_INTERFACE.md](TUI_INTERFACE.md) - General UI navigation
