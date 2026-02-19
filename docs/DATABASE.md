# Database Schema and Persistence

This guide documents the database structure used by Mostrix to persist critical local state. The database is essential for key management, trade recovery, and maintaining order history.

## Database Location

Mostrix uses SQLite for local data persistence. The database file is located at:

```text
~/.mostrix/mostrix.db
```

The database is automatically created on first startup if it doesn't exist.

**Source**: `src/db.rs:14`

```14:15:src/db.rs
    let app_dir = home_dir.join(format!(".{}", name));
    let db_path = app_dir.join(format!("{}.db", name));
```

## Database Initialization

On first startup, Mostrix:

1. Creates the `~/.mostrix/` directory if it doesn't exist
2. Creates the SQLite database file
3. Creates the necessary tables:
   - **User Mode Tables**: `users` and `orders`
   - **Admin Mode Tables**: `admin_disputes`
4. Generates a new 12-word BIP-39 mnemonic if no user exists
5. Creates the initial user record
6. Runs database migrations for existing databases (if needed)

**Source**: `src/db.rs:66`

```66:73:src/db.rs
        // Check if a user exists, if not, create one
        let user_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await?;
        if user_count.0 == 0 {
            let mnemonic = Mnemonic::generate(12)?.to_string();
            User::new(mnemonic, &pool).await?;
        }
```

### Database Migrations

For existing databases, Mostrix automatically runs migrations to add new columns or update the schema as needed. Migrations are:

- **Atomic**: All migration steps are wrapped in transactions to ensure consistency.
- **Safe**: Migrations check for column existence before attempting to add them.
- **Error-aware**: Non-column-related errors (connection issues, table missing, etc.) are properly propagated rather than triggering incorrect migrations.

Recent migrations for the `admin_disputes` table add the following fields:

- **`initiator_info` / `counterpart_info`**: JSON-encoded user info for each party.
- **`fiat_code`**: Fiat currency code for the disputed order.
- **`dispute_id`**: Persistent dispute identifier (separate from order `id`).
- **`buyer_chat_last_seen` / `seller_chat_last_seen`**: Per‑party chat cursor used for incremental NIP‑59 fetches and chat restore at startup.
- **`buyer_shared_key_hex` / `seller_shared_key_hex`**: Hex‑encoded per‑dispute shared keys derived between the admin key and each party’s trade pubkey, used as the identity for the shared‑keys admin chat system.

**Source**: `src/db.rs:113`

```113:238:src/db.rs
/// Run database migrations for existing databases
async fn migrate_db(pool: &SqlitePool) -> Result<()> {
    // Migration: Add initiator_info and counterpart_info columns if they don't exist
    // Check if columns exist by attempting to query them and checking for specific SQLite errors
    async fn check_column_exists(pool: &SqlitePool, column_name: &str) -> Result<bool> {
        // ...
    }

    // Check if columns exist
    let has_initiator_info = check_column_exists(pool, "initiator_info").await?;
    let has_counterpart_info = check_column_exists(pool, "counterpart_info").await?;
    let has_fiat_code = check_column_exists(pool, "fiat_code").await?;
    let has_dispute_id = check_column_exists(pool, "dispute_id").await?;
    let has_buyer_chat_last_seen = check_column_exists(pool, "buyer_chat_last_seen").await?;
    let has_seller_chat_last_seen = check_column_exists(pool, "seller_chat_last_seen").await?;

    // Only run migration if at least one column is missing
    if !has_initiator_info
        || !has_counterpart_info
        || !has_fiat_code
        || !has_dispute_id
        || !has_buyer_chat_last_seen
        || !has_seller_chat_last_seen
    {
        log::info!("Running migration: Adding missing columns to admin_disputes table");

        // Wrap all ALTER TABLE statements in a transaction for atomicity
        let mut tx = pool.begin().await?;

        // ... ALTER TABLE statements for each missing column ...

        tx.commit().await?;
        log::info!("Migration completed successfully");
    }

    Ok(())
}
```

Migrations are automatically executed when an existing database is detected during startup.

## Mode Separation

Mostrix operates in two distinct modes, each using different database tables:

