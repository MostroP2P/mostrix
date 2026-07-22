# AGENTS.md

## UI / TUI guidelines

- **Always design TUI panels to degrade gracefully on narrow terminals.** When
  horizontal space is limited, prefer a simpler, still-readable layout over
  decoration (readability over beauty on small screens). Concretely: drop or
  wrap secondary decoration, collapse multi-column layouts into a single column,
  and keep the essential information visible rather than clipping it off-screen.
  The Messages tab (`src/ui/tabs/message_flow_tab.rs`) is the reference example —
  it switches between full and compact layouts via width helpers
  (`use_full_progress`, `use_two_column_trade`) and reserves extra height for
  wrapped text on narrow panels.

## Build / test

Standard Cargo workflow (toolchain pinned in `rust-toolchain.toml`):

- Build: `cargo build`
- Lint: `cargo fmt --all -- --check` and `cargo clippy --all-targets --all-features -- -D warnings`
- Test: `cargo test --all-features`

TUI render logic can be verified deterministically in unit tests with
`ratatui::backend::TestBackend` (render a widget to a fixed-size buffer and
assert on the output) — no live relay/Lightning needed.
