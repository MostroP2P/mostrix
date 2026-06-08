# Mostrix Documentation

Index of architecture and feature guides for the Mostrix TUI client. The [root README](../README.md) links here as the main documentation entry point.

## Core runtime & data

- **Startup & Configuration**: [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) — Boot sequence, settings (`blossom_servers`), background tasks, DM router wiring, reconnect; main loop **drains save/send-attachment and operation-result channels before draw** (150 ms refresh)
- **DM listener & router**: [DM_LISTENER_FLOW.md](DM_LISTENER_FLOW.md) — `listen_for_order_messages`, TrackOrder vs waiter, startup `fetch_events` replay, in-memory `OrderMessage` list; **`Action::CantDo`** ignored in `handle_trade_dm_for_order` (errors use waiter / `OperationResult`, not Messages upserts)
- **Message Flow & Protocol**: [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) — How Mostrix talks to Mostro over Nostr (orders, GiftWrap, restarts, cooperative cancel / `TradeClosed`); **maker bond** (`send_new_order` → `PayBondInvoice` / `PaymentRequestRequired`, deferred `NewOrder` after payment); **My Trades user order chat** relay sync, own-message echo skip, attachment receive/save, **outbound send** (Ctrl+O picker, trade-key Blossom auth, mobile-compatible wire JSON, upload-then-send retry / **Ctrl+Shift+O**, `pending_order_attachment_sends`), **JSON transcript persistence** (Ctrl+S after restart)
- **PoW & outbound events**: [POW_AND_OUTBOUND_EVENTS.md](POW_AND_OUTBOUND_EVENTS.md) — Instance `pow` (kind 38385), `nostr_pow_from_instance`, Gift Wrap outer mining (`gift_wrap_from_seal_with_pow`)
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

