# Mostrix Documentation

Index of architecture and feature guides for the Mostrix TUI client. The [root README](../README.md) links here as the main documentation entry point.

## Core runtime & data

- **Startup & Configuration**: [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) — Boot sequence, settings, background tasks, DM router wiring, reconnect
- **DM listener & router**: [DM_LISTENER_FLOW.md](DM_LISTENER_FLOW.md) — `listen_for_order_messages`, TrackOrder vs waiter, startup `fetch_events` replay, in-memory `OrderMessage` list; **`Action::CantDo`** ignored in `handle_trade_dm_for_order` (errors use waiter / `OperationResult`, not Messages upserts)
- **Message Flow & Protocol**: [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) — How Mostrix talks to Mostro over Nostr (orders, GiftWrap, restarts, cooperative cancel / `TradeClosed`)
- **PoW & outbound events**: [POW_AND_OUTBOUND_EVENTS.md](POW_AND_OUTBOUND_EVENTS.md) — Instance `pow` (kind 38385), `nostr_pow_from_instance`, Gift Wrap outer mining (`gift_wrap_from_seal_with_pow`)
- **Database**: [DATABASE.md](DATABASE.md) — SQLite schema, `orders` / `users` / `admin_disputes`, migrations; **relay → SQLite reconcile** for terminal order statuses (`relay_order_db_reconcile.rs`)
- **Key Management**: [KEY_MANAGEMENT.md](KEY_MANAGEMENT.md) — Deterministic derivation (NIP-06 path), identity vs trade keys

## UI & order flows

- **TUI Interface**: [TUI_INTERFACE.md](TUI_INTERFACE.md) — Navigation, modes, state; create-order form input; My Trades (static `order_chat_static` header vs `build_active_order_chat_list` live fields); Messages timeline (`StepPendingOrder` = no highlighted column while `Pending` / `WaitingTakerBond`)
- **UI constants** (`src/ui/constants.rs`): Shared copy (footers, help, **`StepLabel`** for the Messages tab buy/sell timeline)
- **Buy order flow (spec)**: [buy order flow.md](buy%20order%20flow.md) — Phase 1.5+ optional **anti-abuse bond** (`PayBondInvoice` / `WaitingTakerBond`) included as phase 0 for the taker
- **Sell order flow (spec)**: [sell order flow.md](sell%20order%20flow.md) — Phase 1.5+ optional **anti-abuse bond** for the taker (buyer)
- **Range Orders**: [RANGE_ORDERS.md](RANGE_ORDERS.md) — Variable amount orders and NextTrade payload

## Admin

- **Admin Disputes**: [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) — Tabs, shared-keys chat, workflows
- **Finalize disputes**: [FINALIZE_DISPUTES.md](FINALIZE_DISPUTES.md) — Inline finalize popup (💰 pay / ↩️ refund, inner **Admin settle** / **Admin cancel**); `wait_for_dm` + `CantDo` handling; multi-line success popup; trader `AddBondInvoice` payout popup

## Contributing & tooling

- **Coding Standards**: [CODING_STANDARDS.md](CODING_STANDARDS.md) — Style, re-exports, tests, clippy
- **Settings analysis**: [SETTINGS_ANALYSIS.md](SETTINGS_ANALYSIS.md) — Deeper notes on `settings.toml` / options (buyer `ln_address`, LNURL verify-on-save, **`ConfirmSavedLnAddressForInvoice`** → **YES** auto-submits **`AddInvoice`** when saved address exists; Settings tab **`ADMIN_SETTINGS`** / **`USER_SETTINGS`** tables + **`SettingsMenuAction`** Enter routing)

## Tips

- Run tests and lints before pushing: `cargo test`, `cargo fmt`, `cargo clippy --all-targets --all-features`.
- See [CODING_STANDARDS.md](CODING_STANDARDS.md) for detailed coding guidelines and best practices.

## Implementation plans (AI / contributors)

- Tracked Markdown plans for larger features live under **[`.cursor/plans/`](../.cursor/plans/README.md)** (git-tracked; see root `.gitignore` exceptions). Use them to capture design decisions and link to `src/` paths for codegen and reviews. Current: [admin dispute bond slash](../.cursor/plans/admin_dispute_bond_slash.plan.md).

