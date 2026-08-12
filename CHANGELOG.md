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


## What's Changed in 0.2.4

### 🚀 Features


* scrollable Orders table and Disputes sidebar lists by [@arkanoider](https://github.com/arkanoider)
* adapt Mostrix to mostro-core kind-14 chat envelope by [@arkanoider](https://github.com/arkanoider)
* normalize hex keys to bech32 in admin key setup by [@Arowolokehinde](https://github.com/Arowolokehinde)
* surface remaining actionable STATUS cases (cancel request, order taken) by [@cursoragent](https://github.com/cursoragent)
* mailbox empty state + card padding polish (step 6) by [@cursoragent](https://github.com/cursoragent)
* merge State + timeline into STATUS banner with next-step callout (step 5) by [@cursoragent](https://github.com/cursoragent)
* responsive Messages panel for narrow terminals by [@cursoragent](https://github.com/cursoragent)
* compact glyph stepper + LineGauge progress (step 4) by [@cursoragent](https://github.com/cursoragent)
* add Messages tab header + TRADE snapshot card (step 3) by [@cursoragent](https://github.com/cursoragent)
* restyle Messages tab sidebar with emoji status language by [@arkanoider](https://github.com/arkanoider)

### 🐛 Bug Fixes


* select Orders book by id via shared filtered projection by [@arkanoider](https://github.com/arkanoider)
* truncate dispute sidebar IDs on char boundaries by [@arkanoider](https://github.com/arkanoider)
* select admin disputes by id instead of index by [@Catrya](https://github.com/Catrya)
* silence clippy unneeded_wildcard_pattern on Rust 1.97 by [@amuntri](https://github.com/amuntri)
* keep premium visible on small terminals by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* show premium in offers and take-order confirmation by [@Vidarte-Alberto](https://github.com/Vidarte-Alberto)
* dual-read fetch and live kind-14 subscribe by [@arkanoider](https://github.com/arkanoider)
* silence clippy warnings on Rust 1.96 by [@arkanoider](https://github.com/arkanoider)
* map HoldInvoicePaymentSettled to Rate on timeline by [@arkanoider](https://github.com/arkanoider)
* do not let Release/Rate override unhappy terminal statuses by [@arkanoider](https://github.com/arkanoider)
* keep Release on step 5 while status is still FiatSent by [@arkanoider](https://github.com/arkanoider)
* keep tab text white, color only the frame border green by [@cursoragent](https://github.com/cursoragent)
* map HoldInvoicePaymentAccepted as actionable in STATUS/Next by [@cursoragent](https://github.com/cursoragent)
* address Hermeme Messages-tab review blockers by [@arkanoider](https://github.com/arkanoider)
* cancel own pending orders instead of take on Enter by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* feat(ui): scrollable Orders table and Disputes sidebar lists by [@arkanoider](https://github.com/arkanoider) in [#110](https://github.com/MostroP2P/mostrix/pull/110)
* fix(ui): admin dispute actions targeted a hidden dispute after finalization by [@arkanoider](https://github.com/arkanoider) in [#109](https://github.com/MostroP2P/mostrix/pull/109)
* fix: silence clippy unneeded_wildcard_pattern on Rust 1.97 by [@arkanoider](https://github.com/arkanoider) in [#107](https://github.com/MostroP2P/mostrix/pull/107)
* fix(ui): show premium in offers and take-order confirmation by [@arkanoider](https://github.com/arkanoider) in [#108](https://github.com/MostroP2P/mostrix/pull/108)
* feat(chat): adapt to mostro-core kind-14 envelope (step 2) by [@arkanoider](https://github.com/arkanoider) in [#103](https://github.com/MostroP2P/mostrix/pull/103)
* fix(ui): keep Release on step 5 while status is still FiatSent by [@arkanoider](https://github.com/arkanoider) in [#100](https://github.com/MostroP2P/mostrix/pull/100)
* feat(settings): normalize hex keys to bech32 in admin key setup by [@arkanoider](https://github.com/arkanoider) in [#101](https://github.com/MostroP2P/mostrix/pull/101)
* feat(ui): Messages tab UX restyle by [@arkanoider](https://github.com/arkanoider) in [#97](https://github.com/MostroP2P/mostrix/pull/97)
* fix(ui): keep tab text white, color only the frame border green by [@arkanoider](https://github.com/arkanoider) in [#99](https://github.com/MostroP2P/mostrix/pull/99)
* Feat/messages tab ux restyle by [@arkanoider](https://github.com/arkanoider) in [#98](https://github.com/MostroP2P/mostrix/pull/98)
* pull request #4 from arkanoider/cursor/messages-tab-status-parity-refactor-2f3e by [@arkanoider](https://github.com/arkanoider)
* pull request #5 from arkanoider/cursor/green-tab-frames-2f3e by [@arkanoider](https://github.com/arkanoider)
* pull request #3 from arkanoider/cursor/messages-tab-status-next-fix-2f3e by [@arkanoider](https://github.com/arkanoider)
* feat(ui): Messages tab UX restyle by [@arkanoider](https://github.com/arkanoider) in [#96](https://github.com/MostroP2P/mostrix/pull/96)
* pull request #2 from arkanoider/cursor/messages-tab-step4-2f3e by [@arkanoider](https://github.com/arkanoider)
* pull request #1 from arkanoider/cursor/messages-tab-step3-2f3e by [@arkanoider](https://github.com/arkanoider)

### 🚜 Refactor


* extract filtered-disputes helper to shared module by [@Catrya](https://github.com/Catrya)
* replace 9-field existing-message tuple with a named struct by [@cursoragent](https://github.com/cursoragent)

### 📚 Documentation


* sync README Rust badge with rust-toolchain.toml (1.96.0) by [@github-actions[bot]](https://github.com/github-actions[bot])
* record narrow-terminal UX degradation as a durable UI guideline by [@cursoragent](https://github.com/cursoragent)

### 🎨 Styling


* unify tab frames with green rounded borders by [@cursoragent](https://github.com/cursoragent)

### 🧪 Testing


* cover dispute selection with mixed-status lists by [@Catrya](https://github.com/Catrya)
* cover thin popup overlays and document TestBackend waves by [@arkanoider](https://github.com/arkanoider)
* cover layout popup helpers at 0% coverage by [@arkanoider](https://github.com/arkanoider)

### ⚙️ Miscellaneous Tasks


* drop stray bind and log admin chat send target by [@Catrya](https://github.com/Catrya)
* bumped MSRV version by [@arkanoider](https://github.com/arkanoider)
* bump rust toolchain to 1.96.0 by [@arkanoider](https://github.com/arkanoider)
* removed some comment and logic for n/N key cancel - no more double path esc or n by [@arkanoider](https://github.com/arkanoider)
* removed some comment and logic for Y key confirmation - no more double path enter or Y by [@arkanoider](https://github.com/arkanoider)
* add coverage and README badge workflows by [@arkanoider](https://github.com/arkanoider)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#110](https://github.com/MostroP2P/mostrix/pull/110)
* [@Catrya](https://github.com/Catrya) made their contribution
* [@Vidarte-Alberto](https://github.com/Vidarte-Alberto) made their contribution
* [@amuntri](https://github.com/amuntri) made their contribution
* [@github-actions[bot]](https://github.com/github-actions[bot]) made their contribution
* [@Arowolokehinde](https://github.com/Arowolokehinde) made their contribution
* [@cursoragent](https://github.com/cursoragent) made their contribution

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.2.3...0.2.4

<!-- generated by git-cliff -->
