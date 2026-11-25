# Changelog

All notable changes to the project ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/), [SemVer](https://semver.org/)). Every PR adds its line under "Unreleased"; a release moves those lines under its version and date.

## [Unreleased]

## [0.5.0] - 2026-11-25

### Added

- 1.2.16: data parts of 17 plugins with an on-screen part, all off by default: ValidUser, ValidReply, MessageLinkEmbeds, NewGuildSettings, OverrideForumDefaults (traffic fixes and settings), PauseInvitesForever, ReadAllNotificationsButton (commands), PlatformIndicators, TypingIndicator, UserVoiceShow, VcNarrator, MemberCount, BetterSessions, ImplicitRelationships, MutualGroupDMs, WhoReacted, MessageLatency (collected state shown by `/phoecord` commands or notifications). Shared trackers for them in `phoecord-plugins/src/state.rs`, `FnCmd` for closure commands. Their on-screen parts come in Phase 2.
- 1.2.18: data parts of ShowHiddenChannels (`/phoecord hidden`), ShowHiddenThings (`/phoecord hiddenthings`) and Experiments (`/phoecord experiments`), off by default. The READY rewrites (hidden channels in the list, the STAFF flag) sit behind their own extra settings, also off, because they are the most ban-prone changes in the project.
- 1.4.5: releases. A `v*` tag (pushed by the owner) runs `.github/workflows/release.yml`: the `phoecord` CLI for Linux x86_64, Windows x86_64 (with the loader as `version.dll`) and macOS arm64 and x86_64, each archived with `LICENSE`, `NOTICE`, `LICENSES/` and the terms, plus the native installers from `package.yml`, one `SHA256SUMS` over every file, and a GitHub release whose notes are the version's CHANGELOG section and a note on unsigned builds (SmartScreen, Gatekeeper, how to verify the checksums). A tag with `-` makes a prerelease; the run stops when the tag does not match the version in `Cargo.toml`; a manual run is a dry run. `package.yml` no longer runs on tags itself: `release.yml` calls it. Release rules in `GOVERNANCE.md`, failures and antivirus warnings in `RUNBOOK.md`.
- 1.5.6: `phoecord setup --loader` (Windows) installs the native loader as `version.dll` into the newest `app-x.y.z` folder of the chosen Discord; every `phoecord launch` and autostart (`phoecord run`) puts it back after a Discord update moves the client to a new folder. A `version.dll` from another program is never overwritten (ownership by SHA-256 recorded in `install.log`), and `phoecord uninstall` removes every copy PhoeCord made, old app folders included, and nothing else. On macOS and Linux the option reports that it is Windows-only. English and Russian.
- 1.4.3: the installer window `phoecord-installer` (English and Russian): terms of use first, Discord clients with a live status and one main action (install, add, open), repair and uninstall with step-by-step progress, and a Plugins tab that installs WASM packages from a file, a dropped file or an HTTPS link after showing their events and rights in plain words, switches them and removes them. Garnet theme, animated tabs, dialogs, switches and progress, with a reduce-motion setting; `--dry-run` keeps every change in a sandbox folder. A Discord installed somewhere else is added with "Choose folder…" (kept in `clients` in `config.toml`, also used by `launch` and `setup`); when no Discord is found the window says where it looked. Built-in plugins are not listed in the window: they will be managed inside Discord. It ships as a native installer: a setup `.exe` and an `.msi` on Windows, `PhoeCord.app` in a `.dmg` on macOS (unsigned), with its own icon, an opening animation, a short first-start onboarding (what PhoeCord does, the terms, which Discord, done) and the repository link in About. `LICENSE`, `NOTICE` and `LICENSES/` ship as files in every installer (next to the executables on Windows, in `Contents/Resources` of the `.app`).
- 1.1.11: `upstream = "socks5://host:port"` or `"http://host:port"` in `config.toml` sends every outgoing connection of the proxy (intercepted REST and Gateway, and tunnelled) through a SOCKS5 or HTTP-CONNECT proxy, for VPN and zapret users; a dead or refusing upstream gives a clear error; `phoecord selftest` honours it.
- 1.2.9: `phoecord import-vencord [--path FILE]` finds the settings of Vencord, Vesktop or Equicord and switches on the PhoeCord plugins whose `Manifest::vencord` matches an enabled Vencord plugin (English and Russian).
- Stage 1.1 core refactor: the proxy, REST and Gateway run through Plugin API v1 and `Chain`; `ProxyHost` provides scoped settings, KV, account state and notifications. The `phoecord` CLI now has `run`, `launch`, `setup`, `trust`, `untrust`, `plugins`, `enable`, `disable`, `toggle`, `paths` and `selftest`.
- Cargo workspace of 8 crates: `core`, `proxy`, `plugin-runtime`, `plugin-sdk`, `plugins`, `loader`, `installer`, `cli`.
- Plugin API v1 (`phoecord-core::plugin`): `Manifest`, `Event`, `EventKind`, `Verdict`, `Host`.
- Plugin chain `phoecord-plugin-runtime::Chain`: the first `Drop`/`Reply` stops the chain.
- AOB signature scanner in `phoecord-loader` (`??` masks, ambiguous matches rejected).
- Docs: README, ARCHITECTURE, ROADMAP, CONTRIBUTING, SECURITY, AGENTS; `docs/` index, code map, governance, runbook, secrets, support, project map; templates for plans, incidents, releases, release checks and rulings; the Phase 1 plan.
- `scripts/check.sh` / `check.ps1` (all checks), `scripts/gen_tasks.py` (`TASKS.md`), `scripts/gen_decisions.py` (`docs/DECISIONS.md`).
- Issue and PR templates; CI on Linux, Windows and macOS plus an MSRV job.
- Localization planned: English first, Russian next (ROADMAP 1.7).

- `phoecord-i18n`: Fluent localizer with English and Russian bundles, fallback ru → en → key, OS language detection (stage 1.7.1).
- `scripts/check_i18n.py`: fails when a Russian translation is missing a key or a variable; part of `check.sh` and CI (stage 1.7.5).

- `phoecord-loader` builds as a proxy `version.dll`: lazy export forwarding to the system DLL, a `DllMain` that only starts a worker, client build detection (file version + `.text` SHA-256), `signatures/*.toml` with unique AOB matches and RIP resolution, inline detours with removal on unload (stages 1.5.1-1.5.4).
- `silent_typing` plugin: your typing indicator is never sent (stage 1.2.1).
- `clear_urls` plugin: strips tracking parameters (`utm_*`, `fbclid`, `gclid`, `si` on YouTube and Spotify, …) from links in messages you send or edit; extra rules in `[plugins.clear_urls] extra_rules` (stage 1.2.7).

- `docs/research/gpui.md`: GPUI map, a release + LTO reference build (`research/gpui-reference`), anchors that survive in a stripped binary, and ranked hook candidates (palette, settings tabs, message row) for Phase 2 (stage 1.6).

- Plugins `game_activity_toggle`, `relationship_notifier` and `fake_nitro` (emoji part), all off by default (stages 1.2.3-1.2.5).
- `message_logger` plugin and `phoecord log`: deleted and edited messages stay visible and are written to `messages.jsonl` (stage 1.2.6).
- `/phoecord plugins` and `/phoecord toggle <id>` slash commands, answered locally (stage 1.2.8).
- Plugin API (minor, `API_VERSION` stays 1): `RestResponse.body` is mutable; new `Host` methods `emit_dispatch`, `plugin_enabled`, `set_plugin_enabled`, `append_data`.
- Installer library: client discovery on 3 OSes, CA + autostart install with an install log, full rollback (stages 1.4.1, 1.4.2, 1.4.4; not wired to the CLI yet).
- WASM plugin runtime: `wit/phoecord.wit`, wasmtime 38 backend behind the same `Chain`, 5 ms / 32 MiB limits, plugin packages (`plugin.toml` + `plugin.wasm`, `events` and `rights`) with reload (stages 1.3.1-1.3.5).
- WASM plugins: a slow event costs a strike (3 in a minute disable the plugin) instead of disabling it at 5 ms; new rights `emit_dispatch`, `plugins`, `data`; plugin packages copied into or removed from `plugins/` load and unload without a restart, and `phoecord run` now loads them (stage 1.3.4).
- Issues are the place for every problem; leaks of personal data or secrets go privately to `nandak070` on Discord (`SECURITY.md`). The README shows contributors automatically.
- `phoecord setup` now runs the installer library (CA, trust and autostart, logged in `install.log`), and `phoecord uninstall` undoes it (stages 1.4.2, 1.4.4).
- Language setting: `phoecord --lang ru`, `language` in `config.toml`, or the OS language; `setup`, `uninstall` and `paths` already speak Russian, the rest of the CLI follows in 1.7.3 (stage 1.7.2).
- The whole CLI speaks Russian, `--help` included (stage 1.7.3).
- Terms of use (`TERMS.md`, `TERMS.ru.md`, version 1): `phoecord setup` shows them and installs nothing until you type `yes` (`да`); a new version asks again (stage 1.4.7).
- Plugins `i_love_spam`, `no_blocked_messages` and `no_reply_mention`, off by default (stage 1.2.10); ImageLink and NoPendingCount are UI-only and move to Phase 2.
- Plugins `text_replace`, `send_timestamps`, `anonymise_file_names` and `silent_message_toggle` with `/phoecord silent`, off by default (stage 1.2.11); BetterGifAltText is UI-only and moves to Phase 2.
- The first third-party plugin example: `clear_urls` as a WASM package in `examples/plugins/clear_urls/`, loaded without a restart (stage 1.3.8).
- `phoecord launch --channel stable|ptb|canary` (no silent default to Stable); `launch` reuses a running proxy; `setup --channel` adds a Start-menu shortcut that starts that Discord through PhoeCord; `uninstall` stops a running proxy and removes PhoeCord's own CA files (stage 1.4.8).
- Plugins can call Discord's API as your own client session (`Host::rest`, rate-limited) and outside HTTPS hosts they list (`Host::fetch`, `fetch = [...]` in `plugin.toml`); the session header stays in memory only (stage 1.1.13).
- Plugin API (minor, `API_VERSION` stays 1): `HostResponse.bytes` (raw non-JSON answers, e.g. images) and `Host::rest_files` (upload files to Discord as the client does); both reach WASM plugins through `wit/phoecord.wit` (right `rest`). Unblocks petpet (1.2.12) and USRBG / Decor (1.2.14).
- `setup` trusts a recreated local CA (it compares fingerprints with the trust store), and `run`/`launch` warn when the CA is not trusted (stage 1.4.9).

### Removed

- Docs: `PROJECT_MAP.md`, `SUPPORT.md`, `SECRETS.md` (merged into `GOVERNANCE.md`), `TASKS.md` and `scripts/gen_tasks.py`, `incidents/`, `reports/`, release, incident and plan templates. Task status now lives only in `docs/phase-1/README.md`.

### Ported from the v0 proxy

- `val`, `etf`, REST routes → `phoecord-core`.
- `codec`, `ca`, `trust` → `phoecord-proxy`. The CA is renamed "PhoeCord local CA".
- The `no_track` plugin → `phoecord-plugins`, on the new Plugin API.

### Changed

- License: MIT → **GPL-3.0-or-later**, like Vencord. `NOTICE` holds the copyright and exceptions; the plugin SDK, `wit/` and templates stay MIT; the v0 code's MIT notice moved to `LICENSES/MIT-v0.txt`. Vencord code may now be adapted with attribution.

- All documentation is English-only.
- `docs/PORTING_PLUGINS.md`: step-by-step guide to porting any plugin (layer choice, Vencord → PhoeCord concept map, native and WASM, tests, worked example).
- `docs/PLUGINS.md`: every Vencord plugin and the owner's userplugins mapped to a layer and a task; Phase 1 tasks 1.2.10–1.2.19 for Vencord parity on the network.
- Decided: WASM plugin runtime, egui installer GUI, no code signing (like Vencord).

### Fixed

- 1.4.3, owner's Windows test of the installer window (PR #66): "Open Discord" starts `phoecord launch` with no console window, keeps its output in `launch.log` in the data folder and watches its first seconds, so the window shows what `phoecord` said when it stops instead of reporting a start; `launch` no longer exits when it has no console to wait on for Ctrl+C; `certutil`, `reg`, `powershell`, `taskkill` and Discord's `Update.exe` start without a console window; `phoecord-installer.exe` and `phoecord.exe` carry the app icon (taskbar, Explorer, Settings > Apps); the Russian setup is fully Russian; the installers are named `PhoeCord_<version>_…`; the `Package` workflow computes `SHA256SUMS` on Windows; Uninstall says that the app itself stays until it is removed in Settings > Apps (Windows) or moved to the Trash (macOS).
- 1.4.3, owner's second look at the installer window (PR #66): a Discord reached through a junction or symlink (say `%LOCALAPPDATA%\DiscordCanary` on another drive, also picked with "Choose folder…") is listed once, and "Choose folder…" on a listed folder says so; the onboarding shows on the first start of each installed version, even when the terms were accepted before; the opening animation is new (about 1.4 s): the flame catches with a flash, shock rings and embers, the name comes in letter by letter, and the window opens out of the flame as an iris; it no longer runs out during the first slow frames.
- `trust.rs`: an unused `PathBuf` import broke `-D warnings` builds on Windows and macOS.
