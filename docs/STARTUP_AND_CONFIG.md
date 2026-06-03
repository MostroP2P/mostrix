# Startup and Configuration

This guide explains Mostrix’s boot sequence and configuration surfaces.

## Overview
- Entry: `src/main.rs:98`
- Initializes database, derives identity keys, initializes settings, then logger, terminal (raw mode), shared state, Nostr client, and background tasks.
- Enters the main event loop to handle UI updates and user input.

## Initialization Sequence

### 1. Database Initialization
The database is initialized at startup to ensure the schema is ready.

**Source**: `src/db.rs`

- Creates the SQLite database file at `~/.mostrix/mostrix.db`.
- Ensures tables exist (`orders`, `users`).
- If the `users` table is empty, `User::new()` generates a new 12-word BIP-39 mnemonic and persists it in the `users` table (this mnemonic is the root for user identity/trade key derivation).
- For existing databases, runs migrations automatically to keep the schema up to date.

### 2. Settings Initialization
Mostrix uses centralized settings management in `src/settings.rs`.

**Source**: `src/settings.rs`

```rust
pub fn init_settings(identity_keys: Option<Keys>)
    -> Result<InitSettingsResult, anyhow::Error>
```

- On first run, `settings.toml` is generated from an embedded template compiled into the binary (rather than copying from the repo root).
- If `identity_keys` is provided (derived from the DB identity/index-0 key), Mostrix derives the `nsec_privkey` for `settings.toml` so DB keys and settings keys match.
- The returned `InitSettingsResult.did_generate_new_settings_file` indicates whether this process generated a brand-new `settings.toml`.
- When `did_generate_new_settings_file` is `true`, `main.rs` shows the `BackupNewKeys` popup overlay immediately on the current initial tab, prompting the user to save the generated 12-word mnemonic.

**Error Handling**: Startup failures in `init_settings()` are propagated as `anyhow::Error` (causing a clean process exit with an error message). If settings are accessed later at runtime before initialization (via the `SETTINGS` global), those failures are surfaced as user-friendly messages using `OperationResult::Error` instead of panicking. This ensures graceful degradation and clear feedback to users in both cases.

### 3. Logger Setup
Logging is configured via `setup_logger` in `src/main.rs`.

**Source**: `src/main.rs:41`
```41:63:src/main.rs
fn setup_logger(level: &str) -> Result<(), fern::InitError> {
    let log_level = match level.to_lowercase().as_str() {
        // ... level mapping ...
    };
    Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}] [{}] - {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(log_level)
        .chain(fern::log_file("app.log")?) // Writes to app.log
        .apply()?;
    Ok(())
}
```
- Sets the log level based on the `log_level` field in `settings.toml`.
- Outputs log messages to `app.log`.

### 4. TUI Initialization
The TUI uses `ratatui` with the `crossterm` backend.

**Source**: `src/main.rs:104`
```104:112:src/main.rs
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(
        out,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;
```
- Enables terminal raw mode.
- Enters the alternate screen and enables mouse capture.

## Configuration Structure

The `Settings` struct defines all available configuration options.

**Source**: `src/settings.rs`
```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub mostro_pubkey: String,
    pub nsec_privkey: String,
    pub admin_privkey: String,
    pub relays: Vec<String>,
    pub log_level: String,
    pub currencies_filter: Vec<String>,
    #[serde(default = "default_user_mode")]
    pub user_mode: String, // "user" or "admin", default "user"
    #[serde(default)]
    pub ln_address: String, // Lightning address for buyer receive; empty = unset
}
```

### Fields:
- **`mostro_pubkey`**: The public key of the Mostro instance to interact with.
- **`nsec_privkey`**: The user's Nostr private key (nsec format).
- **`admin_privkey`**: The admin's private key, required for solving disputes when in admin mode.
- **`relays`**: A list of Nostr relay URLs to connect to.
- **`log_level`**: The verbosity of logging (e.g., "debug", "info", "warn", "error").
- **`currencies_filter`**: Optional list of fiat currency **filters** (ISO codes).  
  - When empty, all currencies published by the Mostro instance are shown.  
  - When non-empty (e.g. `["USD"]`, `["USD", "EUR"]`), only orders whose fiat code is in this list are displayed.
- **`user_mode`**: Either "user" or "admin". Controls the UI and available actions.
- **`ln_address`**: Optional **Lightning address** (`user@domain.com`) used when the local user acts as **buyer** (receive via LNURL-pay). The embedded template includes `ln_address = ""`. Older `settings.toml` files without this key still load (`#[serde(default)]` yields an empty string). **Saving from the Settings tab** runs an async check that the LNURL metadata URL returns JSON with `tag: "payRequest"` before writing disk (`spawn_verify_and_save_ln_address_task` in `src/ui/key_handler/async_tasks.rs`, helper in `src/util/ln_address.rs`). The spawned task reports on **`ln_address_result_tx`** (`LnAddressVerifyResult`), not on `order_result_tx`, so settings verification does not share the order/dispute result queue. **Clear** removes the value without a network call.

