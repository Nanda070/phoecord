# Phase 1: implementation plan

**Goal:** PhoeCord works end-to-end on today's Discord with ≥ 8 plugins, WASM plugins, English and Russian, a GUI installer and a proven loader, before Phoenix Canary ships.
**Spec:** [`spec.md`](spec.md) · **Ledger:** [`ledger.md`](ledger.md) · **For people:** [`../README.md`](../README.md)

Tasks are grouped by stage. Each has files, steps and "done when". Statuses and owners are in the "Tasks" table of [`README.md`](../README.md), the only place status is tracked. Every task PR also does the living-docs steps from `docs/AGENTS.md`; they are not repeated below.

## Areas

| Area | Owner | Files |
|:---|:---|:---|
| Plugin API, core | **Nanda** | `crates/phoecord-core/**`, `wit/**` |
| Proxy, CA, trust | **Nanda** | `crates/phoecord-proxy/**` |
| Runtime | **Nanda** (SDK examples: contributors) | `crates/phoecord-plugin-runtime/**`, `crates/phoecord-plugin-sdk/**`, `examples/**` |
| Plugins | **Contributors** (`fake_nitro`, `message_logger`, commands: Nanda) | `crates/phoecord-plugins/**` |
| Loader | **Nanda** | `crates/phoecord-loader/**`, `signatures/**` |
| Installer, CLI | **Nanda** | `crates/phoecord-installer/**`, `crates/phoecord-cli/**` |
| Localization | **Nanda** (core), **contributors** (translations, CI check) | `crates/phoecord-i18n/**`, `crates/*/i18n/**` |
| Research | **Nanda** (reference build, Zed themes: contributors) | `docs/research/**`, `research/**` |
| Repository rules, CI, docs | **Nanda** | `.github/**`, `AGENTS.md`, `docs/**`, `scripts/**` |

## Stage 1.0. Housekeeping

### 1.0.1. Merge the Phase 1 plan and docs structure (Nanda)

**Files:** `docs/**`, `AGENTS.md`, `CLAUDE.md`, `README.md`, `ARCHITECTURE.md`, `ROADMAP.md`, `CONTRIBUTING.md`, `SECURITY.md`, `.github/**`, `scripts/**`

- Remove upstream mentions; keep the `LICENSE` copyright lines.
- Mirror the VALORANT docs structure; English-only docs.
- `scripts/gen_tasks.py`, `scripts/gen_decisions.py`, `scripts/check.{sh,ps1}`; CI job for generated docs.
- PR #4 from `docs/phase-1-plan`, green CI, merged by Nanda (`ea73482`).

**Done when:** `main` has `docs/phase-1/`, `docs/DECISIONS.md` is generated and fresh, and no Cyrillic text is left in the repository.

### 1.0.2. Dependabot (Nanda)

**Files:** `.github/workflows/ci.yml`, `.github/dependabot.yml`

- MSRV job pinned via `dtolnay/rust-toolchain@master` + `toolchain: "1.88"`.
- Close PR #1 (`rust-toolchain` 1.88 → 1.120) with a comment pointing to the pin.
- PR #2 (`actions/checkout` 4 → 7) and PR #3 (cargo group: `rcgen` 0.14.10, `time` 0.3.55) folded into one PR on `chore/deps-and-1.0-close`; #2 and #3 closed in its favour.

**Done when:** no open Dependabot PRs; the MSRV job still runs 1.88.

## Stage 1.1. The proxy on the new API

### 1.1.1. Proxy `Host` (Nanda)

**Files:** `crates/phoecord-proxy/src/host.rs`, `crates/phoecord-proxy/src/lib.rs`

- `ProxyHost<'a>`: `setting` from the plugin's config table, `kv_get/kv_set` over `data/plugins/<id>/kv.json` (atomic write), `self_id` from `Account`, `notify` via `notify-rust` with a per-plugin rate limit (5 / minute) and `PHOECORD_NO_NOTIFY`, `log` via `tracing` with the plugin id as a field.
- Unit tests: KV round trip, rate limit, missing settings.

**Done when:** `ProxyHost` passes its tests and `Chain` can run with it in a unit test.

### 1.1.2. `config.toml` (Nanda)

**Files:** `crates/phoecord-proxy/src/config.rs`

