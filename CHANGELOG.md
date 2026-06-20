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


## What's Changed in 0.2.1

### 🚀 Features


* apply v2 first-contact PoW on NewOrder and take actions by [@arkanoider](https://github.com/arkanoider)
* transport-aware DM listener for v2 inbound (step 6) by [@arkanoider](https://github.com/arkanoider)
* unify inbound DM parse on unwrap_incoming (v2 step 5) by [@arkanoider](https://github.com/arkanoider)
* send Mostro DMs via wrap_message_with (v2 outbound) by [@arkanoider](https://github.com/arkanoider)
* add transport-aware DM filters and listener subscriptions by [@arkanoider](https://github.com/arkanoider)
* parse protocol_version and prepare mostro-core 0.13 transport by [@arkanoider](https://github.com/arkanoider)
* handle maker PayBondInvoice on order creation (Phase 5) by [@arkanoider](https://github.com/arkanoider)
* Ctrl+O send attachment picker and mobile wire JSON by [@arkanoider](https://github.com/arkanoider)
* animated startup splash during post-terminal boot by [@arkanoider](https://github.com/arkanoider)
* Phase B outbound attachment send pipeline by [@arkanoider](https://github.com/arkanoider)
* receive attachments, fix chat scroll, skip own relay echoes by [@arkanoider](https://github.com/arkanoider)

### 🐛 Bug Fixes


* await old listener before draining DM subscriptions by [@arkanoider](https://github.com/arkanoider)
* unsubscribe only listener subs on transport respawn by [@arkanoider](https://github.com/arkanoider)
* rabbit rants by [@arkanoider](https://github.com/arkanoider)
* gh actions fix by [@arkanoider](https://github.com/arkanoider)
* fix bot rants by [@arkanoider](https://github.com/arkanoider)
* rabbit rants by [@arkanoider](https://github.com/arkanoider)
* bot rants fixed by [@arkanoider](https://github.com/arkanoider)
* trade-key Blossom auth and send retry after upload by [@arkanoider](https://github.com/arkanoider)
* persist attachment transcripts and restore chat UX after restart by [@arkanoider](https://github.com/arkanoider)
* rabbit rants by [@arkanoider](https://github.com/arkanoider)

### 💼 Other


* docs(protocol): complete v2 migration docs and tests (step 9) by [@arkanoider](https://github.com/arkanoider) in [#92](https://github.com/MostroP2P/mostrix/pull/92)
* feat(protocol): apply v2 first-contact PoW on NewOrder and take actions by [@arkanoider](https://github.com/arkanoider) in [#91](https://github.com/MostroP2P/mostrix/pull/91)
* feat(protocol): transport-aware DM listener for v2 inbound (step 6) by [@arkanoider](https://github.com/arkanoider) in [#90](https://github.com/MostroP2P/mostrix/pull/90)
* feat(protocol): unify inbound DM parse on unwrap_incoming (v2 step 5) by [@arkanoider](https://github.com/arkanoider) in [#89](https://github.com/MostroP2P/mostrix/pull/89)
* feat(protocol): send Mostro DMs via wrap_message_with (v2 outbound) by [@arkanoider](https://github.com/arkanoider) in [#88](https://github.com/MostroP2P/mostrix/pull/88)
* feat(protocol): transport-aware DM filters and listener subscriptions by [@arkanoider](https://github.com/arkanoider) in [#87](https://github.com/MostroP2P/mostrix/pull/87)
* feat(protocol): parse protocol_version and bump mostro-core 0.13.0 by [@arkanoider](https://github.com/arkanoider) in [#86](https://github.com/MostroP2P/mostrix/pull/86)
* added deepwiki link by [@arkanoider](https://github.com/arkanoider)
* feat(bond): handle maker PayBondInvoice on order creation (Phase 5) by [@arkanoider](https://github.com/arkanoider) in [#85](https://github.com/MostroP2P/mostrix/pull/85)
* feat(my-trades): Ctrl+O send attachment picker and mobile wire JSON by [@arkanoider](https://github.com/arkanoider) in [#84](https://github.com/MostroP2P/mostrix/pull/84)
* Merge commit 'd90cf877e3b0e6cb87349370a3457e3abb0264b7' by [@arkanoider](https://github.com/arkanoider)
* feat: animated startup splash during post-terminal boot by [@arkanoider](https://github.com/arkanoider) in [#83](https://github.com/MostroP2P/mostrix/pull/83)
* feat(my-trades): outbound attachment send pipeline by [@arkanoider](https://github.com/arkanoider) in [#82](https://github.com/MostroP2P/mostrix/pull/82)
* fix(my-trades): persist attachment transcripts and restore chat UX at startup by [@arkanoider](https://github.com/arkanoider) in [#81](https://github.com/MostroP2P/mostrix/pull/81)
* feat(my-trades): receive attachments by [@arkanoider](https://github.com/arkanoider) in [#80](https://github.com/MostroP2P/mostrix/pull/80)

### 📚 Documentation


* complete v2 migration docs and tests (step 9) by [@arkanoider](https://github.com/arkanoider)

### ⚙️ Miscellaneous Tasks


* removed unused parameter from the code base by [@arkanoider](https://github.com/arkanoider)
* cargo fmt by [@arkanoider](https://github.com/arkanoider)
* bot rants by [@arkanoider](https://github.com/arkanoider)
* rabbit rant by [@arkanoider](https://github.com/arkanoider)
* fix rants by [@arkanoider](https://github.com/arkanoider)

## Contributors
* [@arkanoider](https://github.com/arkanoider) made their contribution in [#92](https://github.com/MostroP2P/mostrix/pull/92)

**Full Changelog**: https://github.com/MostroP2P/mostrix/compare/v0.2.0...0.2.1

<!-- generated by git-cliff -->
