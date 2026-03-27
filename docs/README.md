# Mostrix Documentation

Quick links to architecture and feature guides for the Mostrix TUI client.

## Core Guides

- **Startup & Configuration**: [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) - Boot sequence, settings, and initialization
- **TUI Interface**: [TUI_INTERFACE.md](TUI_INTERFACE.md) - User interface architecture, navigation, and state management
- **UI constants** (`src/ui/constants.rs`): Shared copy (footers, help, **`StepLabel`** text for the Messages tab six-step buy/sell trade timeline)
- **Key Management**: [KEY_MANAGEMENT.md](KEY_MANAGEMENT.md) - Deterministic key derivation, identity keys, and trade keys
- **Message Flow & Protocol**: [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) - How Mostrix communicates with Mostro daemon and handles messages
- **DM listener & router**: [DM_LISTENER_FLOW.md](DM_LISTENER_FLOW.md) - `listen_for_order_messages`, TrackOrder vs waiter path, in-memory `OrderMessage` list
- **Buy order flow (spec)**: [buy order flow.md](buy%20order%20flow.md) - Buy listing maker/taker phases, cancellation, Messages tab Enter, stepper, and rating (spec + implementation notes)
- **Sell order flow (spec)**: [sell order flow.md](sell%20order%20flow.md) - Sell listing maker/taker phases and Messages timeline stepper
- **Range Orders**: [RANGE_ORDERS.md](RANGE_ORDERS.md) - Variable amount orders and NextTrade payload mechanism
- **Admin Disputes**: [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) - Admin mode dispute resolution workflows and tabs
- **Database**: [DATABASE.md](DATABASE.md) - Database schema, tables, and data persistence
- **Coding Standards**: [CODING_STANDARDS.md](CODING_STANDARDS.md) - Code quality guidelines and best practices

## Tips

- Run tests and lints before pushing: `cargo test`, `cargo fmt`, `cargo clippy --all-targets --all-features`.
- See [CODING_STANDARDS.md](CODING_STANDARDS.md) for detailed coding guidelines and best practices.

## Implementation plans (AI / contributors)

- Tracked Markdown plans for larger features live under **[`.cursor/plans/`](../.cursor/plans/README.md)** (git-tracked; see root `.gitignore` exceptions). Use them to capture design decisions and link to `src/` paths for codegen and reviews.