- `Config { listen, intercept, language, plugins: BTreeMap<String, toml::Table> }`, `Paths::resolve()` with `PHOECORD_CONFIG`, `PHOECORD_DATA`.
- Defaults merged from manifests; `enabled` key per plugin; `no_track` on by default.
- `load_or_init`, `save` preserving unknown sections; reload every 2 s by mtime; a broken file logs `config not reloaded` and keeps the old config.
- Tests for all of the above.

**Done when:** editing `config.toml` while running switches a plugin within 2 s, and a broken edit does not.

### 1.1.3. Account state behind `Host` (Nanda)

**Files:** `crates/phoecord-proxy/src/account.rs`, `crates/phoecord-proxy/src/host.rs`, `crates/phoecord-core/src/plugin.rs` (API additions of spec §3.1)

- Port v0 `state.rs` as `Account`: tracks `READY`, `GUILD_CREATE`, emoji, channel → guild, names.
- Add `Host::guild_emojis`, `Host::channel_guild`, `Host::emit_send` (`api` PR, CHANGELOG line).
- The account mutex is held per event, not for the whole chain.

**Done when:** a fixture READY fills `Account` and the new `Host` methods return the expected values.

### 1.1.4. Proxy server (Nanda)

**Files:** `crates/phoecord-proxy/src/server.rs`

- Port v0 `proxy.rs`: `CONNECT` only, loopback only, MITM when `port == 443 && intercepts(host)`, `copy_bidirectional` otherwise.
- `Proxy` struct of spec §3.2.
- Tests: non-loopback refused; non-intercepted host tunnelled byte-for-byte (local echo server).

**Done when:** `phoecord run` accepts a `CONNECT` and tunnels a non-Discord host.

### 1.1.5. REST through `Chain` (Nanda)

**Files:** `crates/phoecord-proxy/src/rest.rs`, `crates/phoecord-plugin-runtime/src/lib.rs`

- Port v0 `rest.rs`: kept-alive upstream, hop-by-hop headers stripped.
- `Chain::rest_response`; request body decoded only if wanted; `Replied` answered locally; `Changed` re-serialized with a correct `Content-Length`.
- Test: `no_track` makes `POST /api/v9/science` return 204 without an upstream call.

**Done when:** with Discord running through `phoecord run`, analytics calls are answered locally and everything else works.

### 1.1.6. Gateway through `Chain` (Nanda)

**Files:** `crates/phoecord-proxy/src/gateway.rs`, `crates/phoecord-plugin-runtime/src/lib.rs`

- Port v0 `gateway.rs`; `Chain::send`; decode only if any plugin wants Gateway events.
- After the first changed frame, re-compress with our context; emit `extra` payloads after their frame; flush `emit_send` queue.
- Tests over fixtures (1.1.9): unchanged stream passes byte-identical; changed stream stays decodable.

**Done when:** a test plugin that rewrites `MESSAGE_CREATE` content is visible in a real client, JSON and ETF.

### 1.1.7. CLI (Nanda)

**Files:** `crates/phoecord-cli/src/**`, `crates/phoecord-installer/src/launch.rs`

- `clap` commands: `run`, `launch [-- cmd]`, `setup`, `trust`, `untrust`, `plugins`, `enable`, `disable`, `toggle`, `paths`, `--version`.
- Port v0 `launch.rs` into the installer crate; the CLI calls library functions only.
- All user-facing strings collected in one module per crate, ready for 1.7.

**Done when:** a fresh machine goes from `cargo install` to a running client with `phoecord setup && phoecord launch`.

### 1.1.8. `selftest` (Nanda)

**Files:** `crates/phoecord-cli/src/selftest.rs`

- Port v0 `selftest.rs`: REST through the proxy with the local CA; Gateway HELLO round trip for JSON/zlib-stream and ETF/zstd-stream; no login.
- A manual-only CI job (`workflow_dispatch`) that runs it.

**Done when:** `phoecord selftest` prints "all checks passed" against the real Discord.

### 1.1.9. Gateway fixtures (anyone)

**Files:** `tests/fixtures/**`, `tests/fixtures/README.md`

- Record READY, GUILD_CREATE, MESSAGE_CREATE/UPDATE/DELETE, PRESENCE_UPDATE, TYPING_START from a throwaway account, both encodings.
- Scrub tokens, emails, phone numbers, real user ids and names (a script in `scripts/scrub_fixture.py`, `-I` safe).
- README: how each fixture was recorded and scrubbed.

**Done when:** fixtures load in tests and contain no personal data (reviewed by Nanda).

