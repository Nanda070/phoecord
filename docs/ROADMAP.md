# PhoeCord roadmap

The plan from rebuilding the v0 codebase to a v1.0 release on Project Phoenix. How it all fits together: [`ARCHITECTURE.md`](ARCHITECTURE.md). Tick a box in the same PR that does the work. Task-level detail for the current phase: [`docs/phase-1/`](phase-1/README.md).

| Phase | When | Goal | Versions |
|:---|:---|:---|:---|
| **0. Foundation** | November 2026 | Workspace, docs, CI | ✅ |
| **1. Before the Phoenix Canary public beta** | until Canary ships | Network layer works on today's Discord, dynamic plugins, installer and loader prototypes, localization | `0.2` – `0.6` |
| **2. Phoenix Canary release** | after Canary ships | Reverse engineering, signatures, UI hooks and themes, v1.0 | `0.7` – `1.0` |

Phase 2 has no dates: it starts when Discord releases Phoenix Canary. Until then we do everything we can on today's client.

---

## Phase 0. Foundation ✅

- [x] Audit of the v0 codebase and migration map: [`docs/phase-1/ai/spec.md`](phase-1/ai/spec.md)
- [x] Cargo workspace: `core`, `proxy`, `plugin-runtime`, `plugin-sdk`, `plugins`, `loader`, `installer`, `cli`
- [x] Ported `val`, `etf`, routes, `codec`, `ca`, `trust`, keeping the copyright
- [x] Plugin API v1 (`Manifest`, `Event`, `Verdict`, `Host`) and `Chain`
- [x] AOB scanner with tests
- [x] README, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, issue and PR templates, CI on 3 OSes

---

## Phase 1. Before the Phoenix Canary public beta

### 1.1 Core refactor: the proxy on the new API · `0.2`

- [x] Move `proxy.rs`, `rest.rs`, `gateway.rs` into `phoecord-proxy`, replace `App::on_*` with `Chain`
- [x] A proxy `Host`: settings from `config.toml`, KV in the data folder, `self_id` from READY, rate-limited `notify`
- [x] `config.toml`: free-form `[plugins.<id>]` sections instead of a field per plugin; keep live reload
- [x] Move `state.rs` (account knowledge) behind `Host`, drop the global `Mutex` held for the whole chain
- [x] CLI: `run`, `launch`, `setup`, `trust`, `untrust`, `plugins`, `enable`, `disable`, `toggle`, `paths`
- [x] `selftest`: REST + Gateway (JSON/zlib, ETF/zstd) against the real Discord without logging in
- [ ] Recorded Gateway payloads without personal data for tests (`tests/fixtures/`)
- [ ] Benchmark: Gateway frame latency through the chain ≤ 1 ms p99 on a 5 MB READY

### 1.2 Built-in plugins on the Plugin API · `0.3`

- [x] `no_track`
- [x] `silent_typing`
- [ ] `unindent`
- [ ] `game_activity_toggle`
- [ ] `relationship_notifier`
- [ ] `fake_nitro` (emoji; stickers and stream quality separately)
- [ ] `message_logger` (cache, deleted and edited, `phoecord log`)
- [ ] New: `clear_urls`, strip tracking parameters from outgoing messages
- [ ] New: PhoeCord slash commands (`/phoecord plugins`, `/phoecord toggle`), a local reply to `POST /interactions`
- [ ] `phoecord import-vencord` via the `vencord` field in manifests
- [ ] Vencord parity, network parts: every 🌐 plugin and the data part of every 🌐 + 🎨 plugin in [`docs/PLUGINS.md`](PLUGINS.md), including the owner's userplugins (tasks 1.2.10–1.2.19)

### 1.3 Dynamic plugin engine (WASM) · `0.4`

- [ ] `wit/phoecord.wit`: the `plugin` world (exports `on-event`, `wants`, `manifest`) and the `host` interface
- [ ] A `wasmtime` backend in `phoecord-plugin-runtime` behind the same `Chain`
- [ ] Limits: per-event epoch timeout, memory limit, disabling a plugin on panic
- [ ] Package format: `plugin.toml` + `plugin.wasm`, a `plugins/` folder in PhoeCord's data, hot loading
- [ ] Rights from the manifest (`events`, `notify`, `kv`): the host never passes a plugin what it did not subscribe to
- [ ] `phoecord-plugin-sdk`: bindings + the `#[phoecord::plugin]` macro, example `examples/hello-plugin`
- [ ] A TypeScript plugin template via `componentize-js`
- [ ] `clear_urls` built as a WASM plugin: proof that an external plugin works without rebuilding PhoeCord

### 1.4 GUI installer prototype · `0.5`

- [ ] `phoecord-installer`: find Stable / PTB / Canary on Windows, macOS, Linux (Flatpak, Snap)
- [ ] Install and remove the CA, autostart the proxy (service / LaunchAgent / systemd user unit)
- [ ] GUI on egui: a client list and "Install", "Uninstall", "Repair" buttons
- [ ] Full rollback: CA, DLL, autostart, folders, one button
- [ ] Releases with `SHA256SUMS` on GitHub Releases (unsigned, like Vencord)

### 1.5 DLL sideloading prototype · `0.6`

