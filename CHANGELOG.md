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


## What's Changed in 0.2.0

### 🚀 Features


* chain AddBondInvoice reply into next trade popup by [@arkanoider](https://github.com/arkanoider)
* wait for Mostro on finalize and improve success UI by [@arkanoider](https://github.com/arkanoider)
* inline bond slash UI on dispute finalize popup by [@arkanoider](https://github.com/arkanoider)
* wire BondSlashChoice through dispute finalize execute path by [@arkanoider](https://github.com/arkanoider)
* add BondSlashChoice and bump mostro-core for dispute bond slash by [@arkanoider](https://github.com/arkanoider)
* enable wayland-data-control feature for arboard clipboard by [@AndreaDiazCorreia](https://github.com/AndreaDiazCorreia)
* role-aware waiting-phase trade status popups by [@arkanoider](https://github.com/arkanoider)
* PayBondInvoice popup for Mostro Phase 1.5 anti-abuse bond by [@arkanoider](https://github.com/arkanoider)
* add impoved reconcile method for order status by [@arkanoider](https://github.com/arkanoider)
* reconcile SQLite order status from Mostro nostr snapshots by [@arkanoider](https://github.com/arkanoider)
* Step 4 of new mostro-core primitived: introduced chat primitives by [@arkanoider](https://github.com/arkanoider)
* aligned nostr-sdk and mostro-core to latest version by [@arkanoider](https://github.com/arkanoider)
* auto-submit AddInvoice on saved-LN YES; SettingsMenuAction by [@arkanoider](https://github.com/arkanoider)
* confirm saved Lightning address before AddInvoice by [@arkanoider](https://github.com/arkanoider)
* verify LNURL-pay before saving ln_address and AddInvoice by [@arkanoider](https://github.com/arkanoider)
* introduction of ln address for user who are buyers by [@arkanoider](https://github.com/arkanoider)
* add optional ln_address for buyer Lightning address by [@arkanoider](https://github.com/arkanoider)
* add add-solver permission selector and payload support by [@arkanoider](https://github.com/arkanoider)
* improved order in execution header, splitted static data from dinamyc ones by [@arkanoider](https://github.com/arkanoider)
* lot of ui improvements in message flow - right click paste should work now by [@arkanoider](https://github.com/arkanoider)
* feat: by [@arkanoider](https://github.com/arkanoider)
* hold-invoice YES/NO/CANCEL popup and cooperative cancel by [@arkanoider](https://github.com/arkanoider)
* fix: use runtime admin keys and validate admin-add-solver response by [@arkanoider](https://github.com/arkanoider)
* accept hex public/secret keys in Add Solver and Setup Admin Key by [@mostronatorcoder[bot]](https://github.com/mostronatorcoder[bot])

### 🐛 Bug Fixes


* bot rants by [@arkanoider](https://github.com/arkanoider)
* rabbit rant by [@arkanoider](https://github.com/arkanoider)
* rabbit rants by [@arkanoider](https://github.com/arkanoider)
* fix mostronator rants by [@arkanoider](https://github.com/arkanoider)
* rabbit rant by [@arkanoider](https://github.com/arkanoider)
* handle NewOrder book republish after pre-Active taker cancel by [@arkanoider](https://github.com/arkanoider)
* fix(dm): hydrate OrderMessage.is_mine after trade-DM upsert by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* allow c and n in create-order form text fields by [@arkanoider](https://github.com/arkanoider)
* ignore CantDo in trade DM order hydration by [@arkanoider](https://github.com/arkanoider)
* improving bad visualization of orders state in ui by [@arkanoider](https://github.com/arkanoider)
* fixing some UI message and popup messages by [@arkanoider](https://github.com/arkanoider)
* fixed missing sats amount in bond invoice popup by [@arkanoider](https://github.com/arkanoider)
* rabbit rants by [@arkanoider](https://github.com/arkanoider)
* small fix - removed wrong filtering on order flow tab by [@arkanoider](https://github.com/arkanoider)
* fixed bot rants by [@arkanoider](https://github.com/arkanoider)
* rabbit rants ( nipticks ) by [@arkanoider](https://github.com/arkanoider)
* updated docs and minor fix on add solver command menu by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* Feat: antiabuse bond enabled and bond slash activation in admin finalization by [@arkanoider](https://github.com/arkanoider) in [#79](https://github.com/MostroP2P/mostrix/pull/79)
* Gate admin bond slash UI on instance bond_enabled and handle AddBondInvoice. by [@arkanoider](https://github.com/arkanoider)
* feat(admin): Step3 - inline bond slash UI on dispute finalize popup by [@arkanoider](https://github.com/arkanoider) in [#78](https://github.com/MostroP2P/mostrix/pull/78)
* Update docs/FINALIZE_DISPUTES.md by [@arkanoider](https://github.com/arkanoider)
* feat(admin): Step 2 - wire BondSlashChoice through dispute finalize execute path by [@arkanoider](https://github.com/arkanoider) in [#77](https://github.com/MostroP2P/mostrix/pull/77)
* feat(admin): Step1 - bond resolution foundation for dispute finalization by [@arkanoider](https://github.com/arkanoider) in [#76](https://github.com/MostroP2P/mostrix/pull/76)
* fix(dm): handle NewOrder book republish after pre-Active taker cancel by [@arkanoider](https://github.com/arkanoider) in [#74](https://github.com/MostroP2P/mostrix/pull/74)
* fix(clipboard): enable wlr-data-control for Wayland invoice copy by [@arkanoider](https://github.com/arkanoider) in [#75](https://github.com/MostroP2P/mostrix/pull/75)
* feat(ui): role-aware waiting-phase trade status popups by [@arkanoider](https://github.com/arkanoider) in [#73](https://github.com/MostroP2P/mostrix/pull/73)
* fix(ui): allow c and n in create-order form text fields by [@arkanoider](https://github.com/arkanoider) in [#72](https://github.com/MostroP2P/mostrix/pull/72)
* UI improvements by [@arkanoider](https://github.com/arkanoider) in [#69](https://github.com/MostroP2P/mostrix/pull/69)
* Update src/ui/orders.rs by [@arkanoider](https://github.com/arkanoider)
* feat(ui): PayBondInvoice popup for Mostro Phase 1.5 anti-abuse bond by [@arkanoider](https://github.com/arkanoider) in [#68](https://github.com/MostroP2P/mostrix/pull/68)
* feat: relay snapshot DB reconcile for stale order status by [@arkanoider](https://github.com/arkanoider) in [#67](https://github.com/MostroP2P/mostrix/pull/67)
* feat: migrate chat to mostro-core primitives by [@arkanoider](https://github.com/arkanoider) in [#66](https://github.com/MostroP2P/mostrix/pull/66)
* Step 3 - refactor(dm): parse inbound GiftWrap with mostro_core unwrap_message by [@arkanoider](https://github.com/arkanoider) in [#65](https://github.com/MostroP2P/mostrix/pull/65)
* Step  for new mostro-core gift wrap: send GiftWrap via mostro-core nip59 by [@arkanoider](https://github.com/arkanoider) in [#64](https://github.com/MostroP2P/mostrix/pull/64)
* Step 1 for new mostro-core gift wrap: aligned nostr-sdk and mostro-core to latest version by [@arkanoider](https://github.com/arkanoider) in [#63](https://github.com/MostroP2P/mostrix/pull/63)
* feat(ui): auto-submit AddInvoice on saved-LN YES - created enum for settings options by [@arkanoider](https://github.com/arkanoider) in [#62](https://github.com/MostroP2P/mostrix/pull/62)
* feat(ui): confirm saved Lightning address before AddInvoice - step 3 by [@arkanoider](https://github.com/arkanoider) in [#61](https://github.com/MostroP2P/mostrix/pull/61)
* feat(ln): verify LNURL-pay before saving ln_address and AddInvoice - Phase 2 by [@arkanoider](https://github.com/arkanoider) in [#60](https://github.com/MostroP2P/mostrix/pull/60)
* Feature: ln address for buyers invoice - Step 1 by [@arkanoider](https://github.com/arkanoider) in [#59](https://github.com/MostroP2P/mostrix/pull/59)
* feat: add add-solver permission selector and payload support by [@arkanoider](https://github.com/arkanoider) in [#58](https://github.com/MostroP2P/mostrix/pull/58)
* feat: accept hex pubkey/seckey in Add Solver and Setup Admin Key by [@arkanoider](https://github.com/arkanoider) in [#56](https://github.com/MostroP2P/mostrix/pull/56)
* Merge branch 'main' into feat/accept-hex-keys by [@arkanoider](https://github.com/arkanoider)
* feat(ui): hold-invoice YES/NO/CANCEL popup and cooperative cancel by [@arkanoider](https://github.com/arkanoider) in [#54](https://github.com/MostroP2P/mostrix/pull/54)

### 🚜 Refactor


* parse inbound GiftWrap with mostro_core unwrap_message by [@arkanoider](https://github.com/arkanoider)
* send GiftWrap via mostro-core nip59 by [@arkanoider](https://github.com/arkanoider)
* moved a function from main to key helpers by [@arkanoider](https://github.com/arkanoider)
* doing some improvement on my trades and messages tab - testing by [@arkanoider](https://github.com/arkanoider)
* improving persistence of a history of orders in my trades tab - added cancel for single order or all completed ones by [@arkanoider](https://github.com/arkanoider)

### ⚙️ Miscellaneous Tasks


* cargo fmt fix by [@arkanoider](https://github.com/arkanoider)
* rabbit rant by [@arkanoider](https://github.com/arkanoider)
* final rants by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* rabbit rant on docs by [@arkanoider](https://github.com/arkanoider)
* fix rabbit rants by [@arkanoider](https://github.com/arkanoider)
* mostronator rant by [@arkanoider](https://github.com/arkanoider)
* chore: rabbit rants by [@arkanoider](https://github.com/arkanoider)
* fixed docs by [@arkanoider](https://github.com/arkanoider)
* fix ln address rant of mostronator by [@arkanoider](https://github.com/arkanoider)
* bot rants by [@arkanoider](https://github.com/arkanoider)
* rabbit rant by [@arkanoider](https://github.com/arkanoider)
* rabbit rant by [@arkanoider](https://github.com/arkanoider)
* fixed a rabbit rant by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* small code improvements by [@arkanoider](https://github.com/arkanoider)
* rabbit rant by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* fixed some formats and ui layout in coop cancel menu by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#79](https://github.com/MostroP2P/mostrix/pull/79)
* [@AndreaDiazCorreia](https://github.com/AndreaDiazCorreia) made their contribution
* [@mostronatorcoder[bot]](https://github.com/mostronatorcoder[bot]) made their contribution

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.1.9...0.2.0

<!-- generated by git-cliff -->
