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


## What's Changed in 0.2.7

### 🚀 Features


* rename K_conv to Shared key in Observer/My Trades UI and add clipboard copy by [@arkanoider](https://github.com/arkanoider)

### 🐛 Bug Fixes


* restore Linux clipboard ownership in shared copy helper by [@arkanoider](https://github.com/arkanoider)
* guard admin dispute upsert against finalized resurrection (MOSTRO-074) by [@arkanoider](https://github.com/arkanoider)
* keep last_trade_index monotonic after delayed save_order by [@arkanoider](https://github.com/arkanoider)
* propagate trade-index DB errors and reserve atomically (MOSTRO-082) by [@arkanoider](https://github.com/arkanoider)
* reject null request_id on protocol waiter path (MOSTRO-072) by [@arkanoider](https://github.com/arkanoider)
* verify Mostro author on relay-fed order/dispute intake by [@arkanoider](https://github.com/arkanoider)
* respawn fetch/DM tasks when unsubscribe_all fails on reload by [@arkanoider](https://github.com/arkanoider)
* right-align buyer and seller messages in dispute chat by [@arkanoider](https://github.com/arkanoider)
* address PR #135 feedback from ermeme and CodeRabbit by [@arkanoider](https://github.com/arkanoider)
* close taken disputes after cooperative cancel by [@arkanoider](https://github.com/arkanoider)
* bind DM upsert to routed order id and preserve trade keys by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* feat(ui): rename K_conv to Shared key in Observer/My Trades and add clipboard copy by [@arkanoider](https://github.com/arkanoider) in [#134](https://github.com/MostroP2P/mostrix/pull/134)
* fix: respawn fetch/DM tasks after failed unsubscribe on reload by [@arkanoider](https://github.com/arkanoider) in [#136](https://github.com/MostroP2P/mostrix/pull/136)
* fix(admin): close taken disputes after cooperative cancel by [@arkanoider](https://github.com/arkanoider) in [#135](https://github.com/MostroP2P/mostrix/pull/135)

### 🚜 Refactor


* remove Signer pubkey from Observer tab and disclosure popup by [@arkanoider](https://github.com/arkanoider)

### ⚙️ Miscellaneous Tasks


* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* fix cargo fmt by [@arkanoider](https://github.com/arkanoider)
* fix cargo fmt by [@arkanoider](https://github.com/arkanoider)
* fix cargo fmt by [@arkanoider](https://github.com/arkanoider)

### 🛡️ Security


* fix(security): guard admin dispute upsert against finalized resurrection (MOSTRO-074) by [@arkanoider](https://github.com/arkanoider) in [#141](https://github.com/MostroP2P/mostrix/pull/141)
* fix(security): propagate trade-index DB errors and reserve atomically (MOSTRO-082) by [@arkanoider](https://github.com/arkanoider) in [#140](https://github.com/MostroP2P/mostrix/pull/140)
* fix(security): reject null request_id on protocol waiter path (MOSTRO-072) by [@arkanoider](https://github.com/arkanoider) in [#138](https://github.com/MostroP2P/mostrix/pull/138)
* fix(security): verify Mostro author on relay-fed order/dispute intake by [@arkanoider](https://github.com/arkanoider) in [#137](https://github.com/MostroP2P/mostrix/pull/137)
* fix(security): prevent cross-order DM upsert clobber and trade-key swap by [@arkanoider](https://github.com/arkanoider) in [#133](https://github.com/MostroP2P/mostrix/pull/133)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#134](https://github.com/MostroP2P/mostrix/pull/134)

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.2.6...0.2.7

<!-- generated by git-cliff -->
