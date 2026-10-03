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


## What's Changed in 0.3.5

### 🚀 Features


* show user-closed resolution in dispute header by [@arkanoider](https://github.com/arkanoider)
* dedupe user-closed dispute admin popup by [@arkanoider](https://github.com/arkanoider)
* listen for admin user-resolved dispute DMs by [@arkanoider](https://github.com/arkanoider)
* treat cooperatively-canceled as terminal by [@arkanoider](https://github.com/arkanoider)
* show Serbero handoffs on Disputes Pending by [@grunch](https://github.com/grunch)
* take over a dispute Serbero holds (Ctrl+T) by [@grunch](https://github.com/grunch)
* show Serbero's messages to the solver per dispute by [@grunch](https://github.com/grunch)
* read the open time from published_at by [@grunch](https://github.com/grunch)
* single Background Alerts toggle in settings by [@arkanoider](https://github.com/arkanoider)
* alert on new events while terminal unfocused by [@arkanoider](https://github.com/arkanoider)

### 🐛 Bug Fixes


* harden user-closed dispute status writes by [@arkanoider](https://github.com/arkanoider)
* name failed openings in the admin alerts help by [@grunch](https://github.com/grunch)
* address review of the handoff banner by [@grunch](https://github.com/grunch)
* size non-compact help popups by wrapped rows by [@grunch](https://github.com/grunch)
* keep dispute help and take-over hints visible on narrow terminals by [@grunch](https://github.com/grunch)
* keep the take-over origin through confirmation by [@grunch](https://github.com/grunch)
* address review of the take-over picker by [@grunch](https://github.com/grunch)
* scope the Serbero inbox to current key and senders by [@grunch](https://github.com/grunch)
* address review of Serbero solver DMs by [@grunch](https://github.com/grunch)
* ignore an unusable duplicate open-time tag by [@grunch](https://github.com/grunch)

### 💼 Other


* feat: notify admin when users close a dispute by [@arkanoider](https://github.com/arkanoider) in [#204](https://github.com/MostroP2P/mostrix/pull/204)
* feat(ui): show Serbero handoffs on Disputes Pending by [@grunch](https://github.com/grunch) in [#203](https://github.com/MostroP2P/mostrix/pull/203)
* feat(disputes): take over a dispute Serbero holds (Ctrl+T) by [@grunch](https://github.com/grunch) in [#202](https://github.com/MostroP2P/mostrix/pull/202)
* Merge branch 'feat/serbero-solver-dms' into feat/takeover-serbero-dispute by [@grunch](https://github.com/grunch)
* Merge branch 'feat/serbero-solver-dms' into feat/takeover-serbero-dispute by [@grunch](https://github.com/grunch)
* feat(disputes): show Serbero's messages to the solver per dispute by [@grunch](https://github.com/grunch) in [#201](https://github.com/MostroP2P/mostrix/pull/201)
* feat(disputes): read the open time from published_at by [@arkanoider](https://github.com/arkanoider) in [#196](https://github.com/MostroP2P/mostrix/pull/196)
* feat(ui): alert on new events while terminal is unfocused by [@arkanoider](https://github.com/arkanoider) in [#197](https://github.com/MostroP2P/mostrix/pull/197)

### 🚜 Refactor


* correlate admin take with request_id by [@arkanoider](https://github.com/arkanoider)
* clarify open-time vs event stamp by [@arkanoider](https://github.com/arkanoider)

### 📚 Documentation


* describe tested mostro-core/mostrod pair by [@arkanoider](https://github.com/arkanoider)
* align mostro-core pin notes with 0.16.0 by [@arkanoider](https://github.com/arkanoider)
* refresh relay dispute reconcile module note by [@arkanoider](https://github.com/arkanoider)

### 🧪 Testing


* keep a valid open time over a bad duplicate by [@grunch](https://github.com/grunch)
* read the dispute open time from published_at by [@grunch](https://github.com/grunch)

### ⚙️ Miscellaneous Tasks


* cargo fmt fix by [@arkanoider](https://github.com/arkanoider)
* bump mostro-core to 0.16.0 by [@arkanoider](https://github.com/arkanoider)
* cargo fmt fix by [@arkanoider](https://github.com/arkanoider)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#204](https://github.com/MostroP2P/mostrix/pull/204)
* [@grunch](https://github.com/grunch) made their contribution in [#203](https://github.com/MostroP2P/mostrix/pull/203)

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.3.4...0.3.5

<!-- generated by git-cliff -->
