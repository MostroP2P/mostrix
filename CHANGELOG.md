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


## What's Changed in 0.1.8

### 🚀 Features


* added correct pow management by [@arkanoider](https://github.com/arkanoider)
* Clear local orders on user seed rotation and warn in UI  Delete persisted `orders` in the same DB transaction as user replacement (`replace_all_atomic` and async key rotation). Update the generate-keys confirmation copy so users know local order rows are cleared. by [@arkanoider](https://github.com/arkanoider)
* moved fatal error hook in fatal.rs by [@arkanoider](https://github.com/arkanoider)
* relay replay on startup, filter_giftwrap_to_recipient, hydration empty() by [@arkanoider](https://github.com/arkanoider)
* completed mechanism for resynch of messages at startup by [@arkanoider](https://github.com/arkanoider)
* added synch mechanism for orders at reboot by [@arkanoider](https://github.com/arkanoider)
* improving no internet popup fix required by [@arkanoider](https://github.com/arkanoider)
* introducing network status and offline overlay by [@arkanoider](https://github.com/arkanoider)
* working on message tab order flow - buy order by [@arkanoider](https://github.com/arkanoider)
* new ui for messages start commit by [@arkanoider](https://github.com/arkanoider)
* created helper function build_order_from_small_order to avoid code duplication by [@arkanoider](https://github.com/arkanoider)
* pruned stale waiters in subscription manager by [@arkanoider](https://github.com/arkanoider)
* added a fallback error UI popup if some mutex are poisoned by [@arkanoider](https://github.com/arkanoider)
* added a struct to avoid long list in parameters function call, improved messages fetch avoiding double nip59 extraction by [@arkanoider](https://github.com/arkanoider)
* improving message flow and descriptive popup - TBD by [@arkanoider](https://github.com/arkanoider)
* created enum for formfield variable by [@arkanoider](https://github.com/arkanoider)
* improved UI of order creation tab by [@arkanoider](https://github.com/arkanoider)
* improving order ui tab by [@arkanoider](https://github.com/arkanoider)
* shortened time interval to update keys list of subscription by [@arkanoider](https://github.com/arkanoider)
* introduced subscription to relay to replace timed fetches by [@arkanoider](https://github.com/arkanoider)
* After soft reload, tasks no longer use stale OnceLock settings snapshot; they always use current runtime settings loaded from disk at action time. by [@arkanoider](https://github.com/arkanoider)
* used two rows for mnemonic by [@arkanoider](https://github.com/arkanoider)
* added mostro pubkey automatic reload by [@arkanoider](https://github.com/arkanoider)
* add non-blocking seed view and channel bundle refactor by [@arkanoider](https://github.com/arkanoider)
* soft-reload runtime after key rotation and fixed rabbit rants by [@arkanoider](https://github.com/arkanoider)
* sync settings key with DB identity, add generate-new-keys flow and docs updates by [@arkanoider](https://github.com/arkanoider)

### 🐛 Bug Fixes


* clippy and docs update for new pow logic by [@arkanoider](https://github.com/arkanoider)
* rabbit rants fixes by [@arkanoider](https://github.com/arkanoider)
* feat(trades): cooperative cancel (UI + DB), TradeClosed cleanup, monotonic status by [@arkanoider](https://github.com/arkanoider)
* removed useless bloat code for canceled orders by [@arkanoider](https://github.com/arkanoider)
* simplified removal from message list of recreated orders after cancel by [@arkanoider](https://github.com/arkanoider)
* fixed missing messages by [@arkanoider](https://github.com/arkanoider)
* enforced error in case mostro does not send payload with order on order creation by [@arkanoider](https://github.com/arkanoider)
* removed pending orders from fetched ones at startup by [@arkanoider](https://github.com/arkanoider)
* fix(admin,dm): hydrate admin keys, replay skew, TERMINAL_DM_STATUSES query by [@arkanoider](https://github.com/arkanoider)
* fixing bot rants by [@arkanoider](https://github.com/arkanoider)
* started fixing rabbit rants by [@arkanoider](https://github.com/arkanoider)
* missing wait_for_dm response fixed by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* fix for pasting observer key in win10 cmd by [@arkanoider](https://github.com/arkanoider)
* HIGH priority fix for mostronator review by [@arkanoider](https://github.com/arkanoider)
* improved description of settings reload variable by [@arkanoider](https://github.com/arkanoider)
* rabbit rant by [@arkanoider](https://github.com/arkanoider)
* rabbit rants - nipticks - issue on buy order flow by [@arkanoider](https://github.com/arkanoider)
* fix latest rabbit rants by [@arkanoider](https://github.com/arkanoider)
* fix clippy by [@arkanoider](https://github.com/arkanoider)
* rabbit rants by [@arkanoider](https://github.com/arkanoider)
* old listener is aborted only after the new session is validated by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* feat: added correct pow management by [@arkanoider](https://github.com/arkanoider) in [#51](https://github.com/MostroP2P/mostrix/pull/51)
* fix: fix for pasting observer key in win10 cmd by [@arkanoider](https://github.com/arkanoider) in [#50](https://github.com/MostroP2P/mostrix/pull/50)
* Merge branch 'main' into fix-paste-observer by [@arkanoider](https://github.com/arkanoider)
* Order flow in message and synch message at boot by [@arkanoider](https://github.com/arkanoider) in [#48](https://github.com/MostroP2P/mostrix/pull/48)
* - FlowStep/StepLabels*, message_trade_timeline_step, StepLabel in constants by [@arkanoider](https://github.com/arkanoider)
* refactor DM routing and add live subscription updates by [@grunch](https://github.com/grunch) in [#47](https://github.com/MostroP2P/mostrix/pull/47)
* feat: shortened time interval to update keys list of subscription by [@arkanoider](https://github.com/arkanoider) in [#42](https://github.com/MostroP2P/mostrix/pull/42)
* Merge branch 'order-ui-improvement' into subscription-messages by [@arkanoider](https://github.com/arkanoider)
* refactor with subscriptions by [@arkanoider](https://github.com/arkanoider)
* feat: sync first-run key bootstrap and add key regeneration flow by [@grunch](https://github.com/grunch) in [#44](https://github.com/MostroP2P/mostrix/pull/44)
* Merge branch 'main' into feat/key-regeneration-first-launch-backup by [@arkanoider](https://github.com/arkanoider)
* docs: update README for auto-generated settings (#40) by [@grunch](https://github.com/grunch) in [#43](https://github.com/MostroP2P/mostrix/pull/43)

### 🚜 Refactor


* added a helper for subscription and aligned docs for AI guides by [@arkanoider](https://github.com/arkanoider)
* refactor DM routing and add live order/dispute updates with reconciliation by [@arkanoider](https://github.com/arkanoider)
* testing the flow of message change by [@arkanoider](https://github.com/arkanoider)

### 📚 Documentation


* adding docs to generate the correct UI flow by [@arkanoider](https://github.com/arkanoider)
* update README for auto-generated settings (#40) by [@mostronatorcoder[bot]](https://github.com/mostronatorcoder[bot])

### ⚙️ Miscellaneous Tasks


* rabbit rant fix by [@arkanoider](https://github.com/arkanoider)
* fix on fmt and rabbit fix by [@arkanoider](https://github.com/arkanoider)
* added meaningful description in compact label of flow by [@arkanoider](https://github.com/arkanoider)
* improving UI by [@arkanoider](https://github.com/arkanoider)
* fix fmt by [@arkanoider](https://github.com/arkanoider)
* typos by [@arkanoider](https://github.com/arkanoider)
* added debug-notes.md for debugging by [@arkanoider](https://github.com/arkanoider)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#51](https://github.com/MostroP2P/mostrix/pull/51)
* [@grunch](https://github.com/grunch) made their contribution in [#47](https://github.com/MostroP2P/mostrix/pull/47)
* [@mostronatorcoder[bot]](https://github.com/mostronatorcoder[bot]) made their contribution

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.1.7...0.1.8

<!-- generated by git-cliff -->