- **User Mode**: Uses `users` and `orders` tables for trading operations
- **Admin Mode**: Uses `admin_disputes` table for dispute resolution

The tables are designed to be independent, allowing the same database to support both user and admin functionality.

## Tables

### User Mode Tables

#### 1. `users` Table

Stores the user's identity and key derivation state.

**Schema**:

```sql
CREATE TABLE IF NOT EXISTS users (
    i0_pubkey char(64) PRIMARY KEY,
    mnemonic TEXT,
    last_trade_index INTEGER,
    created_at INTEGER
);
```

**Source**: `src/db.rs:55`

#### User Table Fields

| Field | Type | Description |
|-------|------|-------------|
| `i0_pubkey` | `char(64)` | Primary key. The public key derived from the identity key (index 0). Used as the unique identifier for the user. |
| `mnemonic` | `TEXT` | The 12-word BIP-39 mnemonic phrase. **Critical**: This is the root of all key derivation. Must be kept secure. |
| `last_trade_index` | `INTEGER` | The highest trade index that has been used. Used to ensure each new trade gets a unique key. Starts at `NULL` (treated as 0 or 1). |
| `created_at` | `INTEGER` | Unix timestamp when the user record was created. |

#### Purpose

The `users` table is critical for:

- **Key Recovery**: The mnemonic allows recovery of all derived keys
- **Trade Index Tracking**: `last_trade_index` ensures deterministic key derivation for each trade
- **Identity Management**: `i0_pubkey` identifies the user's primary Nostr identity

#### Data Persistence

- **Mnemonic**: Stored in plain text (encrypted at the filesystem level if the OS supports it). This is necessary for key derivation.
- **Trade Index**: Updated every time a new order is created or taken to ensure no key reuse.

**Source**: `src/util/db_utils.rs:25`

```25:27:src/util/db_utils.rs
                if let Err(e) = User::update_last_trade_index(pool, trade_index).await {
                    log::error!("Failed to update user: {}", e);
                }
```

#### 2. `orders` Table

Stores order information and associated trade keys for active orders.

**Schema**:

```sql
CREATE TABLE IF NOT EXISTS orders (
    id TEXT PRIMARY KEY,
    kind TEXT,
    status TEXT,
    amount INTEGER NOT NULL,
    fiat_code TEXT NOT NULL,
    min_amount INTEGER,
    max_amount INTEGER,
    fiat_amount INTEGER NOT NULL,
    payment_method TEXT NOT NULL,
    premium INTEGER NOT NULL,
    trade_keys TEXT,
    counterparty_pubkey TEXT,
    is_mine INTEGER NOT NULL,
    buyer_invoice TEXT,
    request_id INTEGER,
    created_at INTEGER,
    expires_at INTEGER
);
```

**Source**: `src/db.rs:36`

#### Order Table Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | `TEXT` | Primary key. UUID of the order. |
| `kind` | `TEXT` | Order kind: "Buy" or "Sell". |
| `status` | `TEXT` | Current order status (e.g., "Pending", "Active", "Dispute"). |
| `amount` | `INTEGER` | Amount in satoshis. |
| `fiat_code` | `TEXT` | Fiat currency code (e.g., "USD", "EUR"). |
| `min_amount` | `INTEGER` | Minimum amount for range orders (NULL for fixed orders). |
| `max_amount` | `INTEGER` | Maximum amount for range orders (NULL for fixed orders). |
| `fiat_amount` | `INTEGER` | Amount in fiat currency (smallest unit, e.g., cents). |
| `payment_method` | `TEXT` | Payment method (comma-separated list of methods). |
| `premium` | `INTEGER` | Premium amount in satoshis. |
| `trade_keys` | `TEXT` | **Critical**: The trade keys (secret key in hex) for this order. Used to decrypt messages and sign actions for this specific trade. |
| `counterparty_pubkey` | `TEXT` | Public key of the counterparty (buyer or seller) when a trade is active. |
| `is_mine` | `INTEGER` | Boolean (0 or 1). Indicates if this order was created by the local user. |
| `buyer_invoice` | `TEXT` | Lightning invoice provided by the buyer (if applicable). |
| `request_id` | `INTEGER` | Request ID used when creating the order (for tracking responses). |
| `created_at` | `INTEGER` | Unix timestamp when the order was created. |
| `expires_at` | `INTEGER` | Unix timestamp when the order expires (if applicable). |

