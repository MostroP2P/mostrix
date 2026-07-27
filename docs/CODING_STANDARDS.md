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

- **Async UI feedback channels**: When spawning tasks from the key handler, use a **dedicated** `tokio::sync::mpsc` channel when the domain is not order/dispute traffic (for example, **`ln_address_result_tx`** / **`LnAddressVerifyResult`** for Lightning address LNURL verification) instead of overloading **`order_result_tx`**, which already carries creates, takes, disputes, history cleanup, and attachment flows. The main loop can still map a small domain-specific enum into **`OperationResult`** for a single popup renderer.

### 6. Import Usage Rule

**Always import from the crate root at the top of the file; never use full crate paths in the body of the code.**

- At the top of each Rust file, use `use crate::...` to bring needed modules, types, or functions into scope.
- **Do not** reference full module paths (e.g., `crate::ui::DisputeChatMessage`) inside function bodies or code blocks—bring them in with `use` statements instead.
- This improves readability and keeps imports consistent.

**Example (GOOD):**
```rust
use crate::ui::DisputeChatMessage;

fn foo(msg: &DisputeChatMessage) { /* ... */ }
```

**Example (BAD):**
```rust
fn foo(msg: &crate::ui::DisputeChatMessage) { /* ... */ }
```

**Example**: Instead of duplicating key derivation logic, use `User::derive_trade_keys()`.

### 2. Avoid Code Duplication (DRY Principle)

**Don't Repeat Yourself**: If the same logic appears in multiple places, extract it.

- **Extract common patterns**: Create helper functions for repeated operations.
- **Use traits**: When multiple types share behavior, consider using traits.
- **Centralize configuration**: Use the `Settings` struct instead of hardcoding values.

- [`send_dm`](../src/util/dm_utils/mod.rs) in `src/util/dm_utils/mod.rs` is reused across order operations; it wraps via `mostro-core` [`wrap_message_with`](../src/util/mod.rs) (v1 GiftWrap or v2 signed kind 14 per instance info).

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

**Example**: If `handle_key_event` grows too large, split it into `handle_navigation_keys`, `handle_form_input`, etc. Form typing lives in **`src/ui/key_handler/form_input.rs`**; global keys like **`n`** (cancel) and **`c`** (copy) must not swallow characters when **`is_creating_order_text_input`** is true — add guarded arms in `mod.rs` before the generic `Char(_)` handler.

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
pub async fn send_new_order(...) -> Result<OperationResult> {
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

```text
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

```text
// Re-export commonly used items (see src/util/mod.rs)
pub use db_utils::save_order;
pub use dm_utils::{
    handle_message_notification, handle_operation_result, hydrate_startup_active_order_dm_state,
    listen_for_order_messages, parse_dm_events, seed_admin_chat_last_seen, send_dm,
    set_dm_router_cmd_tx, wait_for_dm, OrderDmSubscriptionCmd, StartupDmHydration,
    FETCH_EVENTS_TIMEOUT,
};
pub use filters::{
    create_filter, create_mostro_list_fetch_filter, filter_giftwrap_to_recipient,
    filter_protocol_dm_from_mostro, MOSTRO_LIST_FETCH_EVENT_LIMIT,
};
pub use order_utils::{fetch_events_list, get_orders, send_new_order, take_order};
pub use types::{get_cant_do_description, Event, ListKind};
```

This allows clean imports: `use crate::util::send_dm;` instead of `use crate::util::dm_utils::send_dm;`.

## Testing and Quality

### Before Pushing

Toolchain is pinned in [`rust-toolchain.toml`](../rust-toolchain.toml) (currently **1.96.0**). Run:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --all-features
```

(Or use the Cursor `/build` command, which runs the same checklist.)

### Clippy Warnings

- **Fix all clippy warnings**: Don't ignore clippy suggestions unless there's a good reason. Treat `-D warnings` as the bar for CI/local `/build`.
- **Use `#[allow(clippy::...)]` sparingly**: Only when the warning is a false positive.
- **Rust 1.96 notes** (common findings after the toolchain bump): prefer `sort_by_key` / `std::cmp::Reverse` over `sort_by(|a, b| …cmp…)`; drop redundant `.max(0)` on already-non-negative `usize` values; prefer `match` / `if let` over `is_some()` + `unwrap()`; derive `Default` when the first enum variant is the default (`#[default]`).

### TUI unit tests (`TestBackend`)

Render/popup logic should be covered with **deterministic** `ratatui::backend::TestBackend` tests — no live relays or Lightning.

- **Template**: [`src/ui/helpers/layout.rs`](../src/ui/helpers/layout.rs) (`buffer_contains`, optional `cell_with_bg` for selection highlights). Also used by Messages tab layout tests in [`message_flow_tab.rs`](../src/ui/tabs/message_flow_tab.rs).
- **Placement**: keep unit tests **inline** as `#[cfg(test)] mod tests` (or named modules like `layout_and_render_tests`) in the same `.rs` file. For source files **≥ ~500 LOC** where the test block would dominate, split to a sibling via `#[cfg(test)] #[path = "foo_tests.rs"] mod foo_tests;`.
- **Assertions**: flatten the buffer and assert substrings (titles, labels, help keys). Avoid asserting time-based spinner/blink glyphs.
- **Coverage roadmap**: knock out 0% files in small waves (shared layout helpers → thin popups → chat helpers → tabs → util → large handlers). Live report: [mostro.network/mostrix/coverage](https://mostro.network/mostrix/coverage/). Agent priority list: [`.cursor/rules/test-coverage.mdc`](../.cursor/rules/test-coverage.mdc).

## Naming Conventions

- **Functions**: `snake_case` (e.g., `send_new_order`, `parse_dm_events`)
- **Types/Structs**: `PascalCase` (e.g., `AppState`, `UiMode`, `OperationResult`)
- **Constants**: `UPPER_SNAKE_CASE` (e.g., `FETCH_EVENTS_TIMEOUT`)
- **Modules**: `snake_case` (e.g., `order_utils`, `dm_utils`)

## State Management

- **Use `Arc<Mutex<T>>`**: For shared mutable state across threads.
- **Minimize shared state**: Prefer passing data explicitly when possible.
- **Single source of truth**: `AppState` in `src/ui/mod.rs` is the main UI state.

**Example**: `AppState` uses `Arc<Mutex<>>` for thread-safe access to messages and trade indices.

## Dependencies

- **`mostro-core`**: Pin in [`Cargo.toml`](../Cargo.toml) to the same minor line as the Mostro daemon you test against (currently **0.13.0** — adds `mostro_core::transport::{Transport, wrap_message_with, unwrap_incoming}` for protocol v2). Protocol types (`Action`, `Payload`, `BondResolution`, `CantDoReason`, `Transport`, …) must come from `mostro_core::prelude::*` — do not duplicate wire shapes in Mostrix. Re-export transport helpers from [`src/util/mod.rs`](../src/util/mod.rs) when used across the crate.
- **Bond invoice replies**: shared [`payment_request_operation_result`](../src/util/order_utils/helper.rs) for `take_order` and `send_new_order` — returns `PaymentRequestRequired`, not `Success`.
- **Admin bond slash**: use [`BondSlashChoice`](../src/util/order_utils/bond_resolution.rs) — TUI labels via `label()` (emoji + text); `#[derive(Default)]` with `#[default]` on `None`; state on `ReviewingDisputeForFinalization.bond`; pass through `execute_finalize_dispute(dispute_id, bond, …)` and `bond.to_optional_payload()` on the wire (`None` → `null`); see [FINALIZE_DISPUTES.md](FINALIZE_DISPUTES.md).

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
