# Coding Standards

This document outlines the coding standards and best practices for the Mostrix project. These guidelines ensure code quality, maintainability, and consistency across the codebase.

## Core Principles

### 1. Readability and Reuse

**Priority**: Code should be written for humans first, machines second.

- **Clear naming**: Use descriptive names for functions, variables, and modules.
  - Good: `derive_trade_keys`, `parse_dm_events`, `handle_order_result`
  - Bad: `dtk`, `pde`, `hor`
  
- **Function reuse**: Extract common logic into reusable functions.
  - If you find yourself copying code, create a helper function.
  - Place shared utilities in appropriate modules (`src/util/`, `src/ui/`, etc.).

- **Module organization**: Group related functionality together.
  - UI components in `src/ui/`
  - Protocol logic in `src/util/order_utils/`
  - Database operations in `src/util/db_utils.rs`

**Example**: Instead of duplicating key derivation logic, use `User::derive_trade_keys()`.

### 2. Avoid Code Duplication (DRY Principle)

**Don't Repeat Yourself**: If the same logic appears in multiple places, extract it.

- **Extract common patterns**: Create helper functions for repeated operations.
- **Use traits**: When multiple types share behavior, consider using traits.
- **Centralize configuration**: Use the `Settings` struct instead of hardcoding values.

**Example**: The `send_dm` function in `src/util/dm_utils/mod.rs` is reused across multiple order operations instead of duplicating the NIP-59 wrapping logic.

### 3. Simplicity

**Keep It Simple**: Prefer straightforward solutions over clever ones.

- **Avoid premature optimization**: Write clear code first, optimize only when needed.
- **Prefer explicit over implicit**: Make the code's intent obvious.
- **Use standard library**: Leverage Rust's standard library before adding dependencies.

**Example**: Use `Option` and `Result` types explicitly rather than hiding errors.

### 4. Function Length Limit

**Maximum 300 lines per function**: If a function exceeds this limit, split it into smaller functions.

- **Single Responsibility**: Each function should do one thing well.
- **Extract helpers**: Break complex functions into smaller, focused helpers.
- **Use private functions**: Create internal helper functions when needed.

**Example**: If `handle_key_event` grows too large, split it into `handle_navigation_keys`, `handle_form_input`, etc.

### 5. Module and Function Organization

**Split logic appropriately**: Organize code into logical modules and functions.

- **Module structure**: Group related functionality in modules.
  - `src/util/order_utils/`: Order-related operations
  - `src/util/dm_utils/`: Direct message handling
  - `src/ui/`: UI components and rendering

- **Function organization**: Within a module, organize functions logically.
  - Public API functions first
  - Private helper functions after
  - Group related functions together

- **Use submodules**: For complex modules, split into submodules.
  - Example: `src/util/dm_utils/mod.rs` with helpers in `dm_helpers.rs`

## Rust-Specific Guidelines

### Error Handling

- **Use `Result<T, E>`**: Functions that can fail should return `Result`.
- **Use `anyhow::Result`**: For application-level errors, use `anyhow::Result<T>`.
- **Propagate errors**: Use `?` operator to propagate errors up the call stack.
- **Log errors**: Use `log::error!`, `log::warn!` appropriately.

**Example**:
```rust
pub async fn send_new_order(...) -> Result<OrderResult> {
    let trade_keys = user.derive_trade_keys(next_idx)?; // Propagate error
    // ...
}
```

### Type Safety

- **Use strong types**: Prefer newtypes over primitive types when appropriate.
- **Leverage enums**: Use enums for state machines (e.g., `UiMode`, `UserRole`).
- **Avoid `unwrap()`**: Use `?`, `expect()` with clear messages, or pattern matching.

**Example**: `UiMode` enum clearly represents all possible UI states.

### Async/Await

- **Prefer async/await**: Use `async fn` for I/O operations.
- **Use `tokio` runtime**: Leverage tokio for async operations.
- **Handle timeouts**: Use timeouts for network operations (e.g., `FETCH_EVENTS_TIMEOUT`).

### Documentation

- **Document public APIs**: Use `///` doc comments for public functions and types.
- **Explain "why"**: Document the reasoning behind complex logic, not just "what".
- **Code examples**: Include examples in documentation when helpful.

**Example**:
```rust
/// Derives a trade key for the given index using NIP-06.
/// 
/// This ensures each trade uses a unique key for privacy.
/// The key is derived from the user's mnemonic using the path
/// `m/44'/1237'/38383'/0/{index}`.
pub fn derive_trade_keys(&self, index: i64) -> Result<Keys> {
    // ...
}
```

## Code Organization Patterns

### Module Structure

```
src/
├── main.rs              # Entry point
├── settings.rs          # Configuration
├── db.rs                # Database schema
├── models.rs            # Data models
├── ui/                  # UI components
│   ├── mod.rs          # AppState, UiMode
│   ├── key_handler.rs  # Input handling
│   └── ...             # UI components
└── util/                # Business logic
    ├── mod.rs          # Re-exports
    ├── order_utils/    # Order operations
    ├── dm_utils/       # Message handling
    └── db_utils.rs     # Database helpers
```

### Re-export Pattern

Use `mod.rs` files to re-export commonly used items:

**Source**: `src/util/mod.rs:7`
```7:14:src/util/mod.rs
// Re-export commonly used items
pub use db_utils::save_order;
pub use dm_utils::{
    handle_message_notification, handle_order_result, listen_for_order_messages, parse_dm_events,
    send_dm, wait_for_dm, FETCH_EVENTS_TIMEOUT,
};
pub use filters::{create_filter, create_seven_days_filter};
pub use order_utils::{fetch_events_list, get_orders, send_new_order, take_order};
pub use types::{get_cant_do_description, Event, ListKind};
```

This allows clean imports: `use crate::util::send_dm;` instead of `use crate::util::dm_utils::send_dm;`.

## Testing and Quality

### Before Pushing

Run these commands before committing:

```bash
cargo test              # Run all tests
cargo fmt              # Format code
cargo clippy --all-targets --all-features  # Lint code
```

### Clippy Warnings

- **Fix all clippy warnings**: Don't ignore clippy suggestions unless there's a good reason.
- **Use `#[allow(clippy::...)]` sparingly**: Only when the warning is a false positive.

## Naming Conventions

- **Functions**: `snake_case` (e.g., `send_new_order`, `parse_dm_events`)
- **Types/Structs**: `PascalCase` (e.g., `AppState`, `UiMode`, `OrderResult`)
- **Constants**: `UPPER_SNAKE_CASE` (e.g., `FETCH_EVENTS_TIMEOUT`)
- **Modules**: `snake_case` (e.g., `order_utils`, `dm_utils`)

## State Management

- **Use `Arc<Mutex<T>>`**: For shared mutable state across threads.
- **Minimize shared state**: Prefer passing data explicitly when possible.
- **Single source of truth**: `AppState` in `src/ui/mod.rs` is the main UI state.

**Example**: `AppState` uses `Arc<Mutex<>>` for thread-safe access to messages and trade indices.

## Summary Checklist

When writing or reviewing code, ensure:

- [ ] Code is readable and well-named
- [ ] No code duplication (DRY principle)
- [ ] Functions are under 300 lines
- [ ] Logic is split into appropriate modules
- [ ] Errors are handled properly (`Result`, `?` operator)
- [ ] Public APIs are documented
- [ ] Code passes `cargo fmt` and `cargo clippy`
- [ ] Tests pass (`cargo test`)