#### Purpose

The `orders` table is essential for:

- **Trade Key Persistence**: Stores the trade keys needed to decrypt messages and sign actions for each active trade
- **Order Recovery**: Allows the client to recover active orders on startup
- **State Synchronization**: Enables the "fetch-on-startup" strategy to sync with Mostro daemon
- **Trade History**: Maintains a local record of orders and trades

#### Data Persistence

- **Trade Keys**: Stored as hex-encoded secret keys. **Critical security data** - these keys are needed to decrypt messages for each trade.
- **Order Updates**: Orders are updated (not just inserted) when status changes, using upsert logic.

**Source**: `src/models.rs:154`

### Admin Mode Tables

#### 3. `admin_disputes` Table

Stores dispute information received from Mostro when an admin takes a dispute. This table is used exclusively in admin mode to track and manage disputes that the admin has taken responsibility for resolving.

**Schema** (simplified, see `src/db.rs` for full definition):

```sql
CREATE TABLE IF NOT EXISTS admin_disputes (
    id TEXT PRIMARY KEY,
    dispute_id TEXT NOT NULL,
    kind TEXT,
    status TEXT,
    hash TEXT,
    preimage TEXT,
    order_previous_status TEXT,
    initiator_pubkey TEXT NOT NULL,
    buyer_pubkey TEXT,
    seller_pubkey TEXT,
    initiator_full_privacy INTEGER NOT NULL,
    counterpart_full_privacy INTEGER NOT NULL,
    initiator_info TEXT,
    counterpart_info TEXT,
    premium INTEGER NOT NULL,
    payment_method TEXT NOT NULL,
    amount INTEGER NOT NULL,
    fiat_amount INTEGER NOT NULL,
    fiat_code TEXT NOT NULL,
    fee INTEGER NOT NULL,
    routing_fee INTEGER NOT NULL,
    buyer_invoice TEXT,
    invoice_held_at INTEGER,
    taken_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    buyer_chat_last_seen INTEGER,
    seller_chat_last_seen INTEGER,
    buyer_shared_key_hex TEXT,
    seller_shared_key_hex TEXT
);
```

**Source**: `src/db.rs` and `SolverDisputeInfo` struct from Mostro protocol

#### Dispute Table Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | `TEXT` | Primary key. UUID of the **order** associated with this dispute. This is also the ID sent to Mostro when performing admin finalization actions (AdminSettle/AdminCancel). |
| `kind` | `TEXT` | Order kind: "Buy" or "Sell". |
| `status` | `TEXT` | Current dispute status (e.g., "WaitingBuyer", "WaitingSeller", "Resolved"). |
| `hash` | `TEXT` | Lightning invoice hash (if applicable). NULL if not available. |
| `preimage` | `TEXT` | Lightning invoice preimage (if available). NULL if not available. |
| `order_previous_status` | `TEXT` | The order's status before the dispute was initiated. |
| `initiator_pubkey` | `TEXT` | Public key of the user who initiated the dispute. |
| `buyer_pubkey` | `TEXT` | Public key of the buyer (if applicable). NULL if not available. |
| `seller_pubkey` | `TEXT` | Public key of the seller (if applicable). NULL if not available. |
| `initiator_full_privacy` | `INTEGER` | Boolean (0 or 1). Indicates if the initiator is using full privacy mode. |
| `counterpart_full_privacy` | `INTEGER` | Boolean (0 or 1). Indicates if the counterparty is using full privacy mode. |
| `initiator_info` | `TEXT` | JSON-encoded `UserInfo` struct for the initiator (if available). NULL if not available. |
| `counterpart_info` | `TEXT` | JSON-encoded `UserInfo` struct for the counterparty (if available). NULL if not available. |
| `premium` | `INTEGER` | Premium amount in satoshis. |
| `payment_method` | `TEXT` | Payment method used for the order. |
| `amount` | `INTEGER` | Amount in satoshis. |
| `fiat_amount` | `INTEGER` | Amount in fiat currency (smallest unit, e.g., cents). |
| `fiat_code` | `TEXT` | Fiat currency code (e.g., "USD", "EUR") for the disputed order. |
| `fee` | `INTEGER` | Fee amount in satoshis. |
| `routing_fee` | `INTEGER` | Lightning routing fee in satoshis. |
| `buyer_invoice` | `TEXT` | Lightning invoice provided by the buyer (if applicable). NULL if not available. |
| `invoice_held_at` | `INTEGER` | Unix timestamp when the invoice was held/created (if available). |
| `taken_at` | `INTEGER` | Unix timestamp when the admin took the dispute. |
| `created_at` | `INTEGER` | Unix timestamp when the dispute was created. |
| `buyer_chat_last_seen` | `INTEGER` | Last processed NIP‑59 chat timestamp for the buyer side (used for incremental fetch and restore). |
| `seller_chat_last_seen` | `INTEGER` | Last processed NIP‑59 chat timestamp for the seller side (used for incremental fetch and restore). |
| `buyer_shared_key_hex` | `TEXT` | Hex‑encoded shared key (secret) derived via ECDH between the admin key and the buyer’s trade pubkey; used as the identity for buyer‑side admin chat. |
| `seller_shared_key_hex` | `TEXT` | Hex‑encoded shared key (secret) derived via ECDH between the admin key and the seller’s trade pubkey; used as the identity for seller‑side admin chat. |

