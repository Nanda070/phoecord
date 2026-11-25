# Changelog

All notable changes to the project ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/), [SemVer](https://semver.org/)). Every PR adds its line under "Unreleased"; a release moves those lines under its version and date.

## [Unreleased]

### Added

- 1.3.6: the plugin SDK. `phoecord-plugin-sdk` re-exports the guest bindings and adds a `Plugin` trait, the `#[phoecord::plugin]` attribute and `Tree` / `Outcome` helpers; `examples/hello-plugin` logs every `MESSAGE_CREATE` and has its own CI job.
- 1.5.5: the loader reports to the proxy. `phoecord run` / `launch` open a per-user Windows named pipe (`\\.\pipe\phoecord-<SID>`, only your Windows user may connect, no remote clients) and log one line per loader that connects: `loader connected: build …, hash …, UI layer off (no signatures)`. The loader (`version.dll`) sends its version, the client build and `.text` hash and whether the UI layer is on, retries five times in the background and gives up quietly when no proxy runs (one line in `loader.log`). The protocol (JSON lines, 64 KiB cap) also carries settings for the loader (the `[loader]` table of `config.toml`) and UI events, for Phase 2. No effect on macOS and Linux.
- Plugin timers: `Host::schedule(after_ms, tag)` and `Host::cancel_schedule(tag)` (also WASM imports `schedule` / `cancel-schedule`); a due timer is a `PHOECORD_TIMER` dispatch to the scheduling plugin only. Additive, Plugin API stays v1. Needed by the 1.2.19 userplugins with delayed loops.
- `Host::fetch_with(method, url, headers, body)` (WASM `fetch-with`, same `fetch` right): `fetch` with the plugin's own headers, credential and hop-by-hop names refused; `fetch` host entries may be `*.example.com` (exactly one extra label). Additive, Plugin API stays v1.
- 1.2.19: network parts of the owner's userplugins FakeDeafen (`/phoecord fakedeafen`), FollowUser (`/phoecord follow`, `unfollow`), FakeDeafenDetector (`/phoecord flagged`; detection needs a voice `SPEAKING` feed the proxy does not see yet), AntiRickroll (`/phoecord rickrolls`), localDevBadges (badges on your own profile, local only), VoiceChatUtilities (`/phoecord vcu`), rmcord (`/phoecord rmcord`), SilentDelete (`/phoecord silentdelete`, `silentpurge`) and QuestMaster (`/phoecord quests`; game and stream paths emulated on the wire, ACHIEVEMENT bypass through `*.discordsays.com`), ported 1:1 from the owner's scripts, all off by default.
- 1.7.4: `Host::t(key, args)`, a plugin's own strings in the user's language (additive, Plugin API stays v1; a host without bundles returns the key). Lookup: the plugin's bundle in the language of `--lang`, then config `language`, then the OS, then its English bundle, then the key. WASM packages ship `i18n/<lang>.ftl` next to `plugin.toml` (loaded with the package and on hot reload; WIT `host.t`, no right needed); built-in plugins ship `crates/phoecord-plugins/i18n/{en,ru}/<plugin id>.ftl`. The notifications of RelationshipNotifier, BetterSessions and VcNarrator and the `/phoecord` answers (`plugins`, `toggle`, `sessions`, "plugin is off") now appear in Russian with `language = "ru"`. `check_i18n.py` also checks that every plugin bundle has its `ru` twin and is registered. The installer window keeps a package's `i18n/<lang>.ftl` when it installs from a file, a drop or a link (at most 16 files of 256 KB each); the runtime skips a `.ftl` over 256 KB. The other plugins' `/phoecord` answers stay English for now.
- `unindent` plugin: code blocks you send or edit lose the indentation shared by all their lines (stage 1.2.2).
- 1.2.12: command plugins, all off by default: CustomCommands (`/phoecord tags`, `/phoecord tag`: text snippets with `{{arg=default}}` arguments, sent or shown only to you), FriendInvites (`/phoecord create-friend-invite`, `view-friend-invites`, `revoke-friend-invites`), petpet (`/phoecord petpet`: a headpat GIF of an avatar or a Discord-hosted picture, uploaded to the channel), SpotifyShareCommands (`/phoecord track`, `album`, `artist`: the link of what you are listening to), and the data parts of ServerInfo (`/phoecord serverinfo`), ViewRaw (`/phoecord viewraw`: raw JSON of a message, channel, server, role or user) and UnsuppressEmbeds (`/phoecord unsuppress`). For plugin authors: `commands::invocation` reads a `/phoecord` call with its channel, and `commands::kept_answer` shows the answer a plugin computed as itself; an empty answer shows nothing. The plugins crate now depends on `image` (PNG, JPEG, GIF).
- 1.2.13: presence plugins, all off by default, working on the presence the client sends (identify and op 3): AutoDNDWhilePlaying (shows `statusToSet`, default Do Not Disturb, while a "Playing" activity is on; your status setting itself is not changed), IgnoreActivities (Vencord's filter: `listMode`, `idsList`, the `ignore*` switches, `ignoredActivities`; `/phoecord activities`, `/phoecord ignoreactivity`), CustomIdle (`idleTimeout` 0 = never idle or above 10 minutes, `remainInIdle` with `/phoecord exitidle`), CustomRPC (Vencord's settings, images by asset name or link; `/phoecord rpc`) and MusicRichPresence (Last.fm or ListenBrainz now playing every 16 s). For plugin authors: `presence.rs` finds the presence in identify and op 3, and `Owned` adds one activity of a plugin's own without stacking copies.

### Changed

- macOS: the minimum is now macOS 13 (Ventura), because the client is started with `open --env`, which macOS 13 added. Macs on macOS 11 and 12 cannot install 0.5.1.
- `Host::emit_send` from a REST event (for example a `/phoecord` command) or a timer goes to the live gateway connection at once instead of being lost; every plugin-queued send passes the Send chain once.
- The per-plugin `Host::rest` limit allows bursts: 5 calls per 5 s (was 1 per second); a refusal says `retry in X s`. The 5/s overall cap and Discord's buckets are unchanged.
- 1.5.8: the loader (`version.dll`) is cheap: Electron child processes (`--type=` on the command line) get export forwarding only, no worker, and the main process identifies the client build from the exe already mapped in memory instead of reading the whole file (about 140 ms and 224 MB per process before); the log line `client build: …` now ends with the detection time in ms. The `.text` hash is unchanged, so existing `signatures/*.toml` keep matching.

### Fixed

- Twelve data parts from 1.2.16 (MemberCount, MessageLinkEmbeds, ReadAllNotificationsButton, PauseInvitesForever, PlatformIndicators, UserVoiceShow, TypingIndicator, VcNarrator, NewGuildSettings, ValidReply, ValidUser, OverrideForumDefaults) read gateway dispatches as bare data while the proxy hands plugins the whole `{op, t, d}` frame, so they saw nothing in a real session; they now read `d` through `state::dispatch_data`, with a real-frame test each.
- macOS: Discord started by PhoeCord could not read its own data ("Operation not permitted", `installer.db` could not be opened, the updater showed "Installation paused"), because as a child of PhoeCord.app it was treated as PhoeCord.app reaching into another app's data. A `.app` client is now started through LaunchServices (`open -n -a <Discord>.app --env ... --args --proxy-server=...`), so it is its own process. Windows and Linux are unchanged.
- macOS: the downloaded `PhoeCord.app` said "damaged and can't be opened". The app is now ad-hoc signed after packaging and the `.dmg` is refreshed with the signed copy (`scripts/package/sign-macos.sh`); the installers job checks the signature with `codesign --verify`. macOS now shows the ordinary "unidentified developer" prompt with Open Anyway.
- Release notes start with "Which file do I download?" and the macOS note has the `xattr -dr com.apple.quarantine` fallback.
- `phoecord` with no command no longer silently starts the proxy: it prints what PhoeCord is and the main commands (English and Russian) and exits. Autostart (launchd, systemd, Run key), shortcuts and the installer window already pass `run` or `launch`, so entries written by 0.5.0 keep working without a rewrite.

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
