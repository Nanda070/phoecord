# Instructions for AI agents (Claude Code, Cursor): Phase 2

Phase 2 brings PhoeCord to Desktop Phoenix: in-process transport, UI hooks, the theme engine and v1.0. Owner: Nanda.

## Step 0: ask who the developer is

At the start of every new session, before reading code or changing anything, ask:

> Who is the developer: **Nanda**, **ken**, **buslan**, **matvee** or a contributor?

| Developer | Who | Area | Tasks |
|:---|:---|:---|:---|
| **Nanda** | owner: core RE, decisions, Plugin API, security, release, review and merging | binary inspection, Plugin API, the Phoenix installer and installer 1.0, signature tables, themes, context menus, optimistic delete, modals, profiles, client behaviour patches, toolbox, developer docs, v1.0 scope, security audit, release | 2.1.1, 2.1.2, 2.1.5, 2.1.6, 2.1.7; 2.2.1, 2.2.8, 2.2.10; 2.3.2, 2.3.5, 2.3.6, 2.3.11, 2.3.12, 2.3.14, 2.3.20, 2.3.23; 2.4.2, 2.4.3, 2.4.4, 2.4.5, 2.4.6, 2.4.7 |
| **ken** | proxy, transport, wire format, degradation | proxy behavior, gateway wire, proxy reach, HTTP transport hook, Gateway registry detours, outgoing gateway hook, in-process transport wiring, Canary loader check, degradation, settings tab, top plugins on Phoenix | 2.1.3, 2.1.4, 2.1.8; 2.2.2, 2.2.3, 2.2.4, 2.2.7, 2.2.11; 2.3.3, 2.3.7; 2.4.1, 2.4.8 |
| **buslan** | discovery, tooling, installer, signatures | signature verification on two builds, the CDN picture hook, FixImagesQuality on Phoenix, chat input, voice and audio, account panel, media, channel and server lists | 2.2.9; 2.3.8, 2.3.9, 2.3.15–2.3.19 |
| **matvee** | RE support, locators, UI hooks, theme engine | recon report, gateway fixtures, typed locators, `phoecord check`, theme engine, MessageLogger UI, the generic view-render wrap, user names, overlay, themes picker | 2.1.9, 2.1.10; 2.2.5, 2.2.6; 2.3.1, 2.3.4, 2.3.10, 2.3.13, 2.3.21, 2.3.22 |
| **Contributor** | anyone, through a PR that Nanda merges | no task assigned; ask Nanda before taking one | — |

If unsure, stop and ask the human to check `docs/phase-2/README.md`.

## Step 1: read

1. `docs/AGENTS.md`, `CONTRIBUTING.md` and `SECURITY.md`.
2. `ARCHITECTURE.md` (layers, Plugin API, two transports, loader, localization).
3. The last 30 lines of `docs/phase-2/ai/ledger.md`.
4. `docs/phase-2/ai/spec.md`, the source of truth.
5. Your task in `docs/phase-2/ai/plan.md`.
6. `docs/research/phoenix-desktop.md` (sunbird study) and `docs/research/gpui.md` (GPUI study).

## Step 2: work

- Branches: `feat/<short>`, `fix/<short>`, `docs/<short>` from a fresh `origin/main`. One task per PR. Nanda merges with **Rebase and merge** after green CI, so every commit lands in `main`: keep commits clean. A branch with `wip` or fixup commits is squash-merged instead.
- Before pushing: `bash scripts/check.sh` (Windows: `powershell -File scripts/check.ps1`).
- Do not change `crates/phoecord-core/src/plugin.rs` unless the task says so and the developer is Nanda.
- `unsafe` only in `crates/phoecord-loader`, each block with a `// SAFETY:` comment.
- Tests never touch the network and never need a Discord token. Gateway tests use fixtures from `tests/fixtures/`.
- User-facing strings through `.ftl` files, never hard-coded.
- Never commit tokens, `ca.key`, `ca.pem`, traffic dumps, Discord client binaries or disassembly of them.
- In the same PR: a line under "Unreleased" in `docs/CHANGELOG.md`, `docs/CODE_MAP.md`, the plugin's row in `docs/PLUGINS.md`, and the status in the task table of `docs/phase-2/README.md` (the only place progress is tracked).
- Everything you write (docs, comments, commits, PRs, ledger lines) is in English.

## Reverse engineering rules (Phase 2 specific)

- **No Discord binaries in the repository.** No executables, DLLs, `.text` dumps, disassembly listings or memory dumps. Signatures and locators describe patterns; the binary itself stays on the developer's machine.
- **No code from sunbird.** sunbird has no licence. `docs/research/phoenix-desktop.md` describes its approaches in our own words; PhoeCord's implementation must be written independently. Names from Discord's binary (function names, crate paths, element ids) are facts about the binary, not sunbird's code.
- **Ghidra/IDA projects stay local.** The project file, its database and any annotations are not committed. Only the resulting signatures (`.toml`) and typed locators (Rust code in `phoecord-loader`) go into the repository.
- **Signatures are verified on two adjacent builds.** A signature that matches only one build is not merged.
- **No access-gate bypass.** PhoeCord does not bypass Phoenix's account gate. Phase 2 starts because Phoenix is officially available.
- **Hook targets are checked, not assumed.** An inline entry hook needs a prologue of whole instructions with no RIP-relative operand and enough bytes for a jump; a function that does not fit is refused, and only the feature that depends on it is disabled.

## Ledger format

One line per event, new lines at the bottom, UTC:

```
YYYY-MM-DD HH:MM | <id> | claimed / paused / resumed / blocked / review / complete | <name> | <text>
Ruling: what was decided (owner Nanda, YYYY-MM-DD, "quote") — why: … — risk: …
```