#### Purpose

The `admin_disputes` table is essential for:

- **Dispute Tracking**: Maintains a local record of all disputes the admin has taken.
- **State Persistence**: Allows the admin to see active disputes across application restarts.
- **Resolution Context**: Stores all necessary information for resolving disputes (parties, amounts, invoices, etc.).
- **Privacy Mode Tracking**: Records which parties are using full privacy mode, which affects communication methods.
- **Chat Restore Cursors**: `buyer_chat_last_seen` and `seller_chat_last_seen` persist the last processed NIP‑59 timestamps so that admin chat can resume incrementally after restart without replaying the full history.

#### Data Persistence

- **Dispute Reception**: When an admin takes a dispute, Mostro sends a `SolverDisputeInfo` message containing all dispute details
- **Local Storage**: The dispute information is stored locally in this table for quick access
- **Status Updates**: The dispute status is updated as the resolution process progresses
- **JSON Fields**: `initiator_info` and `counterpart_info` are stored as JSON-encoded strings for complex nested data

**Data Validation**:

When saving a dispute to the database, the following fields are validated:

- **Required Fields**: `buyer_pubkey` and `seller_pubkey` must be present. If either is missing, the dispute cannot be saved and an error is returned. This ensures data integrity and prevents incomplete dispute records.
- **Validation Location**: Validation occurs in `AdminDispute::new()` before any database operations.

**Note**: The `admin_disputes` table is populated when an admin takes a dispute from the Mostro network. The admin receives a `SolverDisputeInfo` struct via direct message from Mostro, which is then persisted to this table.

