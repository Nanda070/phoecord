# Phase 1: spec

Checked against code: `6b3a7f5`, 23.11.2026.

**Plan:** [`plan.md`](plan.md) · **Ledger:** [`ledger.md`](ledger.md) · **For people:** [`../README.md`](../README.md)

## 1. Goal

At the end of Phase 1, on today's Discord (Stable, PTB, Canary) on Windows, macOS and Linux:

1. `phoecord setup` + `phoecord launch` (or the GUI installer) gives a working client with **≥ 8 network plugins**: `no_track`, `silent_typing`, `unindent`, `game_activity_toggle`, `relationship_notifier`, `fake_nitro`, `message_logger`, `clear_urls` (+ `/phoecord` commands).
2. `phoecord selftest` passes for JSON/zlib-stream and ETF/zstd-stream.
3. A third-party **WASM plugin** dropped into `plugins/` runs without rebuilding PhoeCord, limited to its manifest's events and rights; `clear_urls` ships as one.
4. Gateway frame latency through the chain is **≤ 1 ms p99** on a 5 MB READY, native and WASM.
5. The app is available in **English and Russian**; every English key has a Russian translation.
6. `phoecord-loader` as `version.dll` starts and unloads cleanly in the current Electron client on **3 consecutive client versions**, finds signatures from `signatures/*.toml`, and talks to the proxy over IPC.
7. `docs/research/gpui.md` names concrete hook candidates (palette, settings tabs, message render) with how to find each in a release binary.
8. Uninstall removes the CA, the loader, autostart and all folders.

## 2. How it works today

### 2.1 In the repository

| What | Where | State |
|:---|:---|:---|
| Plugin API v1 | `crates/phoecord-core/src/plugin.rs:16` (`API_VERSION`), `:47` (`Event`), `:82` (`Verdict`), `:94` (`Host`), `:108` (`Plugin`) | designed, used by one plugin |
| Plugin chain | `crates/phoecord-plugin-runtime/src/lib.rs:45` (`run`), `:78` (`rest_request`), `:98` (`dispatch`), `:120` (`NullHost`) | works, tested; no `RestResponse` / `Send` helpers yet |
| Gateway codecs | `crates/phoecord-proxy/src/codec.rs:32` (`Codec::from_query`), `:72` (`Inflater`), `:139` (`Deflater`) | ported, tested |
| Local CA | `crates/phoecord-proxy/src/ca.rs:18` (`CONSTRAINED_TO`), `:19` (`CA_NAME`), `:61` (`load_or_create`), `:113` (`server_config`) | ported, name-constraint tests pass |
| Trust stores | `crates/phoecord-proxy/src/trust.rs:44` (`install`), `:96` (`uninstall`) | ported; Windows/macOS import fixed |
| Built-in plugins | `crates/phoecord-plugins/src/lib.rs:13` (`builtin`) | `no_track` only |
| AOB scanner | `crates/phoecord-loader/src/aob.rs:24` (`parse`), `:64` (`find_unique`) | works, tested |
| CLI | `crates/phoecord-cli/src/main.rs` | scaffold, prints status |
| Tests | 17 across the workspace | green on 3 OSes in CI |

### 2.2 The v0 codebase (outside this repository)

The v0 proxy is a single crate of ~4,200 lines (tokio, hyper 1, rustls 0.23 with ring, tokio-tungstenite) that already works against today's Discord. Its leaf modules are ported (§2.1); the rest is ported in stage 1.1. Nanda keeps the v0 sources at commit `5ade8b9`.

### 2.3 Migration map

