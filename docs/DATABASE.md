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
3. Creates the necessary tables (`users` and `orders`)
4. Generates a new 12-word BIP-39 mnemonic if no user exists
5. Creates the initial user record

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

## Tables

### 1. `users` Table

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

### 2. `orders` Table

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

### Trade Index and Keys

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

## Stateless Message Recovery

Mostrix uses a "fetch-on-startup" strategy rather than storing all messages locally:

1. **No Message Database**: Messages are not stored in the database
2. **Active Order Tracking**: Only order IDs and trade keys are persisted
3. **Startup Sync**: On startup, the client:
   - Loads all orders from the database
   - Re-derives trade keys for each order
   - Queries Nostr relays for recent messages
   - Reconstructs the current state from the latest messages

This approach ensures:

- **Always Up-to-Date**: State is synchronized with Mostro daemon on every startup
- **No State Drift**: Local state matches what Mostro knows
- **Simpler Architecture**: No need to maintain a message database

For more details, see [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md#stateless-recovery).

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

This is the first version of the database schema. As Mostrix evolves, additional tables or fields may be added for:

- **Dispute Management**: Storing dispute information and resolution history
- **Admin State**: Admin-specific data and solver information
- **Message Caching**: Optional local message cache for offline access
- **Settings Persistence**: User preferences and UI state
- **Analytics**: Trade history and statistics (privacy-preserving)

When new tables or fields are added, this documentation will be updated accordingly.

## Related Documentation

- [KEY_MANAGEMENT.md](KEY_MANAGEMENT.md) - Key derivation and management
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) - Message handling and stateless recovery
- [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) - Database initialization during startup
