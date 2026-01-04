# Mostrix Documentation

Quick links to architecture and feature guides for the Mostrix TUI client.

## Core Guides

- **Startup & Configuration**: [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) - Boot sequence, settings, and initialization
- **TUI Interface**: [TUI_INTERFACE.md](TUI_INTERFACE.md) - User interface architecture, navigation, and state management
- **Key Management**: [KEY_MANAGEMENT.md](KEY_MANAGEMENT.md) - Deterministic key derivation, identity keys, and trade keys
- **Message Flow & Protocol**: [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) - How Mostrix communicates with Mostro daemon and handles messages
- **Range Orders**: [RANGE_ORDERS.md](RANGE_ORDERS.md) - Variable amount orders and NextTrade payload mechanism
- **Coding Standards**: [CODING_STANDARDS.md](CODING_STANDARDS.md) - Code quality guidelines and best practices

## Tips

- Run tests and lints before pushing: `cargo test`, `cargo fmt`, `cargo clippy --all-targets --all-features`.
- See [CODING_STANDARDS.md](CODING_STANDARDS.md) for detailed coding guidelines and best practices.