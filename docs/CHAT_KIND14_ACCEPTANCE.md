# Kind-14 P2P chat — acceptance (mostrix#102)

Tracks Step 8 of the kind-14 migration. Canonical criteria come from
[mostrix#102](https://github.com/MostroP2P/mostrix/issues/102) and the
[protocol chat spec](https://mostro.network/protocol/chat.html).

**Status:** automated proofs green on `main`; live two-client smoke remains a
manual checklist below (relay storage caveat in the protocol docs).

## Criteria → evidence

| #102 criterion | Automated proof | Manual |
|---|---|---|
| Two clients exchange kind-14 chat; published events do **not** put either trade pubkey as outer author or sole `#p` | `chat_wrap_unwrap_roundtrip_preserves_sender_and_content`, `acceptance_kind14_wire_hides_trade_pubkeys` in `src/util/chat_utils.rs` | Two Mostrix builds on a live trade; inspect outer events on `wss://relay.mostro.network` |
| Non-`pub(K_sign)` authors never decrypt; relay filter excludes them | `resolve_chat_target_ignores_kind14_from_unknown_author`, `live_chat_filters_kind14_uses_authors_not_p_tags` (`chat_listener.rs`); allow-list rejects in `user_solver_chat_roundtrip_*` / `unwrap_rejects_*` (`chat_utils.rs`) | Publish junk kind-14 to `#p=pub(K_conv)` with a random author — client must ignore |
| Flood at conversation address does not degrade order/DM/dispute UI | `rate_limiter_allows_burst_then_rejects`, `outer_lru_rejects_duplicates_and_evicts`, `try_emit_drops_when_queue_full_without_blocking` (`chat_security.rs`); listener uses per-`ChatKeyId` buckets + bounded update channel | Hold My Trades / Messages usable while a peer floods |
| Restart does not re-download unbounded history | `clamp_chat_since_cursor_caps_future_poison`, `track_clamps_future_since_cursor`; hydrate uses `since` + `limit(100)` + 7-day lookback (`fetch_chat_messages_for_shared_key`) | Quit mid-chat, relaunch — only recent history hydrates |
| Disclosing `K_conv` = read-only for solvers | `conversation_disclosure_is_k_conv_not_k_sign_secret`, `observer_k_conv_only_unwraps_kind14`, `observer_k_conv_cannot_author_kind14`, `observer_cannot_unwrap_legacy_giftwrap_with_k_conv` | Shift+K → Observer paste `K_conv` only → read history; no send path |
| Dispute admin↔party uses the same envelope | `user_solver_chat_roundtrip_accepts_only_conversation_parties`; `dispute_chat_allowed_signers_includes_admin_and_party` | Admin Disputes in Progress chat both ways |
| Protocol v2 Mostro DMs still work (author routing intact) | `filter_protocol_dm_v2_*` (`filters.rs`); chat live filter is `authors=[pub(K_sign)]` so node-authored kind-14 stays on the DM listener | Order create/take/pay/release on a v2 node |

## Dual-read window

While [`CHAT_ACCEPT_LEGACY_GIFTWRAP`](../src/util/chat_utils.rs) is `true`:

- **Outbound** P2P / dispute chat: kind 14 only.
- **Inbound**: kind 14 + legacy GiftWrap (`#p` = ECDH pubkey).
- **Observer**: kind 14 + `K_conv` only (cannot unwrap GiftWrap).

Flip the const to `false` after coordinated deprecation with mobile / other clients.

## Manual smoke (optional for CI)

1. Two clients complete a trade chat round-trip on kind 14.
2. Confirm outer `pubkey == pub(K_sign)` and `#p == [pub(K_conv)]` (no trade keys).
3. Observer with disclosed `K_conv` (and optional locator) reads history; cannot send.
4. Protocol DMs on v1 and v2 instances still function (see [DM_LISTENER_FLOW.md](DM_LISTENER_FLOW.md#manual-verification-protocol-v2)).

## Related

- Migration steps 1–7 shipped via kind-14 PRs (core bump, adapters, listener, security, dual-read, Observer, docs).
- Tracking issue: [#102](https://github.com/MostroP2P/mostrix/issues/102).