Proof-of-work for published events is taken from the Mostro instance status event (kind 38385, tag `pow`), not from `settings.toml`.

## Nostr & Background Tasks

### Nostr Client Connection
Mostrix initializes a `nostr_sdk::Client` with the user's keys, adds configured relays, and
connects using a panic-safe wrapper (`connect_client_safely`).

Current startup behavior:

- Trims relay strings and skips empty entries before adding.
- Computes `relays_reachable` with `any_relay_reachable` for offline UI behavior.
- Calls `connect_client_safely(&client)` (instead of raw `client.connect().await`) to prevent
  background panic crashes when connectivity is unstable.
- Logs a warning if no configured relays are reachable at boot.

### Background Tasks

Several background tasks are spawned to keep the UI and data in sync:

1. **Order Refresh**: Periodically fetches pending orders from Mostro.
2. **Relay order DB reconcile** (startup + ~30s orders updater): `run_relay_order_db_reconcile_once` (bulk terminal sync from nostr order events) and `run_targeted_relay_order_db_reconcile_tick` (round-robin per-order fetch for local non-terminal trades with keys). See `relay_order_db_reconcile.rs` and **MESSAGE_FLOW_AND_PROTOCOL.md** (Relay → SQLite section).
3. **Trade Message Listener**: Listens for new messages related to active orders.
4. **Network Status Monitor**:
   - `spawn_network_status_monitor` runs every 5 seconds.
   - Re-checks relay reachability from disk settings and emits `NetworkStatus::Offline/Online`.
   - On `Offline`, startup overlay text indicates automatic retry.
   - On `Online`, `main.rs` triggers `reload_runtime_session_after_reconnect(...)` to reconnect
     and reload runtime background tasks.
5. **Shared chat relay poll** (`admin_chat_interval`, 2 seconds in `src/main.rs`):
   - **Admin role**: triggers `spawn_admin_chat_fetch` → `fetch_admin_chat_updates` (see `src/util/order_utils/fetch_scheduler.rs`).
     - For each in-progress dispute, rebuilds per-party shared `Keys` from `buyer_shared_key_hex` / `seller_shared_key_hex` stored in the `admin_disputes` table.
     - Fetches NIP‑59 `GiftWrap` events addressed to each shared key's public key (ECDH-derived, same model as `mostro-chat`).
     - Uses per‑party `last_seen_timestamp` values to request only new events.
     - Delegates application of updates to `ui::helpers::apply_admin_chat_updates` (implemented in `src/ui/helpers/startup.rs`), which:
       - Appends new `DisputeChatMessage` items into `AppState.admin_dispute_chats`.
       - Persists updated buyer/seller chat cursors in the `admin_disputes` table (`buyer_chat_last_seen`, `seller_chat_last_seen`).
   - **User role**: triggers `spawn_user_order_chat_fetch` → `fetch_user_order_chat_updates` on the same timer (shared keys from `order_chat_shared_key_hex` or `trade_keys` + `counterparty_pubkey`; applied via `apply_user_order_chat_updates`).
   - A **single-flight guard** (`CHAT_MESSAGES_SEMAPHORE`: `AtomicBool`) ensures only one shared-key chat fetch runs at a time; overlapping ticks skip spawning a new fetch until the current one completes.

**Source**: `src/main.rs` (background task setup), `src/util/order_utils/fetch_scheduler.rs` (admin chat scheduler), `src/ui/helpers/startup.rs` (`apply_admin_chat_updates`)

6. **DM Router Wiring (trade messages)**:
   - App channel creation includes `dm_subscription_tx` / `dm_subscription_rx`.
   - `set_dm_router_cmd_tx(dm_subscription_tx.clone())` publishes the sender globally for `wait_for_dm` (returns `Result`; startup fails fast if the mutex is poisoned).
   - Before spawning the listener, `hydrate_startup_active_order_dm_state` loads non-terminal orders from SQLite and returns `active_order_trade_indices` plus `order_last_seen_dm_ts` cursors; `main.rs` seeds the shared active-order map.
   - `listen_for_order_messages(..., order_last_seen_dm_ts, ..., dm_subscription_rx)` runs as the single router loop consuming:
     - `TrackOrder` commands for long-lived trade subscriptions.
     - `RegisterWaiter` commands for one-shot request/response waits.
   - After bootstrapping per-order GiftWrap subscriptions, the listener performs a **`fetch_events` replay** (`fetch_and_replay_startup_trade_dms`) so the Messages UI is populated from relay history (in-memory messages are not stored in the DB). Replay uses `notify: false` to avoid duplicate popups/badge noise.
   - This unifies in-flight response handling and background trade notifications on top of one notification stream.

