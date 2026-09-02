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


## What's Changed in 0.3.0

### 🚀 Features


* unify post-restore hydrate orchestrator by [@arkanoider](https://github.com/arkanoider)
* rebuild peer order chat from relay after session restore by [@arkanoider](https://github.com/arkanoider)
* await trade DM replay after session restore by [@arkanoider](https://github.com/arkanoider)
* sync popup, retry, and startup alignment by [@arkanoider](https://github.com/arkanoider)
* sync last trade index from Mostro after restore by [@arkanoider](https://github.com/arkanoider)
* batch-fetch order details after restore-session by [@arkanoider](https://github.com/arkanoider)
* copy seed words with C on View Seed popup by [@arkanoider](https://github.com/arkanoider)
* import seed words with wipe and auto-restore by [@arkanoider](https://github.com/arkanoider)
* add full local session wipe utility by [@arkanoider](https://github.com/arkanoider)
* recover dispute id and solver chat by [@amuntri](https://github.com/amuntri)
* log the request and the outcome by [@amuntri](https://github.com/amuntri)
* recover orders and disputes from Mostro via Settings by [@amuntri](https://github.com/amuntri)

### 🐛 Bug Fixes


* record peer chat inner ids only after transcript save by [@arkanoider](https://github.com/arkanoider)
* clear stale chat cursors after session wipe by [@arkanoider](https://github.com/arkanoider)
* spawn post-restore trade DM replay off the UI loop by [@arkanoider](https://github.com/arkanoider)
* use catch-up fetch when trade DM cursor is missing by [@arkanoider](https://github.com/arkanoider)
* use trade side for matched maker Active rows by [@arkanoider](https://github.com/arkanoider)
* map DB order status to Messages-tab actions on sync by [@arkanoider](https://github.com/arkanoider)
* address review items for sync safety and popup layout by [@arkanoider](https://github.com/arkanoider)
* correlate LastTradeIndex responses by request_id by [@arkanoider](https://github.com/arkanoider)
* fail closed when LastTradeIndex omits trade_index by [@arkanoider](https://github.com/arkanoider)
* preserve local order fields on insert_from_restore collision by [@arkanoider](https://github.com/arkanoider)
* persist peer chat keys on fresh restore insert by [@arkanoider](https://github.com/arkanoider)
* harden settings writes and staged-wipe cleanup by [@arkanoider](https://github.com/arkanoider)
* rollback settings + DB together on import/wipe failure by [@arkanoider](https://github.com/arkanoider)
* address CodeRabbit review — TUI degrade + staged wipe by [@arkanoider](https://github.com/arkanoider)
* support Ctrl+V paste and multiline seed normalize by [@arkanoider](https://github.com/arkanoider)
* address review round 3 by [@amuntri](https://github.com/amuntri)
* data-integrity hardening from review by [@amuntri](https://github.com/amuntri)
* actually send SessionRestored from the restore task by [@amuntri](https://github.com/amuntri)
* size the operation-result popup from the real terminal width by [@amuntri](https://github.com/amuntri)
* address review — sender check, role inference, UI resync by [@amuntri](https://github.com/amuntri)

### 💼 Other


* feat(restore): unify post-restore hydrate orchestrator (step 4) by [@arkanoider](https://github.com/arkanoider) in [#159](https://github.com/MostroP2P/mostrix/pull/159)
* feat(restore): peer chat hydrate and cursor hygiene after session restore by [@arkanoider](https://github.com/arkanoider) in [#158](https://github.com/MostroP2P/mostrix/pull/158)
* feat(restore): await trade DM replay after session restore (step 2) by [@arkanoider](https://github.com/arkanoider) in [#157](https://github.com/MostroP2P/mostrix/pull/157)
* fix(restore): map DB status to Messages-tab actions (step 1) by [@arkanoider](https://github.com/arkanoider) in [#156](https://github.com/MostroP2P/mostrix/pull/156)
* feat(trade-index): on-the-fly sync popup + startup alignment by [@arkanoider](https://github.com/arkanoider) in [#153](https://github.com/MostroP2P/mostrix/pull/153)
* feat(restore): stage 3 — LastTradeIndex sync + shared helper by [@arkanoider](https://github.com/arkanoider) in [#152](https://github.com/MostroP2P/mostrix/pull/152)
* feat(restore): orchestrator stage 2 — batch order details from Mostro by [@arkanoider](https://github.com/arkanoider) in [#151](https://github.com/MostroP2P/mostrix/pull/151)
* feat(restore): recover orders and disputes from Mostro via Settings by [@arkanoider](https://github.com/arkanoider) in [#149](https://github.com/MostroP2P/mostrix/pull/149)

### 📚 Documentation


* add session restore acceptance checklist (step 6) by [@arkanoider](https://github.com/arkanoider)
* document post-restore hydrate pipeline (steps 2–4) by [@arkanoider](https://github.com/arkanoider)
* align comments with post-restore chat hydrate by [@arkanoider](https://github.com/arkanoider)

### ⚙️ Miscellaneous Tasks


* cargo fmt fix by [@arkanoider](https://github.com/arkanoider)
* cargo fmt fix by [@arkanoider](https://github.com/arkanoider)
* cargo fmt fix by [@arkanoider](https://github.com/arkanoider)
* update comments by [@arkanoider](https://github.com/arkanoider)
* update comments by [@arkanoider](https://github.com/arkanoider)
* update comments by [@arkanoider](https://github.com/arkanoider)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#159](https://github.com/MostroP2P/mostrix/pull/159)
* [@amuntri](https://github.com/amuntri) made their contribution

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.2.9...0.3.0

<!-- generated by git-cliff -->
