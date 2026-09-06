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


## What's Changed in 0.3.1

### 🐛 Bug Fixes


* join aborted fetch tasks before key-reload respawn by [@arkanoider](https://github.com/arkanoider)
* snapshot waiter catch-up ids before spawn by [@arkanoider](https://github.com/arkanoider)
* promote reputation row on PaymentRequestRequired by [@arkanoider](https://github.com/arkanoider)
* keep Order Chat body visible under 60×15 shell chrome by [@arkanoider](https://github.com/arkanoider)
* keep reputation DMs informational and header responsive by [@arkanoider](https://github.com/arkanoider)
* show Order Chat amount, payment, and taker ratings by [@arkanoider](https://github.com/arkanoider)
* bind waiter catch-up to ids snapshotted at spawn by [@arkanoider](https://github.com/arkanoider)
* wait for a live relay and correlate waiter replies by [@arkanoider](https://github.com/arkanoider)
* connect new client before aborting key-reload listener by [@arkanoider](https://github.com/arkanoider)
* keep waiters registered while decrypting incoming DMs by [@arkanoider](https://github.com/arkanoider)
* await aborted reconnect tasks and retry waiter subscribe by [@arkanoider](https://github.com/arkanoider)
* keep wait_for_dm waiters across reconnect (MOSTRO-80) by [@arkanoider](https://github.com/arkanoider)
* keep live outer ids retryable until persist succeeds by [@arkanoider](https://github.com/arkanoider)
* skip inner ids until transcript persist succeeds by [@arkanoider](https://github.com/arkanoider)
* match AddBondInvoice timeout to wait_for_dm by [@arkanoider](https://github.com/arkanoider)
* keep v1 protocol warning visible on short terminals by [@arkanoider](https://github.com/arkanoider)
* defer AddInvoice DB hydration until trusted amount exists (MOSTRO-078) by [@arkanoider](https://github.com/arkanoider)
* fail-closed AddInvoice validation and gate listener framing (MOSTRO-078) by [@arkanoider](https://github.com/arkanoider)
* validate take-sell AddInvoice sats against book order (MOSTRO-078) by [@arkanoider](https://github.com/arkanoider)
* merge DM last_seen cursors by max on listener respawn by [@arkanoider](https://github.com/arkanoider)
* dispatch TrackOrder via current global DM sender by [@arkanoider](https://github.com/arkanoider)
* address CodeRabbit review on task alarms and backoff by [@arkanoider](https://github.com/arkanoider)
* buffer router cmds during listener backoff and replay chats by [@arkanoider](https://github.com/arkanoider)
* respawn critical background tasks individually on failure (MOSTRO-079) by [@arkanoider](https://github.com/arkanoider)
* preserve cache on transient instance-info fetch failures by [@arkanoider](https://github.com/arkanoider)
* preserve cached transport when instance-info auth rejects relay data by [@arkanoider](https://github.com/arkanoider)
* bot rants fixed by [@arkanoider](https://github.com/arkanoider)
* authenticate kind-38385 instance info before applying transport (MOSTRO-075) by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* fix(liveness): keep wait_for_dm waiters across reconnect (MOSTRO-80) by [@arkanoider](https://github.com/arkanoider) in [#170](https://github.com/MostroP2P/mostrix/pull/170)
* Merge branch 'main' into fix/mostro-080-resurrect-waiters-on-reconnect by [@arkanoider](https://github.com/arkanoider)
* fix(ui): show Order Chat amount, payment, and taker ratings by [@arkanoider](https://github.com/arkanoider) in [#169](https://github.com/MostroP2P/mostrix/pull/169)
* fix(chat): skip inner ids until transcript persist succeeds by [@arkanoider](https://github.com/arkanoider) in [#168](https://github.com/MostroP2P/mostrix/pull/168)
* refactor(chat): drop GiftWrap dual-read, kind 14 only by [@arkanoider](https://github.com/arkanoider) in [#167](https://github.com/MostroP2P/mostrix/pull/167)
* Merge commit 'a1937dba2fa5fc30b95c2734f069d0d5cb09c1f2' by [@arkanoider](https://github.com/arkanoider)
* refactor(protocol): speak NIP-44 only for Mostro DMs by [@arkanoider](https://github.com/arkanoider) in [#166](https://github.com/MostroP2P/mostrix/pull/166)
* fix(liveness): per-task respawn on background task failure (MOSTRO-079) by [@arkanoider](https://github.com/arkanoider) in [#161](https://github.com/MostroP2P/mostrix/pull/161)

### 🚜 Refactor


* drop GiftWrap dual-read, kind 14 only by [@arkanoider](https://github.com/arkanoider)
* speak NIP-44 only for Mostro DMs by [@arkanoider](https://github.com/arkanoider)
* drop unused show_result_toast and Applied instance-info path by [@arkanoider](https://github.com/arkanoider)

### 📚 Documentation


* describe kind-14 hydration, drop gift-wrap alias by [@arkanoider](https://github.com/arkanoider)
* align comments with per-task background supervision by [@arkanoider](https://github.com/arkanoider)

### ⚙️ Miscellaneous Tasks


* removed useless check of v1 by [@arkanoider](https://github.com/arkanoider)
* fix comments by [@arkanoider](https://github.com/arkanoider)
* cargo fmt fix by [@arkanoider](https://github.com/arkanoider)

### 🛡️ Security


* fix(security): validate take-sell AddInvoice sats against book order (MOSTRO-078) by [@arkanoider](https://github.com/arkanoider) in [#164](https://github.com/MostroP2P/mostrix/pull/164)
* fix(security): authenticate kind-38385 instance info before applying transport (MOSTRO-075) by [@arkanoider](https://github.com/arkanoider) in [#160](https://github.com/MostroP2P/mostrix/pull/160)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#170](https://github.com/MostroP2P/mostrix/pull/170)

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.3.0...0.3.1

<!-- generated by git-cliff -->