See **[DM_LISTENER_FLOW.md](DM_LISTENER_FLOW.md)** for GiftWrap filter modes (`StartupCatchUp`, `StartupSince`, `LiveOnly`), waiter vs `TrackOrder` ordering, and replay details.

### Admin Chat Restore at Startup

In addition to the background scheduler, Mostrix restores admin chat state during startup:

- All persisted admin disputes are loaded from the `admin_disputes` table.
- For disputes in `InProgress` state, `ui::helpers::recover_admin_chat_from_files`:
  - Reads chat transcripts from `~/.mostrix/disputes_chat/<dispute_id>.txt` (if present).
  - Reconstructs `AppState.admin_dispute_chats` so the "Disputes in Progress" tab immediately shows prior messages.
  - Updates in‑memory `admin_chat_last_seen` entries for Buyer and Seller based on file timestamps.
- Subsequent background NIP‑59 fetches use the stored `buyer_chat_last_seen` / `seller_chat_last_seen` values as cursors, ensuring:
  - **Instant UI restore** after restart.
  - **Incremental network sync** without replaying the full chat history from relays.

### User order chat restore at startup (My Trades)

For **User** role, Mostrix restores peer-to-peer order chat alongside trade DMs:

- Cached transcripts live under `~/.mostrix/orders_chat/<order_id>.txt` and are loaded into `AppState.order_chats` by `load_user_order_chats_at_startup`.
- **Attachment rows in transcripts** are stored as **JSON** (`image_encrypted` / `file_encrypted` via `serialize_attachment_for_transcript`) so **Ctrl+S** and file counts work immediately after restart; legacy `[Image: … - Ctrl+S to save]` lines are hydrated in memory when relay returns the same attachment at the same timestamp.
- An immediate relay fetch (`fetch_user_order_chat_updates`) merges any newer gift-wrap messages; subsequent polls run every **2 seconds** on the shared `admin_chat_interval` timer via `spawn_user_order_chat_fetch` in `src/util/order_utils/fetch_scheduler.rs`.
- `apply_user_order_chat_updates` skips relay echoes of the local trade pubkey; peer dedup is scoped to existing **Peer** rows so optimistic **You** sends are not mirrored as **Peer** and do not suppress unrelated peer text at the same timestamp. See [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) — "User order chat local cache".

## Main Event Loop

The TUI runs in a `tokio::select!` loop that handles (among others):

1. **Fatal errors**: `fatal_error_rx` — aborts background work and shows an error popup.
2. **Network status**: `network_status_rx` — offline overlay vs reconnect + runtime reload.
3. **Order / dispute / attachment / observer async results**: `order_result_rx` — `OperationResult`; includes dispute-list refresh side effects for certain `Info` messages and My Trades DB resync for `OrderHistoryDeleted`.
4. **Lightning address verify-and-save (settings)**: `ln_address_result_rx` — `LnAddressVerifyResult`; mapped to `OperationResult::Info` / `Error` and passed to **`handle_operation_result`** so UI behavior matches other operation-result popups without mixing traffic into `order_result_rx`.
5. **Key rotation / seed words / message notifications / admin & user chat fetches / Mostro instance info / user input / periodic ticks**: see `src/main.rs` (`create_app_channels` in `src/ui/key_handler/async_tasks.rs` lists all paired senders and receivers). User order chat results arrive on `user_order_chat_updates_rx` and are applied via `apply_user_order_chat_updates`.

**Source**: `src/main.rs` (outer `loop` + `tokio::select!` + `terminal.draw`).

```text
// Simplified shape (not exhaustive — see src/main.rs for full select!)
loop {
    tokio::select! {
        // fatal_error_rx, network_status_rx, ...
        result = order_result_rx.recv() => { apply_order_result(...) }
        ln_address_verify = ln_address_result_rx.recv() => { /* map LnAddressVerifyResult → OperationResult */ }
        // key_rotation_rx, seed_words_rx, message_notification_rx, ...
        maybe_event = events.next() => { /* handle_key_event, paste, mouse */ }
        _ = refresh_interval.tick() => { /* 150 ms — redraw even without input */ }
    }
    // Before every frame (not only on keypress):
    drain_save_attachment_queue(...)   // start Blossom downloads queued by Ctrl+S popups
    drain_order_result_queue(...)    // apply OperationResult (e.g. "Saved to …") for async tasks
    expire_attachment_toast(&mut app);
    terminal.draw(|f| ui_draw(f, &app, &orders, Some(&status_line)))?;
}
```

**Why drain before draw:** My Trades **Enter** on the save-attachment popup may enqueue the download asynchronously (DB lookup for decryption key). Without draining `save_attachment_rx` / `order_result_rx` on each frame, the success popup could appear only after an unrelated keypress. The **150 ms** `refresh_interval` tick plus this drain keeps attachment save feedback timely.
