# Session restore hydrate — acceptance

Tracks **step 6** of the post-restore hydrate plan (steps 1–5 shipped in PRs
[#156](https://github.com/MostroP2P/mostrix/pull/156)–[#158](https://github.com/MostroP2P/mostrix/pull/158);
orchestrator in [#159](https://github.com/MostroP2P/mostrix/pull/159)).

**Goal:** after seed import or **Settings → Restore Session**, the user sees
restored trades on **Messages** and **My Trades** (including peer chat history)
**without** restarting Mostrix.

Architecture overview: [STARTUP_AND_CONFIG.md](STARTUP_AND_CONFIG.md) —
"Session restore hydrate".

## Criteria → evidence

| Criterion | Automated proof | Manual |
|---|---|---|
| DB status → Messages-tab `Action` after restore | `history_action_for_db_order_tests` in `src/ui/helpers/startup.rs` | Restore an in-progress buy/sell; Messages column matches trade phase |
| Trade-DM replay uses catch-up when no `last_seen_dm_ts` | `trade_dm_replay_no_cursor_*` in `src/util/dm_utils/mod.rs` | Post-wipe restore: Messages tab not empty for active orders |
| Trade-DM replay uses `UntrackedFallback` without live subscription | `trade_dm_replay_uses_untracked_fallback_*` in `src/util/dm_utils/mod.rs` | Same as above immediately after restore (before listener re-track) |
| Stale chat cursors cleared before hydrate | `clear_session_chat_projection_tests` in `src/ui/helpers/startup.rs` | Restore after seed import: own peer lines visible (not dropped) |
| Peer transcript maps You/Peer and sorts by time | `transcript_maps_you_and_peer_and_sorts_by_timestamp` in `src/ui/helpers/startup.rs` | My Trades chat shows both sides of history |
| Inner event ids recorded only after transcript save | Code path in `rebuild_peer_order_chat_transcript` (after `rewrite_order_chat_messages`) | Failed disk write would allow retry (hard to trigger manually) |
| Orchestrator runs trade-DM + peer chat in parallel | `hydrate_after_session_restore` + `tokio::join!` in `src/ui/helpers/startup.rs` | Log shows both summary lines within seconds |
| Completion always emitted (incl. zero peer orders) | `hydrate_with_no_eligible_work_returns_empty_report`, `spawn_post_restore_hydrate_emits_completion_with_empty_peer_orders` | Restore identity with no chat-eligible orders — app stays usable |
| `track_startup_chats` re-runs after hydrate | `PostRestoreHydrateCompleted` handler in `src/main.rs` | New peer messages arrive live after restore without restart |
| `SessionRestored` (not `Info`) triggers hydrate | `restore_completion_result` tests in `src/util/order_utils/execute_restore.rs` | N/A (regression guard) |

## Out of scope (this acceptance pass)

- Admin dispute chat transcript rebuild after restore
- Solver-channel (`UserChatChannel::Solver`) post-restore relay rebuild
- First-launch restore UX beyond the same `SessionRestored` pipeline

## Prerequisites (manual smoke)

1. **Live Mostro** instance and reachable relays (same as normal trading).
2. **Test identity** with at least one **active** order that has:
   - Trade protocol DMs on relay (Messages tab history), and
   - Peer order chat messages on relay (My Trades chat panel).
3. Optional second scenario: identity with active orders but **no** counterparty
   chat key yet (zero peer-chat hydrate) — for criterion #8.

Suggested log level for manual runs: `RUST_LOG=info` (or `mostrix=info`).

## Manual smoke checklist

Run each scenario **without** quitting Mostrix between restore and verification.

### A — Full restore (primary)

| # | Step | Expected |
|---|------|----------|
| A1 | Note current **Messages** step and **My Trades** peer chat transcript | Baseline for comparison |
| A2 | **Settings** → **Restore Session** → confirm **Yes** | Success popup; no crash |
| A3 | Open **My Trades** | Restored order(s) listed |
| A4 | Open **Messages** | Restored order(s) listed; timeline step matches trade phase (not stuck at empty / wrong column) |
| A5 | Wait ~5–30 s (relay hydrate) | No duplicate bond/invoice popups |
| A6 | **My Trades** → select order → peer chat panel | **You** and **Peer** lines from history (not empty; not all **Peer**) |
| A7 | Send a new chat line → Enter | Appears as **You** once; relay echo does **not** duplicate |
| A8 | Receive a counterparty message (or simulate from other client) | Appears as **Peer**; UI responsive |

**Log grep (optional):**

```text
Post-restore trade DM replay:
Post-restore peer chat rebuild:
```

Both lines should appear after A2. On `PostRestoreHydrateCompleted`, chat router
re-tracks without restart.

### B — Edge: no peer-chat-eligible orders

| # | Step | Expected |
|---|------|----------|
| B1 | Restore an identity whose active orders lack shared key + counterparty pubkey | Success popup |
| B2 | Use app normally (Orders / Messages tabs) | No hang; no fatal error |
| B3 | Check logs | `Post-restore peer chat rebuild: attempted=0` (or skipped orders); hydrate still completes |

### C — Seed import + key reload (cursor hygiene)

| # | Step | Expected |
|---|------|----------|
| C1 | Import seed / regenerate keys (staged wipe flow) | App reloads session |
| C2 | Restore or continue with active trades | Peer chat and Messages hydrate; no stale identity cursors |

## Sign-off

| Field | Value |
|-------|-------|
| Mostrix version / commit | |
| Mostro instance | |
| Date | |
| Tester | |
| A (full restore) | ☐ pass ☐ fail |
| B (zero peer chat) | ☐ pass ☐ skip ☐ fail |
| C (seed import) | ☐ pass ☐ skip ☐ fail |
| Notes | |

## Related

- [DM_LISTENER_FLOW.md](DM_LISTENER_FLOW.md) — post-restore trade-DM replay (§4)
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md) — peer chat echo rules
- [KEY_MANAGEMENT.md](KEY_MANAGEMENT.md) — `clear_session_chat_projection` on key reload