**Source**: `SolverDisputeInfo` struct definition (see [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md#dispute-information-structure))

**Source**: `src/models.rs:154`

```154:172:src/models.rs
        // Try insert; if id already exists, perform an update instead
        let insert_result = order.insert_db(pool).await;

        if let Err(e) = insert_result {
            // If the error is due to unique constraint (id already present), update instead
            let is_unique_violation = match e.as_database_error() {
                Some(db_err) => {
                    let code = db_err.code().map(|c| c.to_string()).unwrap_or_default();
                    code == "1555" || code == "2067"
                }
                None => false,
            };

            if is_unique_violation {
                order.update_db(pool).await?;
            } else {
                return Err(e.into());
            }
        }
```

## Key Data Relationships

### User Mode: Trade Index and Keys

The relationship between `users.last_trade_index` and `orders.trade_keys` is critical:

1. **Order Creation**: When creating a new order:
   - `last_trade_index` is read from the `users` table
   - A new trade key is derived using `trade_index = last_trade_index + 1`
   - The trade keys are stored in the `orders` table
   - `last_trade_index` is updated in the `users` table

2. **Trade Recovery**: On startup:
   - All orders are loaded from the `orders` table
   - Trade keys are retrieved for each active order
   - The client can decrypt messages and interact with active trades

**Source**: `src/util/order_utils/send_new_order.rs:84`

```84:87:src/util/order_utils/send_new_order.rs
    let user = User::get(pool).await?;
    let next_idx = user.last_trade_index.unwrap_or(1) + 1;
    let trade_keys = user.derive_trade_keys(next_idx)?;
    let _ = User::update_last_trade_index(pool, next_idx).await;
```

## Message Recovery Strategy

Mostrix uses a hybrid message recovery strategy that combines stateless fetch-on-startup for trade messages with lightweight state for admin chat:

- **Orders and Trades**:
  - Messages are not stored in the database.
  - Only order IDs and trade keys are persisted.
  - On startup the client:
    - Loads all orders from the database.
    - Re-derives trade keys for each order.
    - Queries Nostr relays for recent messages.
    - Reconstructs the current state from the latest messages.

- **Admin Chat (Disputes in Progress)**:
  - Per‑dispute chat transcripts are stored as human‑readable text files:

    ```text
    ~/.mostrix/<dispute_id>.txt
    ```

  - Each file contains a chronological log of messages with headers like `Admin to Buyer - dd-mm-yyyy - HH:MM:SS`.
  - At startup, `recover_admin_chat_from_files` rebuilds `admin_dispute_chats` in memory from these files and computes the latest buyer/seller timestamps.
  - These timestamps are persisted in `admin_disputes.buyer_chat_last_seen` and `admin_disputes.seller_chat_last_seen` via `update_chat_last_seen_by_dispute_id` (unified function that handles both parties based on an `is_buyer` flag and returns affected row count).
  - Background NIP‑59 fetches use the stored timestamps as cursors (7-day rolling window) to request only newer events, providing:
    - **Instant UI restore** for existing disputes.
    - **Incremental network sync** without replaying full history.

This approach keeps the core message flow largely stateless while giving admin chat a robust, restart‑safe experience.

For more details, see:

- `recover_admin_chat_from_files` and `apply_admin_chat_updates` in `src/ui/helpers.rs`.
- `update_chat_last_seen_by_dispute_id` in `src/models.rs` (unified DB update with row-affected verification).
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md#stateless-recovery) for protocol‑level behavior.

## Security Considerations

### Sensitive Data

The database contains highly sensitive information:

1. **Mnemonic Phrase**: The root of all key derivation. If compromised, all keys can be derived.
2. **Trade Keys**: Secret keys for each active trade. If compromised, an attacker can decrypt messages and sign actions for those trades.

### Protection Measures

- **File Permissions**: The database file should have restrictive permissions (readable/writable only by the user)
- **Filesystem Encryption**: Consider using encrypted filesystems or disk encryption
- **Backup Security**: If backing up the database, ensure backups are encrypted
- **No Network Exposure**: The database is local-only and never exposed to the network

## Database Operations

### Common Operations

1. **User Creation**: `User::new()` - Creates a new user with a generated mnemonic
2. **User Retrieval**: `User::get()` - Gets the single user record
3. **Trade Index Update**: `User::update_last_trade_index()` - Updates the trade index counter
4. **Order Creation**: `Order::new()` - Creates or updates an order record
5. **Order Retrieval**: `Order::get_by_id()` - Retrieves an order by ID

**Source**: `src/models.rs` for all database operation implementations.

## Future Evolution

As Mostrix evolves, additional tables or fields may be added for:

- **Message Caching**: Optional local message cache for offline access
- **Settings Persistence**: User preferences and UI state
- **Analytics**: Trade history and statistics (privacy-preserving)
- **Dispute Resolution History**: Tracking resolution actions and outcomes
- **Admin State**: Additional admin-specific data and solver information

When new tables or fields are added, this documentation will be updated accordingly.

## Related Documentation

- [KEY_MANAGEMENT.md](KEY_MANAGEMENT.md) - Key derivation and management
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) - Message handling and stateless recovery
- [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) - Database initialization during startup
- [ADMIN_DISPUTES.md](ADMIN_DISPUTES.md) - Admin mode dispute resolution workflows and dispute information structure
