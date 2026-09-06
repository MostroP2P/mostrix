# PoW and outbound Nostr events

This document describes how Mostrix applies **NIP-13 proof-of-work** to events it publishes toward Mostro and related flows. It is intended for contributors and AI-assisted codegen so outbound signing behavior stays consistent with the Mostro instance policy.

## Source of difficulty

- The Mostro **instance status** event (kind **38385**) includes optional tags:
  - **`pow`** — unsigned integer; parsed into [`MostroInstanceInfo.pow`](../src/util/mostro_info.rs) (`Option<u32>`).
  - **`protocol_version`** — `"1"` or `"2"`; displayed on the Mostro Info tab. [`transport_from_instance`](../src/util/mostro_info.rs) always returns `Nip44Direct` (v1 GiftWrap is unsupported).
- There is **no** `pow` field in [`Settings`](../src/settings.rs) or in the generated `settings.toml` template.
- Optional **`pow_first_contact`** on kind **38385** (when the daemon publishes it): parsed into [`MostroInstanceInfo.pow_first_contact`](../src/util/mostro_info.rs). When absent, effective first-contact toll = base `pow`.
- **Effective bits** for signing: [`nostr_pow_from_instance`](../src/util/mostro_info.rs) maps base `pow`; [`nostr_pow_for_protocol_dm`](../src/util/mostro_info.rs) selects per-action bits for [`send_dm`](../src/util/dm_utils/mod.rs).

## Cached instance info at runtime

- [`AppState.mostro_info`](../src/ui/app_state.rs) holds the latest **client-authenticated** `MostroInstanceInfo` (see [`fetch_mostro_instance_info`](../src/util/mostro_info.rs)).
- [`AppState.transport`](../src/ui/app_state.rs) mirrors resolved [`Transport`](../src/util/mod.rs). Updated via [`set_mostro_info`](../src/ui/app_state.rs) (stale `created_at` ignored).
- [`EnterKeyContext`](../src/ui/key_handler/mod.rs) threads `mostro_info` into async work without re-fetching per message.
- [`send_dm`](../src/util/dm_utils/mod.rs) takes `mostro_instance: Option<&MostroInstanceInfo>` and computes `pow = nostr_pow_for_protocol_dm(mostro_instance, action)` once per send.

At startup (and on reload/reconnect via [`dm_transport_for_mostro`](../src/ui/key_handler/async_tasks.rs)), instance info is fetched **before** the DM listener spawns when relays are reachable.

## Protocol v2 (NIP-44 direct) — outbound

[`send_dm`](../src/util/dm_utils/mod.rs) uses [`wrap_message_with`](../src/util/mod.rs) from `mostro-core` with `Transport::Nip44Direct`:

- **PoW** on the **signed kind-14** event (`WrapOptions.pow`).
- **NIP-40 expiration**: default **now + 30 days** when the caller passes `expiration: None` (`default_dm_expiration`).

First-contact actions may need higher PoW than instance `pow` (`pow_first_contact` on the daemon). Mostrix applies `max(pow, pow_first_contact)` for `NewOrder`, `TakeBuy`, and `TakeSell` via [`nostr_pow_for_protocol_dm`](../src/util/mostro_info.rs). When the kind-38385 `pow_first_contact` tag is absent, the effective toll defaults to base `pow` (same as the daemon).

## Chat vs protocol PoW

- **Protocol DMs toward Mostro**: instance PoW + [`wrap_message_with`](../src/util/mod.rs) (`Transport::Nip44Direct`).
- **Shared-key chat** (admin dispute, user order, observer): `mostro_core::chat` kind 14 (`K_sign` / `K_conv`) — **no PoW**.

## Call sites (high level)

Order flows under [`src/util/order_utils/`](../src/util/order_utils/), admin actions in [`src/ui/key_handler/admin_handlers.rs`](../src/ui/key_handler/admin_handlers.rs), and rating handlers in [`src/ui/key_handler/message_handlers.rs`](../src/ui/key_handler/message_handlers.rs) pass cached `mostro_instance` into `send_dm`.

## Related docs

- [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) — instance info at boot and on reconnect
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) — protocol overview and v2 status
