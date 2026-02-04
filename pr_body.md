## Summary

This PR implements a fully functional admin chat system for dispute resolution using NIP-59 gift wrap events, along with comprehensive documentation and code quality improvements.

### Key Features

- **NIP-59 Admin Chat**: Real-time encrypted chat between admin and dispute parties (buyer/seller) using gift wrap events
- **Chat Restore at Startup**: Persists chat transcripts to `~/.mostrix/<dispute_id>.txt` and restores them on restart with incremental NIP-59 sync
- **Finalization Popup**: Improved dispute finalization UI with Pay Buyer / Refund Seller actions
- **Modular Key Handler**: Refactored monolithic key_handler.rs into focused modules

### Code Quality

- Removed unused shared-key chat code (SharedChatKeys, derive_shared_chat_keys, etc.)
- Extracted reusable helpers (update_last_seen_timestamp, chat navigation helpers)
- Added comprehensive test suite for parsing, validation, and DB operations

### Documentation

- Added 11 new documentation files in `docs/` covering architecture, protocols, and workflows
- Updated README with current feature set

## Test Plan

- [x] Unit tests pass (`cargo test`)
- [x] Clippy clean (`cargo clippy --all-targets --all-features`)
- [x] Manual testing of admin chat send/receive
- [x] Chat restore verified after restart
- [x] Finalization popup actions tested
