# Instructions for AI agents (Claude Code, Cursor): Phase 1

Phase 1 makes PhoeCord fully work on today's Discord through the network layer, adds the WASM plugin runtime and English/Russian localization, and prototypes the installer and the DLL loader before Phoenix Canary ships. Owner: Nanda.

## Step 0: ask who the developer is

At the start of every new session, before reading code or changing anything, ask:

> Who is the developer: **Nanda** or a contributor?

| Developer | Who | Area | Tasks |
|:---|:---|:---|:---|
| **Nanda** | owner: decisions, Plugin API, reviews, merges, releases | core, proxy, CA and trust, loader, installer, runtime, i18n core | see "Who does what" in `docs/phase-1/README.md` |
| **Contributor** | anyone, through a PR that Nanda merges | plugins, SDK examples, tests and fixtures, translations, docs, research | 1.1.9, 1.2.1–1.2.4, 1.2.7, 1.2.9, 1.3.6, 1.3.7, 1.6.2, 1.6.4, 1.7.3, 1.7.5 |

If unsure, stop and ask the human to check `docs/phase-1/README.md`.

## Step 1: read

1. `docs/AGENTS.md`, `CONTRIBUTING.md` and `SECURITY.md`.
2. `ARCHITECTURE.md` (layers, Plugin API, runtime choice, loader, localization).
3. The last 30 lines of `docs/phase-1/ai/ledger.md`.
4. `docs/phase-1/ai/spec.md`, the source of truth.
5. Your task in `docs/phase-1/ai/plan.md`.

## Step 2: work

- Branches: `feat/<short>`, `fix/<short>`, `docs/<short>` from a fresh `origin/main`. One task per PR. Nanda merges with **Squash and merge** after green CI.
- Before pushing: `bash scripts/check.sh` (Windows: `powershell -File scripts/check.ps1`).
- Do not change `crates/phoecord-core/src/plugin.rs` unless the task says so and the developer is Nanda. A contributor who needs an API change writes a `blocked` line in the ledger and tells their human.
- Porting a Vencord plugin: read its Vencord source first and reuse its logic, data and setting names, with the attribution header (`docs/PORTING_PLUGINS.md` §0).
- Porting a v0 module: follow the migration map in `ai/spec.md` §2.3; keep behaviour and tests, change only the wiring to `Chain` and `Host`.
- `unsafe` only in `crates/phoecord-loader`, each block with a `// SAFETY:` comment.
- Tests never touch the network and never need a Discord token. Gateway tests use fixtures from `tests/fixtures/` with personal data removed.
- User-facing strings: English source text; once 1.7.1 lands, through `.ftl` files, never hard-coded.
- Never commit tokens, `ca.key`, `ca.pem`, traffic dumps or Discord client binaries. Never touch the copyright lines in `LICENSE`.
- In the same PR: a line under "Unreleased" in `docs/CHANGELOG.md`, `docs/CODE_MAP.md`, the plugin's row in `docs/PLUGINS.md`, and the status in the task table of `docs/phase-1/README.md` (the only place progress is tracked).
- Do not change a decision of `README.md` silently: a `Ruling:` line in the ledger and the edit in the same commit, then `python scripts/gen_decisions.py`.
- Everything you write (docs, comments, commits, PRs, ledger lines) is in English.

## Ledger format

One line per event, new lines at the bottom, UTC:

```
YYYY-MM-DD HH:MM | <id> | claimed / paused / resumed / blocked / review / complete | <name> | <text>
Ruling: what was decided (owner Nanda, YYYY-MM-DD, "quote") — why: … — risk: …
```
