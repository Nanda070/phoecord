# Code map: which file does what

Checked against code: `a965f75`, 23.11.2026. Whoever re-checks this document (or a section of it) against the code updates this line (rule in `AGENTS.md`).

A reference to the repository: which file holds what, why it exists and what is easy to break in it. Also every config key, environment variable and on-disk file the app uses. This is a living document and describes the `main` branch.

> **Mandatory for people and AI agents.** Added, renamed, moved or removed a crate, module, file, config key or environment variable: update this file in the same commit. A change after which this map is wrong is not done. The rule is in [`AGENTS.md`](AGENTS.md).

Related: [`ARCHITECTURE.md`](ARCHITECTURE.md) explains how the layers work; [`PLUGINS.md`](PLUGINS.md) lists every plugin and its status.

**How to read.** `phoecord-core` is pure data and contracts, no I/O. `phoecord-proxy` talks to the network. `phoecord-plugins` holds plugin logic only. `phoecord-loader` is the only crate allowed to use `unsafe`.

---

## 1. Repository root

| Path | What it is |
|:---|:---|
| `Cargo.toml` | Workspace: members `crates/*`, default member `phoecord-cli`, shared `[workspace.package]` (edition 2024, MSRV 1.88, GPL-3.0-or-later), shared dependency versions, lints, release profile |
| `Cargo.lock` | Locked versions; committed because the workspace ships binaries |
| `README.md` | Product overview |
| `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md` | Process, security policy, conduct |
| `LICENSE`, `NOTICE`, `LICENSES/` | GPL-3.0 text; copyright and exceptions; MIT notices for the v0 code (`MIT-v0.txt`) and the plugin SDK (`MIT-sdk.txt`) |
| `assets/logo.svg` | Logo used in the README |
| `.github/workflows/ci.yml` | CI: rustfmt, generated docs freshness, check/clippy/test on Linux, Windows, macOS, MSRV 1.88 |
| `.github/workflows/package.yml` | Native installers (NSIS `.exe` + WiX `.msi` on Windows, `.app` in a `.dmg` on macOS arm64 and Intel) with `SHA256SUMS`, as workflow artifacts; on PRs touching the installer, by hand, and called by `release.yml` |
| `.github/workflows/release.yml` | Releases on a `v*` tag: checks the tag against `Cargo.toml`, builds and archives the CLI (and `version.dll` on Windows) for 4 targets, reuses `package.yml` for installers, writes `SHA256SUMS`, publishes the GitHub release (prerelease for `-` tags); manual run is a dry run |
| `.github/ISSUE_TEMPLATE/` | Bug, feature and plugin forms; blank issues disabled |
| `.github/PULL_REQUEST_TEMPLATE.md` | PR form with the living-docs checklist |
| `.github/dependabot.yml` | Weekly cargo updates (grouped), monthly GitHub Actions updates; Dependabot opens the `dependabot/*` branches |
| `.editorconfig`, `.gitattributes`, `.gitignore` | LF everywhere, 4-space Rust, 2-space YAML/TOML; secrets and dumps ignored |
| `signatures/` | AOB signature tables per client build (`README.md` documents the format; no Discord signatures yet) |
| `research/` | Out-of-workspace research projects: `gpui-reference` (GPUI app built like a shipping client) and `aob-probe` (checks AOB anchors against a binary); excluded from the workspace |
| `.github/CODEOWNERS`, `.github/workflows/assign.yml` | Every PR is assigned to Nanda070 and requests Nanda070's review |
| `scripts/` | §4 |
| `docs/` | §5 |

## 2. Crates

| Crate | Path | Kind | Depends on (ours) |
|:---|:---|:---|:---|
| `phoecord-core` | `crates/phoecord-core/` | lib | none |
| `phoecord-transport` | `crates/phoecord-transport/` | lib | core, plugin-runtime, i18n |
| `phoecord-proxy` | `crates/phoecord-proxy/` | lib | core, transport, plugin-runtime, loader (IPC types and pipe only, without the `dll` feature) |
| `phoecord-plugin-runtime` | `crates/phoecord-plugin-runtime/` | lib | core |
| `phoecord-plugin-sdk` | `crates/phoecord-plugin-sdk/` | lib (guest side) | none, by design |
| `phoecord-plugins` | `crates/phoecord-plugins/` | lib | core |
| `phoecord-loader` | `crates/phoecord-loader/` | cdylib + rlib | none |
| `phoecord-installer` | `crates/phoecord-installer/` | lib | core |
| `phoecord-cli` | `crates/phoecord-cli/` | bin `phoecord` | core, proxy, transport, plugin-runtime, plugins |
| `phoecord-i18n` | `crates/phoecord-i18n/` | lib | none |
| `phoecord-installer-gui` | `crates/phoecord-installer-gui/` | bin `phoecord-installer` | core, i18n, installer, plugin-runtime, plugins, proxy |

## 3. Files by crate

