## Verifying the Release
In order to verify the release, you'll need to have gpg or gpg2 installed on your system. Once you've obtained a copy (and hopefully verified that as well), you'll first need to import the keys that have signed this release if you haven't done so already:
```bash
curl https://raw.githubusercontent.com/MostroP2P/mostrix/main/keys/negrunch.asc | gpg --import
curl https://raw.githubusercontent.com/MostroP2P/mostrix/main/keys/arkanoider.asc | gpg --import
```
Once you have the required PGP keys, you can verify the release (assuming manifest.txt.sig.negrunch, manifest.txt.sig.arkanoider and manifest.txt are in the current directory) with:
```bash
gpg --verify manifest.txt.sig.negrunch manifest.txt
gpg --verify manifest.txt.sig.arkanoider manifest.txt

gpg: Signature made fri 10 oct 2025 11:28:03 -03
gpg:                using RSA key 1E41631D137BA2ADE55344F73852B843679AD6F0
gpg: Good signature from "Francisco Calderón <fjcalderon@gmail.com>" [ultimate]

gpg: Signature made fri 10 oct 2025 11:28:03 -03
gpg:                using RSA key 2E986CA1C5E7EA1635CD059C4989CC7415A43AEC
gpg: Good signature from "Arkanoider <github.913zc@simplelogin.com>" [ultimate]

```
That will verify the signature of the manifest file, which ensures integrity and authenticity of the archive you've downloaded locally containing the binaries. Next, depending on your operating system, you should then re-compute the sha256 hash of the archive with `shasum -a 256 <filename>`, compare it with the corresponding one in the manifest file, and ensure they match exactly.


## What's Changed in 0.2.5

### 🚀 Features


