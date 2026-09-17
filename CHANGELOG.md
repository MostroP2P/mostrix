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


## What's Changed in 0.3.3

### 🚀 Features


* simplify cancel affordance on pay/bond invoice popups by [@arkanoider](https://github.com/arkanoider)
* prefer quadrant QR over sextants on narrow terminals by [@arkanoider](https://github.com/arkanoider)
* keep Ack/Cancel visible on PayInvoice QR popups by [@arkanoider](https://github.com/arkanoider)
* show a scannable QR on PayInvoice popups by [@arkanoider](https://github.com/arkanoider)
* wake counterparty after peer order chat attachment send by [@arkanoider](https://github.com/arkanoider)
* wake counterparty after peer order chat text send by [@arkanoider](https://github.com/arkanoider)
* wake disputant after dispute chat send by [@arkanoider](https://github.com/arkanoider)
* add push_server_url with production default by [@arkanoider](https://github.com/arkanoider)
* add mostro-push-server notify module by [@arkanoider](https://github.com/arkanoider)
* INSERT/COMMAND layers for My Trades chat by [@arkanoider](https://github.com/arkanoider)
* refresh order details from Mostro with Shift+U by [@amuntri](https://github.com/amuntri)

### 🐛 Bug Fixes


* keep one-shot hydration on the complete fetch path by [@arkanoider](https://github.com/arkanoider)
* scope single connected relay and gate grace on data by [@arkanoider](https://github.com/arkanoider)
* return on first responsive relay, not just Connected by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* rabbit rant by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* rabbit comment fix by [@arkanoider](https://github.com/arkanoider)
* ermeme comment fix by [@arkanoider](https://github.com/arkanoider)
* bind My Trades chat drafts to order and channel by [@arkanoider](https://github.com/arkanoider)
* address My Trades INSERT/COMMAND review blockers by [@arkanoider](https://github.com/arkanoider)
* keep local columns and refresh DB-built rows on Shift+U by [@amuntri](https://github.com/amuntri)
* scope Shift+U refresh to its order and drop stale snapshots by [@amuntri](https://github.com/amuntri)
* confirm Shift+U refresh and show it in Order Chat hints by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* perf(relay): scope UI fetches to connected relays by [@arkanoider](https://github.com/arkanoider) in [#189](https://github.com/MostroP2P/mostrix/pull/189)
* feat(ui): show a scannable QR on PayInvoice popups by [@arkanoider](https://github.com/arkanoider) in [#188](https://github.com/MostroP2P/mostrix/pull/188)
* docs(push): document chat-recipient wake and push_server_url switch by [@arkanoider](https://github.com/arkanoider) in [#187](https://github.com/MostroP2P/mostrix/pull/187)
* test(push): add wake_target delivery-gate helper and tests by [@arkanoider](https://github.com/arkanoider) in [#186](https://github.com/MostroP2P/mostrix/pull/186)
* feat(push): wake counterparty after peer order chat attachment send by [@arkanoider](https://github.com/arkanoider) in [#185](https://github.com/MostroP2P/mostrix/pull/185)
* feat(push): wake counterparty after peer order chat text send by [@arkanoider](https://github.com/arkanoider) in [#184](https://github.com/MostroP2P/mostrix/pull/184)
* feat(push): wake disputant after dispute chat send by [@arkanoider](https://github.com/arkanoider) in [#183](https://github.com/MostroP2P/mostrix/pull/183)
* refactor(chat): return relay-accepted bool from chat send fns by [@arkanoider](https://github.com/arkanoider) in [#182](https://github.com/MostroP2P/mostrix/pull/182)
* feat(settings): add push_server_url with production default by [@arkanoider](https://github.com/arkanoider) in [#181](https://github.com/MostroP2P/mostrix/pull/181)
* feat(push): add mostro-push-server notify module by [@arkanoider](https://github.com/arkanoider) in [#180](https://github.com/MostroP2P/mostrix/pull/180)
* feat(ui): INSERT/COMMAND layers for My Trades chat by [@arkanoider](https://github.com/arkanoider) in [#178](https://github.com/MostroP2P/mostrix/pull/178)
* feat(orders): refresh order details from Mostro with Shift+U by [@arkanoider](https://github.com/arkanoider) in [#145](https://github.com/MostroP2P/mostrix/pull/145)
* chore: relicense project under GPLv3 by [@arkanoider](https://github.com/arkanoider) in [#171](https://github.com/MostroP2P/mostrix/pull/171)

### 🚜 Refactor


* return relay-accepted bool from chat send fns by [@arkanoider](https://github.com/arkanoider)
* move TUI terminal enter/leave into ui::terminal by [@arkanoider](https://github.com/arkanoider)

### 📚 Documentation


* document chat-recipient wake and push_server_url switch by [@arkanoider](https://github.com/arkanoider)

### ⚡ Performance


* scope UI fetches to connected relays by [@arkanoider](https://github.com/arkanoider)

### 🎨 Styling


* rustfmt after rebase onto main by [@arkanoider](https://github.com/arkanoider)

### 🧪 Testing


* add wake_target delivery-gate helper and tests by [@arkanoider](https://github.com/arkanoider)

### ⚙️ Miscellaneous Tasks


* relicense project under GPLv3 by [@grunch](https://github.com/grunch)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#189](https://github.com/MostroP2P/mostrix/pull/189)
* [@amuntri](https://github.com/amuntri) made their contribution
* [@grunch](https://github.com/grunch) made their contribution

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.3.2...0.3.3

<!-- generated by git-cliff -->