### 1.1.10. Chain benchmark (Nanda)

**Files:** `crates/phoecord-proxy/benches/chain.rs`, `docs/chain-bench.md` (one-off report)

- `criterion` bench: decode → chain of all built-in plugins → encode, for READY 5 MB and a MESSAGE_CREATE burst.
- Report p50/p99 with machine and commit.

**Done when:** p99 ≤ 1 ms per frame, or a ledger line explaining the gap and the plan.

## Stage 1.2. Built-in plugins

Each plugin: port or write it on Plugin API v1 from `docs/_template/code/plugin.rs`; tests for the matching rule and a chain test; a config section with defaults; its row in `docs/PLUGINS.md`; user-facing strings ready for 1.7.

### 1.2.1. `silent_typing` (anyone)

**Files:** `crates/phoecord-plugins/src/silent_typing.rs`

- `POST /channels/{id}/typing` → `Reply 204`.

**Done when:** with the plugin on, others never see you typing.

### 1.2.2. `unindent` (anyone)

**Files:** `crates/phoecord-plugins/src/unindent.rs`

- Strip shared indentation in fenced code blocks of `POST`/`PATCH …/messages` bodies; leave other text untouched.

**Done when:** a pasted indented code block is sent flush-left; v0 tests pass.

### 1.2.3. `game_activity_toggle` (anyone)

**Files:** `crates/phoecord-plugins/src/game_activity_toggle.rs`

- Remove game activities from op 3 presence updates; on toggle, `emit_send` the current presence again.

**Done when:** `phoecord toggle game_activity_toggle` hides or shows your game at once.

### 1.2.4. `relationship_notifier` (anyone)

**Files:** `crates/phoecord-plugins/src/relationship_notifier.rs`

- `RELATIONSHIP_REMOVE`, `GUILD_DELETE` (not unavailable), `CHANNEL_RECIPIENT_REMOVE` → `notify`; ignore your own actions (track recent own REST calls).

**Done when:** being unfriended or kicked shows a desktop notification; your own actions do not.

### 1.2.5. `fake_nitro` (Nanda)

**Files:** `crates/phoecord-plugins/src/fake_nitro.rs`

- Rewrite unusable emoji in outgoing messages as `[name](cdn link)` or a bare link, size from settings; unlock pickers by patching `READY` user flags; keep the real tier in `Account`.

**Done when:** an emoji from another server is sent as a linked image; your real Nitro status is never sent upstream.

### 1.2.6. `message_logger` and `phoecord log` (Nanda)

**Files:** `crates/phoecord-plugins/src/message_logger.rs`, `crates/phoecord-cli/src/log.rs`

- LRU cache of `MESSAGE_CREATE` (size from settings); on `MESSAGE_DELETE` rewrite to a `MESSAGE_UPDATE` marking it deleted; on `MESSAGE_UPDATE` keep previous versions; ignore own messages by default.
- Append to `data/plugins/message_logger/messages.jsonl` if `write_log`; `phoecord log -n 20`.

**Done when:** a deleted message stays visible with a "deleted" marker and shows up in `phoecord log`.

### 1.2.7. `clear_urls` (anyone)

**Files:** `crates/phoecord-plugins/src/clear_urls.rs`

- Strip known tracking parameters (`utm_*`, `fbclid`, `gclid`, `si` on YouTube/Spotify, …) from URLs in outgoing message content; extra rules from settings.

**Done when:** a link with `utm_source` is sent without it; non-tracking parameters survive.

### 1.2.8. `/phoecord` slash commands (Nanda)

**Files:** `crates/phoecord-plugins/src/commands.rs`

- Merge PhoeCord commands into `GET …/application-commands` search responses; answer their `POST /interactions` locally with an ephemeral-looking message dispatched to the client.

**Done when:** `/phoecord plugins` lists plugins in the client without anything reaching Discord.

### 1.2.9. `phoecord import-vencord` (anyone)

**Files:** `crates/phoecord-installer/src/vencord.rs`, `crates/phoecord-cli/src/**`

- Find Vencord / Vesktop / Equicord `settings.json`; map enabled plugins by `Manifest::vencord`; print what changed.

**Done when:** a Vencord user's matching plugins are enabled with one command.

### 1.2.10–1.2.19. Vencord parity, network parts

