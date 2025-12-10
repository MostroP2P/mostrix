# Mostrix Overview

## 🎯 Main Purpose

**Mostrix** is a Text User Interface (TUI) client for **Mostro**, a peer-to-peer Bitcoin exchange built on top of the **Nostr** protocol.

It allows users to:
- **View the Order Book**: See pending buy and sell orders from Mostro.
- **Create Orders**: Publish new buy or sell orders to the Mostro network.
- **Take Orders**: Interact with existing orders to start a trade.
- **Manage Trades**: Handle trade progression (e.g., paying invoices, releasing funds) through direct messages.
- **Handle Disputes**: Manage disputes if orders encounter problems.
- **Secure Interaction**: Manage identity and trade keys locally.

## ⚠️ Critical Points & Architecture

### 🔑 Key Management (Crucial)
Mostrix strictly follows the [Mostro Protocol Key Management](https://mostro.network/protocol/key_management.html) to ensure privacy and security:
- **Deterministic Derivation**: Keys are derived using NIP-06 with the path `m/44'/1237'/38383'/0/X`.
- **Identity Key (Index 0)**: Used for reputation and identity.
- **Trade Keys (Index 1+)**: Every new order or trade uses a fresh key derived from the next available index.
- **State Requirement**: The database **must** accurately track the `trade_index` for every order. Associating the correct index with the order ID is essential to sign/decrypt messages for that specific trade.

### 📡 Message Persistence (Stateless Approach)
To avoid the complexity of a local message database, Mostrix adopts a "fetch-on-startup" strategy:
- **No Local Message DB**: The client does not store the full history of messages in its own database.
- **Startup Sync**: On startup, Mostrix iterates through active orders (identified by stored Order UUIDs and Trade Keys) and queries Mostro for the latest state/messages.
- **Recovery**: This ensures the user always sees the most up-to-date information from the Mostro daemon without state drift.

## 📂 Project Structure & Modules

The codebase is organized into logical domains:

### 1. Core & Infrastructure
- **`src/main.rs`**: The entry point. Initializes the logger, database connection, Nostr client, and runs the main event loop (handling UI ticks and background tasks).
- **`src/settings.rs`**: Handles configuration loading (relays, keys, etc.) from `Settings.toml` or environment variables.
- **`src/db.rs` / `src/models.rs`**: Defines the SQLite database schema and Rust structs (`User`, `Order`) for persisting critical local state (keys and trade indices).

### 2. User Interface (`src/ui/`)
This module handles everything related to the TUI rendering and user interaction.
- **`mod.rs`**: Defines `AppState`, `UiMode` (the state machine for the UI), and the main `ui_draw` function.
- **`key_handler.rs`**: Centralized logic for processing keyboard events based on the current `UiMode`.
- **`tab_content.rs` / `tabs.rs`**: Manages the main navigation tabs (Orders, Messages, etc.) and their rendering logic.
- **`orders_tab.rs`**: Renders the main order book table.
- **`order_form.rs`**: Handles the UI for creating a new order (input fields, validation).
- **`order_take.rs`**: Manages the "Take Order" confirmation screen and range order inputs.
- **`order_confirm.rs`**: Renders the final confirmation popup before publishing an order.
- **`order_result.rs` / `waiting.rs`**: UI components for operation results (success/error) and loading states.

### 3. Business Logic & Protocol (`src/util/`)
This module implements the core functionality for interacting with the Mostro protocol.
- **`order_utils.rs`**: The heavy lifter for the Mostro protocol. Contains logic for:
  - Creating new orders (signing events).
  - Taking orders (`take_order` function).
  - Handling `PaymentRequest` and other protocol messages.
- **`dm_utils.rs`**: Handles NIP-59/NIP-17 encrypted Direct Messages. Responsible for listening to relay events, decrypting DMs, and parsing them into Mostro actions.
- **`db_utils.rs`**: Helper functions for high-level database operations (e.g., saving a new trade index).
- **`filters.rs`**: Helpers for creating Nostr subscription filters.

## 📦 Dependencies

### Core Protocol
- **`mostro-core`**: Implements the Mostro protocol data structures and logic.
- **`nostr-sdk`**: Handles Nostr protocol interactions (relays, keys, events, NIP support).
- **`lightning-invoice` / `lnurl-rs`**: For handling Bitcoin Lightning Network invoices and addresses.

### User Interface
- **`ratatui`**: The library used for building the TUI (widgets, layout, rendering).
- **`crossterm`**: Low-level terminal manipulation (input handling, raw mode).

### Infrastructure
- **`tokio`**: Asynchronous runtime.
- **`sqlx`**: Async SQL database driver (SQLite).
- **`config`**: Configuration management.
- **`anyhow` / `log` / `fern`**: Error handling and logging.
