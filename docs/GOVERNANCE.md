# Governance

## Team

| Who | Role | Discord | Telegram |
|:---|:---|:---|:---|
| **Nanda** | Project lead | `nandak070` | [@nanda070](https://t.me/nanda070) |

Copyright holder: **Cheterin Group** (see [`LICENSE`](../LICENSE)).

## Who decides what

- **Nanda** leads development: architecture, the Plugin API, task priorities, review and merging into `main`, releases, repository access.
- **Contributors** work through branches and Pull Requests; plugins, tests, fixtures, docs and research are open to everyone.

Disputed technical questions are decided by Nanda. A decision is recorded as a `Ruling:` line in the plan's ledger ([`_template/ruling.md`](_template/ruling.md)) and collected in [`DECISIONS.md`](DECISIONS.md).

## How changes flow

Per [`CONTRIBUTING.md`](../CONTRIBUTING.md): a branch prefixed `feat/` `fix/` `refactor/` `chore/` `docs/` → Conventional Commits → a Pull Request into `main` → green CI → review and **Squash and merge by the owners only**. Releases are cut from `main` only, by Nanda ([`_template/release.md`](_template/release.md)).

Changes to the CA, TLS, trust stores, the proxy and the loader always need the owner's review ([`SECURITY.md`](../SECURITY.md)).

## Access

- The repository is public under MIT; write access is granted and revoked by Nanda.
- Repository secrets: only Nanda ([`SECRETS.md`](SECRETS.md)).
- When someone leaves, their access is revoked the same day.
