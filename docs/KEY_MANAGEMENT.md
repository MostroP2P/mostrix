# Key Management and Identity

Mostrix strictly follows the Mostro protocol's key management specifications to ensure privacy, security, and deterministic recoverability of user accounts and trades.

## Deterministic Derivation (NIP-06)

Mostrix uses **BIP-39 mnemonics** and **NIP-06** for deterministic key derivation. All keys are derived from a single 12-word seed phrase generated upon the first startup.

### Derivation Path
The project uses the standard Mostro derivation path:
`m/44'/1237'/38383'/0/X`

Where `X` is the index:
- **`X = 0`**: Identity Key.
- **`X >= 1`**: Trade Keys.

## Identity Key (Index 0)

The **Identity Key** is the user's long-term Nostr identity. It is used for:
- Building reputation across the Mostro network.
- Signing the **Seal** (kind 13) in NIP-59 Gift Wrap events in "Normal Mode".
- Acting as the primary point of contact for the Mostro daemon for rating updates.

## Trade Keys (Index 1+)

To maximize privacy, Mostrix derives a **fresh ephemeral trade key** for every new order or taken trade.

- **Role**: Signs the **Rumor** (kind 1) inside the NIP-59 Gift Wrap.
- **Privacy**: Ensures that trades are not easily linkable to the user's primary identity by external observers.

## Admin Shared Keys for Disputes

In admin mode, Mostrix also uses **per‑dispute shared keys** for the dispute chat system. These are not derived directly from the mnemonic path above, but from an ECDH operation between the admin identity key and each party’s trade pubkey.

- **Derivation**:
  - When an admin takes a dispute, the client derives two shared secrets using:
    - The admin secret key (`admin_privkey` from `settings.toml`), and
    - The buyer’s trade pubkey / seller’s trade pubkey from the dispute.
  - ECDH is performed via `nostr_sdk::util::generate_shared_key`, and the resulting bytes are wrapped into a `Keys` instance.
  - These keys are persisted as hex‑encoded secrets in the `admin_disputes` table as:
    - `buyer_shared_key_hex`
    - `seller_shared_key_hex`

- **Usage**:
  - The shared keys act as **per‑(dispute, party) chat identities**:
    - Outgoing admin chat messages are sent as NIP‑59 `GiftWrap` events addressed to the shared key’s public key.
    - Incoming messages are fetched by querying `Kind::GiftWrap` events to that same shared key pubkey and decrypting with the shared secret.
  - Both admin and counterparty can independently derive the same shared key, mirroring the `mostro-chat` model.
  - Per‑party last‑seen timestamps (`buyer_chat_last_seen`, `seller_chat_last_seen`) are used together with these keys to implement incremental, restart‑safe admin chat sync.

- **Validation**: When saving a new dispute, if buyer and seller pubkeys differ but the two derived shared keys are identical, the client logs an error (`Shared keys for dispute … are identical for different buyer/seller pubkeys; chat may be broken`). This guards against bad relay data or parsing issues. A unit test in `src/util/chat_utils.rs` asserts that different counterparty pubkeys yield different shared keys.

## NIP-59 Gift Wrap Structure

Mostrix implements NIP-59 to communicate with the Mostro daemon. The key usage within this structure depends on the selected privacy mode.

### 1. Normal Mode (Reputation Enabled)
In this mode, Mostro can link the trade to your identity key for reputation purposes, but other Nostr users cannot.
- **Wrap (Kind 1059)**: Signed by a random ephemeral key.
- **Seal (Kind 13)**: Signed by the **Identity Key (Index 0)**.
- **Rumor (Kind 1)**: Signed by the **Trade Key (Index N)**.

### 2. Full Privacy Mode
In this mode, Mostro cannot link the trade to your identity key. You operate anonymously without reputation.
- **Wrap (Kind 1059)**: Signed by a random ephemeral key.
- **Seal (Kind 13)**: Signed by the **Trade Key (Index N)**.
- **Rumor (Kind 1)**: Signed by the **Trade Key (Index N)**.

## Trade Index Incrementation
Whenever a user creates or takes an order, the `last_trade_index` is incremented and stored in the database.

**Implementation**: `src/util/order_utils/take_order.rs:66`
```66:68:src/util/order_utils/take_order.rs
    let next_idx = user.last_trade_index.unwrap_or(1) + 1;
    let trade_keys = user.derive_trade_keys(next_idx)?;
    let _ = User::update_last_trade_index(pool, next_idx).await;
```

## Database Persistence

### Derivation Logic
The derivation logic for trade keys uses the `trade_index` as the child index in the derivation path.

**Implementation**: `src/models.rs:86`
```86:96:src/models.rs
    pub fn derive_trade_keys(&self, trade_index: i64) -> Result<Keys> {
        let account: u32 = NOSTR_ORDER_EVENT_KIND as u32;
        let keys = Keys::from_mnemonic_advanced(
            &self.mnemonic,
            None,
            Some(account),
            Some(trade_index as u32),
            Some(0),
        )?;
        Ok(keys)
    }
```

## Database Persistence

Maintaining the state of trade indices is **critical**. If the `trade_index` associated with an order is lost, the client will be unable to decrypt messages from Mostro or the counterparty for that specific trade.

### The `users` Table
The local SQLite database stores the mnemonic and the latest index used.

**Source**: `src/db.rs:55`
```55:60:src/db.rs
            CREATE TABLE IF NOT EXISTS users (
                i0_pubkey char(64) PRIMARY KEY,
                mnemonic TEXT,
                last_trade_index INTEGER,
                created_at INTEGER
            );
```

### The `orders` Table
Each order entry also stores the specific `trade_keys` (or the index) used, allowing the client to re-derive the correct key during startup synchronization or when receiving DMs.

## Stateless Recovery Strategy

Mostrix avoids storing full message histories locally. Instead, it uses the deterministic nature of the keys:
1. On startup, the client retrieves all active order IDs and their associated `trade_index` from the database.
2. It re-derives the corresponding `Trade Keys`.
3. It queries Nostr relays for recent `GiftWrap` events (NIP-59) directed to those specific trade public keys.
4. This allows the client to reconstruct the current state of any active trade without needing a heavy local message database.