Every row of [`docs/PLUGINS.md`](../../PLUGINS.md) whose "Where" column names a task from 1.2.10 to 1.2.19 is built in that task, one PR per task (one plugin per commit). Each plugin follows the same steps as 1.2.1: Plugin API v1, the plugin template, matching-rule tests, a chain test, default config, `docs/PLUGINS.md` "Where" cell changed to ✅.

| Task | Needs from the host (spec §3.1) | Owner |
|:---|:---|:---|
| 1.2.10 Message filters | `RestResponse` and `Dispatch` rewriting only | anyone |
| 1.2.11 Outgoing text | `RestRequest` body rewriting; multipart file names for AnonymiseFileNames | anyone |
| 1.2.12 Commands | the `/phoecord` command framework from 1.2.8; `Host::rest` for FriendInvites and UnsuppressEmbeds | anyone |
| 1.2.13 Presence | `Send` op 3 rewriting; `Host::emit_send`; `Host::fetch` for MusicRichPresence (Last.fm, ListenBrainz) | anyone |
| 1.2.14 Profiles | `RestResponse` rewriting of profile responses; `Host::fetch` for USRBG and Decor | anyone |
| 1.2.15 External services | `Host::fetch` (DeArrow API, Tenor/Giphy) | anyone |
| 1.2.16 Data parts | `Host::rest` (ValidUser, ValidReply, NewGuildSettings); `Dispatch` `extra` payloads | Nanda |
| 1.2.17 Translate etc. | `Host::fetch` (translation backend, ReviewDB API); XSOverlay over a local UDP right | anyone |
| 1.2.18 Hidden things | `Dispatch` `READY` / `GUILD_CREATE` rewriting; experiment assignments | Nanda |
| 1.2.19 Userplugins | `Host::rest` with the rate limiter (VoiceChatUtilities, rmcord); `Send` op 4 (FakeDeafen, FollowUser) | Nanda |

**Done when:** every 🌐 row of `docs/PLUGINS.md` is ✅ and every 🌐 + 🎨 row has its data part ✅ (on-screen part waits for Phase 2).

## Stage 1.3. Dynamic plugin engine (WASM)

### 1.3.1. `wit/phoecord.wit` (Nanda)

**Files:** `wit/phoecord.wit`

- World `plugin`: exports `manifest`, `wants`, `on-event`; imports `host`. `val` as a recursive variant; `event`, `verdict` as variants mirroring `plugin.rs`.

**Done when:** `wit-bindgen` generates host and guest bindings without errors; the file is reviewed against `plugin.rs`.

### 1.3.2. WASM backend (Nanda)

**Files:** `crates/phoecord-plugin-runtime/src/wasm.rs`

- `WasmPlugin: Plugin` on wasmtime with the component model; one `Store` per plugin; `Val` conversion both ways.

**Done when:** a test guest built in CI passes the same chain tests as a native plugin.

### 1.3.3. Limits (Nanda)

**Files:** `crates/phoecord-plugin-runtime/src/wasm.rs`

- Epoch interruption (5 ms per event, configurable), `StoreLimits` (32 MB), trap → plugin disabled until restart, event treated as `Pass`.
- Test guests: panic, infinite loop, memory hog.

**Done when:** each bad guest is disabled and the chain keeps working.

### 1.3.4. Packages and hot loading (Nanda)

**Files:** `crates/phoecord-plugin-runtime/src/package.rs`

- Load `data/plugins/<id>/plugin.toml` + `plugin.wasm`; validate `api`; `events` globs → `wants`; watcher reloads a changed package between events.

**Done when:** copying a package into the folder enables it without restarting PhoeCord.

### 1.3.5. Rights (Nanda)

**Files:** `crates/phoecord-plugin-runtime/src/wasm.rs`, `crates/phoecord-plugin-runtime/src/package.rs`

- `rights` gate `Host` methods; a denied call returns an error to the guest and is logged once; events outside `events` are never delivered.

**Done when:** a test guest without `notify` cannot notify, and one subscribed to REST never sees Gateway events.

### 1.3.6. SDK and example (anyone)

**Files:** `crates/phoecord-plugin-sdk/**`, `examples/hello-plugin/**`

- Guest bindings re-exported; `#[phoecord::plugin]` macro; `examples/hello-plugin` that logs every `MESSAGE_CREATE`; a CI job that builds it for `wasm32-wasip2`.

**Done when:** `cargo build --target wasm32-wasip2` in the example produces a package that 1.3.4 loads.

