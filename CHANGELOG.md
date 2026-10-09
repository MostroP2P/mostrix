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


## What's Changed in 0.3.6

### 🚀 Features


* Ctrl+K Actions for dispute and Observer
* keycap command bars for dispute and Observer
* simplify My Trades command hints
* Telegram notifications for solvers through mostro-watchdog by [@grunch](https://github.com/grunch)
* improve chat copy range selection by [@arkanoider](https://github.com/arkanoider)
* add opt-in OSC 52 chat copy fallback by [@arkanoider](https://github.com/arkanoider)
* copy observer chat messages by [@arkanoider](https://github.com/arkanoider)
* copy solver direct messages by [@arkanoider](https://github.com/arkanoider)
* copy My Trades chat messages by [@arkanoider](https://github.com/arkanoider)
* copy dispute chat messages by [@arkanoider](https://github.com/arkanoider)
* polish reputation status segment by [@arkanoider](https://github.com/arkanoider)
* show own reputation on the status bar by [@arkanoider](https://github.com/arkanoider)
* refresh own reputation at startup and after trades by [@arkanoider](https://github.com/arkanoider)
* cache own reputation on silent channel update by [@arkanoider](https://github.com/arkanoider)
* fetch own user-info via mostro-core 0.17.3 by [@arkanoider](https://github.com/arkanoider)
* Take Order privacy toggle and My Trades surfaces by [@arkanoider](https://github.com/arkanoider)
* honor full_privacy on trade follow-up DMs by [@arkanoider](https://github.com/arkanoider)
* add full-privacy mode on New Order by [@arkanoider](https://github.com/arkanoider)

### 🐛 Bug Fixes


* keep contextual keycaps at 60x15
* zeroize async Observer and attachment keys
* restore dispute Enter send and pin display
* harden Ctrl+K Actions after review
* unwatch disputes the users close and bound watch sends on link by [@grunch](https://github.com/grunch)
* harden chat copy after review by [@arkanoider](https://github.com/arkanoider)
* address chat copy review findings by [@arkanoider](https://github.com/arkanoider)
* fit Observer help on short terminals by [@arkanoider](https://github.com/arkanoider)
* keep copy-cancel help on smallest terminals by [@arkanoider](https://github.com/arkanoider)
* fit compact trade help on short terminals by [@arkanoider](https://github.com/arkanoider)
* preserve copy guard and fit trade help by [@arkanoider](https://github.com/arkanoider)
* address chat clipboard review findings by [@arkanoider](https://github.com/arkanoider)
* preallocate OSC 52 clipboard buffer by [@arkanoider](https://github.com/arkanoider)
* move observer clear to Ctrl+L by [@arkanoider](https://github.com/arkanoider)
* drop stale mostro_info on A→B reconnect by [@arkanoider](https://github.com/arkanoider)
* retry reputation after instance info by [@arkanoider](https://github.com/arkanoider)
* clear reputation on reconnect switch by [@arkanoider](https://github.com/arkanoider)
* refresh own reputation only after purchase by [@arkanoider](https://github.com/arkanoider)
* ignore out-of-order own reputation replies by [@arkanoider](https://github.com/arkanoider)
* refresh own reputation via main loop by [@arkanoider](https://github.com/arkanoider)
* keep status bar within its three rows by [@arkanoider](https://github.com/arkanoider)
* refetch own reputation after session resets by [@arkanoider](https://github.com/arkanoider)
* ignore stale own-reputation channel updates by [@arkanoider](https://github.com/arkanoider)
* pass instance info for live reputation PoW by [@arkanoider](https://github.com/arkanoider)
* fetch user-info after DM listener starts by [@arkanoider](https://github.com/arkanoider)
* save range child under tracked id; prune consumed NextTrade binds at startup by [@arkanoider](https://github.com/arkanoider)
* clear NextTrade binds on key rotation; hand off range child id by [@arkanoider](https://github.com/arkanoider)
* wipe binds, track NextTrade, fail save by [@arkanoider](https://github.com/arkanoider)
* key NextTrade binds; ignore Peer None by [@arkanoider](https://github.com/arkanoider)
* bind NextTrade parent; refuse Shift+U by [@arkanoider](https://github.com/arkanoider)
* avoid racing init_db on shared home DB by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* Merge commit '4b380d26b0dd446825c84151f3896c7c7d6bfcb2'
* pull request #217 from MostroP2P/feat/my-trades-command-bar
* feat: Telegram notifications for solvers through mostro-watchdog by [@arkanoider](https://github.com/arkanoider) in [#215](https://github.com/MostroP2P/mostrix/pull/215)
* Feat/chat clipboard selection by [@arkanoider](https://github.com/arkanoider) in [#216](https://github.com/MostroP2P/mostrix/pull/216)
* feat: show own reputation on the status bar by [@arkanoider](https://github.com/arkanoider) in [#212](https://github.com/MostroP2P/mostrix/pull/212)
* docs: clarify admin_privkey vs nsec_privkey for dispute solvers by [@grunch](https://github.com/grunch) in [#213](https://github.com/MostroP2P/mostrix/pull/213)
* feat: full-privacy orders (create, take, follow-ups) by [@arkanoider](https://github.com/arkanoider) in [#210](https://github.com/MostroP2P/mostrix/pull/210)

### 📚 Documentation


* align comments with keycap command bars
* document chat copy and SSH setup by [@arkanoider](https://github.com/arkanoider)
* sync comments with own-reputation API by [@arkanoider](https://github.com/arkanoider)
* qualify dispute resolution by solver permission by [@grunch](https://github.com/grunch)
* clarify admin_privkey vs nsec_privkey for dispute solvers by [@grunch](https://github.com/grunch)
* align range-child binding, early subscribe, id handoff and fail-closed save by [@arkanoider](https://github.com/arkanoider)

### 🧪 Testing


* align chat copy tests with range selection by [@arkanoider](https://github.com/arkanoider)
* cover chat copy interaction boundaries by [@arkanoider](https://github.com/arkanoider)
* cover wipe, bind conflict, persist fail by [@arkanoider](https://github.com/arkanoider)
* cover NextTrade bind retry and Peer None by [@arkanoider](https://github.com/arkanoider)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#215](https://github.com/MostroP2P/mostrix/pull/215)
* [@grunch](https://github.com/grunch) made their contribution

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.3.5...0.3.6

<!-- generated by git-cliff -->
