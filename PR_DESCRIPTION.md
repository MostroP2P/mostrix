# Admin chat: UX, validation, and attachment compatibility

## Summary

Follow-up to the ECDH shared-key admin chat and file-attachment work. This PR improves admin chat compatibility with the Mostro protocol, attachment decryption when the key is not embedded, readability of seller messages, and adds validation and tests.

## Changes

### Admin chat send/receive
- **Send**: Admin chat messages use a custom NIP-44 gift wrap (signed text note encrypted with an ephemeral key, PoW from settings) to align with [Mostro protocol chat](https://mostro.network/protocol/chat.html).
- **Receive**: Unwrap path uses NIP-44 decryption only (no NIP-59 fallback) to match the above format.

### Attachments (Blossom)
- **`blossom::derive_shared_key(admin_keys, sender_pubkey)`**: ECDH shared key for ChaCha20-Poly1305 decryption (mirrors mostro-cli with roles swapped).
- **Ctrl+S**: When saving an attachment, if the message has no embedded decryption key, the client derives it from the admin key and the sender’s pubkey (buyer/seller from the selected dispute).
- **Nonce parsing**: Attachment JSON nonce supports both hex (24 chars, as from Mostro CLI) and base64.

### UX
- **Seller chat color**: Seller messages use **Magenta** instead of Red for better readability in the dispute chat.

### Validation and tests
- **Shared-key sanity check** (`models.rs`): When saving a dispute, if buyer and seller pubkeys differ but the two derived shared keys are identical, the client logs an error so broken chat can be detected (e.g. bad relay data).
- **Unit test** (`chat_utils`): `derive_shared_key_hex_different_users_different_keys` asserts that different counterparty pubkeys yield different shared keys.
- **Clippy**: Test module moved to end of file to satisfy `items_after_test_module`.

### Other
- **reqwest**: `http2` feature enabled in `Cargo.toml`.
- **Docs**: ADMIN_DISPUTES.md and KEY_MANAGEMENT.md updated (Seller=Magenta, shared-key validation).

## Breaking changes

None. Behavior changes are backward-compatible (nonce format, optional key derivation, log-only validation).

## Checklist

- [x] Unit tests pass (`cargo test`)
- [x] Clippy clean (`cargo clippy --all-targets --all-features`)
- [x] Format applied (`cargo fmt --all`)
- [ ] Manual testing: admin chat send/receive, attachment send + Ctrl+S save (with and without embedded key)

## Base branch

Branch `file-attachment` is based on prior work (ECDH shared keys, file attachments in dispute chat). Target for this PR: **main** (or the repo’s default branch) after `file-attachment` is merged, or **file-attachment** if this is meant to be part of that feature branch.
