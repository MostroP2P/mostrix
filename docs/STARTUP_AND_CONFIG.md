# Startup and Configuration

This guide explains Mostrix’s boot sequence and configuration surfaces.

## Overview
- Entry: `src/main.rs:98`
- Initializes database, derives identity keys, initializes settings, then logger, terminal (raw mode), shared state, Nostr client, and background tasks.
- Shows an animated **startup splash** (Mostro wordmark + loading dots) while post-terminal init runs; see [Startup splash](#startup-splash) below.
- Enters the main event loop to handle UI updates and user input.

## Startup splash

After the terminal enters alternate screen mode, Mostrix draws a full-screen splash until background boot work finishes (`src/startup.rs`, `src/ui/startup_splash.rs`).

- **Wordmark**: multi-line logo from the project art (same style as the desktop `logo.txt` export).
- **Loading dots**: reusable glyph `<>` repeated 1–4 times on the last logo row, each prefixed by a space, cycled every ~400 ms while the splash tick runs (~150 ms redraw interval).
- **Phase text**: short status under the art (`Starting…`, `Connecting to relays…`, `Loading market data…`, `Restoring chats…`, `Almost ready…`) updated as init steps complete.
- **Minimum display**: splash stays visible for at least ~800 ms so fast boots do not flash the screen.
- **Narrow terminals**: if the terminal is narrower than the padded logo width, a one-line `mostro is loading` + dots + phase is shown instead.
- **Background**: full-screen fill via `fill_splash_background` (solid `BACKGROUND_COLOR` block, same pattern as the Exit tab) so ASCII art and status text do not show mismatched per-span backgrounds.
- **CI / scripts**: set `MOSTRIX_NO_SPLASH=1` to skip the splash loop and run init directly.

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
    #[serde(default)]
    pub blossom_servers: Vec<String>, // Blossom upload hosts; empty = built-in defaults
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
- **`blossom_servers`**: Optional list of HTTPS Blossom bases for **My Trades attachment upload** (**Ctrl+O** send). When empty, Mostrix uses `DEFAULT_BLOSSOM_SERVERS` in `src/util/blossom.rs` (same defaults as Mostro Mobile). Example in repo `settings.toml`: commented `# blossom_servers = ["https://blossom.primal.net", …]`. Resolved at send time via `blossom_servers_from_settings` in `src/util/send_attachment.rs` (main loop reloads settings from disk when draining the send queue).

Proof-of-work for published events is taken from the Mostro instance status event (kind 38385, tag `pow`), not from `settings.toml`.

### Mostro instance info (kind 38385)

Background and manual refresh (Mostro Info tab → Enter) fetch the daemon status event and update UI state:

- **`AppState.mostro_info`**: parsed tags (`pow`, `bond_enabled`, `protocol_version`, LND metadata, …) — see [`mostro_info_from_tags`](../src/util/mostro_info.rs).
- **`AppState.transport`**: resolved wire transport for **protocol DMs** via [`transport_from_instance`](../src/util/mostro_info.rs). Updated through [`AppState.set_mostro_info`](../src/ui/app_state.rs) (startup await, main loop `MostroInfoFetchResult`, reconnect, invalid-pubkey clear).
- **Startup**: when relays are reachable, [`run_post_terminal_startup`](../src/startup.rs) **awaits** [`fetch_mostro_instance_info`](../src/util/mostro_info.rs) before spawning the DM listener so the first subscription uses the correct transport (v1 GiftWrap or v2 kind 14). On fetch failure or offline boot, transport defaults to GiftWrap.

Displayed on the **Mostro Info** tab: protocol version (`1` / `2` / unknown) and wire transport label (GiftWrap vs NIP-44 direct).

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
5. **Shared-key chat subscription router** (`listen_for_chat_messages` in `src/util/chat_listener.rs`):
   - A **single long-lived task** (spawned in `src/startup.rs`, respawned on reconnect/key reload) maintains batched live subscriptions over **all** tracked chats — kind 14 `authors = [pub(K_sign)]`, plus GiftWrap `#p` while `CHAT_ACCEPT_LEGACY_GIFTWRAP` is true — the same model as Mostro Mobile's `SubscriptionManager`. No timed polling.
   - **Track/untrack** via the global command channel (`ChatRouterCmd`, published by `set_chat_router_cmd_tx`). Helpers: `track_order_chat` / `untrack_order_chat` / `track_dispute_chat` / `untrack_dispute_chat`.
   - On `TrackChatKey`, the router hydrates history once via dual-read fetch (`fetch_chat_messages_for_shared_key`: kind 14 by `pub(K_sign)`, plus GiftWrap `#p` while `CHAT_ACCEPT_LEGACY_GIFTWRAP` is true; filtered by the last-seen cursor; unwrapped with the track's inner-signer allow-list). Buffered track/untrack commands are drained and applied before a **single** batched resubscribe (idempotent re-tracks are no-ops). Subscribe failures retry with short backoff. Empty `allowed_signers` lists are not tracked.
   - Live traffic is kind 14, routed by the derived `pub(K_sign)` outer author. While `CHAT_ACCEPT_LEGACY_GIFTWRAP` is true (mostrix#102 dual-read window), legacy `kind: 1059` GiftWraps are also routed by the event’s `#p` tag (ECDH shared pubkey). Both are decrypted with `unwrap_giftwrap_with_shared_key` against the track’s inner-signer allow-list and emitted on the existing `admin_chat_updates` / `user_order_chat_updates` channels. Outbound chat is always kind 14.
   - **Application**: `ui::helpers::apply_admin_chat_updates` / `apply_user_order_chat_updates` (in `src/ui/helpers/startup.rs`) merge updates, persist cursors (`buyer_chat_last_seen` / `seller_chat_last_seen`, order transcripts), and dedupe. Unknown dispute-chat inner signers are dropped (not labeled Admin).
   - **Startup track set (option B)**: `track_startup_chats` emits tracks for `Order::get_startup_active_orders` rows (active + `success`) with a resolvable shared key **and** counterparty trade pubkey (User), and each `InProgress` dispute's buyer/seller shared key plus party/admin allow-list (Admin).
   - **Untrack** when an order reaches `TERMINAL_DM_STATUSES` or its row is removed/reverted (DM router hooks), or a dispute leaves `InProgress`. Chat is **not** untracked on `success`; on-disk transcripts preserve history for untracked terminal orders.

**Source**: `src/util/chat_listener.rs` (router), `src/startup.rs` + `src/ui/helpers/startup.rs` (`track_startup_chats`), `src/util/dm_utils/mod.rs` (track/untrack hooks), `src/ui/helpers/startup.rs` (`apply_admin_chat_updates`)

6. **DM Router Wiring (trade messages)**:
   - App channel creation includes `dm_subscription_tx` / `dm_subscription_rx`.
   - `set_dm_router_cmd_tx(dm_subscription_tx.clone())` publishes the sender globally for `wait_for_dm` (returns `Result`; startup fails fast if the mutex is poisoned).
   - Before spawning the listener, `hydrate_startup_active_order_dm_state` loads non-terminal orders from SQLite and returns `active_order_trade_indices` plus `order_last_seen_dm_ts` cursors; `main.rs` seeds the shared active-order map.
   - `listen_for_order_messages(client, mostro_pubkey, transport, pool, …, order_last_seen_dm_ts, …, dm_subscription_rx)` runs as the single router loop consuming:
     - `TrackOrder` commands for long-lived trade subscriptions.
     - `RegisterWaiter` commands for one-shot request/response waits.
   - After bootstrapping per-order protocol-DM subscriptions (`ensure_order_dm_subscription`), the listener performs a **`fetch_events` replay** (`fetch_and_replay_startup_trade_dms`) so the Messages UI is populated from relay history (in-memory messages are not stored in the DB). Replay uses `notify: false` to avoid duplicate popups/badge noise.
   - **Startup transport:** `startup.rs` awaits instance info when relays are reachable, then spawns the listener with resolved `app.transport`.
   - **Reload / reconnect transport:** [`dm_transport_for_mostro`](../src/ui/key_handler/async_tasks.rs) re-fetches instance info and updates `app.transport` **before** respawning the listener (key reload, fetch-scheduler reload, network reconnect).
   - This unifies in-flight response handling and background trade notifications on top of one notification stream.

See **[DM_LISTENER_FLOW.md](DM_LISTENER_FLOW.md)** for `DmSubscriptionMode` (`StartupCatchUp`, `StartupSince`, `LiveOnly`), [`filter_protocol_dm_from_mostro`](../src/util/filters.rs), waiter vs `TrackOrder` ordering, and replay details.

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
- Disk restore via `load_user_order_chats_at_startup` seeds `AppState.order_chats` and `order_chat_last_seen`. Relay history is hydrated once by the **shared-key chat subscription router** when `track_startup_chats` emits `TrackChatKey` — no separate startup poll and no timed polling.
- `apply_user_order_chat_updates` on the **peer** channel maps relay rows from the local trade key to **You** unless the inner event id is already known or an optimistic **You** line exists at the same timestamp (live-send echo). The **solver** channel still skips all local-trade-key rows. Peer dedup matches only existing **Peer** rows at the same `(timestamp, content)` (or same attachment / legacy placeholder) so an optimistic **You** line cannot hide a real counterparty message in the same second. See [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) — "User order chat local cache".
- After **session restore** (Settings → Restore Session, without restart), peer transcripts are rebuilt from relay in the background and `track_startup_chats` is re-emitted when hydrate completes — see [Session restore hydrate](#session-restore-hydrate-without-restart) below.

### Session restore hydrate (without restart)

When the user confirms **Restore Session** (`execute_restore_session` → `Action::RestoreSession`), Mostro returns the identity's active orders and disputes; Mostrix rebuilds SQLite and must re-hydrate in-memory UI state **without** a full app restart.

**Entry**: Settings → **Restore Session** (`UiMode::ConfirmRestoreSession`) or first-launch restore path in `main.rs`. Success MUST emit [`OperationResult::SessionRestored`](../src/ui/orders.rs) (not plain `Info`) so `apply_order_result` runs the hydrate pipeline (`restore_completion_result` in `src/util/order_utils/execute_restore.rs`).

**Synchronous steps** (`apply_order_result` on `SessionRestored`, User role):

1. **`clear_session_chat_projection`** — wipe in-memory peer/solver chat transcripts, `order_chat_last_seen` / `user_dispute_chat_last_seen`, maker-book cache, attachment send state, and related maps. Stale cursors from the prior identity would otherwise bound relay fetches incorrectly; empty transcripts plus old echo-skip rules would drop the user's own relay rows during rebuild. Also called from **`clear_runtime_session_state`** after seed import / key reload (`src/ui/key_handler/async_tasks.rs`).
2. **DB → UI projection** — `refresh_my_trades_maker_book_cache` + `sync_user_order_history_messages_from_db` (same paths as cold startup; `history_action_for_db_order` maps DB status → Messages-tab `Action`).
3. **Restore success popup** — `handle_operation_result` shows the `SessionRestored` message.

**Background hydrate** (non-blocking; unified orchestrator in `src/ui/helpers/startup.rs`):

- **`spawn_post_restore_hydrate`** runs **`hydrate_after_session_restore`** in a `tokio::spawn` task. Trade-DM replay and peer-chat rebuild run **in parallel** (`tokio::join!`).
- **Trade protocol DMs (Messages tab, pipe A)** — `prepare_post_restore_trade_dm_replay` seeds `active_order_trade_indices` / `startup_popup_floor_ts`, then **`replay_active_trade_dms`** (`src/util/dm_utils/mod.rs`) fetches per active order. Filter: **`trade_dm_replay_fetch_filter`** — when `last_seen_dm_ts` is missing (fresh restore / post-wipe), **no `since`** (limit-only catch-up, mirrors `DmSubscriptionMode::StartupCatchUp`); when a cursor exists, `since` + 12h lookback + GiftWrap envelope skew. Uses **`UntrackedFallback`** when the DM router has no live subscription yet. Updates `AppState.messages` in place (`notify: false`).
- **Peer order chat (My Trades chat panel, pipe B)** — **`rebuild_peer_order_chats_after_restore`** relay-fetches shared-key history per active order (`fetch_chat_messages_for_shared_key`), dedupes by inner event id, maps **You** / **Peer** from inner signer, sorts by timestamp, persists via **`rewrite_order_chat_messages`**, then records inner ids only after a successful save.
- **Completion** — always emits silent **`OperationResult::PostRestoreHydrateCompleted { report: RestoreHydrateReport }`** (aggregates `TradeDmReplaySummary` + `PeerOrderChatRestoreSummary` + hydrated order ids), even when there are zero peer-chat-eligible orders.

**Main loop on `PostRestoreHydrateCompleted`**:

1. **`apply_restored_peer_order_chats_from_disk`** — load rebuilt `orders_chat/<order_id>.txt` into `AppState.order_chats` and seed `order_chat_last_seen`.
2. **`track_startup_chats`** — re-seed the shared-key chat router so live subscriptions cover restored orders.

**Out of scope (current restore hydrate)**: admin dispute chat transcript rebuild; solver-channel post-restore relay rebuild (solver channel still uses blanket local-trade-key skip in `apply_user_order_chat_updates`).

**Manual verification** (seed import + restore without restart):

1. Import seed / confirm restore → success popup; My Trades and Messages tabs list restored orders.
2. **Messages** tab shows correct flow step per active order (trade-DM replay; no duplicate invoice popups).
3. **My Trades** peer chat shows full **You** / **Peer** history (not empty or all-Peer).
4. Send a new peer chat line — appears as **You** and is not duplicated when the relay echo arrives.
5. Optional: restore with zero peer-chat-eligible orders — app remains usable; `track_startup_chats` still runs (no stuck empty chat state).

**Source**: `src/main.rs` (`apply_order_result`), `src/ui/helpers/startup.rs` (`clear_session_chat_projection`, `hydrate_after_session_restore`, `spawn_post_restore_hydrate`, `RestoreHydrateReport`), `src/util/dm_utils/mod.rs` (`replay_active_trade_dms`, `trade_dm_replay_fetch_filter`), `src/util/order_utils/execute_restore.rs`.

## Main Event Loop

The TUI runs in a `tokio::select!` loop that handles (among others):

1. **Fatal errors**: `fatal_error_rx` — aborts background work and shows an error popup.
2. **Network status**: `network_status_rx` — offline overlay vs reconnect + runtime reload.
3. **Order / dispute / attachment / observer async results**: `order_result_rx` — `OperationResult`; includes dispute-list refresh side effects for certain `Info` messages and My Trades DB resync for `OrderHistoryDeleted`.
4. **Lightning address verify-and-save (settings)**: `ln_address_result_rx` — `LnAddressVerifyResult`; mapped to `OperationResult::Info` / `Error` and passed to **`handle_operation_result`** so UI behavior matches other operation-result popups without mixing traffic into `order_result_rx`.
5. **Key rotation / seed words / message notifications / admin & user chat fetches / Mostro instance info / user input / periodic ticks**: see `src/main.rs` (`create_app_channels` in `src/ui/key_handler/async_tasks.rs` lists all paired senders and receivers, including **`save_attachment_tx`/`rx`** for Ctrl+S downloads and **`send_order_attachment_tx`/`rx`** for outbound My Trades uploads via `SendOrderAttachmentJob`). User order chat results arrive on `user_order_chat_updates_rx` and are applied via `apply_user_order_chat_updates`.

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
    drain_save_attachment_queue(...)        // start Blossom downloads queued by Ctrl+S popups
    drain_send_order_attachment_queue(...)  // Ctrl+O / Ctrl+Shift+O jobs: encrypt → Blossom → DM
    drain_order_result_queue(...)           // apply OperationResult (e.g. "Saved to …", "Attachment sent: …")
    expire_attachment_toast(&mut app);
    terminal.draw(|f| ui_draw(f, &app, &orders, Some(&status_line)))?;
}
```

**Why drain before draw:** My Trades **Enter** on the save-attachment popup may enqueue the download asynchronously (DB lookup for decryption key). Outbound sends enqueue on `send_order_attachment_rx` the same way (`FromPath` or `RetryPrepared`). Without draining `save_attachment_rx`, `send_order_attachment_rx`, and `order_result_rx` on each frame, success/error popups (including upload-ok/send-failed) could appear only after an unrelated keypress. The **150 ms** `refresh_interval` tick plus this drain keeps attachment save and send feedback timely.
