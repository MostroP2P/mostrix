# Admin Dispute Finalization

## Overview

This document describes how admins finalize disputes in Mostrix after reviewing the case and communicating with the buyer and seller.

### Implementation status (anti-abuse bond slash)

| Layer | Status | Notes |
|-------|--------|--------|
| **`mostro-core` 0.11.3** | Done | `BondResolution`, `Payload::BondResolution`, `CantDoReason::InvalidPayload` |
| **`BondSlashChoice`** | Done | [`src/util/order_utils/bond_resolution.rs`](../src/util/order_utils/bond_resolution.rs) — wire mapping + unit tests |
| **Execute layer** (`execute_admin_settle` / `cancel`) | Done | Accepts `BondSlashChoice`; uses `to_optional_payload()` on the wire |
| **TUI** (slash picker + confirm summary) | Done | Inline bond button + overlay; confirm shows `bond.label()` recap |
| **`bond_enabled` gating** (kind 38385) | Pending | Skip slash UI when instance bonds are off |

Protocol references: [Admin Settle](https://mostro.network/protocol/admin_settle_order.html), [Admin Cancel](https://mostro.network/protocol/admin_cancel_order.html). Daemon bond payout (`Action::AddBondInvoice`, Mostro PR [#738](https://github.com/MostroP2P/mostro/pull/738)) is documented under trade flows, not admin finalization.

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
5. **Open Finalization**: Press Shift+F to open finalization popup
6. **Review Full Details**: Popup shows complete dispute information
7. **Choose actions on one popup** (Left/Right): **💰 Pay buyer** (`AdminSettle`), **↩️ Refund seller** (`AdminCancel`), or **Bond** (body shows [`BondSlashChoice::label()`](../src/util/order_utils/bond_resolution.rs), default 🔓 *No bond slash*). **Esc** closes the popup (no separate Exit button).
8. **Bond slash submenu** (optional): With **Bond** focused, **Enter** opens overlay **⚔️ Bond resolution**; ↑/↓ among four labeled choices; **Enter** applies; **Esc** closes submenu only.
9. **Confirm**: **Enter** on pay/refund opens Yes/No — title e.g. `⚠️ Confirm 💰 Pay buyer`, description + **Bond:** recap (`bond.label()`).
10. **Execute**: **Enter** on Yes — sends encrypted DM to Mostro.

**Current UI:** single finalize popup (7–8) → confirm with bond recap (9) → execute (10). `bond_enabled` gating (hide bond button) still pending.

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

- **Effect**: Press **Esc** on the finalize popup to return to dispute management without taking action
- **Use Case**: Need more information, want to continue chatting with parties

### Bond resolution (anti-abuse bonds)

Independent of settle vs cancel: the admin chooses whether to **slash** posted anti-abuse bonds. Valid on both `admin-settle` and `admin-cancel` only.

| Choice | TUI label (`label()`) | `slash_seller` | `slash_buyer` | When to use |
|--------|----------------------|----------------|---------------|-------------|
| No bond slash | 🔓 No bond slash | false | false | Release bonds; no penalty |
| Slash buyer bond | ⚔️ Slash buyer bond | false | true | Buyer at fault (e.g. false claim on sell order) |
| Slash seller bond | ⚔️ Slash seller bond | true | false | Seller at fault |
| Slash both bonds | ⚔️ Slash both bonds | true | true | Both parties violated rules |

Mostrix maps these via [`BondSlashChoice`](../src/util/order_utils/bond_resolution.rs): `to_optional_payload()` sends `payload: null` for **no slash** and `Payload::BondResolution` only when a side is slashed. Use `to_payload()` if you need an explicit `{false, false}` object (same server semantics as null).

If the daemon rejects a slash (e.g. side has no bond row), Mostro may reply with `CantDo(InvalidPayload)` — surfaced as *"Invalid payload - check bond slash choices or message format"* ([`get_cant_do_description`](../src/util/types.rs)).

After a slash, the non-slashed party may receive `Action::AddBondInvoice` to claim their share of the bond (see Mostro anti-abuse bond spec / PR #738); that is handled on the **trader** path, not in the admin finalization popup.

## UI Components

### Finalization Popup

The popup displays comprehensive dispute information:

**Header Section**:

- Order ID (full UUID) - the order associated with this dispute
- Dispute ID (full UUID) - the unique dispute identifier
- Dispute type and status
- Creation and Taken timestamps

> **Note**: The UI displays both Order ID and Dispute ID. Previous documentation only mentioned "Dispute ID (full UUID)" which was incomplete. ✅ **Resolved in this PR**.

**Parties Section**:

- Buyer information: pubkey (truncated), role indicator (🟢 BUYER), privacy status ("Privacy: Yes/No"), rating with operating days
- Seller information: pubkey (truncated), role indicator (🔴 SELLER), privacy status ("Privacy: Yes/No"), rating with operating days
- Initiator indicator (shows "(Initiator)" suffix on the party who started the dispute)

> **Note**: Privacy status is displayed as text labels "Yes" or "No" (not emoji indicators). The emojis (🟢/🔴) are used for role indicators (BUYER/SELLER), not privacy. Previous documentation described "privacy status (🟢 info available / 🔴 private)" which was incorrect. ✅ **Resolved in this PR**.

**Financial Section**:

- Amount in satoshis
- Fiat amount with currency code (e.g., "1000 USD")
- Premium percentage
- Payment method (if available)

> **Note**: The fiat currency code IS displayed alongside the amount. Previous documentation listed "Fiat amount and currency" but did not clarify the format. ✅ **Confirmed working**.

**Action Buttons** (three columns, Left/Right focus):

| Button | When selected | Enter |
|--------|---------------|-------|
| **💰 Pay buyer** | Green highlight | Open confirm (settle) |
| **↩️ Refund seller** | Red highlight | Open confirm (cancel) |
| **Bond** | Primary highlight; body = `bond.label()` | Open bond overlay submenu |

Finalized disputes: pay/refund buttons are dimmed (body `—`); use **Esc** to leave. Bond focus may remain on index 2 for display.

### Keyboard Navigation

**In Dispute List**:

- Up/Down: Select dispute in sidebar
- Tab: Switch between buyer/seller chat party
- Shift+I: Toggle chat input enabled/disabled
- Type: Start typing message in input box (when input enabled)
- Enter: Send message
- Shift+F: Open finalization popup
- PageUp/PageDown: Scroll through chat history
- End: Jump to bottom of chat (latest messages)
- Backspace: Delete characters from input (when input enabled)

**In Finalization Popup**:

- Left/Right: Navigate 💰 Pay buyer | ↩️ Refund seller | Bond
- Enter on Pay/Refund: Open confirmation
- Enter on Bond slash: Open bond submenu overlay
- Esc: Close popup (or close submenu first if open)

**In Bond Slash Submenu (overlay)**:

- Up/Down: Highlight choice (no slash, slash buyer, slash seller, slash both)
- Enter: Apply choice and return to main finalize popup
- Esc: Close submenu without applying

## Protocol Details

### Message Structure

Both finalization actions use `Message::new_dispute` with the **order** UUID (from `admin_disputes.id`), not the dispute UUID:

```rust
use crate::util::order_utils::BondSlashChoice;

let bond = BondSlashChoice::SlashBuyer; // example

Message::new_dispute(
    Some(order_id),
    None,
    None,
    Action::AdminSettle, // or AdminCancel
    bond.to_optional_payload(), // None → null; slash variants → BondResolution
)
```

Example JSON (settle + slash buyer), per [protocol](https://mostro.network/protocol/admin_settle_order.html):

```json
{
  "dispute": {
    "version": 1,
    "id": "<order-uuid>",
    "action": "admin-settle",
    "payload": {
      "bond_resolution": {
        "slash_seller": false,
        "slash_buyer": true
      }
    }
  }
}
```

> **Note:** Mostrix serializes `Message::Dispute` (not the `order` wrapper shown in some protocol examples); `mostro-core` accepts both shapes on decode. The `id` field is always the **order** id.

Internally, Mostrix:

- Looks up the dispute in the local `admin_disputes` table by its **dispute_id**.
- Reads the corresponding **order ID** from the `id` column.
- Uses that order ID as the first parameter of `Message::new_dispute`, matching what Mostro expects for finalization actions.

### Execute API

Call chain from the TUI (today):

1. [`execute_finalize_dispute_action`](../src/ui/key_handler/admin_handlers.rs) — spawns async task with `bond` from `ConfirmFinalizeDispute` (chosen on the finalize popup / overlay).
2. [`execute_finalize_dispute`](../src/util/order_utils/execute_finalize_dispute.rs) — DB guards, then dispatches settle or cancel with the same `bond`.
3. [`execute_admin_settle`](../src/util/order_utils/execute_admin_settle.rs) / [`execute_admin_cancel`](../src/util/order_utils/execute_admin_cancel.rs) — `Message::new_dispute(..., bond.to_optional_payload())`, logs include `bond.log_context()`.

Success toasts use the same bond phrase (e.g. `settled (buyer paid) (no bond slash)`).

### UI state (`AdminMode`)

Finalize flow uses a single [`ReviewingDisputeForFinalization`](../src/ui/admin_state.rs) mode (no separate full-screen bond step):

- `dispute_id`, `selected_button_index` (0=pay, 1=refund, 2=bond)
- `bond: BondSlashChoice` (default `None`)
- `slash_submenu_open`, `slash_submenu_index` — overlay while picking bond

Confirm: [`ConfirmFinalizeDispute`](../src/ui/admin_state.rs) carries `is_settle`, `bond`, `selected_button` (Yes/No).

### Confirmation popup

Rendered by [`dispute_finalization_confirm.rs`](../src/ui/dispute_finalization_confirm.rs): outcome title with emoji, short description, **Bond:** line with `bond.label()`, Yes/No. **Esc** or **No** returns to finalize popup preserving `bond`.

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
- **Data integrity error**: Missing required fields (buyer_pubkey or seller_pubkey)

All errors are displayed in a result popup with appropriate error messages. The finalization popup includes robust error handling:

- **Dispute Not Found**: If a dispute ID is invalid or the dispute is no longer available, a clear error popup is displayed with the dispute ID and instructions to close it (Press ESC or ENTER).
- **Data Integrity Error**: If a dispute is missing required fields (`buyer_pubkey` or `seller_pubkey`), a dedicated error popup is displayed explaining that the database entry is incomplete and the dispute cannot be finalized. This validation happens both when taking a dispute (prevents saving incomplete data) and when viewing the finalization popup.
- **User-Friendly Messages**: All error messages are descriptive and help users understand what went wrong.
- **Safe Display**: Dispute IDs and other data are safely truncated to prevent display issues with unexpected data lengths.

**Source**: `src/ui/dispute_finalization_popup.rs:22`, `src/models.rs` (AdminDispute::new validation)

## Chat System

### Features

The chat interface provides real-time communication with dispute parties:

**Visual Design**:

- **Color-coded senders**: Each message displays a header in the format "Sender - date - time" where the sender name is color-coded:
  - Cyan: Admin messages
  - Green: Buyer messages
  - Red: Seller messages
- **Dynamic input box**: Automatically grows from 1 to 10 lines based on message length
- **Focus indicators**: Bold yellow border when typing, gray when inactive
- **Chat history**: Scrollable message history per dispute

**Message Management**:

- **Per-dispute storage**: Each dispute has its own chat history (stored in `admin_dispute_chats`)
- **Party filtering**: Messages are filtered by the active chat party:
  - **Admin messages**: Only shown in the chat view of the party they were sent to (tracked via `target_party` field)
  - **Buyer messages**: Only shown when viewing the Buyer chat
  - **Seller messages**: Only shown when viewing the Seller chat
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
- **Send behavior**: Enter sends message, Shift+F opens finalization popup
- **Clear on send**: Input automatically clears after sending

### Chat Footer

The footer shows context-sensitive shortcuts:

**When typing (input enabled)**:

```text
Tab: Switch Party | Enter: Send | Shift+I: Disable | Shift+F: Finalize | PgUp/PgDn: Scroll | End: Bottom | ↑↓: Select Dispute
```

**When typing (input disabled)**:

```text
Tab: Switch Party | Shift+I: Enable | Shift+F: Finalize | PgUp/PgDn: Scroll | ↑↓: Navigate Chat | End: Bottom | ↑↓: Select Dispute
```

**When not typing**:

```text
Tab: Switch Party | Shift+F: Finalize | ↑↓: Select Dispute | PgUp/PgDn: Scroll Chat | End: Bottom
```

## Best Practices

1. **Always chat first**: Communicate with both parties before finalizing
2. **Review all evidence**: Check chat history, payment proofs, timestamps
3. **Consider reputation**: Factor in user ratings and operating days (shown in header)
4. **Document reasoning**: All chat messages are stored per dispute for review
5. **Be impartial**: Base decisions on facts, not party behavior alone
6. **Check privacy**: Privacy labels ("Yes" = private mode / "No" = public mode) indicate whether user info may be limited
7. **Switch parties**: Use Tab to alternate between buyer and seller chats
8. **Scroll history**: Use PageUp/PageDown to review full conversation history, or End to jump to latest
9. **Toggle input**: Use Shift+I to disable input when navigating to prevent accidental typing
10. **Monitor scrollbar**: Visual scrollbar on the right shows your position in the chat history

## Related Files

- `src/util/order_utils/bond_resolution.rs` - `BondSlashChoice`, `Payload::BondResolution` mapping, wire tests
- `src/ui/dispute_finalization_popup.rs` - Finalize popup (💰 pay / ↩️ refund / Bond + overlay trigger)
- `src/ui/dispute_bond_slash_popup.rs` - `render_bond_slash_overlay` on finalize popup
- `src/ui/dispute_finalization_confirm.rs` - Yes/No confirm with bond recap
- `src/util/order_utils/execute_admin_settle.rs` - AdminSettle + `BondSlashChoice` payload
- `src/util/order_utils/execute_admin_cancel.rs` - AdminCancel + `BondSlashChoice` payload
- `src/util/order_utils/execute_finalize_dispute.rs` - DB checks + dispatches settle/cancel
- `src/ui/disputes_in_progress_tab.rs` - Main disputes UI with chat interface
- `src/ui/key_handler/enter_handlers.rs` - Enter key handling and chat message sending
- `src/ui/key_handler/mod.rs` - Chat input handling and clipboard operations
- `src/ui/mod.rs` - AppState with chat storage (DisputeChatMessage, ChatSender)
- `src/models.rs` - AdminDispute data model

## See Also

- [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) - Admin dispute management overview
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) - Mostro protocol details
- [TUI_INTERFACE.md](TUI_INTERFACE.md) - General UI navigation
