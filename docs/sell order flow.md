# Sell order flow (stub)

This document will mirror [buy order flow.md](buy%20order%20flow.md) for **`kind: sell`** listings: maker vs taker phases, cancellation, and **Messages** tab Enter / popup behavior.

**Status:** TBD — define after the buy flow spec is reviewed and frozen.

## Planned sections (outline)

1. Scope and definitions (sell listing, maker, taker).
2. Sources of truth (`order.status`, `action`, role + kind).
3. Maker flow (sell listing) — ordered phases and typical `Status` values.
4. Taker flow (sell listing) — ordered phases and typical `Status` values.
5. Cancellation (before `active` vs at `active` / cooperative cancel).
6. TUI: Messages tab Enter — invoice, confirm, cancel, dispute (same rules as buy, role-inverted where appropriate).
7. Open questions.

## References

- [Mostro protocol ORDER.md](https://github.com/MostroP2P/protocol/blob/main/ORDER.md)
- [Mostro protocol ACTIONS.md](https://github.com/MostroP2P/protocol/blob/main/ACTIONS.md)
- [MESSAGE_FLOW_AND_PROTOCOL.md](MESSAGE_FLOW_AND_PROTOCOL.md)
