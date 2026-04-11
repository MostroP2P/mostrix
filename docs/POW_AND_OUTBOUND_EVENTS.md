# PoW and outbound Nostr events

This document describes how Mostrix applies **NIP-13 proof-of-work** to events it publishes toward Mostro and related flows. It is intended for contributors and AI-assisted codegen so outbound signing behavior stays consistent with the Mostro instance policy.

## Source of difficulty

- The Mostro **instance status** event (kind **38385**) includes an optional `pow` tag (unsigned integer). Mostrix parses this into [`MostroInstanceInfo.pow`](../src/util/mostro_info.rs) (`Option<u32>`).
- There is **no** `pow` field in [`Settings`](../src/settings.rs) or in the generated `settings.toml` template. Legacy configs may still contain `pow = …`; serde typically ignores unknown keys when deserializing.
- **Effective bits** for signing: [`nostr_pow_from_instance`](../src/util/mostro_info.rs) maps `Option<&MostroInstanceInfo>` → `u8` by taking `info.pow`, clamping to `u8::MAX`, and using **0** when info is missing or `pow` is `None`.

## Cached instance info at runtime

- [`AppState.mostro_info`](../src/ui/app_state.rs) holds the latest fetched `MostroInstanceInfo`.
- [`EnterKeyContext`](../src/ui/key_handler/mod.rs) includes `mostro_info: Option<MostroInstanceInfo>` so Enter/spawn paths can pass the same snapshot into async work without re-fetching relays per message.
- [`send_dm`](../src/util/dm_utils/mod.rs) takes `mostro_instance: Option<&MostroInstanceInfo>` and computes `pow = nostr_pow_from_instance(mostro_instance)` once per send.

If instance info has not been loaded yet (e.g. slow startup), PoW may be **0** until a successful fetch or manual refresh (Mostro Info tab / background refresh tasks). Users may see rejects from strict instances until 38385 is cached.

## Private direct messages (NIP-17 / kind 14)

[`create_private_dm_event`](../src/util/dm_utils/dm_helpers.rs) builds the published event with `EventBuilder::new(Kind::PrivateDirectMessage, …).pow(pow).…` so the **event that hits relays** is mined to the required difficulty.

## Gift Wrap (NIP-59 / kind 1059)

Mostro protocol traffic uses encrypted Gift Wraps. The **rust-nostr** helper `EventBuilder::gift_wrap` composes seal → wrap but, in the versions Mostrix uses, does **not** apply PoW to the **outer** Gift Wrap event (the one relays and daemons index).

Mostrix therefore:

1. Builds the **rumor** (inner unsigned note) as today (including `.pow(pow)` on the rumor builder where applicable).
2. Builds and signs the **seal** with `EventBuilder::seal` + `sign`.
3. Wraps with a local [`gift_wrap_from_seal_with_pow`](../src/util/dm_utils/dm_helpers.rs) that mirrors upstream `gift_wrap_from_seal` (NIP-44 encrypt seal JSON, kind 1059, tweaked `created_at`, ephemeral keys) but adds **`.pow(pow)`** on the **Gift Wrap** `EventBuilder` **before** `sign_with_keys`, so the **published** envelope id satisfies instance PoW policy.

Admin dispute chat gift wraps use the same instance-derived PoW via [`send_admin_chat_message_via_shared_key`](../src/util/chat_utils.rs) and `nostr_pow_from_instance`.

## Call sites (high level)

Anything that publishes to Mostro should receive cached instance info where possible: order flows under [`src/util/order_utils/`](../src/util/order_utils/), admin dispute actions in [`src/ui/key_handler/admin_handlers.rs`](../src/ui/key_handler/admin_handlers.rs), and message/rating handlers in [`src/ui/key_handler/message_handlers.rs`](../src/ui/key_handler/message_handlers.rs).

## Related docs

- [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) — settings shape (no local `pow`)
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) — protocol overview; links here for PoW detail
