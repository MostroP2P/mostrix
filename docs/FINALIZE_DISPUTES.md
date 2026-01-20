# Admin Dispute Finalization

## Overview

This document describes how admins finalize disputes in Mostrix after reviewing the case and communicating with the buyer and seller.

## User Flow

1. **Navigate to Disputes**: Admin opens the "Disputes in Progress" tab
2. **Select Dispute**: Use Up/Down arrows to select a dispute from the left sidebar
3. **Review Details**: View dispute information in the header (parties, amounts, ratings, privacy)
4. **Chat with Parties**: Use Tab to switch between buyer and seller chat views
5. **Open Finalization**: Press Enter on selected dispute to open finalization popup
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
- Buyer information: pubkey (truncated), role, privacy status, rating
- Seller information: pubkey (truncated), role, privacy status, rating
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
- Up/Down: Select dispute
- Tab: Switch between buyer/seller chat
- Enter: Open finalization popup

**In Finalization Popup**:
- Left/Right: Navigate between action buttons
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

All errors are displayed in a result popup with appropriate error messages.

## Best Practices

1. **Always chat first**: Communicate with both parties before finalizing
2. **Review all evidence**: Check chat history, payment proofs, timestamps
3. **Consider reputation**: Factor in user ratings and operating days
4. **Document reasoning**: Keep notes on why you chose a specific action
5. **Be impartial**: Base decisions on facts, not party behavior alone
6. **Check privacy**: Understand if parties want public or private resolution

## Related Files

- `src/ui/dispute_finalization_popup.rs` - Popup rendering logic
- `src/util/order_utils/execute_admin_settle.rs` - AdminSettle implementation
- `src/util/order_utils/execute_admin_cancel.rs` - AdminCancel implementation
- `src/ui/disputes_in_progress_tab.rs` - Main disputes UI
- `src/ui/key_handler/enter_handlers.rs` - Enter key handling
- `src/models.rs` - AdminDispute data model

## See Also

- [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) - Admin dispute management overview
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) - Mostro protocol details
- [TUI_INTERFACE.md](TUI_INTERFACE.md) - General UI navigation
