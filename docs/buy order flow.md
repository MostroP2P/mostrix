# Buy order flow (first spec)

This document describes the **buy listing** trade lifecycle in Mostrix terms: **maker** vs **taker**, protocol alignment, cancellation expectations, and **target** behavior for the **Messages** tab (Enter key and popups). It is a **draft for review**; the [Mostro protocol](https://github.com/MostroP2P/protocol) is authoritative when this doc disagrees.

## Scope and definitions

- **Buy listing**: an order on the book with `kind: buy` (see `mostro_core::order::Kind::Buy`, serialized as `buy`).
- **Maker (buy listing)**: the local user **created** the buy order.
- **Taker (buy listing)**: the local user **took** another party’s buy order (in practice this is less common phrasing; here we mean the flow where the counterparty sequence matches the **taker** column below). Adjust naming in review if you prefer “seller takes buy” vs “buyer is maker” only.
- **Counterparty**: the other side of the trade (buyer or seller role in the fiat/LN sense, per protocol).

Related docs:

- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md)
- Protocol: [ORDER.md](https://github.com/MostroP2P/protocol/blob/main/ORDER.md), [ACTIONS.md](https://github.com/MostroP2P/protocol/blob/main/ACTIONS.md)

## Sources of truth (UI and future stepper)

When deciding **which step** the user is in, or whether **Enter** may open an invoice or confirmation popup:

1. **Primary**: `order.status` from the Mostro payload or local DB (`mostro_core::order::Status`, kebab-case strings such as `waiting-payment`, `waiting-buyer-invoice`, `active`, `fiat-sent`, `success`).
2. **Secondary**: the message **`action`** when `Payload::Order` is missing or incomplete (e.g. peer-only payloads).
3. **Role context**: maker vs taker and listing `kind: buy` must be applied together; **action alone is not sufficient** for correct gating once this spec is implemented.

Status strings match `mostro_core` (examples): `pending`, `waiting-payment`, `waiting-buyer-invoice`, `active`, `fiat-sent`, `success`, `cooperatively-canceled`, `canceled`, `dispute`, …

## Happy path: buy listing as maker

High-level phases (who acts):

1. **Waiting-payment (seller)** — counterparty (seller) satisfies the hold-invoice / payment side as required by the daemon.
2. **Add-invoice (buyer)** — buyer (typically the maker on a buy listing) submits their Lightning invoice via `add-invoice`.
3. **Send fiat (buyer)** — buyer marks fiat sent (`fiat-sent`) when appropriate.
4. **Release (seller)** — seller releases sats (`release` / related actions).
5. **Rate counterpart** — rating step after successful completion.

Typical status alignment (review against live mostrod):

| Phase (human) | Typical `Status` (kebab-case) | Notes |
|---------------|-------------------------------|--------|
| Seller pays / hold path | `waiting-payment` | Maps to seller-side payment; exact transitions depend on daemon. |
| Buyer pastes invoice | `waiting-buyer-invoice` | Often paired with `add-invoice` action in DMs. |
| Trade live | `active` | After LN + invoice setup per protocol. |
| Fiat marked sent | `fiat-sent` | Buyer action. |
| Done | `success` | Then rate per protocol. |

## Happy path: buy listing as taker

High-level phases:

0. **Pay anti-abuse bond (taker / seller)** — *Mostro Phase 1.5+ only, configurable in mostrod*: when the daemon has bonds enabled, the taker (acting as seller on a buy listing) first receives a `pay-bond-invoice` DM with `Status::WaitingTakerBond`. The bond is **locked, not spent** and is refunded on normal trade completion. Mostrix opens the dedicated **🛡️ Anti-abuse Bond Invoice** popup (`render_pay_bond_invoice`). When bonds are **disabled** on the daemon, this phase is skipped and the flow starts directly at step 1 — Mostrix never assumes a bond exists.
1. **Pay-invoice (seller)** — seller pays hold invoice when prompted (`pay-invoice`).
2. **Waiting-buyer-invoice (buyer)** — buyer provides invoice (`add-invoice` / `waiting-buyer-invoice` status).
3. **Send fiat (buyer)** — buyer sends fiat (`fiat-sent`).
4. **Release (seller)** — seller releases.
5. **Rate counterpart**.

Same status vocabulary applies; **order** of states must match [ORDER.md](https://github.com/MostroP2P/protocol/blob/main/ORDER.md) for your mostrod version. `waiting-taker-bond` is the new Phase 1.5+ state and maps to NIP-69 `pending` for external visibility.

## Cancellation (first draft)

- **Before `active`**: protocol generally allows **unilateral** cancel in early phases (each party may cancel without peer agreement). Wording and exact rules are **subject to protocol review**; implement only what mostrod enforces.
- **At `active`**: once the trade is **active** (hold invoice path and buyer invoice in place per daemon), cancellation should move toward **cooperative cancel** (both sides agree). Document UI entry points in the Messages tab (see below).
- **Dispute**: `dispute` / admin-driven terminal states are out of scope for this subsection except as **non-happy-path** exits.

## TUI: Messages tab — Enter key and popups (normative target)

**Scope:** This section mixes **normative targets** with **current behavior** where implemented (see Implementation notes).

**Surface:** **Messages tab only** for phase-gated invoice, confirmation, cancel, and dispute flows. The **Orders** tab remains “take order” on Enter unless a future task changes it.

### Ground rules

- Decide what Enter does using **`order.status` + maker/taker + listing kind (`buy`)**, falling back to **`action`** when the order payload is absent.
- **Do not** open **PayInvoice** or **AddInvoice** popups unless the **local user** is the party that must act in that phase.

### Invoice popups (paste vs pay)

- **AddInvoice** (paste **BOLT11** or **Lightning address**): open only when the **buyer** must submit an invoice and status/action indicates that step for the **local** user. For addresses, Mostrix checks the LNURL endpoint before publishing the DM. An optional saved buyer address lives in **`settings.toml`** (`ln_address`) and is editable from **User → Settings** only. If **`ln_address`** is set, **`AddInvoice`** may open **`ConfirmSavedLnAddressForInvoice`** first — **YES** auto-sends **`AddInvoice`** with that address (**`submit_add_invoice`**); **NO** opens manual invoice entry; see **`present_add_invoice_popup`** / **`apply_saved_ln_address_invoice_choice`** in `src/util/dm_utils/notifications_ch_mng.rs`.
- **PayInvoice** (pay hold invoice): open only when the **seller** must pay and that matches the **local** user in the current phase.
- **PayBondInvoice** (Mostro Phase 1.5+ anti-abuse bond, **optional in mostrod**): open only when the **taker** must lock a bond and `order.status` is `WaitingTakerBond` (or `None` for pre-status DMs). Distinct popup — shield title, "Locked, not spent — refunded on normal completion" disclaimer — and **Primary = Acknowledge** (no DM follow-up; bond is paid in the wallet). Cancel from this popup still sends `Action::Cancel` per Mostro Phase 1.5+ spec.
- If Enter is pressed but the phase does **not** match, **do not** open the invoice modal; show a short informational message or no-op.

### Confirmation actions

For actions that require explicit confirmation (e.g. **`HoldInvoicePaymentAccepted`**, **`FiatSentOk`** / release-style flows), Enter should open the **confirmation** UI (e.g. `ViewingMessage` with yes/no), **not** a generic `OperationResult::Info` line.

### Cancel and dispute from `active`

- From **`active`** (and adjacent states if the protocol allows), Enter should offer paths toward **cooperative cancel** and **dispute** (e.g. submenu or dedicated mode), not only passive info.
- **Before `active`**, unilateral cancel is reachable from the same Messages popup surface for invoice/payment phases (see current implementation pointer below).

### Current implementation pointer (non-normative)

In **`src/ui/key_handler/enter_handlers.rs`**, Messages **Enter** is routed by **`Action`** (and by **`order_id`** where required):

- **`AddInvoice` / `PayInvoice`** → invoice / payment notification popup (`NewMessageNotification`) after any saved-address branch; **`AddInvoice`** may run **`ConfirmSavedLnAddressForInvoice`** first when **`ln_address`** is configured (`present_add_invoice_popup`), and **YES** there skips the popup and submits via **`submit_add_invoice`** (`message_handlers.rs`).
- **`PayBondInvoice`** (Phase 1.5+) → dedicated anti-abuse bond popup (`render_pay_bond_invoice` in `src/ui/message_notification.rs`). Wired through the same `NewMessageNotification` UI mode but with distinct chrome and **Acknowledge** as the primary action. The take-order sync path (`src/util/order_utils/take_order.rs`) also forwards this directly when Mostro's first reply is `PayBondInvoice`, carrying the action through `OperationResult::PaymentRequestRequired { action, … }`.
- **`WaitingBuyerInvoice` / `WaitingSellerToPay`** also map to the same invoice/payment popup modes on Enter.
- Invoice/payment popup action model now includes **primary action + `Cancel Order`** (Left/Right select, Enter confirm), so pre-active cancel is directly available from the popup (also valid during `WaitingTakerBond`).
- **`HoldInvoicePaymentAccepted` / `FiatSentOk`** → confirmation popup (`ViewingMessage` with yes/no where applicable).
- **`Rate`** → **rating popup** (`UiMode::RatingOrder`): choose **1–5** stars, **Enter** submits **`RateUser`** + **`RatingUser`** via **`execute_rate_user`** in `src/util/order_utils/execute_send_msg.rs` (Mostro resolves the counterparty; only **`order_id`** + rating are sent).
- **Else** → informational `OperationResult::Info` (no send).

**Gaps vs this spec:** Enter is still **not** fully phase-aware (e.g. invoice popups are not gated by **`order.status` + local role** alone). Cooperative cancel / dispute entry from Messages as described above remains **TBD**.

## Implementation notes (non-normative)

- **Trade timeline step** (`message_trade_timeline_step` in `src/ui/orders.rs`): returns **`FlowStep`** — either **`BuyFlowStep(StepLabelsBuy)`** or **`SellFlowStep(StepLabelsSell)`**. Inner enums use **`repr(u8)`** discriminants: **`StepPendingOrder = 0`** (no highlighted column — all steps gray) for **`Status::Pending`** / **`WaitingTakerBond`**; phases **1…6** for active trade steps. **Sell** swaps the first two payment columns vs buy (`StepLabelsSell`: `StepBuyerInvoice` = 1, `StepSellerPayment` = 2; see `src/ui/orders.rs`).
- **Pipeline:** **`buy_listing_flow_step`** / **`sell_listing_flow_step`** → early **`Action::Rate`** / **`RateReceived`** → **`listing_step_from_status(order_kind, status)`** (same Mostro statuses, **kind-specific** mapping to columns) → **`buy_listing_flow_step_from_action`** / **`sell_listing_flow_step_from_action`**. **`Status::Success`** does **not** pick step 6 alone; **`Action::Rate`** / **`RateReceived`** run before status so **`rate`** + **`success`** still highlights rate.
- **Text labels** (top/bottom lines per column): **`src/ui/constants.rs`** — **`BUY_ORDER_FLOW_STEPS_*`**, **`SELL_ORDER_FLOW_STEPS_*`**, **`GENERIC_ORDER_FLOW_STEPS_TAKER`**; **`listing_timeline_labels`** in `orders.rs` picks the array by **`order_kind`** and **`is_mine`**. Rendering: **`src/ui/tabs/message_flow_tab.rs`**.
- **Follow-up:** stricter **Enter** / popups (status + role + **`kind`**); see [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md#messages-tab-trade-timeline-stepper-buy-and-sell-listings). Sell detail: [sell order flow.md](sell%20order%20flow.md).

## Open questions (for review)

- Exact **status** sequence for buy-maker vs buy-taker on your mostrod version vs [ORDER.md](https://github.com/MostroP2P/protocol/blob/main/ORDER.md).
- Whether **`settled-hold-invoice`** or **`in-progress`** appear in your DMs and how they should appear in the UI.
- Dispute and **cooperative cancel** action names and when they are valid from the client.
- Confirm **taker** column wording for “user took a buy listing” vs other trade constructions.
