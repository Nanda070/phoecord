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
| GitHub repository admin | GitHub account `Nanda070` | Nanda | Repository settings, secrets, releases |
| GitHub Actions secrets | Repository settings → Secrets; none yet | Nanda | CI |

The project never needs a Discord token: tests do not log in, and `selftest` runs without an account. A user's CA belongs to that user: the project never generates, collects or ships one. Releases are built only by CI.

A secret that reached the repository, a chat, a log or a screenshot counts as leaked: rotate it, do not just delete it.

| Leaked | What to do |
|:---|:---|
| A user's `ca.key` | `phoecord untrust`, delete the data folder, run again: a new CA is created. Only Discord domains were ever at risk |
| GitHub access | Revoke the token or remove the collaborator, review recent pushes and releases |

Tell Nanda right away in every case. Write access is granted and revoked by Nanda; when someone leaves, their access is revoked the same day.
