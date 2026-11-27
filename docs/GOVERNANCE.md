# Governance, access and secrets

Checked against code: `a965f75`, 23.11.2026.

## Team

| Who | Role | Discord | Telegram |
|:---|:---|:---|:---|
| **Nanda** | Project lead | `nandak070` | [@nanda070](https://t.me/nanda070) |

Copyright holder: **Cheterin Group** (see [`NOTICE`](../NOTICE)). License: GPL-3.0-or-later ([`LICENSE`](../LICENSE)); the plugin SDK, `wit/` and templates are MIT.

## Who decides what

- **Nanda** leads development: architecture, the Plugin API, task priorities, review and merging into `main`, releases, repository access.
- **Contributors** work through branches and Pull Requests ([`CONTRIBUTING.md`](../CONTRIBUTING.md)); plugins, tests, fixtures, docs and research are open to everyone. Every pull request is assigned to Nanda070 and goes through Nanda070's review (`.github/CODEOWNERS` and the `assign` workflow do this automatically).
- Disputed technical questions are decided by Nanda, recorded as a `Ruling:` line in the plan's ledger ([`_template/ruling.md`](_template/ruling.md)) and collected in [`DECISIONS.md`](DECISIONS.md).
- Changes to the CA, TLS, trust stores, the proxy and the loader always need the owner's review ([`SECURITY.md`](../SECURITY.md)).
- Every problem, bug and plugin idea: [Issues](https://github.com/Nanda070/phoecord/issues). Leaks of personal data or secrets, and vulnerabilities: privately, [DM `nandak070` on Discord](https://discord.com/users/1219803151180370021) (user ID `1219803151180370021`), per [`SECURITY.md`](../SECURITY.md).

## Secrets

**Values never go here.** A new key, access or storage place is added to this table in the same commit.

| Secret | Where | Who holds it | Who uses it |
|:---|:---|:---|:---|
| Local CA key (`ca.key`) of each user | The user's own PhoeCord data folder, owner-only permissions | Each user, never the project | The user's proxy |
| Dev console token (`dev-token`) | The user's own PhoeCord data folder, new at every proxy start, owner-only permissions on Unix | Each user, never the project | `phoecord dev console` against the user's own proxy (loopback only) |
| GitHub repository admin | GitHub account `Nanda070` | Nanda | Repository settings, secrets, releases |
| The user's Discord session header (`Authorization`) | In the proxy's memory only, never on disk | Each user, never the project | `Host::rest` (30 min TTL, dropped on 401) |
| GitHub Actions secret `DISCORD_RELEASE_WEBHOOK` | Repository settings → Secrets and variables → Actions | Nanda | `discord-release.yml` (posts the release announcement) |
| Other GitHub Actions secrets | Repository settings → Secrets; none yet | Nanda | CI |

The project never needs a Discord token: tests do not log in, and `selftest` runs without an account. A user's CA belongs to that user: the project never generates, collects or ships one. Releases are built only by CI.

A secret that reached the repository, a chat, a log or a screenshot counts as leaked: rotate it, do not just delete it.

| Leaked | What to do |
|:---|:---|
| A user's `ca.key` | `phoecord untrust`, delete the data folder, run again: a new CA is created. Only Discord domains were ever at risk |
| GitHub access | Revoke the token or remove the collaborator, review recent pushes and releases |

Tell Nanda right away in every case. Write access is granted and revoked by Nanda; when someone leaves, their access is revoked the same day.

## Releases

- **Who tags.** Only Nanda pushes release tags (`v<major>.<minor>.<patch>`, a `-` suffix such as `v0.5.0-rc.1` makes a prerelease). Contributors never push `v*` tags, to the main repository or to a fork.
- **Before the tag.** `[workspace.package] version` in `Cargo.toml` equals the tag without `v` (the release run stops otherwise, because installer names and `phoecord --version` come from it), and `docs/CHANGELOG.md` has a `## [<version>]` section: it becomes the release notes. Without one, GitHub's generated notes are used.
- **What CI does.** `.github/workflows/release.yml` builds the CLI for Linux, Windows and macOS (Intel and Apple Silicon) and the loader `version.dll` on Windows, packs each with `LICENSE`, `NOTICE`, `LICENSES/` and the terms, adds the installers from `package.yml`, writes one `SHA256SUMS` over every file and leaves the GitHub release as a draft with the unsigned-build note. It uses only `GITHUB_TOKEN`; no secrets. A tag fails if `crates/phoecord-installer/update-key.pub` holds no key.
- **Signing and publishing.** Nanda runs `scripts/sign-release.ps1 -Tag v<version>` on their own computer: it downloads every file of the draft, checks each against `SHA256SUMS`, signs `SHA256SUMS` with the offline minisign key (trusted comment `phoecord <version>`), checks the signature with the committed public key, uploads `SHA256SUMS.minisig` and publishes. Installed copies accept only releases signed this way (audit A1, 2.4.6). The key is made once with `scripts/new-update-key.ps1`; the secret key and its password are kept offline with a backup and never go into the repository, GitHub secrets or CI. A lost key cuts installed copies off automatic updates until they are reinstalled by hand with a build that carries the new public key. A manual run (`workflow_dispatch`, dry run by default) builds everything and publishes nothing.
- **Announcing.** Every published release gets a short update in the community Discord (what changed, what is new, download links). It is automatic: `.github/workflows/discord-release.yml` runs `scripts/notify_release.py` when a release is published (not for drafts or prereleases) and posts through the webhook kept in the secret `DISCORD_RELEASE_WEBHOOK`; the URL never goes into the tree. The text is the `docs/CHANGELOG.md` section of that version, shortened, so write the section for readers. Resend by hand: Actions, "Announce release", Run workflow (dry run by default), or `python scripts/notify_release.py --tag v<version> --dry-run`. If the webhook leaks, delete it in the Discord channel's integrations and set a new secret.
- **Unsigned binaries.** No code signing of the binaries at any stage; only `SHA256SUMS` is signed, with minisign ([`DECISIONS.md`](DECISIONS.md)).

