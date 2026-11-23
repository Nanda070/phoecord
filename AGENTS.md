# Rules for AI agents (Claude Code, Cursor) and people: the PhoeCord repository

The single source of truth for how work is done here. [`CONTRIBUTING.md`](CONTRIBUTING.md) repeats only the git steps for humans; which document to read for which task is in [`docs/README.md`](docs/README.md).

## First

1. Read this file, [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`SECURITY.md`](SECURITY.md).
2. Working on a plan: read that plan's `AGENTS.md` (for Phase 1: [`docs/phase-1/AGENTS.md`](docs/phase-1/AGENTS.md)) and the last 30 lines of its `ai/ledger.md`.
3. Looking for a file: [`docs/CODE_MAP.md`](docs/CODE_MAP.md). Looking for how something works: [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Living docs: always update

A change that leaves these documents wrong is not done. Update them in the same commit:

| You changed | Update |
|:---|:---|
| Added, renamed, moved or removed a crate, module, file, config key or env variable | [`docs/CODE_MAP.md`](docs/CODE_MAP.md) |
| Anything users or plugin authors notice | [`docs/CHANGELOG.md`](docs/CHANGELOG.md), "Unreleased" |
| Finished a roadmap item | the box in [`ROADMAP.md`](ROADMAP.md) and the task status in the plan's `README.md`, then `python scripts/gen_tasks.py` |
| The Plugin API, the runtime choice, how we reach Phoenix | [`ARCHITECTURE.md`](ARCHITECTURE.md) |
| A new secret, key or place where one is stored | [`docs/SECRETS.md`](docs/SECRETS.md) (never the value) |
| A new kind of breakage or a new diagnostic | [`docs/RUNBOOK.md`](docs/RUNBOOK.md) |
| A plugin added or its behaviour changed | its row in [`docs/PLUGINS.md`](docs/PLUGINS.md), [`docs/PROJECT_MAP.md`](docs/PROJECT_MAP.md) and the plugin table in `ROADMAP.md` |
| A decision by the owner | a `Ruling:` line in the plan's `ai/ledger.md`, then `python scripts/gen_decisions.py` |

`TASKS.md` and `docs/DECISIONS.md` are generated: never edit them by hand. `scripts/check.sh` fails if they are stale.

A large document starts with a line `Checked against code: <sha>, <date>.` Whoever re-checks it against the code updates that line.

## How to work

- Branch from a fresh `origin/main` with a prefix: `feat/`, `fix/`, `refactor/`, `chore/`, `docs/`. Commits follow Conventional Commits, scope = crate name without `phoecord-`.
- One task per PR. The PR template's checklist is part of the work, not decoration.
- Plans follow the template in [`docs/_template/plan/`](docs/_template/plan/): a small plan is one `README.md`; a large one adds `AGENTS.md`, `CLAUDE.md`, `ai/spec.md`, `ai/plan.md`, `ai/ledger.md`.
- An owner decision is never changed silently: a new `Ruling:` line plus the edit in the same commit. Format: [`docs/_template/ruling.md`](docs/_template/ruling.md).
- Breakage reports: [`docs/_template/incident.md`](docs/_template/incident.md) into [`docs/incidents/`](docs/incidents/). Releases: [`docs/_template/release.md`](docs/_template/release.md).
- Language: all docs, comments, commit messages and PRs are **English**. User-facing strings of the app are localized (English source, Russian translation; ARCHITECTURE.md, "Localization").

## Never

- Commit Discord tokens, `ca.key`, `ca.pem`, traffic dumps, `.env` files, Discord client binaries or disassembly of them.
- Remove or edit the copyright lines in [`LICENSE`](LICENSE).
- Use `unsafe` outside `crates/phoecord-loader`, or without a `// SAFETY:` comment.
- Write tests that touch the network or need a Discord token.
- Change `crates/phoecord-core/src/plugin.rs` without a task that says so and the owner's review.
- Add features for mass messaging, self-bots, evading bans or moderation, or collecting other people's data.
- Make a plugin open sockets or files directly instead of going through `Host`.
- Merge into `main` yourself. Only the owners merge.

## Checks before merging

```bash
bash scripts/check.sh            # Windows: powershell -File scripts/check.ps1
```

It runs, in order and stopping at the first failure: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `python scripts/gen_tasks.py --check`, `python scripts/gen_decisions.py --check`. CI runs the same on Linux, Windows and macOS. Put the result in the PR's "Checks" section or link the green CI run.
