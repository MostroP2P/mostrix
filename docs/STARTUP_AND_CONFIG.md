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

**Source**: `src/settings.rs:8`
```8:19:src/settings.rs
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub mostro_pubkey: String,
    pub nsec_privkey: String,
    pub admin_privkey: String,
    pub relays: Vec<String>,
    pub log_level: String,
    pub currencies_filter: Vec<String>,
    pub pow: u8,
    #[serde(default = "default_user_mode")]
    pub user_mode: String, // "user" or "admin", default "user"
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
- **`pow`**: Proof-of-work difficulty requirement for publishing events.
- **`user_mode`**: Either "user" or "admin". Controls the UI and available actions.

## Nostr & Background Tasks

### Nostr Client Connection
Mostrix initializes a `nostr_sdk::Client` with the user's keys and connects to the configured relays.

**Source**: `src/main.rs:118`
```118:127:src/main.rs
    let my_keys = settings
        .nsec_privkey
        .parse::<Keys>()
        .map_err(|e| anyhow::anyhow!("Invalid NSEC privkey: {}", e))?;
    let client = Client::new(my_keys);
    // Add relays.
    for relay in &settings.relays {
        client.add_relay(relay).await?;
    }
    client.connect().await;
```

### Background Tasks

Several background tasks are spawned to keep the UI and data in sync:

1. **Order Refresh**: Periodically fetches pending orders from Mostro.
2. **Trade Message Listener**: Listens for new messages related to active orders.
3. **Admin Chat Scheduler** (shared-key model):
   - In the main event loop, when `user_role == Admin`, a 5-second interval triggers `spawn_admin_chat_fetch` (see `src/util/order_utils/fetch_scheduler.rs`).
   - A **single-flight guard** (`CHAT_MESSAGES_SEMAPHORE`: `AtomicBool`) ensures only one admin chat fetch runs at a time; overlapping ticks skip spawning a new fetch until the current one completes.
   - For each in-progress dispute, rebuilds per-party shared `Keys` from `buyer_shared_key_hex` / `seller_shared_key_hex` stored in the `admin_disputes` table.
   - Fetches NIP‑59 `GiftWrap` events addressed to each shared key's public key (ECDH-derived, same model as `mostro-chat`).
   - Uses per‑party `last_seen_timestamp` values to request only new events.
   - Delegates application of updates to `ui::helpers::apply_admin_chat_updates`, which:
     - Appends new `DisputeChatMessage` items into `AppState.admin_dispute_chats`.
     - Persists updated buyer/seller chat cursors in the `admin_disputes` table (`buyer_chat_last_seen`, `seller_chat_last_seen`).

**Source**: `src/main.rs` (background task setup), `src/util/order_utils/fetch_scheduler.rs` (admin chat scheduler), `src/ui/helpers.rs` (`apply_admin_chat_updates`)

4. **DM Router Wiring (trade messages)**:
   - App channel creation includes `dm_subscription_tx` / `dm_subscription_rx`.
   - `set_dm_router_cmd_tx(dm_subscription_tx.clone())` publishes the sender globally for `wait_for_dm`.
   - `listen_for_order_messages(..., dm_subscription_rx)` runs as the single router loop consuming:
     - `TrackOrder` commands for long-lived trade subscriptions.
     - `RegisterWaiter` commands for one-shot request/response waits.
   - This unifies in-flight response handling and background trade notifications on top of one notification stream.

### Admin Chat Restore at Startup

In addition to the background scheduler, Mostrix restores admin chat state during startup:

- All persisted admin disputes are loaded from the `admin_disputes` table.
- For disputes in `InProgress` state, `ui::helpers::recover_admin_chat_from_files`:
  - Reads chat transcripts from `~/.mostrix/<dispute_id>.txt` (if present).
  - Reconstructs `AppState.admin_dispute_chats` so the "Disputes in Progress" tab immediately shows prior messages.
  - Updates in‑memory `admin_chat_last_seen` entries for Buyer and Seller based on file timestamps.
- Subsequent background NIP‑59 fetches use the stored `buyer_chat_last_seen` / `seller_chat_last_seen` values as cursors, ensuring:
  - **Instant UI restore** after restart.
  - **Incremental network sync** without replaying the full chat history from relays.

## Main Event Loop

The TUI runs in a `tokio::select!` loop that handles three types of events:
1. **Order Results**: Results from asynchronous order-related operations.
2. **Message Notifications**: New direct messages from counterparties or Mostro.
3. **User Input**: Keyboard and paste events processed via `key_handler.rs`.
4. **UI Refresh**: Periodic ticks to ensure the UI stays up-to-date.

**Source**: `src/main.rs:184`
```184:279:src/main.rs
    loop {
        tokio::select! {
            result = order_result_rx.recv() => {
                // ... handle order results ...
            }
            notification = message_notification_rx.recv() => {
                // ... handle notifications ...
            }
            maybe_event = events.next() => {
                // ... handle keyboard/paste events ...
            }
            _ = refresh_interval.tick() => {
                // Refresh the UI
            }
        }
        // ... UI drawing ...
        terminal.draw(|f| ui_draw(f, &app, &orders, Some(&status_line)))?;
    }
```
