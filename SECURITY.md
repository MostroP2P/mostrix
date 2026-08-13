# Security Policy

Mostrix is a terminal client for peer-to-peer trading over the Mostro protocol. It
handles private keys, Nostr messages, and Lightning payment data, so we take
security reports seriously and appreciate the effort of researchers who report
issues responsibly.

## Supported Versions

Security fixes are applied to the latest released version and to the `main`
branch. Older releases are not backported.

| Version | Supported |
| ------- | --------- |
| `main`  | Yes (development) |
| 0.2.x   | Yes (latest release) |
| < 0.2.0 | No |

Mostrix is under active development. Users are strongly encouraged to run the
latest release.

## Reporting a Vulnerability

**Do not open a public GitHub issue or pull request for a security
vulnerability.**

Report vulnerabilities privately to:

**security@mostro.network**

Please include as much of the following as possible:

- Affected version or commit hash.
- Affected component (for example: key management, DM listener, dispute chat,
  settings loading, database layer, release workflow).
- A description of the vulnerability and its impact.
- Step-by-step reproduction instructions, or a proof of concept.
- Environment details: operating system, Rust toolchain version, Mostro instance
  and relays used.
- Any suggested mitigation or patch, if you have one.

If you need to send sensitive material, note it in your initial email and we will
arrange an encrypted channel.

## Response Process

- **Acknowledgement:** we aim to confirm receipt within 72 hours.
- **Initial assessment:** we aim to provide a severity assessment and a
  preliminary plan within 7 days.
- **Progress updates:** we will keep you informed as the fix is developed.
- **Fix and release:** the fix is released, and the advisory is published once
  users have had a reasonable opportunity to upgrade.

## Disclosure Policy

We follow coordinated disclosure. Please give us a reasonable window to release a
fix before disclosing the issue publicly. We will credit reporters in the release
notes and the security advisory unless anonymity is requested.

Mostrix does not currently operate a paid bug bounty program.

## Scope

### In scope

- Source code in this repository.
- Key generation, derivation, storage, and rotation (see
  [docs/KEY_MANAGEMENT.md](docs/KEY_MANAGEMENT.md)).
- Handling and validation of Nostr events and Mostro protocol messages,
  including NIP-44 and NIP-59 gift-wrapped direct messages.
- Local data handling: SQLite database, `settings.toml`, log files.
- Build and release workflows under [.github/workflows/](.github/workflows/), and
  the integrity of published release artifacts.

### Out of scope

- The Mostro daemon and other MostroP2P components. Report those in their own
  repositories, starting with
  [MostroP2P/mostro](https://github.com/MostroP2P/mostro).
- Vulnerabilities in third-party dependencies, which should be reported upstream.
  Tell us as well if the issue is exploitable through Mostrix.
- Nostr relays, Lightning nodes, and other external infrastructure not operated
  by this project.
- Issues that require an already-compromised host, physical access to an unlocked
  machine, or a malicious local user with the same privileges as the Mostrix
  process.
- Social engineering and denial-of-service testing against public relays or
  Mostro instances.

## Security Considerations for Users

These are known properties of the current design, not vulnerabilities:

- **Private keys are stored unencrypted on disk.** The BIP-39 mnemonic is stored
  in the SQLite database, and `nsec_privkey` and `admin_privkey` are stored in
  `settings.toml`. Protect these files with appropriate filesystem permissions
  and full-disk encryption, and never share them.
- **Back up your mnemonic.** Mostrix shows the 12-word mnemonic once, when keys
  are generated or rotated. Losing it means losing access to your identity and
  reputation.
- **Log files may contain sensitive metadata.** Review logs before attaching them
  to a bug report.
- **Verify release binaries.** Releases are signed with the PGP keys published in
  [keys/](keys/). Verification instructions are included in the release notes and
  in [CHANGELOG.md](CHANGELOG.md).
