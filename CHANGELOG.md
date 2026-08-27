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


## What's Changed in 0.2.9

### 🚀 Features


* show a dismissible buyer popup on payment-failed by [@arkanoider](https://github.com/arkanoider)
* surface payment-failed as a buyer notification by [@arkanoider](https://github.com/arkanoider)
* Shift+R orphan picker instead of bulk recover by [@arkanoider](https://github.com/arkanoider)
* paste into dispute chat and delete local disputes by [@arkanoider](https://github.com/arkanoider)
* recover missing taken disputes with Shift+R by [@arkanoider](https://github.com/arkanoider)

### 🐛 Bug Fixes


* let the key-rotation confirmation be arrowed to NO by [@amuntri](https://github.com/amuntri)
* require sticky marker to reopen post-retry AddInvoice by [@arkanoider](https://github.com/arkanoider)
* center payment-failed popup body text by [@arkanoider](https://github.com/arkanoider)
* structure payment-failed popup text for readability by [@arkanoider](https://github.com/arkanoider)
* do not reopen AddInvoice from PaymentFailed by status alone by [@arkanoider](https://github.com/arkanoider)
* show warning emoji for payment-failed Messages rows by [@arkanoider](https://github.com/arkanoider)
* reopen replacement invoice and wrap bodies by [@arkanoider](https://github.com/arkanoider)
* keep post-retry add-invoice usable and labeled by [@arkanoider](https://github.com/arkanoider)
* route bracketed paste to settings inputs by [@arkanoider](https://github.com/arkanoider)
* reserve two footer rows for mid-width disputes hints by [@arkanoider](https://github.com/arkanoider)
* restore Shift+R confirm feedback and show CantDo reasons by [@arkanoider](https://github.com/arkanoider)
* log only real DB errors during relay order reconcile by [@arkanoider](https://github.com/arkanoider)
* address high-priority CodeRabbit review items for admin disputes by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* fix(ui): let the key-rotation confirmation be arrowed to NO by [@arkanoider](https://github.com/arkanoider) in [#144](https://github.com/MostroP2P/mostrix/pull/144)
* feat(payment-failed): show a dismissible buyer popup on payment-failed (step 2) by [@arkanoider](https://github.com/arkanoider) in [#148](https://github.com/MostroP2P/mostrix/pull/148)
* feat(payment-failed): surface payment-failed as a buyer notification (step 1) by [@arkanoider](https://github.com/arkanoider) in [#147](https://github.com/MostroP2P/mostrix/pull/147)
* feat(admin): recover, paste, and delete for disputes in progress by [@arkanoider](https://github.com/arkanoider) in [#146](https://github.com/MostroP2P/mostrix/pull/146)

### 🚜 Refactor


* add Order::try_get_by_id for optional local lookups by [@arkanoider](https://github.com/arkanoider)

### 🧪 Testing


* lock in post-retry AddInvoice popup behavior by [@arkanoider](https://github.com/arkanoider)

### ⚙️ Miscellaneous Tasks


* fix cargo fmt by [@arkanoider](https://github.com/arkanoider)
* Windows artifact workflow without publishing Latest by [@arkanoider](https://github.com/arkanoider)
* fix cargo fmt by [@arkanoider](https://github.com/arkanoider)

### ◀️ Revert


* drop branch Windows artifact CI workflow by [@arkanoider](https://github.com/arkanoider)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#144](https://github.com/MostroP2P/mostrix/pull/144)
* [@amuntri](https://github.com/amuntri) made their contribution

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.2.8...0.2.9

<!-- generated by git-cliff -->