### `phoecord-core`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | Re-exports `Val` and the Plugin API types | Anything re-exported here is public API for plugins |
| `src/val.rs` | `Val`: one value type for JSON and ETF; keeps atoms, big ints, binaries, so ETF round-trips unchanged | Changing a variant changes how every Gateway frame re-encodes |
| `src/etf.rs` | Erlang External Term Format decode/encode (the desktop client's Gateway encoding) | Compressed terms (tag 80) need `flate2` |
| `src/route.rs` | `route_of` (`/api/v9/x` → `/x`), `message_route_channel` | Plugins match on the normalized route |
| `src/plugin.rs` | Plugin API v1: `API_VERSION`, `Layer`, `Manifest`, `Event`, `EventKind`, `Verdict`, `Host`, `Plugin` | **The contract.** Changes need an `api` PR and the owner's review |

### `phoecord-transport`

The transport boundary (task 1.1.12): everything between "an event arrived" and "here is what to do with it", with no sockets, TLS or HTTP. A transport (the proxy today, the loader's in-process hooks later) calls `Engine` and supplies a `Network`.

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | `Engine`: owns `Chain`, `Shared` and the live `Config`; `on_request`, `on_response`, `on_dispatch`, `on_send`, `chain_queued`, `fire_timer`, `apply`, `sync_packages`; result types `RequestOutcome`, `DispatchResult`, `SendResult`, `Effects` | No network or async types here: the loader must be able to link it. Keep the order of effects (locals, then frames) |
| `src/host.rs` | `ProxyHost` and `Shared` (KV, rate limits, notifications, account, timers, strings), the `Network` trait a transport implements for `Host::rest` / `Host::fetch`, `NoNetwork` | Host state must be bound to the plugin currently running in `Chain`; the `upstream` string is passed through unparsed |
| `src/config.rs`, `src/account.rs` | Live plugin configuration and account state available to plugins | Config reload must re-enable the chain and preserve the last live state |
| `src/timers.rs` | `Timers`: the store of plugin timers from `Host::schedule` (50 ms min, 24 h max, 64 per plugin, memory only), `timer_payload` | The clock is the transport's: it calls `next_due` / `take_due` and `Engine::fire_timer` |

### `phoecord-proxy`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | `Proxy`: TLS setup, reload task, the live-gateway registry and the `locals` broadcast around an `Engine` (`Proxy` derefs to it); delivers `Effects` to the connections; re-exports `config`, `account`, `host` from `phoecord-transport` | The proxy calls the chain only through `Engine` |
| `benches/chain.rs` | Criterion bench: decode → `Proxy::on_dispatch` with every built-in plugin → encode, for a ~5 MB READY, a MESSAGE_CREATE burst and a REST body; prints p50/p99. Report: `docs/chain-bench.md` | Generated payloads only, never real traffic; `-- --test` is the smoke run |
| `GET /__phoecord` (in `src/server.rs`) | Identity answer (`phoecord <version>`) for loopback peers, so `launch` can reuse a running proxy | |
| `src/server.rs`, `src/rest.rs`, `src/gateway.rs` | Proxy loop, REST and Gateway traffic routed through `Chain` | Keep REST decisions and Gateway encode/compress round-trips lossless |
| `src/codec.rs` | `Codec` from the Gateway query (`encoding`, `compress`), `Inflater` / `Deflater` for zlib-stream and zstd-stream | Compression context is per connection: once a frame changes, everything after must use our context |
| `src/ca.rs` | `Authority`: local CA load-or-create, name constraint (`CONSTRAINED_TO`), leaf certificates per SNI, rustls `ServerConfig` | `CA_NAME` is what `untrust` looks for; tests check the constraint blocks other domains |
| `src/trust.rs` | Install/remove the CA: Windows `certutil -user`, macOS keychain, Linux NSS (incl. Flatpak, Snap) | Platform code behind `cfg`; build on all 3 OSes before merging |
| `src/process.rs` | `command` / `hide_console`: child processes that open no console window on Windows (`CREATE_NO_WINDOW`) | Every child of `phoecord` and the window goes through it: started without a console, a console program would flash its own window |

| `src/hostnet.rs` | `Net` (the proxy's `Network` for the engine): the client's session headers kept in memory only (30 min TTL, dropped on 401), `Host::rest` to the client's own Discord API host with route checks, per-plugin 1/s + global 5/s caps and Discord's rate-limit buckets; `Host::rest_files` (multipart upload, 10 files, 25 MB); `Host::fetch` to allow-listed HTTPS hosts (5 MB cap, no redirects, no credentials, raw `bytes` for non-JSON answers); a small dedicated runtime so plugin calls never block the proxy | Never log or persist the session headers; `Debug` is redacted |
| `src/timers.rs` | The `drive` task: sleeps until the earliest plugin timer and fires it through `Proxy::fire_timer` (store: `phoecord-transport::timers`) | Holds only a `Weak<Proxy>` |
| `src/ipc.rs` | The proxy's end of the loader IPC (1.5.5): `spawn` (pipe server thread on Windows, no-op elsewhere), `serve_connection` (any `BufRead`/`Write`), `ConfigSettings` (`[loader]` in `config.toml`) | Settings are only the `[loader]` table, scalar values; garbage drops the connection; at most 8 connections |
| `src/live.rs` | `LiveGateways`: registry of bridged gateway connections; sends queued outside a frame go to the newest one at once | A `LiveGuard` unregisters on drop |
| `tests/hostnet.rs` | Loopback fake Discord and outside servers: capture/reuse, refusals, rate limits, allow-list, caps, a `Chain` test | |

### `phoecord-plugin-runtime`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | `Chain` (built-in segment, then a dynamic segment swapped by `replace_dynamic`; first `Drop`/`Reply` stops), `Outcome`, `NullHost` for tests | The stop rule is part of the API semantics; the lock is never held while a plugin runs |
| `src/wasm.rs` | `WasmRuntime` (engine + epoch clock), `WasmLimits`, `WasmPlugin`, `Right`, `Setup`, `tree` ↔ `Val` | Guest trees are untrusted: keep the bounds, depth and single-use checks |
| `src/package.rs` | `plugin.toml` format and rights table (documented at the top), `PackagePlugin` with reload, `load_dir`, `PackageSet::sync` (load new folders, unload removed ones); `i18n/<lang>.ftl` of a package go to `PluginStrings` on load and reload (`PackageSet::with_strings`) | A broken new version must leave the old one running; the `.ftl` files are part of the reload stamp |
| `tests/guest/` | A separate workspace: the test guest, built for `wasm32-wasip2` by the tests (CI installs the target) | Slow-event tests use a zero soft limit so they do not depend on machine speed |
| `tests/strings.rs` | `Host::t` for packages: a string from the package's own `.ftl`, English fallback, hot reload of an edited `.ftl`, unload | Uses the guest's `STRINGS` dispatch |
| `tests/clear_urls_wasm.rs` | Builds `examples/plugins/clear_urls`, loads it as a package (also into a running chain) and checks it behaves like the native plugin | Needs the `wasm32-wasip2` target |
| `benches/overhead.rs` | Per-event cost of a WASM plugin vs a native one on a MESSAGE_CREATE payload (`cargo bench`) | Input for benchmark task 1.1.10 |
| `wit/phoecord.wit` (repo root) | The `plugin` world and `host` interface; mirrors `phoecord-core::plugin` | Change together with `plugin.rs` |

### `phoecord-plugin-sdk`

| File | What |
|:---|:---|
| `src/lib.rs` | `API_VERSION` only; bindings and the `#[phoecord::plugin]` macro are task 1.3.6. MIT-licensed (`license = "MIT"`), unlike the rest of the workspace |

### `phoecord-plugins`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | `builtin()`: every built-in plugin in default order; uniqueness test for ids; `I18N` (the plugins' `.ftl` bundles) and `register_strings` | Order is run order; a new `i18n/<lang>/<id>.ftl` must be listed in `I18N` (`check_i18n.py` checks) |
| `i18n/{en,ru}/<plugin id>.ftl` | The plugin's own strings behind `Host::t`: `commands` (the `/phoecord` answers), `relationship_notifier`, `better_sessions`, `vc_narrator` | Every `en` key needs a `ru` twin with the same variables; keys of other plugins' subcommand answers go to `commands.ftl` |
| `src/no_track.rs` | NoTrack: answers analytics and crash-report routes locally with 204 | `wants` must stay as narrow as `on_event` |
| `src/clear_urls.rs` | ClearURLs: strips tracking query parameters from links in sent and edited messages; `extra_rules` setting | Only `content` of JSON bodies; messages with attachments (multipart) are not cleaned yet |
| `src/silent_typing.rs` | SilentTyping: answers `POST /channels/{id}/typing` locally with 204 | Only that exact route; `wants` must stay as narrow as `on_event` |
| `src/unindent.rs` | Unindent: removes the indentation shared by every line of fenced code blocks in sent and edited messages | The opening line (language) and text outside blocks must stay byte for byte |
| `src/i_love_spam.rs` | iLoveSpam: clears the likely-spammer bit (`1<<20`) from user `flags` / `public_flags` in messages, history and member events | Keep every other bit |
| `src/no_blocked_messages.rs` | NoBlockedMessages: learns blocked (type 2) and ignored users, drops their live messages; settings `apply_to_ignored_users` (true), `filter_history` (false) | Filtering history makes pages shorter than `limit`, which can stop the client loading older messages; that is why it is off by default |
| `src/no_reply_mention.rs` | NoReplyMention: sets `allowed_mentions.replied_user = false` on replies per Vencord's rules; settings `user_list`, `role_list`, `should_ping_listed`; remembers authors of 5000 messages | An unknown replied-to message counts as "not listed" |
| `src/text_replace.rs` | TextReplace: string and regex rules on outgoing `content` (send and edit), JavaScript replacement syntax emulated; uses `regex` | JS-only regex syntax makes a rule invalid; invalid rules are skipped and logged once |
| `src/send_timestamps.rs` | SendTimestamps: `` `12:34` `` → `<t:unix:t>`; local offset from the `time` crate, `utc_offset_minutes` fallback on Unix | DST: the next occurrence is computed per calendar day |
| `src/anonymise_file_names.rs` | AnonymiseFileNames: renames `files[].filename` in `POST /channels/{id}/attachments` and the matching `attachments[].filename` in the message | Files inside a multipart message body are not renamed (the proxy never rewrites multipart) |
| `src/silent_message_toggle.rs` | SilentMessageToggle: `flags \|= 1<<12` on outgoing messages; `/phoecord silent [on\|off]` | The toggle is in memory and starts off |
| `src/valid_user.rs` | ValidUser: adds unknown mentioned users to `mentions` via `GET /users/{id}`; per-message and per-minute lookup budgets | Off by default |
| `src/valid_reply.rs` | ValidReply: fills a missing `referenced_message` from the history page or `GET /channels/{id}/messages?around=` | Off by default |
| `src/message_link_embeds.rs` | MessageLinkEmbeds: attaches a rich embed per linked message; `/phoecord linkembeds` | Off by default |
| `src/new_guild_settings.rs` | NewGuildSettings: one `PATCH /users/@me/guilds/{id}/settings` for a server joined after READY; `/phoecord newguildsettings` | Writes to Discord: once per server, tracked in kv |
| `src/override_forum_defaults.rs` | OverrideForumDefaults: rewrites `default_forum_layout` / `default_sort_order` in gateway and REST payloads | Off by default |
| `src/pause_invites_forever.rs` | PauseInvitesForever: `/phoecord pauseinvites` adds/removes `INVITES_DISABLED` in guild `features` | Writes to Discord only on the command |
| `src/read_all_notifications.rs` | ReadAllNotificationsButton: tracks read states; `/phoecord readall` acks up to 100 channels in one `ack-bulk` | Only acks channels that already have a read state |
| `src/platform_indicators.rs` | PlatformIndicators: `client_status` per user from presences; `/phoecord platforms` | Read-only: always returns `Pass` |
| `src/typing_indicator.rs` | TypingIndicator: TYPING_START per channel with 10 s expiry; `/phoecord typing` | Read-only: always returns `Pass` |
| `src/user_voice_show.rs` | UserVoiceShow: voice states by server and channel; `/phoecord voice` | Read-only: always returns `Pass` |
| `src/vc_narrator.rs` | VcNarrator: `notify` on joins, leaves, moves, mute/deafen in your voice channel; Vencord message templates | Read-only: always returns `Pass` |
| `src/member_count.rs` | MemberCount: total, online and voice counts per server; `/phoecord members` | Read-only: always returns `Pass` |
| `src/better_sessions.rs` | BetterSessions: `GET /auth/sessions` on READY, command and an interval checked on traffic; known ids in kv; `/phoecord sessions` | Read-only: always returns `Pass` |
| `src/implicit_relationships.rs` | ImplicitRelationships: `GET /users/@me/affinities/users` minus relationships; `/phoecord implicit` | Read-only: always returns `Pass` |
| `src/mutual_group_dms.rs` | MutualGroupDMs: group DMs (type 3) and recipients from READY and CHANNEL_*; `/phoecord mutualgdms` | Read-only: always returns `Pass` |
| `src/who_reacted.rs` | WhoReacted: `/phoecord reactions` lists reactions or up to 100 reactors via REST | Read-only: always returns `Pass` |
| `src/message_latency.rs` | MessageLatency: nonce vs id snowflake delay per MESSAGE_CREATE (Vencord's rules); `/phoecord latency` | Read-only: always returns `Pass` |
| `src/show_hidden_channels.rs` | ShowHiddenChannels: computes VIEW_CHANNEL from roles and overwrites; `/phoecord hidden`; optional READY rewrite | `rewrite_payload` and its READY rewrite stay off by default (ban-prone) |
| `src/show_hidden_things.rs` | ShowHiddenThings: member timeouts, paused invites and DMs, community features; `/phoecord hiddenthings` | Read-only: always returns `Pass` |
| `src/experiments.rs` | Experiments: experiment assignments from READY and `GET /experiments`; `/phoecord experiments`; optional STAFF flag | `enableIsStaff` stays off by default (ban-prone) |
| `src/fake_deafen.rs` | FakeDeafen (owner's userplugin): sets `self_mute`/`self_deaf` on outbound op 4 while `/phoecord fakedeafen` is on; the toggle re-sends your voice state at once | Rewrites outbound op 4 only |
| `src/follow_user.rs` | FollowUser (owner's userplugin): follows `followUserId` or `/phoecord follow` across voice channels with op 4 sends | Sends op 4 at once; permissions unknown to plugins |
| `src/fake_deafen_detector.rs` | FakeDeafenDetector (owner's userplugin): mute/deafen claims from voice states, flags `SPEAKING` while claimed silent; `/phoecord flagged` | No `SPEAKING` feed in the proxy yet: detection is inert |
| `src/anti_rickroll.rs` | AntiRickroll (owner's userplugin, MIT): known hosts/video IDs + custom rules over messages and history pages; `/phoecord rickrolls` | Read-only, no outside calls |
| `src/local_dev_badges.rs` | localDevBadges (owner's userplugin): adds STAFF / Early Verified Bot Developer / Active Developer badges and flags to your own user in READY, USER_UPDATE, `/users/@me` and your profile | Inbound rewrite for self only |
| `src/voice_chat_utilities.rs` | VoiceChatUtilities (owner's userplugin): `/phoecord vcu` PATCHes every member of a voice channel (disconnect, mute, deafen, move), one per timer step with the script's `waitAfter` / `waitSeconds` | Retries a member after a local refusal or a 429 |
| `src/rmcord.rs` | rmcord (owner's userplugin, GPL-3.0): the script's search-and-delete engine as a timer state machine; `/phoecord rmcord` (handled from its own `POST /interactions` so timers and REST run as rmcord) | A local refusal counts as a 429 |
| `src/silent_delete.rs` | SilentDelete (owner's userplugin): placeholder post, `deleteDelay`, delete; `/phoecord silentdelete`, `/phoecord silentpurge` (handled from its own `POST /interactions`) | Any error ends the job, as in the script |
| `src/quest_master/` | QuestMaster (owner's userplugin): `mod.rs` the timer-driven quest engine (request queue, enroll, video progress, heartbeats, claim) and `/phoecord quests`; `data.rs` quest parsing and helpers; `tests.rs` | Game/stream emulated on the wire (presence + desktop heartbeats, unverified live); ACHIEVEMENT via `fetch_with` |
| `src/state.rs` | Shared gateway-state trackers for data parts: `Shared<T>`, `Guilds`, `VoiceStates`/`VoiceChange`, bounded `Users` and `Ring<T>` | Trackers keep ids, names and counts only |
| `src/testutil.rs` | Test helpers shared by the plugin tests: `Cfg`, recording `Rec` with scripted `rest`, `j` | |
| `src/game_activity_toggle.rs` | GameActivityToggle: removes "Playing" activities from op 3 presence updates | The proxy re-sends the last presence on toggle |
| `src/relationship_notifier.rs` | RelationshipNotifier: `notify` on unfriend, server and group removal; your own actions in the last 60 s are ignored | Own-action memory must match the REST routes |
| `src/fake_nitro.rs` | FakeNitro (emoji): rewrites unusable emoji in outgoing messages as CDN links; `premium_type` 2 in `READY` only for tier 0 | Learns the real tier from `READY`; needs Ctrl+R after enabling |
| `src/message_logger.rs` | MessageLogger: LRU cache of messages, deletes turned into marked updates, edit history, `messages.jsonl` via `Host::append_data` | Ids may be JSON strings or ETF ints |
| `src/commands.rs` | `/phoecord plugins` and `toggle`: injects the command into search/index answers, answers `POST /interactions` locally; `Subcommand` trait | Command shapes not yet checked on a live client |

### `phoecord-loader`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | Module list and design notes | |
| `src/aob.rs` | `Pattern`: parse `48 8B ?? …`, `find_all`, `find_unique` (ambiguous = miss) | All-wildcard patterns are rejected on purpose |
| `build.rs`, `exports.txt` | Generates one assembly forwarding stub per `version.dll` export and `/EXPORT` link args (MSVC x86_64) | A Windows test checks `exports.txt` against the real DLL |
| `src/proxy_dll.rs` | `FORWARD_TABLE`, `resolve_export`: loads the real DLL from `GetSystemDirectoryW` on first call | Never load by name: it would resolve to this DLL |
| `src/entry.rs` | `DllMain` (only `DisableThreadLibraryCalls` + `CreateThread`, and no worker at all when the command line has `--type=`), the worker: log, detect the build from the mapped image (`GetModuleHandleW(null)`, logs the time in ms), load signatures | No work under the loader lock; panics are caught; child processes must still forward every export |
| `src/log.rs` | Loader log in `%LOCALAPPDATA%\PhoeCord\loader.log` | |
| `src/ipc.rs` | Loader ↔ proxy protocol (1.5.5): `Msg` / `Envelope` (`hello`, `status`, `settings.get/set/value`, `ui.event`), `encode_line` / `decode_line` / `read_line` with the 64 KiB cap, `pipe_name`, `CONNECT_DELAYS_MS`, `describe_connection` (the proxy's log line) | No OS code, used by the proxy too: a change is a protocol change, bump `PROTOCOL_VERSION` |
| `src/pipe.rs` | Windows named pipe: `PipeServer` (user-only DACL from the user's SID, `PIPE_REJECT_REMOTE_CLIENTS`, first instance), `connect`, `send_all`, `current_user_sid` | All the IPC `unsafe` lives here, each block with `// SAFETY:`; no work under the loader lock (only the worker calls it) |
| `src/pe.rs`, `src/version.rs` | PE parsing (file or mapped image: `Pe::parse_mapped`, `Pe::image_size`), file version resource, `.text` SHA-256 (`detect` for a file, `detect_mapped` for memory, same hash), `is_child_process` | Pure, tested on every OS; the hash must stay equal for file and image, `signatures/*.toml` depend on it |
| `src/signatures.rs` | `SignatureSet::load_dir`, `select`, `resolve_all` (`find_unique` + `rip`) | Format documented in `signatures/README.md` |
| `src/hook.rs` | Inline detours (`retour`), installed with other threads suspended, `uninstall_all` | Hooks must be removed before `FreeLibrary` |
| `examples/host.rs` | Throwaway exe that loads `version.dll` from its folder for manual checks | |

### `phoecord-installer`

| File | What |
|:---|:---|
| `src/lib.rs` | `Channel` (Stable, PTB, Canary, Phoenix), module list |
| `src/discover.rs` | `discover()`: Stable/PTB/Canary on Windows, macOS, Linux (PATH, Flatpak, Snap); `discover_with(extra)` adds the folders from `clients`, one entry per real folder (canonical paths, so a junction or symlink to a listed folder is not listed twice); `is_listed` for Choose folder; `clients_at(dir)` checks a folder the user picked; `discover_in(&Roots)` for tests |
| `src/system.rs` | `Os`, `Env`, the `System` trait (trust store, Run key, launchctl, systemd); `RealSystem` is the only code that touches the machine |
| `src/ca.rs`, `src/autostart.rs` | Install steps: local CA + trust; Run key / LaunchAgent / systemd user unit |
| `src/ca.rs` (installer) | `ensure_ca`: trusts the CA when the fingerprint stored in `install.log` differs or `System::is_trusted` says no; `is_untrusted` drives the `run`/`launch` warning | A failing check must not re-prompt on every `setup` |
| `src/shortcut.rs` | Start-menu shortcut "Discord … (PhoeCord)" that runs `phoecord launch --channel …`: Windows `.lnk` (PowerShell + WScript.Shell, every value quoted), Linux `.desktop` | Created and removed through `System`, logged in `install.log` |
| `src/launch.rs` | `choose` (the client for `--channel`, or the only one installed; several without `--channel` is an error), `client_command` (a macOS `.app` client starts through `open -n -a ... --env ... --args`), `probe_proxy` (is PhoeCord already on the port: `GET /__phoecord`) | `launch` never defaults silently to Stable |
| `src/vencord.rs` | `phoecord import-vencord`: finds Vencord / Vesktop / Equicord `settings.json` (or `--path`), maps enabled plugins by `Manifest::vencord` | Reads only; never writes Vencord's files; one file is listed once on case-insensitive file systems |
| `src/terms.rs` | `TERMS_VERSION`, both terms texts (`include_str!` of `TERMS.md` / `TERMS.ru.md`), `needs_acceptance`, `accept` (stored in the data folder and logged so `uninstall` removes it) | Bump `TERMS_VERSION` whenever the text changes, so users are asked again |
| `src/system.rs` additions | `stop_proxies` (only `run`/`launch` processes of PhoeCord's own binaries, never the calling process; `select_proxy_pids` is tested), `shortcut_create`, `Env.start_menu`, `Env.applications_dir` | |
| `src/install.rs`, `src/uninstall.rs` | `install()` (idempotent), `install_with_progress` (reports each `Phase` start/end for the window) and `uninstall_all()` (undoes the log newest-first, `UninstallReport`) |
| `src/log.rs` | `install.log` (JSON lines in the data folder): every step the installer made, so rollback removes only what it created |
| `src/loader.rs` | Loader DLL step (1.5.6, opt-in `setup --loader`, Windows only): `install_loader` records `Step::LoaderWanted` and copies `version.dll` (next to the `phoecord` binary, `default_source`) into the newest `app-x.y.z` (`discover::newest_app_dir`); `ensure_loader` re-applies it on `run`/`launch` after Discord updates; every copy is logged as `Step::LoaderCopy` with its SHA-256 | A `version.dll` whose hash is not recorded for that file is foreign: never overwritten, never removed by `uninstall` (`Kept::NotOurs`); `ensure_loader` never fails its caller |

### `phoecord-installer-gui`

The installer window (stage 1.4.3). Every change to the machine goes through `phoecord-installer`, the same code as `phoecord setup` / `uninstall`.

| File | What | Easy to break |
|:---|:---|:---|
| `src/main.rs` | Window options, icon, `--dry-run` / `--demo` / `--demo-empty` / `--open <path>` | `windows_subsystem = "windows"`: no console on Windows |
| `src/app.rs` | `App`: header with tabs (adapts to narrow windows), tab transition, dialogs (scrim + card), toasts, drop overlay, messages from jobs | Dialog card is an `Area` anchored at the centre; its scroll area sets `min_scrolled_height` or it shrinks to half the window |
| `src/backend.rs` | `Mode` (real or dry-run sandbox with a simulated trust store), `read_status` (clients, install log, trust, proxy probe), background jobs (install, uninstall, launch, plugin jobs) | `Env.proxy_exe` is the `phoecord` binary next to the window (or `PHOECORD_BIN`), never the window itself |
| `src/packages.rs` | Plugin packages: stage a `.zip` / folder / `plugin.toml` into `plugins-staging/`, check it, `commit` into `plugins/<id>` and switch it on, `remove`, HTTPS-only `download` (32 MB cap) | Zip paths outside the archive are refused; only `plugin.toml`, `plugin.wasm`, README and LICENSE are copied |
| `src/theme.rs` | The one palette, spacing, radii, type scale, Onest fonts + Phosphor icons, the app icon as window icon and in-app mark | Contrast is tested; add colours here, never inline |
| `src/splash.rs` | Opening animation (~1.4 s): the flame rises and catches (flash, two shock rings, embers), the name comes in letter by letter, then the window opens out of the flame as an iris and comes in (`reveal`); click or key skips, off with reduced motion | Own clock capped per frame, so slow first frames do not eat it; drawn last in `update`, above everything |
| `src/onboarding.rs` | First start: welcome, terms, pick a Discord, done (starts the install) | Shown on the first start of each app version: the `onboarded` file in the data folder holds the version that finished it; the terms step is skipped when already accepted |
| `src/motion.rs` | Interruptible tweens with custom easing, durations, reduce-motion switch | Tweens request repaints only while they move |
| `src/widgets.rs` | Buttons, segmented tabs, switch, pill, banner, field, spinner, check mark | Every widget reports itself to accesskit and shows a focus ring |
| `src/terms.rs` | Draws `TERMS.md` from its Markdown (headings, bullets, bold, code, links as text) | |
| `src/tabs/*.rs` | Install, Plugins (+ dialogs), Settings, About, terms screen | Strings only through `gui-*` keys (tested in `text.rs`) |
| `assets/installer/` | NSIS header and sidebar, WiX banner and dialog, DMG background | Written by `scripts/package/make_art.py` |
| `assets/icon/` | App icon: `icon.svg` (source), `icon-256.png` (window and in-app mark), `icon-1024.png`, `icon.ico` (Windows), `icon.icns` (macOS) | Regenerate the rest from the SVG when it changes (`python scripts/package/make_art.py`, which also draws `assets/installer/`) |
| `assets/installer/nsis/Russian.nsh` | Russian strings for cargo-packager's own NSIS pages (`custom-language-file` in `Cargo.toml`) | UTF-8 with BOM; keys must match cargo-packager's `English.nsh` for the pinned version |
| `build.rs` (here and in `phoecord-cli`) | Embeds `assets/icon/icon.ico` and version info in the Windows `.exe` files (`winresource`) | Taskbar, Explorer and Settings > Apps read the icon from the exe; `with_icon` covers only the title bar |
| `assets/fonts/Onest-*.ttf` | Onest 400/500/600, static instances of the variable font | SIL OFL 1.1, `LICENSES/OFL-onest.txt` |

### `phoecord-i18n`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | `Localizer` (`new`, `t`, `t_args`, `language`), `normalize`, `detect_os_language`, `resolve_language`, `SUPPORTED`; `PluginStrings` (per plugin and language bundles behind `Host::t`) | Bundles are `include_str!`-ed: a new language needs a `source()` arm |
| `i18n/en/main.ftl`, `i18n/ru/main.ftl` | Shared strings; English is the source | Every `en` key needs a `ru` twin with the same variables (`check_i18n.py`) |

### `phoecord-cli`

| File | What | Easy to break |
|:---|:---|:---|
| `src/main.rs` | Commands: `run`, `launch`, `setup` (installer `install()`), `uninstall` (installer `uninstall_all()`), `trust`, `untrust`, `plugins`, `enable`, `disable`, `toggle`, `log`, `selftest`, `paths`; `run` loads WASM packages | `plugins`/`enable` know built-ins only, not WASM packages; every user-facing string comes from `crates/phoecord-i18n/i18n/{en,ru}/main.ftl`; `setup` asks to accept `TERMS.md` first (`--accept-terms` skips the prompt); `setup --loader` installs the loader DLL, and `run`/`launch` call `ensure_loader` first |
| `src/help.rs` | `--help` built at runtime from the `Localizer` (clap needs static strings, so the help is rebuilt per language); `lang_from_args` reads `--lang` before clap runs | A new subcommand or flag needs `cmd-*` / `cli-arg-*` keys in both `.ftl` files |
| `src/log.rs`, `src/selftest.rs` | `phoecord log` output, `phoecord selftest` | |

## 4. Scripts

| Script | What |
|:---|:---|
| `scripts/check.sh`, `scripts/check.ps1` | All checks in order, stop at first failure (see `AGENTS.md`) |
| `scripts/check_i18n.py` | Every key of `crates/*/i18n/en/*.ftl` exists in the `ru` file with the same variables; exit 1 otherwise |
| `examples/plugins/clear_urls/` | The first third-party plugin example: `clear_urls` as a WASM package (`plugin.toml` id `clear_urls_wasm`, `wit-bindgen`), its own cargo workspace, excluded from the main one; README shows build and install |
| `scripts/update_contributors.py` | Rewrites the README contributors block (avatars between `<!-- contributors:* -->` markers) from GitHub's contributors list, bots left out; `--check` exits 1 if stale. Run after merging a new contributor's PR |
| `scripts/package/make_art.py`, `scripts/package/name-files.sh`, `scripts/package/README.md` | Renders the app icons and installer pictures from `icon.svg`; renames the packaged `.exe`/`.msi` to `PhoeCord_<version>_…`; how to build the installers locally (`cargo packager`, config in `crates/phoecord-installer-gui/Cargo.toml`) |
| `scripts/dev/New-TestVm.ps1` | Creates a throwaway Hyper-V VM (Gen 2, Secure Boot, vTPM, Default Switch) for the manual checks 1.4.6 and 1.5.7, so they never touch your own Discord; run elevated |
| `scripts/gen_decisions.py` | Builds `docs/DECISIONS.md` from `Ruling:` lines of `docs/<plan>/ai/ledger.md`; `--check` exits 1 if stale |

## 5. Docs

See [`README.md`](README.md) in this folder. Project-level documents live here too: [`ARCHITECTURE.md`](ARCHITECTURE.md), [`ROADMAP.md`](ROADMAP.md), [`AGENTS.md`](AGENTS.md) + [`CLAUDE.md`](CLAUDE.md) (rules for people and AI agents). [`PORTING_PLUGINS.md`](PORTING_PLUGINS.md) is the porting guide; [`PLUGINS.md`](PLUGINS.md) is the plugin catalogue (every Vencord plugin and userplugin mapped to a layer and a task).

## 6. Config, environment and files on disk

Planned in 1.1; the table is filled as each item lands.

| Name | Kind | Default | Used by |
|:---|:---|:---|:---|
| `listen` | `config.toml` | `127.0.0.1:18463` | proxy |
| `upstream` | `config.toml` | unset (direct) | proxy (`phoecord-proxy::egress`): `socks5://host:port` or `http://host:port`; applies to new connections without a restart |
| `intercept` | `config.toml` | Discord API and Gateway hosts | proxy |
| `language` | `config.toml` | OS language, fallback `en` | all user-facing text; order: `--lang` flag, then this key, then the OS language, then `en` (`phoecord_i18n::resolve_language`); `phoecord paths` shows the result and its source |
| `clients` | `config.toml` | empty | extra folders with a Discord client installed outside the default places, added by "Choose folder…" in the installer window; `launch`, `setup` and the window look in them too (`discover_with`) |
| `--lang <code>` | CLI flag | unset | overrides `language` for one command |
| `[plugins.<id>]` | `config.toml` | per plugin | `Host::setting` |
| `PHOECORD_CONFIG`, `PHOECORD_DATA` | env | OS config/data dirs | paths |
| `PHOECORD_LOG` | env | `phoecord=info` | logging |
| `PHOECORD_NO_NOTIFY` | env | unset | `Host::notify` |
| `ca.pem`, `ca.key` | data folder | created on first run | `phoecord-proxy::ca` |
| `plugins/<id>/kv.json` | data folder | empty map | `Host::kv_*` |
| `plugins/message_logger/messages.jsonl` | data folder | created when `write_log` is on | `message_logger`, `phoecord log` |
| `install.log` | data folder | created by the installer | `phoecord-installer` rollback |
| `plugins-staging/` | data folder | created while a plugin waits for the user's yes in the installer window, removed after | `phoecord-installer-gui::packages` |
| `launch.log` | data folder | written each time the installer window starts Discord | `phoecord-installer-gui::backend`: `phoecord launch` output, shown when it stops |
| `PHOECORD_BIN` | env | the `phoecord` binary next to the window | `phoecord-installer-gui`: which binary autostart and shortcuts run |
| `PHOECORD_GUI_SANDBOX` | env | `<temp>/phoecord-gui-dry-run` | `phoecord-installer --dry-run`: the sandbox folder |
