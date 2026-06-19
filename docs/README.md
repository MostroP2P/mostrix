# Mostrix Documentation

Index of architecture and feature guides for the Mostrix TUI client. The [root README](../README.md) links here as the main documentation entry point.

## Core runtime & data

- **Startup & Configuration**: [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) — Boot sequence, settings (`blossom_servers`), background tasks, DM router wiring, reconnect; main loop **drains save/send-attachment and operation-result channels before draw** (150 ms refresh)
- **DM listener & router**: [DM_LISTENER_FLOW.md](DM_LISTENER_FLOW.md) — `listen_for_order_messages` (`mostro_pubkey` + `transport`), TrackOrder vs waiter, [`filter_protocol_dm_from_mostro`](../src/util/filters.rs) subscriptions, startup `fetch_events` replay; event gate still kind 1059 until step 6
- **Message Flow & Protocol**: [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) — How Mostrix talks to Mostro over Nostr (orders, GiftWrap, restarts, cooperative cancel / `TradeClosed`); **protocol v2 discovery** (`protocol_version` on kind 38385 → `AppState.transport`; wire cutover in progress — see [Protocol v2 (NIP-44)](#protocol-v2-nip-44-in-progress)); **maker bond** (`send_new_order` → `PayBondInvoice` / `PaymentRequestRequired`, deferred `NewOrder` after payment); **My Trades user order chat** relay sync, own-message echo skip, attachment receive/save, **outbound send** (Ctrl+O picker, trade-key Blossom auth, mobile-compatible wire JSON, upload-then-send retry / **Ctrl+Shift+O**, `pending_order_attachment_sends`), **JSON transcript persistence** (Ctrl+S after restart)
- **PoW & outbound events**: [POW_AND_OUTBOUND_EVENTS.md](POW_AND_OUTBOUND_EVENTS.md) — Instance `pow` (kind 38385), `nostr_pow_from_instance`, Gift Wrap outer mining (`gift_wrap_from_seal_with_pow`); v2 will apply PoW on signed kind-14 (see protocol v2 section in MESSAGE_FLOW)
- **Database**: [DATABASE.md](DATABASE.md) — SQLite schema, `orders` / `users` / `admin_disputes`, migrations; **relay → SQLite reconcile** for terminal order statuses (`relay_order_db_reconcile.rs`)
- **Key Management**: [KEY_MANAGEMENT.md](KEY_MANAGEMENT.md) — Deterministic derivation (NIP-06 path), identity vs trade keys

## UI & order flows

- **TUI Interface**: [TUI_INTERFACE.md](TUI_INTERFACE.md) — Navigation, modes, state; create-order form input; **My Trades** (`user_my_trades_interactive`, scroll, receive attachments + Ctrl+S save, **Ctrl+O** send picker + **Ctrl+Shift+O** retry, `order_chat_static` vs live projection); Messages timeline (`StepPendingOrder` = no highlighted column while `Pending` / `WaitingTakerBond` / `WaitingMakerBond`)
- **UI constants** (`src/ui/constants.rs`): Shared copy (footers, help, **`StepLabel`** for the Messages tab buy/sell timeline)
- **Buy order flow (spec)**: [buy order flow.md](buy%20order%20flow.md) — Phase 1.5+ taker bond and Phase 5+ maker bond (`PayBondInvoice` / `WaitingTakerBond` / `WaitingMakerBond`)
- **Sell order flow (spec)**: [sell order flow.md](sell%20order%20flow.md) — Phase 1.5+ taker bond and Phase 5+ maker bond (`PayBondInvoice` / `WaitingTakerBond` / `WaitingMakerBond`)
- **Range Orders**: [RANGE_ORDERS.md](RANGE_ORDERS.md) — Variable amount orders and NextTrade payload

## Admin

- **Admin Disputes**: [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) — Tabs, shared-keys chat, workflows
- **Finalize disputes**: [FINALIZE_DISPUTES.md](FINALIZE_DISPUTES.md) — Inline finalize popup (💰 pay / ↩️ refund, inner **Admin settle** / **Admin cancel**); admin `wait_for_dm` + `CantDo`; multi-line success popup; trader **AddBondInvoice** payout with follow-up popup (`OpenInvoicePopup` / `PaymentRequestRequired`)

## Contributing & tooling

- **Coding Standards**: [CODING_STANDARDS.md](CODING_STANDARDS.md) — Style, re-exports, tests, clippy
- **Settings analysis**: [SETTINGS_ANALYSIS.md](SETTINGS_ANALYSIS.md) — Deeper notes on `settings.toml` / options (buyer `ln_address`, LNURL verify-on-save, **`ConfirmSavedLnAddressForInvoice`** → **YES** auto-submits **`AddInvoice`** when saved address exists; Settings tab **`ADMIN_SETTINGS`** / **`USER_SETTINGS`** tables + **`SettingsMenuAction`** Enter routing)

## Tips

- Run tests and lints before pushing: `cargo test`, `cargo fmt`, `cargo clippy --all-targets --all-features`.
- See [CODING_STANDARDS.md](CODING_STANDARDS.md) for detailed coding guidelines and best practices.

## Implementation plans (AI / contributors)

- Tracked Markdown plans for larger features live under **[`.cursor/plans/`](../.cursor/plans/README.md)** (git-tracked; see root `.gitignore` exceptions). Use them to capture design decisions and link to `src/` paths for codegen and reviews. Example: [admin dispute bond slash](../.cursor/plans/admin_dispute_bond_slash.plan.md). **My Trades attachments**: receive/save, JSON transcripts, outbound send (encrypt → Blossom → DM) — see [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md).

## Protocol v2 (NIP-44) — in progress

Mostrix is gaining **dual-transport** support for Mostro **protocol DMs** (not P2P order chat or admin dispute chat — those stay on GiftWrap).

| Status | What |
|--------|------|
| **Done** | `mostro-core` **0.13.0**; parse `protocol_version` from kind **38385**; [`transport_from_instance`](../src/util/mostro_info.rs); [`AppState.transport`](../src/ui/app_state.rs); Mostro Info tab; [`filter_protocol_dm_from_mostro`](../src/util/filters.rs); [`ensure_order_dm_subscription`](../src/util/dm_utils/dm_helpers.rs) + listener/replay/waiter **subscribe** paths; **await instance info before DM listener** in [`startup.rs`](../src/startup.rs) |
| **Pending** | `wrap_message_with` / `unwrap_incoming` in `send_dm` and inbound parse; event gate `transport.event_kind()` (still hardcoded GiftWrap); restart listener when `transport` changes after manual refresh |

**Note:** **Subscriptions** already use the v2 filter shape when `transport == Nip44Direct`, but **send** still uses GiftWrap and the **notification handler** still drops non–kind-1059 events until steps 5–6 (see **Pending** above).