### 1.3.7. TypeScript template (anyone)

**Files:** `examples/hello-plugin-ts/**`

- `componentize-js` build of a TS plugin against `wit/phoecord.wit`; README with the build steps.

**Done when:** the TS example loads and runs like the Rust one.

### 1.3.8. `clear_urls` as WASM (Nanda)

**Files:** `plugins/clear_urls/**`

- Rebuild 1.2.7 as a WASM package; remove the native one from `builtin()`; ship it in the default `plugins/` folder.

**Done when:** `clear_urls` works identically as an external package.

## Stage 1.4. GUI installer prototype

### 1.4.1. Client discovery (Nanda)

**Files:** `crates/phoecord-installer/src/discover.rs`

- Stable / PTB / Canary on Windows, macOS, Linux (PATH, Flatpak, Snap); version of each.

**Done when:** discovery finds every installed client on the three OSes in a manual check.

### 1.4.2. CA and autostart (Nanda)

**Files:** `crates/phoecord-installer/src/{ca.rs,autostart.rs}`

- Create and trust the CA (via `phoecord-proxy::{ca,trust}`); autostart per OS (spec §3.4); `data/install.log` of every step.

**Done when:** `phoecord setup` installs CA trust and autostart through the logged steps (tests over a fake `System` on all 3 OSes); the reboot check on real machines is 1.4.6.

### 1.4.3. Installer GUI (Nanda)

**Files:** `crates/phoecord-installer-gui/**`

- `eframe` (egui) app with an embedded font that has Cyrillic glyphs; one window: client list, Install / Uninstall / Repair, status; English and Russian (needs 1.7.1).
- **Plugins tab**, so userplugins are easier to add than in Vencord (no rebuild, no terminal): a list of built-in plugins and installed packages with an on/off switch each; "Install from file" (a `.zip` or folder with `plugin.toml` + `plugin.wasm`) and "Install from link" (HTTPS only); before installing, a dialog shows the plugin's name, version, author, `events` and `rights` in plain words and installs only after the user confirms; "Remove" deletes the package folder and its data after a confirm. Packages go into the data folder's `plugins/`, where the proxy picks them up without a restart (1.3.4).

**Done when:** a non-developer installs and removes PhoeCord without a terminal, and installs, switches off and removes a WASM plugin from a file without a terminal.

### 1.4.4. Full rollback (Nanda)

**Files:** `crates/phoecord-installer/src/uninstall.rs`

- Undo every logged step: CA from stores, loader DLL, autostart, folders.

**Done when:** `phoecord uninstall` undoes every logged step, keeps going after a failed one and leaves an empty tree (tests over a fake `System` on all 3 OSes); the check on real machines is 1.4.6.

### 1.4.6. Manual check on real machines (Nanda)

**Files:** none (results go into the ledger line that closes it).

- On Windows, macOS and Linux: `phoecord setup`, reboot, check the proxy runs and Discord connects through it; then `phoecord uninstall` and check no PhoeCord file, trust-store entry or autostart entry is left.

**Done when:** all three OSes pass, or each failure has an issue.

### 1.4.5. Releases (Nanda)

**Files:** `.github/workflows/release.yml`, `docs/GOVERNANCE.md`

- Tag-triggered build for the three OSes; `SHA256SUMS`; unsigned; release notes with the SmartScreen note.
- Submit the loader DLL to Microsoft Defender's false-positive portal for each release.

**Done when:** pushing tag `v0.5.0` produces a GitHub release with binaries and checksums.

## Stage 1.5. DLL sideloading prototype

### 1.5.1. Export forwarding (Nanda)

**Files:** `crates/phoecord-loader/build.rs`, `crates/phoecord-loader/src/proxy_dll.rs`

- Generate forwarders for every export of `version.dll` at build time; load the real DLL from the system directory by full path.

**Done when:** the client starts normally with our `version.dll` next to it and the loader doing nothing else.

### 1.5.2. `DllMain` and worker (Nanda)

**Files:** `crates/phoecord-loader/src/entry.rs`

- `DllMain` only disables thread calls and spawns the worker; the worker logs to `data/loader.log`.

**Done when:** no loader-lock work (reviewed), a log line on every client start.

### 1.5.3. Version detection and signatures (Nanda)

**Files:** `crates/phoecord-loader/src/{version.rs,signatures.rs}`, `signatures/README.md`

