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


## What's Changed in 0.3.4

### 🚀 Features


* add trusted Mostro instance picker and fix info on switch by [@arkanoider](https://github.com/arkanoider)
* adaptive order book filters with Kind, Fiat, and stepped Premium by [@arkanoider](https://github.com/arkanoider)
* add currency-aware payment method picker on new orders by [@arkanoider](https://github.com/arkanoider)
* expose order filter shortcuts by [@ca-ruz](https://github.com/ca-ruz)
* render orders filter UI by [@ca-ruz](https://github.com/ca-ruz)
* add orders filter controls by [@ca-ruz](https://github.com/ca-ruz)
* add order book filter state by [@ca-ruz](https://github.com/ca-ruz)
* manage Blossom servers like relays by [@arkanoider](https://github.com/arkanoider)
* recover the anti-abuse bond QR after Esc and restart by [@arkanoider](https://github.com/arkanoider)
* added shadowbip and mostro-p2p relay as default at first launch by [@arkanoider](https://github.com/arkanoider)

### 🐛 Bug Fixes


* gate Mostro pubkey switch on a successful save by [@arkanoider](https://github.com/arkanoider)
* address Hermeme and CodeRabbit order-filter review by [@arkanoider](https://github.com/arkanoider)
* handle order filter review edge cases by [@ca-ruz](https://github.com/ca-ruz)
* handle constrained order filter help shortcuts by [@ca-ruz](https://github.com/ca-ruz)
* handle caps lock order filter input by [@ca-ruz](https://github.com/ca-ruz)
* handle caps lock order filter shortcuts by [@ca-ruz](https://github.com/ca-ruz)
* address order filter review edge cases by [@ca-ruz](https://github.com/ca-ruz)
* make relay revision selection deterministic by [@ca-ruz](https://github.com/ca-ruz)
* address order filter review feedback by [@ca-ruz](https://github.com/ca-ruz)
* address order filter review feedback by [@ca-ruz](https://github.com/ca-ruz)
* make stale-reply snapshot guard an atomic compare-and-write by [@arkanoider](https://github.com/arkanoider)
* ignore stale AddBondInvoice replies that would regress order state by [@arkanoider](https://github.com/arkanoider)
* treat PayBondInvoice rows at Pending status as bond-pending by [@arkanoider](https://github.com/arkanoider)
* harden expired-invoice UX and persistence error reporting by [@arkanoider](https://github.com/arkanoider)
* keep relay confirm identity and YES/NO on short terminals by [@arkanoider](https://github.com/arkanoider)
* persist before client updates, reconcile by parsed URL by [@arkanoider](https://github.com/arkanoider)
* connect relays added at runtime via and_connect by [@arkanoider](https://github.com/arkanoider)
* robust URL validation and short-terminal remove popup by [@arkanoider](https://github.com/arkanoider)
* read instance name from y tag third element by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* feat(settings): manage Blossom servers like relays by [@arkanoider](https://github.com/arkanoider) in [#194](https://github.com/MostroP2P/mostrix/pull/194)
* origin/main into feat/settings-blossom-servers by [@arkanoider](https://github.com/arkanoider)
* feat(ui): adaptive order book filters (Kind / Fiat / Premium) by [@arkanoider](https://github.com/arkanoider) in [#193](https://github.com/MostroP2P/mostrix/pull/193)
* feat(ui): currency-aware payment method picker on new orders by [@arkanoider](https://github.com/arkanoider) in [#192](https://github.com/MostroP2P/mostrix/pull/192)
* feat(bond): recover the anti-abuse bond QR after Esc and restart by [@arkanoider](https://github.com/arkanoider) in [#191](https://github.com/MostroP2P/mostrix/pull/191)
* Add relay remove/restore and bare-URL add in Settings by [@arkanoider](https://github.com/arkanoider) in [#190](https://github.com/MostroP2P/mostrix/pull/190)
* Add relay remove/restore and bare-URL add in Settings by [@arkanoider](https://github.com/arkanoider)

### 🎨 Styling


* improve order filter popup usability by [@ca-ruz](https://github.com/ca-ruz)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#194](https://github.com/MostroP2P/mostrix/pull/194)
* [@ca-ruz](https://github.com/ca-ruz) made their contribution

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.3.3...0.3.4

<!-- generated by git-cliff -->