- [x] `phoecord-loader` as `version.dll`: forward every export to `System32\version.dll` by full path
- [x] `DllMain` with no work under the loader lock, a worker thread, logging to a file
- [x] Client version detection (exe resources + `.text` hash), the `signatures/*.toml` format
- [x] Inline detours + hook removal on unload
- [ ] Loader ↔ proxy IPC (named pipe): status, settings, events for the UI
- [ ] Reinstall after a Discord update (`app-x.y.z`)
- [ ] Check on today's client: the loader starts and shuts down without a single crash on 3 consecutive versions

### 1.6 GPUI (Zed) research

- [ ] A map of GPUI: `App`, `Window`, elements, how a frame is built, where the theme palette lives
- [ ] A test GPUI app built like Phoenix (release, LTO, strip) as a signature reference
- [ ] Which GPUI functions survive LTO and are recognisable in the binary (strings, panics, `type_name`)
- [ ] How Zed loads themes (JSON) and whether to expect the same from Phoenix
- [ ] Report `docs/research/gpui.md` with hook candidates: palette, settings tab list, message render

### 1.7 Localization: English and Russian · `0.6`

- [x] `phoecord-i18n` helper on Fluent: per-crate `i18n/{en,ru}/main.ftl`, fallback to English
- [ ] `language` in `config.toml`, defaulting to the OS language
- [ ] CLI, notifications and installer strings moved into `.ftl`; Russian translation complete
- [ ] `Host::t()` and per-plugin `.ftl` files (Plugin API minor bump)
- [x] CI check: every `en` key exists in `ru`

**Ready for Phase 2 when:** the network layer is stable on today's Discord, ≥ 8 plugins, a WASM plugin installs without a rebuild, the loader is proven on today's client, the GPUI report is done, the app speaks English and Russian.

---

## Phase 2. Phoenix Canary release

### 2.1 Phoenix reconnaissance · `0.7`

- [ ] Does Phoenix honour `HTTPS_PROXY` / the system proxy; does it trust the system store; does it pin
- [ ] Gateway encoding and compression in Phoenix (ETF? zstd?), new events and fields
- [ ] Choose how to reach the proxy (ARCHITECTURE.md, "Phoenix and the proxy") and implement it
- [ ] Import table: `version.dll` or `winhttp.dll` for sideloading; install path and update scheme
- [ ] `phoecord launch` and the installer find Phoenix

### 2.2 Reverse engineering and AOB signatures · `0.8`

- [ ] A Ghidra (and IDA) project on Phoenix Canary: GPUI symbols imported from the reference build (1.6)
- [ ] Signatures: theme palette, settings tab list, message render, HTTP client creation (if 2.1 needs it)
- [ ] Every signature is unique and verified on two adjacent builds
- [ ] CI check of signatures on `.text` dumps (dumps kept outside the repository)
- [ ] Signature updates without rebuilding the loader; the rule "new Canary build → signatures within 48 hours"

### 2.3 UI hooks and theme engine · `0.9`

- [ ] Theme engine: `theme.toml` → GPUI palette, live switching
- [ ] 3 built-in themes + import of Vencord themes where they translate to a palette
- [ ] A "PhoeCord" tab in the client's settings: enable plugins and edit their settings (English and Russian)
- [ ] `MessageLogger` in the UI: deleted messages struck through, edit history on hover
- [ ] Degradation verified: unknown build → network only, no crashes

### 2.4 Release v1.0 · `1.0`

Scope: every 🌐 and 🌐 + 🎨 plugin of [`docs/PLUGINS.md`](PLUGINS.md) complete (network and on-screen parts), the theme engine, and the 30 most-used 🎨 plugins. The remaining 🎨 plugins follow in 1.x releases until full Vencord parity. The top 10, which must work first:

| # | Plugin | Layer | Status |
|---:|:---|:---|:---|
| 1 | MessageLogger | Network + UI | ⏳ |
| 2 | FakeNitro | Network | ⏳ |
| 3 | NoTrack | Network | ✅ |
| 4 | ClearURLs | Network | ⏳ |
| 5 | SilentTyping | Network | ✅ |
| 6 | ShowHiddenChannels | Network + UI | ⏳ |
| 7 | Translate | Network | ⏳ |
| 8 | PlatformIndicators | Network + UI | ⏳ |
| 9 | PermissionsViewer | UI | ⏳ |
| 10 | ViewIcons | UI | ⏳ |

- [ ] All 10 plugins above work on Phoenix Canary, and the network ones on today's client too
- [ ] The rest of the v1.0 scope above
- [ ] Docs for third-party developers in `docs/plugins/`: quick start (Rust and TypeScript), event reference, rights, publishing, localizing a plugin
- [ ] Plugin API stabilization: `API_VERSION = 1` is frozen, changes are minor-only
- [ ] Installer 1.0 for Windows; macOS and Linux with the network layer
- [ ] Security audit of the CA, the proxy and the loader before release
- [ ] A GitHub release with binaries, `SHA256SUMS` and `docs/CHANGELOG.md`

---

## Later (after 1.0)

- A third-party plugin catalogue with rights and signature checks.
- UI plugins through WASM: declarative elements the loader turns into GPUI.
- The UI layer on macOS and Linux.
- More languages after English and Russian.
- The remaining 🎨 plugins of `docs/PLUGINS.md` until full Vencord parity.