| v0 module | Lines | Goes to | How | Task |
|:---|---:|:---|:---|:---|
| `val.rs`, `etf.rs` | 567 | `phoecord-core` | as is | ✅ |
| `codec.rs`, `ca.rs`, `trust.rs` | 582 | `phoecord-proxy` | as is, CA renamed | ✅ |
| `plugins/mod.rs` (routes) | 216 | `phoecord-core::route` | route helpers only | ✅ |
| `plugins/mod.rs` (trait, `Cx`, `all()`) | | replaced by Plugin API v1 + `Chain` | rewrite | ✅ |
| `proxy.rs` | 115 | `phoecord-proxy::server` | as is, `App` → `Arc<Proxy>` | 1.1.4 |
| `rest.rs` | 206 | `phoecord-proxy::rest` | `App::on_request/on_response` → `Chain::rest_request` / `rest_response` | 1.1.5 |
| `gateway.rs` | 250 | `phoecord-proxy::gateway` | `App::on_dispatch/on_send` → `Chain::dispatch` / `send` | 1.1.6 |
| `app.rs` | 200 | `phoecord-proxy::Proxy` + `ProxyHost` | config reload kept; plugin loop moves to `Chain`; account lock per event | 1.1.1, 1.1.3 |
| `config.rs` | 289 | `phoecord-proxy::config` | one typed field per plugin → free `[plugins.<id>]` tables | 1.1.2 |
| `state.rs` | 244 | `phoecord-proxy::account` | behind `Host` (`self_id`, names for notifications) | 1.1.3 |
| `notify.rs` | 19 | `ProxyHost::notify` | + rate limit, `PHOECORD_NO_NOTIFY` | 1.1.1 |
| `launch.rs` | 88 | `phoecord-installer::launch` | as is | 1.1.7 |
| `main.rs` | 262 | `phoecord-cli` | thin commands over library calls | 1.1.7 |
| `selftest.rs` | 160 | `phoecord-cli::selftest` | as is | 1.1.8 |
| `vencord.rs` | 101 | `phoecord-installer::vencord` | mapping from `Manifest::vencord` instead of a hard-coded table | 1.2.9 |
| `plugins/no_track.rs` | 41 | `phoecord-plugins` | Plugin API v1 | ✅ |
| `plugins/silent_typing.rs` | 42 | `phoecord-plugins` | Plugin API v1 | 1.2.1 |
| `plugins/unindent.rs` | 98 | `phoecord-plugins` | Plugin API v1 | 1.2.2 |
| `plugins/game_activity.rs` | 61 | `phoecord-plugins` | Plugin API v1; re-send status needs `Host` "inject send" (§3.1) | 1.2.3 |
| `plugins/relationship_notifier.rs` | 131 | `phoecord-plugins` | Plugin API v1, `Host::notify` | 1.2.4 |
| `plugins/fake_nitro.rs` | 157 | `phoecord-plugins` | Plugin API v1; needs guild emoji from `Account` via `Host` | 1.2.5 |
| `plugins/message_logger.rs` | 358 | `phoecord-plugins` | Plugin API v1; cache in plugin state, log via `Host::kv_*` or a log file right | 1.2.6 |

Known v0 defects fixed while porting: an unused `PathBuf` import broke `-D warnings` on Windows/macOS (fixed); no MSRV declared while using let-chains (now `rust-version = "1.88"` + MSRV CI job).

## 3. What changes

### 3.1 Plugin API (minor additions only, `API_VERSION` stays 1)

- `Chain::rest_response(host, method, route, status, &Val)` and `Chain::send(host, op, &mut Val)` helpers next to the existing ones.
- `Host::emit_send(op, Val)`: queue a client → server payload (needed by `game_activity_toggle` to re-send presence). The proxy flushes the queue after the current frame.
- `Host::guild_emojis(guild_id) -> Vec<u64>` and `Host::channel_guild(channel_id) -> Option<u64>`: read-only account facts (needed by `fake_nitro`).
- `Host::t(key, args) -> String` (1.7.4).
- Every addition: an `api` PR by Nanda, a CHANGELOG line, the WIT file updated in the same PR once it exists.

### 3.2 Proxy (`phoecord-proxy`)

- `Proxy { config: RwLock<Arc<Config>>, account: Mutex<Account>, chain: Chain, ca, server_tls, upstream_tls, reloads }`.
- `ProxyHost<'a>` implements `Host`, borrowing the config section of the current plugin, the account (locked per event, not per chain) and the plugin's KV directory `data/plugins/<id>/kv.json`.
- `Config`: `listen`, `intercept`, `language`, `plugins: BTreeMap<String, toml::Table>`; `enabled` is a key inside each table; defaults come from each plugin's manifest; `no_track` is on by default. Live reload every 2 s by mtime, as in v0.
- Loopback-only accept, MITM only when `port == 443 && intercepts(host)`, plain tunnel otherwise (unchanged from v0).
- REST: decode the JSON body only if `chain.wants(RestRequest{..})`; `Outcome::Replied` → local response; `Outcome::Changed` → re-serialize the body and fix `Content-Length`.
- Gateway: decode only if any plugin wants any `Dispatch` / `Send`; once a frame changes, compress the rest with our own context (v0 behaviour); `extra` payloads are emitted after the frame with increasing `s` left untouched.
- Errors in one plugin never break the connection: a plugin that panics is disabled for the session and logged.

