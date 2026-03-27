# DM listener / router flow (`listen_for_order_messages`)

This document explains the runtime flow inside `listen_for_order_messages` (in `src/util/dm_utils/mod.rs`), focusing on:

- how the in-memory **message list** (`Vec<OrderMessage>`) is created/updated
- how “preferences”/routing concepts work: **TrackOrder**, **Waiter**, **Database**, **Action**, **Status**, notifications, and terminal cleanup

## Big picture

Mostrix has a **single background task** that:

- maintains relay subscriptions for **active orders** (long-lived)
- supports temporary **request/response waits** (short-lived) used by operations like “create order”, “take order”, “send msg”
- consumes incoming relay GiftWrap events and routes each event into:
  - (A) the **waiter path**: satisfy in-flight `wait_for_dm` calls
  - (B) the **tracked-order path**: update the UI/order-state pipeline

### Core state held by the listener

- **`subscribed_pubkeys: HashSet<PublicKey>`**  
  Pubkeys we believe we currently have an active GiftWrap subscription for (whether that subscription originated from TrackOrder or a Waiter).

- **`subscription_to_order: HashMap<SubscriptionId, (Uuid, i64)>`**  
  The “fast path” routing table: if an event arrives with a known `subscription_id`, we immediately know its `(order_id, trade_index)`.

- **`pubkey_to_subscription: HashMap<PublicKey, SubscriptionId>`**  
  Lets TrackOrder “rebind” a pubkey that was subscribed earlier by a waiter without subscribing twice.

- **`pending_waiters: Vec<PendingDmWaiter>`**  
  Each waiter is a oneshot sender plus the `trade_keys` to test whether the incoming GiftWrap can be decrypted for that operation.

- **`active_order_trade_indices: Arc<Mutex<HashMap<Uuid, i64>>>`** *(shared with the rest of the app)*  
  Tracks which orders are currently “active” and which `trade_index` (hence which trade key) belongs to each `order_id`.

- **`messages: Arc<Mutex<Vec<OrderMessage>>>`** *(shared with UI)*  
  The in-memory list backing the “Messages”/flow UI. Important: this vector is **not a full history**; it stores **one “latest relevant” row per order**.

## Startup bootstrap (how initial subscriptions are created)

On startup, the listener clones `active_order_trade_indices` and for each `(order_id, trade_index)`:

1. derives `trade_keys` from the persisted `User` seed + trade index
2. subscribes to GiftWrap events addressed to the trade pubkey
3. records routing metadata (`subscribed_pubkeys`, `subscription_to_order`, `pubkey_to_subscription`)

The subscription mode is “catch-up” oriented (the helper decides the exact filter details).

## Command “preferences”: TrackOrder vs Waiter

The listener consumes a command channel (`dm_subscription_rx`) with two variants:

### 1) `TrackOrder { order_id, trade_index }`

Use case: “this order is now active; keep listening for updates”.

What happens:

- **Active order map is updated immediately** (`active_order_trade_indices.insert(order_id, trade_index)`)
  - It also removes any “stale” `order_id` entries pointing at the same `trade_index` (to avoid phantom order IDs when the final Mostro-provided ID differs from an optimistic one).
- Derive `trade_keys` from `trade_index`, get `pubkey = trade_keys.public_key()`.
- Ensure a GiftWrap subscription exists for that pubkey. If already subscribed (possibly via a waiter), TrackOrder will reuse it.
- Update routing tables so future relay events with that `subscription_id` are routed directly to `(order_id, trade_index)`.

**Conceptually:** TrackOrder is long-lived; it binds the pubkey to a concrete order and makes the tracked-order path reliable and O(1).

### 2) `RegisterWaiter { trade_keys, response_tx }`

Use case: “I’m about to send a request DM; wait for the first decryptable response for these trade keys”.

What happens:

- Waiters are bounded (`MAX_PENDING_WAITERS`), and periodically garbage-collected (drops closed oneshots).
- If this trade pubkey is not yet subscribed, the listener subscribes to GiftWrap events for it and records the `SubscriptionId` in `pubkey_to_subscription`.
- The waiter is pushed into `pending_waiters`.

**Conceptually:** a Waiter is short-lived. It does not know `order_id`; it only knows “this key should decrypt the response”.

## Incoming GiftWrap event routing (the heart of the flow)

When a relay event arrives (`RelayPoolNotification::Event`) and `event.kind == GiftWrap`:

### Step A — satisfy pending waiters first

For each waiter:

- test whether `nip59::extract_rumor(&waiter.trade_keys, &event)` succeeds
- if it does, send the raw `event` into the waiter oneshot (`response_tx.send(event.clone())`)
- otherwise, keep the waiter pending for the next event

To avoid duplicate decrypt checks, the listener keeps a **per-event decryptability cache**:

- key: `(event_id, trade_pubkey)`
- value: `bool` (decryptable or not)

This cache is reused again in the tracked-order path below.

### Step B — tracked-order path (map event → order_id/trade_index)

The listener tries, in order:

1) **Fast path: route by subscription id**  
If `subscription_to_order` contains `subscription_id`, we have `(order_id, trade_index)`.

2) **Fallback path: resolve by testing active orders**  
If subscription id is unknown (e.g. a waiter created the subscription and TrackOrder hasn’t rebound it yet), the listener scans `active_order_trade_indices` and tries decrypting the event against each derived trade key until one matches.

When an `(order_id, trade_index, trade_keys)` is found, the listener proceeds to parse and dispatch.

## How the “list of messages” is created

### 1) Decrypt & parse into protocol `Message`

For the tracked order (or fallback-resolved order), the listener:

- builds a one-event `Events` set
- calls `parse_dm_events(events, &trade_keys, None)`

`parse_dm_events` returns a sorted list:

