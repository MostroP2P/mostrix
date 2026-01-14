# MostriX 🧌

![Mostro-logo](static/logo.png)

**This is work in progress**

Terminal client for p2p using Mostro protocol.

![tui](static/mostrix.png)

## Requirements:

0. You need Rust version 1.70 or higher to compile.

## Install dependencies:

To compile on Ubuntu/Pop!\_OS, please install [cargo](https://www.rust-lang.org/tools/install), then run the following commands:

```bash
$ sudo apt update
$ sudo apt install -y cmake build-essential pkg-config
```

## Install

```bash
$ git clone https://github.com/MostroP2P/mostrix.git
$ cd mostrix
```

### Settings (`settings.toml`)

Mostrix is configured via a TOML file called `settings.toml`.

- On **first run**, Mostrix:
  - Creates a directory `~/.mostrix/` (or the equivalent in your home directory).
  - Copies the `settings.toml` from the project root into `~/.mostrix/settings.toml`.
- On **subsequent runs**, Mostrix only reads and writes **`~/.mostrix/settings.toml`**.

This means:

- **Before the first run**: edit the `settings.toml` that lives next to `Cargo.toml`.
- **After the first run**: edit `~/.mostrix/settings.toml` (changes to the project-root file will no longer be used).

#### Example `settings.toml`

```toml
# Mostro pubkey, hex format - only a placeholder replace with yours
mostro_pubkey = "82fa8cb978b43c79b2156585bac2c022276a21d2aead6d9f7c575c005be88390"

# Nostr user private key (nsec format, KEEP THIS SECRET)
nsec_privkey = "nsec1zpmjgd00jckr90zpa0wjhjldgrwy0p6cg3m2m4qcqh5fsx3c786q3c5ksu"

# Admin private key - leave empty for normal user mode
admin_privkey = ""

# Nostr relays to connect to
relays = [
  "wss://relay.mostro.network",
  "wss://damus.relay.io",
]

# Log verbosity level: "trace", "debug", "info", "warn", "error"
log_level = "info" - not managed from tui at the moment

# Fiat currencies you want to see / use (ISO codes)
currencies = ["VES", "ARS", "USD"]

# User mode: "user" or "admin" (controls available actions and UI)
user_mode = "user"

# Proof-of-work difficulty for events (0 = disabled, higher = more work)
pow = 0 - not managed from tui at the moment
```

#### Field explanations

- **`mostro_pubkey`**  
  - Public key of the Mostro instance you want to interact with.  
  - Accepts hex format. Use the key of the Mostro deployment you trust.

- **`nsec_privkey`**  
  - Your **Nostr private key** in `nsec…` format.  
  - Used to sign all Nostr events (orders, messages, etc.).  
  - **Treat this like a password** – do not share or commit it to Git.

- **`admin_privkey`**  
  - Private key used when running Mostrix in **admin mode**.  
  - Needed for admin-only flows (e.g., dispute resolution for admins).  
  - Leave it empty if you are a normal user.

- **`relays`**  
  - List of Nostr relay URLs (WebSocket endpoints) that Mostrix will connect to.  
  - You can add/remove relays depending on network connectivity and trust.

- **`log_level`**  
  - Controls how verbose logging is; values map to Rust log levels.  
  - Recommended values: `"info"` for normal use, `"debug"` or `"trace"` for troubleshooting.

- **`currencies`**  
  - List of fiat currencies (by ISO code) you are interested in trading/seeing in the UI.  
  - You can customize this to only show relevant markets, e.g. `["USD", "EUR"]`or leave it empty to clear all filters.

- **`user_mode`**  
  - `"user"` (default): normal user interface and actions.  
  - `"admin"`: enables admin-specific capabilities; typically used with `admin_privkey`.

- **`pow`**  
  - Required proof-of-work difficulty for Nostr events created by Mostrix.  
  - `0` disables additional PoW; higher values increase CPU cost per event but can help with relay anti-spam policies.


### Run

```bash
$ cargo run
```

## TODO list
- [x] Displays order list
- [x] Implement logger
- [x] Create 12 words seed for user runing first time
- [x] Use sqlite (sqlx)
- [x] Create settings.toml
- [x] Create Settings tab
- [x] [Implement keys management](https://mostro.network/protocol/key_management.html)
- [ ] List own orders
- [x] Take Sell orders
- [x] Take Buy orders
- [x] Create Buy Orders
- [x] Create buy orders with LN address
- [x] Create Sell Orders
- [ ] [Peers-to-peer chat](https://mostro.network/protocol/chat.html)
- [ ] Maker cancel pending order
- [x] Fiat sent
- [x] Release
- [ ] Cooperative cancellation
- [x] Buyer: add new invoice if payment fails
- [ ] Rate users
- [ ] Dispute flow (users)
- [ ] Dispute management (for admins)

**Note:** Many parts of the codebase still need thorough testing. Even features marked as complete may require additional testing, bug fixes, and refinement before production use.