### 3.3 Runtime (`phoecord-plugin-runtime`)

- `wit/phoecord.wit` mirrors `plugin.rs`: records for `manifest`, variants for `event` / `verdict`, `host` interface; `Val` crosses as a WIT variant tree.
- `WasmPlugin` implements `Plugin`, so `Chain` stays unchanged. One `Store` per plugin, epoch interruption with a 5 ms deadline per event, `StoreLimits` at 32 MB, no WASI filesystem or sockets.
- Packages: `data/plugins/<id>/{plugin.toml, plugin.wasm, i18n/*.ftl}`; the manifest's `events` globs become the plugin's `wants`; `rights` gate `Host` methods (a denied call returns an error to the guest, logged once).
- Hot loading: a watcher on `data/plugins/`; a changed package is reloaded between events.

### 3.4 Installer (`phoecord-installer`)

- `discover() -> Vec<Client>`: Windows `%LOCALAPPDATA%\Discord*\app-*`, macOS `/Applications/Discord*.app`, Linux PATH, Flatpak, Snap.
- `install(client)`, `repair(client)`, `uninstall_all()`; every step idempotent and logged to `data/install.log` so uninstall can undo exactly what install did.
- Autostart: Windows per-user Run key or scheduled task, macOS LaunchAgent, Linux systemd user unit.
- GUI over the same functions, on egui (`eframe`), in its own crate `phoecord-installer-gui`.

### 3.5 Loader (`phoecord-loader`)

- `version.dll` exports generated from the system DLL's export table at build time (`build.rs`), each forwarding to the real DLL loaded from `GetSystemDirectoryW() + "\version.dll"`.
- `DllMain`: only `DisableThreadLibraryCalls` and spawning the worker thread.
- Worker: read the client version (file version resource) and hash `.text`; find the matching `[build."x.y.z"]` in `signatures/*.toml`; resolve each signature with `Pattern::find_unique` (+ `rip` offset for RIP-relative operands); install hooks; open the named pipe `\\.\pipe\phoecord-<user-sid>`.
- Unknown build or any missing signature → that feature is off, the rest runs; never crash the client.

### 3.6 Localization (stage 1.7)

- Crate `phoecord-i18n`: wraps `fluent-bundle`; `Localizer::new(lang)` loads `en` + the requested language from `include_str!` bundles; `t(key, args)`; missing key → English → the key itself, logged once.
- Language: `config.language` if set, else the OS language (`sys-locale`), else `en`.
- Files: `crates/<crate>/i18n/en/main.ftl`, `crates/<crate>/i18n/ru/main.ftl`; plugin packages ship `i18n/<lang>.ftl`.
- Scope: CLI output, notifications, installer GUI, slash command descriptions. Not translated: logs, error chains for bug reports, docs.

## 4. Tests

- Every ported module keeps its v0 tests; every plugin has unit tests for its matching rule and a chain test with `NullHost`.
- Gateway: round-trip tests over `tests/fixtures/*.{json,etf}` for every codec pair; a test that a changed frame mid-stream leaves the client stream decodable.
- Proxy: an in-process test that a non-loopback peer is refused and that a non-intercepted host is tunnelled byte-for-byte.
- Config: defaults, unknown sections preserved, a broken file does not replace the live config.
- WASM: a test guest that panics, one that loops (hits the epoch deadline), one that allocates past the limit, one that calls a denied `Host` method; all disable the plugin and leave the chain working.
- Loader: AOB tests on synthetic buffers (exist); signature-file parsing; forwarding table generated for every export of the system DLL (Windows-only test).
- i18n: every `en` key exists in `ru`; formatting with arguments; fallback order.
- Benchmark (`cargo bench`, not in CI gate): READY 5 MB, MESSAGE_CREATE burst; result in `docs/reports/`.

## 5. Docs

- `ARCHITECTURE.md`: §3.1 API additions; keep the runtime and installer sections in sync with the code; localization details if they change.
- `docs/CODE_MAP.md`: every new module, the config keys and env variables of §3.2, `data/` layout.
- `docs/PROJECT_MAP.md`: plugin statuses.
- `docs/RUNBOOK.md`: remove the *(1.1)* markers as commands land; add WASM and loader failure rows.
- `docs/SECRETS.md`: the code signing key once it exists.
- `ROADMAP.md`, `docs/CHANGELOG.md`, `TASKS.md` (generated) in every task PR.
