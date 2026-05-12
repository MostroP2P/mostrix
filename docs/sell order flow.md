# Sell order flow (spec + implementation notes)

This document describes the **sell listing** trade lifecycle in Mostrix terms: **maker** (seller who published the order) vs **taker** (buyer who took it), protocol alignment, and how the **Messages** tab timeline stepper maps phases. The [Mostro protocol](https://github.com/MostroP2P/protocol) remains authoritative when this doc disagrees.

Related docs:

- [buy order flow.md](buy%20order%20flow.md) (buy listing counterpart)
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md)

## Scope and definitions

- **Sell listing**: an order on the book with `kind: sell` (`mostro_core::order::Kind::Sell`).
- **Maker (sell listing)**: the local user **created** the sell order (seller).
- **Taker (sell listing)**: the local user **took** someone else’s sell order (buyer in trade terms).

## Sources of truth (stepper and future Enter gating)

Same as buy listings: **`order.status`**, message **`action`**, **`is_mine`** (maker vs taker), and **`order_kind`**. See [buy order flow.md](buy%20order%20flow.md#sources-of-truth-ui-and-future-stepper).

## Happy path: sell listing as maker (seller)

High-level phases (aligned with UI labels in **`SELL_ORDER_FLOW_STEPS_MAKER`** in `src/ui/constants.rs`):

1. **Wait for buyer** — early coordination / hold path per daemon.
2. **Pay hold invoice** — seller satisfies Lightning hold-invoice requirements.
3. **Chat with buyer** — active coordination.
4. **Wait for fiat** — buyer sends fiat off-chain.
5. **Release sats** — seller releases to complete the LN leg.
6. **Rate counterparty**.

Typical `Status` values follow the same global order machine as other trades (`waiting-payment`, `waiting-buyer-invoice`, `active`, `fiat-sent`, `success`, …); validate against your mostrod and [ORDER.md](https://github.com/MostroP2P/protocol/blob/main/ORDER.md).

## Happy path: sell listing as taker (buyer)

Phases (labels from `SELL_ORDER_FLOW_STEPS_TAKER`):

0. **Pay anti-abuse bond (taker / buyer)** — *Mostro Phase 1.5+ only, configurable in mostrod*: when the daemon has bonds enabled, the first DM after `take-sell` is `pay-bond-invoice` with `Status::WaitingTakerBond`. Mostrix opens the dedicated **🛡️ Anti-abuse Bond Invoice** popup (`render_pay_bond_invoice` in `src/ui/message_notification.rs`); bond is **locked, not spent** and refunded on normal completion. Unlike the buy-listing taker, a sell-listing taker only ever receives **`PayBondInvoice`** here (never `PayInvoice`). When bonds are **disabled** on the daemon, this phase is skipped and the flow starts directly at step 1 — Mostrix never assumes a bond exists.
1. **Add invoice** — buyer submits a **BOLT11** payment request or a **Lightning address** (`user@domain.com`) when required; Mostrix verifies LNURL-pay metadata (`payRequest`) before sending `AddInvoice` for addresses. Optional default address: **User → Settings → Set Lightning Address** (`settings.toml` field `ln_address`). When that field is non-empty, this phase may show **`ConfirmSavedLnAddressForInvoice`** first (**YES** = immediate **`AddInvoice`** with saved address via **`submit_add_invoice`**; **NO** = manual invoice popup); see **`notifications_ch_mng.rs`**.
2. **Wait for seller** — seller pays hold / completes prerequisites.
3. **Chat with buyer** — messaging phase (label uses “Buyer” from book side).
4. **Send fiat** — buyer sends fiat.
5. **Wait for sats** — settlement / release.
6. **Rate counterparty**.

## Implementation notes (non-normative)

- **Timeline step resolution** (`src/ui/orders.rs`): **`message_trade_timeline_step`** dispatches on **`order_kind`**. For **`Kind::Sell`**, **`sell_listing_flow_step`** returns **`FlowStep::SellFlowStep(StepLabelsSell)`** with the same pipeline as buy: early **`Action::Rate`** / **`RateReceived`**, then **`listing_step_from_status(Kind::Sell, status)`**, then **`sell_listing_flow_step_from_action`** (maker = seller, taker = buyer). **`Status::Success`** still does not pick step 6 by itself; rate is action-driven. **`Status::WaitingTakerBond`** maps to `StepBuyerInvoice` for sell listings (and `StepSellerPayment` for buy listings) — same pre-trade column as `Pending` / `WaitingPayment` — so the timeline does not jump while the taker is reviewing the bond popup.
- **Labels**: **`listing_timeline_labels`** chooses **`SELL_ORDER_FLOW_STEPS_MAKER`** / **`SELL_ORDER_FLOW_STEPS_TAKER`** from **`src/ui/constants.rs`** when **`order_kind == Sell`**; column **indices** come from **`StepLabelsSell`** (see `orders.rs`).
- **Tests**: `timeline_step_tests` in `src/ui/orders.rs` cover representative sell maker/taker and status cases.

## Open questions

- Fine-grained **status → step** parity for sell vs buy if real DM traces show divergences (split **`listing_step_from_status`** into kind-specific helpers only if needed).
- Messages **Enter** phase-gating for sell (same goals as buy; not fully implemented).