- File version + `.text` hash; parse `signatures/*.toml`; resolve with `Pattern::find_unique` + `rip`.

**Done when:** a test signature resolves to the same function on two client versions.

### 1.5.4. Detours (Nanda)

**Files:** `crates/phoecord-loader/src/hook.rs`

- Inline detours (`retour` or own trampoline) with threads suspended; removal on unload; every `unsafe` with `// SAFETY:`.

**Done when:** a no-op hook on a resolved function installs and uninstalls without a crash 100 times in a row.

### 1.5.5. IPC (Nanda)

**Files:** `crates/phoecord-loader/src/ipc.rs`, `crates/phoecord-proxy/src/ipc.rs`

- Named pipe per user; messages: status, settings get/set, UI events (JSON lines).

**Done when:** the loader reports its status to `phoecord run`, visible in the proxy log.

### 1.5.6. Survive Discord updates (Nanda)

**Files:** `crates/phoecord-installer/src/loader.rs`

- Detect a new `app-x.y.z` folder and reinstall the DLL (or install at the launcher level if that proves robust).

**Done when:** after a Discord update the loader is present on next start without user action.

### 1.5.7. Verification on 3 versions (Nanda)

**Files:** a section in the PR description

- Start, run 10 minutes, quit, on 3 consecutive client versions; record results.

**Done when:** zero crashes across the three versions, report merged.

## Stage 1.6. GPUI (Zed) research

### 1.6.1. A map of GPUI (Nanda)

**Files:** `docs/research/gpui.md` (draft)

- `App`, `Window`, element tree, frame building, where theme colours are read; with source links.

**Done when:** the draft answers "where would a palette, a settings tab list and a message row be built".

### 1.6.2. Reference build (anyone)

**Files:** `research/gpui-reference/**`

- A small GPUI app with a themed list and a settings-like tab view, built release + LTO + strip like a shipping app; build script and the exact GPUI commit.

**Done when:** the binary builds reproducibly and is available to Nanda (not committed).

### 1.6.3. GPUI in a release binary (Nanda)

**Files:** `docs/research/gpui.md`

- What survives LTO: panic strings, `type_name`s, crate paths; draft AOB anchors for the 1.6.1 targets.

**Done when:** at least one unique signature per target in the reference build.

### 1.6.4. Themes in Zed (anyone)

**Files:** `docs/research/gpui.md` (section)

- How Zed loads JSON themes into GPUI; what Phoenix would need for themes-as-files.

**Done when:** the section says whether a file-based theme is plausible and what to check in 2.1.

### 1.6.5. Final report (Nanda)

**Files:** `docs/research/gpui.md`

- Hook candidates with anchors and risks; what to do first in 2.2.

**Done when:** the report is merged and referenced from `ROADMAP.md` 2.2.

## Stage 1.7. Localization: English and Russian

### 1.7.1. `phoecord-i18n` (Nanda)

**Files:** `crates/phoecord-i18n/**`

- `Localizer` over `fluent-bundle`, bundles via `include_str!`, `t(key, args)`, fallback ru → en → key (logged once).

**Done when:** unit tests cover lookup, arguments and fallback.

### 1.7.2. Language setting (Nanda)

**Files:** `crates/phoecord-proxy/src/config.rs`, `crates/phoecord-cli/src/**`

- `language` key; OS language via `sys-locale` when unset; `phoecord --lang ru` override.

**Done when:** on a Russian OS the CLI speaks Russian with no config.

### 1.7.3. Russian translation (anyone)

**Files:** `crates/*/i18n/ru/main.ftl`

- Translate every CLI, notification and installer string; consistent terms (a glossary at the top of the file).

**Done when:** no English string appears in the Russian CLI and installer.

### 1.7.4. Plugin strings (Nanda)

**Files:** `crates/phoecord-core/src/plugin.rs`, `crates/phoecord-plugin-runtime/src/**`

- `Host::t()` (API minor); load `i18n/<lang>.ftl` from plugin packages and from built-in plugins.

**Done when:** a built-in plugin's notification appears in Russian.

### 1.7.5. CI key check (anyone)

**Files:** `scripts/check_i18n.py`, `.github/workflows/ci.yml`, `scripts/check.{sh,ps1}`

- Every key in `en/*.ftl` exists in `ru/*.ftl` with the same arguments; part of `check.sh` and CI.

**Done when:** removing a Russian key fails CI.
