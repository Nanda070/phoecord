# Changelog

All notable changes to the project ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/), [SemVer](https://semver.org/)). Every PR adds its line under "Unreleased"; a release moves those lines under its version and date.

## [Unreleased]

### Added

- Cargo workspace of 8 crates: `core`, `proxy`, `plugin-runtime`, `plugin-sdk`, `plugins`, `loader`, `installer`, `cli`.
- Plugin API v1 (`phoecord-core::plugin`): `Manifest`, `Event`, `EventKind`, `Verdict`, `Host`.
- Plugin chain `phoecord-plugin-runtime::Chain`: the first `Drop`/`Reply` stops the chain.
- AOB signature scanner in `phoecord-loader` (`??` masks, ambiguous matches rejected).
- Docs: README, ARCHITECTURE, ROADMAP, CONTRIBUTING, SECURITY, AGENTS; `docs/` index, code map, governance, runbook, secrets, support, project map; templates for plans, incidents, releases, release checks and rulings; the Phase 1 plan.
- `scripts/check.sh` / `check.ps1` (all checks), `scripts/gen_tasks.py` (`TASKS.md`), `scripts/gen_decisions.py` (`docs/DECISIONS.md`).
- Issue and PR templates; CI on Linux, Windows and macOS plus an MSRV job.
- Localization planned: English first, Russian next (ROADMAP 1.7).

### Ported from the v0 proxy

- `val`, `etf`, REST routes → `phoecord-core`.
- `codec`, `ca`, `trust` → `phoecord-proxy`. The CA is renamed "PhoeCord local CA".
- The `no_track` plugin → `phoecord-plugins`, on the new Plugin API.

### Changed

- All documentation is English-only.
- `docs/PORTING_PLUGINS.md`: step-by-step guide to porting any plugin (layer choice, Vencord → PhoeCord concept map, native and WASM, tests, worked example).
- `docs/PLUGINS.md`: every Vencord plugin and the owner's userplugins mapped to a layer and a task; Phase 1 tasks 1.2.10–1.2.19 for Vencord parity on the network.
- Decided: WASM plugin runtime, egui installer GUI, no code signing (like Vencord).

### Fixed

- `trust.rs`: an unused `PathBuf` import broke `-D warnings` builds on Windows and macOS.