- **dedup**: drops duplicate Nostr event IDs
- **decrypt**: unwraps GiftWrap (NIP-59) and parses JSON into `mostro_core::Message`
- **sort**: ascending by rumor created-at timestamp (oldest → newest)

### 2) Dispatch each parsed trade DM into the UI/DB pipeline

For each `(Message, timestamp, sender)` in the parsed batch:

- call `handle_trade_dm_for_order(...)`
- then apply “terminal trade” cleanup rules (see below)

### 3) `handle_trade_dm_for_order` constructs (and replaces) `OrderMessage`

This function is where `OrderMessage` is created/updated and pushed into `messages`.

Key behaviors:

- **DB refresh/upsert for certain actions**  
  For `add-invoice` and `pay-invoice` where the payload embeds an order, the listener persists/upserts the order row (including request id when available).

- **Status persistence**  
  Updates the order status in SQLite via `update_order_status` using:
  - status derived from `Payload::Order` / `PaymentRequest(Some(order), ...)` + `map_action_to_status`, or
  - action-only inference (`inferred_status_from_trade_action`) when payload is absent.

- **Derive “effective” UI fields with fallbacks**  
  The `OrderMessage` fields like `sat_amount`, `buyer_invoice`, `order_kind`, `is_mine`, `order_status` are computed from a priority order:
  - payload (if present)
  - database row (if present)
  - previous message already stored for that order (if present)

- **Dedup / “is new message” logic**  
  Relay delivery can be out-of-order. The listener decides a message is “new” if:
  - there was no existing message for that order, or
  - the `Action` changed, or
  - the `Action` is the same but the new timestamp is strictly newer

- **“One row per order” storage**  
  The `messages: Vec<OrderMessage>` is treated as “latest per order”:
  - it removes any existing entry with the same `order_id`
  - pushes the newly created `OrderMessage`
  - sorts the whole vector by `timestamp` descending (newest first)

So the “message list” is really a **per-order summary row list**, not a chat transcript.

### 4) Notifications + pending badge count

If the update is both:

- **actionable** (e.g. `pay-invoice` only when an actual invoice exists), and
- **new** (per the logic above),

then the listener:

- increments `pending_notifications`
- sends a UI notification via `message_notification_tx`

## Action vs Status vs Database (how to think about them)

- **`Action`** (`mostro_core::Action`)  
  The *event type* of a protocol step (e.g. `PayInvoice`, `AddInvoice`, `FiatSent`, `Release`, `Canceled`, …). This is always present in the decoded `MessageKind`.

- **`Status`** (`mostro_core::order::Status`)  
  The order’s *state machine position* (e.g. `waiting-payment`, `active`, `fiat-sent`, `success`, …). This may come from:
  - an embedded order payload (`Payload::Order` or `PaymentRequest(Some(order), ...)`)
  - the local DB (previously persisted)
  - inference from certain action-only messages

- **Database (`sqlite`)**  
  Used to persist “critical truth” for recovery and UI:
  - order rows (including `trade_keys`, kind, mine/not-mine, last known status)
  - status updates (`update_order_status`)
  - “upsert from DM” updates for invoice-related actions

**Rule of thumb in the listener:**  
Use payload `Status` when present, otherwise consult DB or infer from `Action`, then publish an `OrderMessage` that carries “effective” fields forward so the UI stays stable even across partial payloads.

## Terminal cleanup (when we stop tracking an order)

Some messages indicate the trade is over. Terminal detection considers:

- explicit terminal actions even when payload is null (e.g. `canceled`)
- terminal order statuses when present in the payload (`success`, `canceled`, `expired`, …)

When a terminal message is detected:

- for **tracked subscriptions** (known `subscription_id`):
  - remove the order from `active_order_trade_indices`
  - remove the pubkey from `subscribed_pubkeys`
  - remove the mapping entry from `subscription_to_order`
  - unsubscribe from the relay subscription

- for **fallback/untracked** (unknown `subscription_id`):
  - remove the order from `active_order_trade_indices`
  - remove the pubkey from `subscribed_pubkeys`
  - do **not** unsubscribe (we may not own that subscription id)

## Mermaid: end-to-end listener flow

```mermaid
flowchart TD
  A[listen_for_order_messages start] --> B[Load User from DB]
  B --> C[Bootstrap subs for active_order_trade_indices]
  C --> D{loop: select}

  D -->|tick| GC[Prune closed waiters]
  D -->|cmd| CMD{DmRouterCmd}
  CMD -->|TrackOrder| TO[Update active_order_trade_indices; ensure subscription; bind subscription_id -> order]
  CMD -->|RegisterWaiter| W[Ensure waiter pubkey subscription; push PendingDmWaiter]

  D -->|relay event| E[GiftWrap event arrives]
  E --> WA[Try match pending waiters (decrypt check)]
  WA --> RB{subscription_id mapped?}
  RB -->|yes| FAST[Derive trade_keys; parse_dm_events; dispatch batch]
  RB -->|no| FB[resolve_order_for_event: scan active orders; find decryptable key]
  FB -->|matched| FAST
  FB -->|no match| DROP[Ignore event]

  FAST --> HT[handle_trade_dm_for_order per message]
  HT --> M[Replace per-order entry in messages; maybe notify]
  M --> TERM{terminal message?}
  TERM -->|yes| CL[cleanup indices + subscription]
  TERM -->|no| D
  CL --> D
```

## Practical “where to look” pointers

- **Router entry point**: `listen_for_order_messages` in `src/util/dm_utils/mod.rs`
- **Message parsing**: `parse_dm_events`
- **Per-order message construction & dedup**: `handle_trade_dm_for_order`
- **Terminal detection**: `trade_message_is_terminal`
- **Fallback routing**: `resolve_order_for_event`

