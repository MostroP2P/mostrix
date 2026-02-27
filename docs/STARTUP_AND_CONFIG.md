# Startup and Configuration

This guide explains Mostrix’s boot sequence and configuration surfaces.

## Overview
- Entry: `src/main.rs:98`
- Initializes settings, database, logger, terminal (raw mode), shared state, Nostr client, and background tasks.
- Enters the main event loop to handle UI updates and user input.

## Initialization Sequence

### 1. Settings Initialization
Mostrix uses a centralized settings management in `src/settings.rs`.

**Source**: `src/settings.rs:39`
```39:71:src/settings.rs
pub fn init_settings() -> &'static Settings {
    SETTINGS.get_or_init(|| {
        // HOME and package name at compile time
        let home_dir = dirs::home_dir().expect("Could not find home directory");
        let package_name = env!("CARGO_PKG_NAME");
        let hidden_dir = home_dir.join(format!(".{package_name}"));
        let hidden_file = hidden_dir.join("settings.toml");

        println!("hidden_file: {:?}", hidden_file);

        // Path to the settings.toml included in the repo (next to Cargo.toml)
        let default_file: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("settings.toml");

        // Create ~/.mostrix if it doesn't exist
        if !hidden_dir.exists() {
            fs::create_dir(&hidden_dir).expect("The configuration directory could not be created");
        }

        // Copy settings.toml if it isn't already in ~/.mostrix
        if !hidden_file.exists() {
            fs::copy(&default_file, &hidden_file).expect("Could not copy default settings.toml");
        }

        // Use the `config` crate to deserialize to the Settings struct
        let cfg = config::Config::builder()
            .add_source(config::File::from(hidden_file.as_path()))
            .build()
            .map_err(|e| anyhow::anyhow!("settings.toml malformed: {}", e))?;

        cfg.try_deserialize::<Settings>()
            .map_err(|e| anyhow::anyhow!("Error deserializing settings.toml: {}", e))
    })
}
```
- Creates `~/.mostrix/` directory if it doesn't exist.
- Copies the default `settings.toml` from the project root if missing.
- Loads configuration using the `config` crate.

**Error Handling**: If settings initialization fails at runtime (e.g., settings accessed before initialization), the application will display user-friendly error messages via `OperationResult::Error` instead of panicking. This ensures graceful degradation and clear feedback to users.

### 2. Database Initialization
The database is initialized at startup to ensure the schema is ready.

**Source**: `src/db.rs:9`
```9:79:src/db.rs
pub async fn init_db() -> Result<SqlitePool> {
    let pool: SqlitePool;
    let name = env!("CARGO_PKG_NAME");
    // ... path construction ...
    if !app_dir.exists() {
        std::fs::create_dir_all(&app_dir)?;
    }

    if !Path::exists(Path::new(&db_path)) {
        if let Err(res) = File::create(&db_path) {
            println!("Error in creating db file: {}", res);
            return Err(res.into());
        }

        pool = SqlitePool::connect(&db_url).await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS orders (
                // ... schema ...
            );
            CREATE TABLE IF NOT EXISTS users (
                // ... schema ...
            );
            "#,
        )
        .execute(&pool)
        .await?;

        // Check if a user exists, if not, create one
        let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await?;
        if user_count.0 == 0 {
            let mnemonic = Mnemonic::generate(12)?.to_string();
            User::new(mnemonic, &pool).await?;
        }
    } else {
        pool = SqlitePool::connect(&db_url).await?;

        // Run migrations for existing databases
        migrate_db(&pool).await?;
    }

    Ok(pool)
}
```
- Creates the SQLite database file at `~/.mostrix/mostrix.db`.
- Executes `CREATE TABLE` queries if it's a new database.
- Generates a new BIP-39 mnemonic if no user exists in the `users` table.
- Runs database migrations automatically for existing databases (adds new columns, updates schema as needed).

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
```8:18:src/settings.rs
#[derive(Debug, Deserialize, Serialize)]
pub struct Settings {
    pub mostro_pubkey: String,
    pub nsec_privkey: String,
    pub admin_privkey: String,
    pub relays: Vec<String>,
    pub log_level: String,
    pub currencies: Vec<String>,
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
- **`currencies`**: A list of fiat currencies the user is interested in.
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
