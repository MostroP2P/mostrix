# Debug Handoff Notes (DM subscriptions / missing take-order notifications)

## Branch / scope
- Branch: `fix-windows-launch`
- Focus area: `src/util/dm_utils/mod.rs` + take/new order subscription timing
- Goal: ensure take-order/new-order flows always produce DM notifications without missing first events

## Current status
- Subscription model for DM listener is in place (`client.notifications()` + dynamic subscribe commands).
- Runtime logs showed at least one reproducible miss:
  - GiftWrap arrived with an unknown `subscription_id` before/without listener map entry.
  - Listener dropped it previously, causing missing notification.

## Key runtime evidence seen
- Example from `app.log`:
  - `Taking order ... trade index ...`
  - `[dm_listener] Ignoring GiftWrap for unknown subscription_id=...`
  - then later:
    - `[take_order] Sending DM subscription command ...`
    - `[dm_listener] Received subscribe command ...`
    - `[dm_listener] Subscribed GiftWrap: subscription_id=...`

Interpretation: first event can arrive before/under different subscription context than tracked by listener map.

## Instrumentation currently present
- `take_order.rs`:
  - logs mapping of response payload IDs and effective order id
  - logs early and post-response subscription command sends
- `dm_utils/mod.rs` listener:
  - logs command receipt
  - logs successful subscribe + subscription_id
  - logs unknown subscription_id events
  - logs routed/parsed/handled messages
  - logs terminal-status cleanup

## Changes already applied during debug
1. **Early subscribe in `take_order`**
   - subscription command sent immediately after deriving trade key/index, before waiting for Mostro reply.
2. **Unknown-subscription fallback path in listener**
   - for GiftWrap with unknown `subscription_id`, try active trade keys and parse/decrypt.
   - if parse succeeds, route message to matched `(order_id, trade_index)`.
3. **Additional hardening from earlier cycles**
   - lock-order deadlock fix (`messages` vs `pending_notifications`)
   - keep latest per-order message row correctly when same timestamp/different action
   - `wait_for_dm` filters by `subscription_id` instead of `event.pubkey`
   - terminal-status cleanup unsubscribes/removes tracked order

## What to test next (first thing tomorrow)
1. Clear `app.log`.
2. Run app, reproduce take-order notification miss.
3. Inspect `app.log` for this sequence:
   - `[take_order] Early subscribe command ...`
   - `[dm_listener] Received subscribe command ...`
   - `[dm_listener] Subscribed GiftWrap ...`
   - if unknown id still appears:
     - `[dm_listener] Unknown subscription_id..., trying active trade-key fallback`
     - `[dm_listener] Fallback routed GiftWrap ...` OR `Fallback failed ...`
4. Confirm whether UI now shows notification/pop-up.

## Open question
- If fallback still fails, next hypothesis is not subscription timing but parse/decrypt mismatch for that specific event/key path (need event metadata + parse counts from logs to isolate).