* Observer reads with disclosed K_conv (step 6) by [@arkanoider](https://github.com/arkanoider)
* gate legacy GiftWrap receive behind dual-read flag by [@arkanoider](https://github.com/arkanoider)
* Step 4 client security — LRU, rate limit, durable inner ids by [@arkanoider](https://github.com/arkanoider)
* expose solver dispute chat by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* add user-to-solver messaging by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* persist solver chat metadata by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* clamp since cursors and pin mostro-core 0.14.2 by [@arkanoider](https://github.com/arkanoider)
* let users open a dispute from My Trades by [@amuntri](https://github.com/amuntri)

### 🐛 Bug Fixes


* retain NIP-33 publish stamp beside dispute open time by [@arkanoider](https://github.com/arkanoider)
* prefer kind-38386 created_at tag for Pending Created by [@arkanoider](https://github.com/arkanoider)
* restore user-solver chat exports after merge by [@arkanoider](https://github.com/arkanoider)
* park list scrollbar thumb at bottom on last row by [@arkanoider](https://github.com/arkanoider)
* shrink shell chrome so short terminals keep content by [@arkanoider](https://github.com/arkanoider)
* single-column pending disputes under 43 cols by [@arkanoider](https://github.com/arkanoider)
* align Orders and Disputes Pending scroll selection by [@arkanoider](https://github.com/arkanoider)
* sync scrollbar with table viewport and add compact layouts by [@Catrya](https://github.com/Catrya)
* fix(disputes): scroll pending disputes table to keep selection visible by [@Catrya](https://github.com/Catrya)
* guard Shift+C/F/R against duplicate submits by [@amuntri](https://github.com/amuntri)
* ignore stale Observer fetches and keep status visible by [@arkanoider](https://github.com/arkanoider)
* clipboard lost after thread exits on X11 by [@ekzyis](https://github.com/ekzyis)
* require inner-signer allow-list on unwrap by [@arkanoider](https://github.com/arkanoider)
* open Add Invoice on take-sell instead of create success by [@arkanoider](https://github.com/arkanoider)
* accept npub or hex for Mostro pubkey in settings by [@arkanoider](https://github.com/arkanoider)
* show dispute shortcut in My Trades footer by [@ca-ruz](https://github.com/ca-ruz)
* harden unsubscribe and notification stream handling by [@arkanoider](https://github.com/arkanoider)
* compact help on narrow terminals by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* keep help visible on short terminals by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* retain live chat metadata by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* abort send when transcript save fails by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* surface rejected trade actions by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* fsync transcript writes before treating save as durable by [@arkanoider](https://github.com/arkanoider)
* persist inner ids only after transcript save succeeds by [@arkanoider](https://github.com/arkanoider)
* display timestamps in local time by [@ca-ruz](https://github.com/ca-ruz)
* address review - widen giftwrap backfill + normalize future cursors by [@arkanoider](https://github.com/arkanoider)
* ignore Shift+D while a dispute request is pending by [@amuntri](https://github.com/amuntri)
* preserve grapheme boundaries in order input by [@ca-ruz](https://github.com/ca-ruz)
* keep My Trades input visible while typing by [@ca-ruz](https://github.com/ca-ruz)
* persist orders table state for smooth scrolling by [@misaelzb](https://github.com/misaelzb)

### 💼 Other


* fix(admin): prefer kind-38386 created_at tag for Pending Created by [@arkanoider](https://github.com/arkanoider) in [#131](https://github.com/MostroP2P/mostrix/pull/131)
* Revert "fix(admin): retain NIP-33 publish stamp beside dispute open time" by [@arkanoider](https://github.com/arkanoider)
* docs(chat): Step 8 — kind-14 acceptance for #102 by [@arkanoider](https://github.com/arkanoider) in [#130](https://github.com/MostroP2P/mostrix/pull/130)
* docs(chat): Step 7 — GiftWrap docs sweep + chat filter tests by [@arkanoider](https://github.com/arkanoider) in [#129](https://github.com/MostroP2P/mostrix/pull/129)
* fix(ui): restore user-solver chat exports after merge by [@arkanoider](https://github.com/arkanoider) in [#128](https://github.com/MostroP2P/mostrix/pull/128)
* feat(chat): add user-to-solver dispute chat by [@arkanoider](https://github.com/arkanoider) in [#119](https://github.com/MostroP2P/mostrix/pull/119)
* Merge branch 'main' into feat/user-solver-chat by [@arkanoider](https://github.com/arkanoider)
* fix(disputes): scroll pending disputes table to keep selection visible by [@arkanoider](https://github.com/arkanoider) in [#125](https://github.com/MostroP2P/mostrix/pull/125)
* fix(mytrades): guard Shift+C/F/R against duplicate submits by [@arkanoider](https://github.com/arkanoider) in [#123](https://github.com/MostroP2P/mostrix/pull/123)
* feat(chat): Step 6 — Observer K_conv disclosure UX by [@arkanoider](https://github.com/arkanoider) in [#126](https://github.com/MostroP2P/mostrix/pull/126)
* fix(clipboard): clipboard lost after thread exits on X11 by [@arkanoider](https://github.com/arkanoider) in [#127](https://github.com/MostroP2P/mostrix/pull/127)
* Merge remote-tracking branch 'origin/main' into feat/user-solver-chat by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* feat(chat): Step 5 — dual-read GiftWrap flag by [@arkanoider](https://github.com/arkanoider) in [#124](https://github.com/MostroP2P/mostrix/pull/124)
* fix(chat): require inner-signer allow-list on unwrap by [@arkanoider](https://github.com/arkanoider) in [#122](https://github.com/MostroP2P/mostrix/pull/122)
* fix: accept npub or hex for Mostro pubkey by [@arkanoider](https://github.com/arkanoider) in [#121](https://github.com/MostroP2P/mostrix/pull/121)
* Fix(ui): Show dispute shortcut in My Trades footer by [@arkanoider](https://github.com/arkanoider) in [#118](https://github.com/MostroP2P/mostrix/pull/118)
* chore: migrate to nostr-sdk 0.45.1 and mostro-core 0.14.3 by [@arkanoider](https://github.com/arkanoider) in [#120](https://github.com/MostroP2P/mostrix/pull/120)
* Merge remote-tracking branch 'origin/main' into feat/user-solver-chat by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* feat(chat): Step 4 — outer LRU, rate limit, durable inner-id dedup by [@arkanoider](https://github.com/arkanoider) in [#117](https://github.com/MostroP2P/mostrix/pull/117)
* Fix(ui): Display timestamps in local time by [@arkanoider](https://github.com/arkanoider) in [#115](https://github.com/MostroP2P/mostrix/pull/115)
* feat(chat): Step 3 — clamp since cursor + mostro-core 0.14.2 by [@arkanoider](https://github.com/arkanoider) in [#104](https://github.com/MostroP2P/mostrix/pull/104)
* feat(dispute): let users open a dispute from My Trades by [@arkanoider](https://github.com/arkanoider) in [#106](https://github.com/MostroP2P/mostrix/pull/106)
* Fix(ui): Keep My Trades input visible while typing by [@arkanoider](https://github.com/arkanoider) in [#112](https://github.com/MostroP2P/mostrix/pull/112)
* fix: persist orders table state for smooth scrolling by [@arkanoider](https://github.com/arkanoider) in [#113](https://github.com/MostroP2P/mostrix/pull/113)
* docs: add security policy by [@arkanoider](https://github.com/arkanoider) in [#105](https://github.com/MostroP2P/mostrix/pull/105)

### 📚 Documentation


* Step 8 — kind-14 acceptance matrix for #102 by [@arkanoider](https://github.com/arkanoider)
* Step 7 — sweep GiftWrap-only P2P wording and harden tests by [@arkanoider](https://github.com/arkanoider)
* describe kind-14 as the active chat transport by [@arkanoider](https://github.com/arkanoider)
* clarify dual-read chat routing and admin signer allow-list by [@arkanoider](https://github.com/arkanoider)
* sync Mostro pubkey comments with npub/hex flow by [@arkanoider](https://github.com/arkanoider)
* add SECURITY.md with vulnerability reporting policy by [@AndreaDiazCorreia](https://github.com/AndreaDiazCorreia)

### ⚙️ Miscellaneous Tasks


* use rustls ring via rustls-no-provider by [@arkanoider](https://github.com/arkanoider)
* bump crypto deps and adapt chacha20poly1305 0.11 API by [@arkanoider](https://github.com/arkanoider)
* migrate to nostr-sdk 0.45.1 and mostro-core 0.14.3 by [@arkanoider](https://github.com/arkanoider)
* align chat type imports by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* update comments by [@arkanoider](https://github.com/arkanoider)
* fix cargo fmt by [@arkanoider](https://github.com/arkanoider)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#131](https://github.com/MostroP2P/mostrix/pull/131)
* [@ekzyis](https://github.com/ekzyis) made their contribution
* [@Vidarte-Alberto](https://github.com/Vidarte-Alberto) made their contribution
* [@Catrya](https://github.com/Catrya) made their contribution
* [@amuntri](https://github.com/amuntri) made their contribution
* [@ca-ruz](https://github.com/ca-ruz) made their contribution
* [@misaelzb](https://github.com/misaelzb) made their contribution
* [@AndreaDiazCorreia](https://github.com/AndreaDiazCorreia) made their contribution

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.2.4...0.2.5

<!-- generated by git-cliff -->
