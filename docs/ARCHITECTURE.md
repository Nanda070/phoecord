# PhoeCord architecture

How PhoeCord is built, why it is built this way, and what breaks when Discord ships an update.

---

## The core problem

Vencord patches the JavaScript of Discord's web client: the code arrives as text, so it can be found with a regex and rewritten. Project Phoenix is a native Rust + GPUI binary. It has:

- no JavaScript and no source to patch;
- **no stable ABI**: struct layout, symbol names, inlining and function order change from build to build, and symbols are stripped;
- frequent updates, so anything tied to addresses lives until the next patch.

The UI and theme layers target **Phoenix only**; on today's Electron client PhoeCord uses the network layer alone.

So PhoeCord splits the work into two layers by one rule: **everything that can be done with data is done on the network; we go into the client process only for what is visible on screen.**

| Layer | What it touches | Depends on | Breaks on a Discord update |
|:---|:---|:---|:---|
| **Network** | REST API, Gateway (websocket) | Discord's protocol: public, stable, versioned (`/api/v9`, `v=10`) | Almost never |
| **Theme** | GPUI palette, fonts, spacing | The client's theme structure | Rarely |
| **UI** | The client's own code: settings tab, message rendering | One specific binary build | On every large update; fixed by updating signatures |

---

## Crates

```text
                         phoecord-cli ───────────────┐
                        /     |      \               │
          phoecord-proxy  phoecord-plugin-runtime  phoecord-installer
                 │          │          │                 │
                 │   phoecord-plugins  │ (wasmtime)      │
                  \         │         /                  │
                   └── phoecord-core ┘ ◄─────────────────┘

   phoecord-loader (cdylib, inside the client) ── IPC ──► phoecord-proxy
   phoecord-plugin-sdk (wasm32, used by plugin authors) — no host dependencies
```

| Crate | Responsibility | Must not |
|:---|:---|:---|
| `phoecord-core` | `Val`, ETF, routes, Plugin API | Do I/O or async, depend on our other crates |
| `phoecord-proxy` | TCP/TLS, `CONNECT`, MITM, REST, Gateway, codecs, CA, trust | Know about specific plugins |
| `phoecord-plugin-runtime` | Plugin chain, manifest loading, WASM host, limits | Do network I/O |
| `phoecord-plugin-sdk` | WIT bindings and macro for guests | Depend on `phoecord-core` or host crates |
| `phoecord-plugins` | Built-in native plugins | Bypass `Host` |
| `phoecord-loader` | Proxy DLL, AOB, hooks, theme engine, IPC with the proxy | Be required by network plugins |
| `phoecord-installer` | Client discovery, CA, DLL install/remove, Vencord import | Act without an explicit user action |
| `phoecord-cli` | `phoecord …` commands | Hold logic that is not in a library |

---

## Network layer

The base is the v0 proxy, which already works with today's Discord; its migration is in the [Phase 1 plan](phase-1/README.md).

1. The client sends `CONNECT host:443` to `127.0.0.1:18463`. Non-loopback connections are refused.
2. A host on the `intercept` list (`discord.com`, `*.discord.com`, `discordapp.com`, `gateway*.discord.gg`) → TLS is terminated with a certificate from the local CA, and a separate TLS connection to Discord is opened and verified against `webpki-roots`. Everything else is `copy_bidirectional` without decryption.
3. **REST**: the path is normalized (`/api/v9/x` → `/x`). The JSON body is decoded only if `Chain::wants(EventKind::RestRequest{..})`. `Verdict::Reply` answers locally (that is how `NoTrack` works).
4. **Gateway**: encoding and compression come from the query (`encoding=json|etf`, `compress=zlib-stream|zstd-stream`). Every frame → `Val` → chain → back. Once any frame has been changed, everything after it is compressed with PhoeCord's own context, so the client always sees a valid stream.
5. If no active plugin wants the Gateway, frames pass through without decoding.

| Plugin | Events |
|:---|:---|
| `NoTrack` | `RestRequest` `POST /science`, `/metrics`, `/error-reporting-proxy` → `Reply 204` |
| `SilentTyping` | `RestRequest` `POST /channels/{id}/typing` → `Reply 204` |
| `MessageLogger` | `Dispatch` `MESSAGE_DELETE` / `MESSAGE_UPDATE` + a `MESSAGE_CREATE` cache; deleted messages shown by rewriting the event, in Phase 2 by a UI hook |
| `FakeNitro` | `RestRequest` `POST …/messages` (emoji → links), `Dispatch` `READY` (Nitro flags) |
| `ClearURLs` | `RestRequest` `POST`/`PATCH …/messages`: strip tracking parameters |
| Slash commands | `RestRequest` `POST /interactions` for PhoeCord-prefixed commands → local `Reply`; the command list is merged into `GET …/application-commands` |

### Phoenix and the proxy

The Electron client honours `--proxy-server` and trusts the system store. How Phoenix behaves is not known yet. Options, in order of preference:

1. Phoenix honours `HTTPS_PROXY` and the system store: nothing is needed, `phoecord launch` is enough.
2. It ignores the proxy but trusts the system: the loader sets the proxy at startup (a hook on HTTP client creation).
3. Pinning or its own root set (`webpki-roots` built into the binary): the loader adds the local CA to the process's root set. The name constraint still applies.

The choice follows the research in ROADMAP 2.1.

### Two transports: proxy today, in-process on Phoenix

The plugin chain does not know where an event comes from, so the same `Chain`, native plugins and WASM sandbox run behind two transports:

| | Proxy (today) | In-process (target on Phoenix) |
|:---|:---|:---|
| Where | `phoecord run`: a local proxy on `127.0.0.1` with a per-install CA | Inside `Discord.exe`: the plugin runtime is linked into the loader (`version.dll`) and hooks the client's own network read/write functions next to the UI hooks |
| Clients | Today's Electron client on Windows, macOS, Linux; Phoenix as a fallback | Phoenix on Windows first |
| Needs | A trusted local CA, a port, a background process | No CA, no port, no extra process; VPNs and zapret see only `Discord.exe` |
| Survives client updates | Yes: only Discord's wire protocol matters | No: network hooks need fresh signatures after an update, like UI hooks |
| Install | CA trust + autostart | Put `version.dll` and the config next to the client; uninstall deletes them |

Why not in-process on today's client: Electron's network stack is Chromium's (BoringSSL) and the Gateway lives in the sandboxed renderer process, so hooks there are fragile and plugins could not read their config or data. Why not proxy-only on Phoenix: the loader is needed there anyway for the UI, so moving the network hooks into the same DLL removes the CA, the port and the extra process. Once 2.1 shows what Phoenix's network stack is, in-process becomes Phoenix's main transport and the proxy stays as the fallback when signatures for a new build are missing.

For VPN and zapret users on the proxy transport, the proxy can send its upstream connections through an upstream proxy (SOCKS5 or HTTP), so Discord traffic takes the same tunnel it would without PhoeCord (task 1.1.11).

---

## Plugin API v1

The v0 proxy gave each plugin a trait with eight hooks and direct access to `Config` and `Account`. That cannot cross a sandbox boundary. PhoeCord reduces the contract to three types (`crates/phoecord-core/src/plugin.rs`):

```rust
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &Manifest;
    fn wants(&self, event: &EventKind) -> bool;          // cheap filter: do not decode what nobody needs
    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict;
}

pub enum Event<'a>   { RestRequest{..}, RestResponse{..}, Dispatch{..}, Send{..} }
pub enum Verdict     { Pass, Changed, Drop, Reply { status, body } }
pub trait Host       { setting, kv_get, kv_set, self_id, notify, log, emit_send, emit_dispatch,
                       plugin_enabled, set_plugin_enabled, append_data, guild_emojis, channel_guild, rest, fetch,
                       t, schedule, cancel_schedule, fetch_with }
```

`Host::rest(method, route, body)` calls Discord's API as the user's own client session: the proxy keeps the client's `Authorization` and client headers in memory only (never on disk, in logs or in a plugin's hands; dropped after 30 minutes without client traffic or on a 401), sends only to the client's own API host with a plain route, and applies 5 calls per 5 s per plugin (bursts allowed, 1/s on average), 5/s overall and Discord's own rate-limit buckets; a refusal says `retry in X s`. `Host::schedule(after_ms, tag)` asks for a `PHOECORD_TIMER` dispatch (`d.tag`) for the calling plugin only (`crates/phoecord-proxy/src/timers.rs`: 50 ms minimum, 24 h maximum, 64 pending per plugin, memory only); `cancel_schedule(tag)` drops them. Sends queued outside a gateway frame (REST events, timers) go to the newest live gateway connection at once (`live.rs`), and every plugin-queued send passes the Send chain once (sends queued while chaining are not chained again). A plugin that sends from a timer should want gateway events, otherwise the connection may be a raw passthrough and the send is dropped with a log line. `Host::fetch` calls an outside HTTPS host only if the plugin lists it (`fetch = [...]` in `plugin.toml`, `Manifest.fetch` for built-ins; an entry `*.example.com` matches exactly one more label); `Host::fetch_with` adds the plugin's own headers, refusing `Host`, `Authorization`, `Cookie`, `Proxy-*` and hop-by-hop names and any CR/LF; no Discord credentials, no cookies, no redirects, a 5 MB answer cap. `HostResponse` carries status, selected headers, JSON body or text, and the raw `bytes` when the answer is not JSON (images, GIF frames). `Host::rest_files(method, route, payload, files)` sends files to Discord's API as `multipart/form-data` (`payload_json` + `files[n]`, at most 10 files and 25 MB), with the same session, route rules and rate limits as `rest`. Both go through `upstream` when it is set.

`RestResponse.body` is mutable: a plugin that returns `Changed` makes the proxy send the rewritten JSON. `emit_dispatch` queues a server → client frame only this client sees (used for local slash command answers); from a REST event the proxy broadcasts it to open gateway connections. `append_data` is the only way a plugin writes a file: one line into `data/plugins/<id>/<file>`.

Chain rules (`phoecord-plugin-runtime::Chain`): plugins run in load order; the first `Drop` or `Reply` stops the chain; `Changed` from any plugin means re-encoding.

Versioning: `API_VERSION` in the manifest. A new `Event` variant or `Host` method is minor (old plugins keep working). Changing an existing one is major; the host keeps an adapter for the previous major version for one cycle.

### The `plugin.toml` manifest

```toml
id = "clear_urls"
name = "ClearURLs"
version = "0.1.0"
api = 1
vencord = "ClearURLs"
layers = ["network"]
events = ["rest_request:POST:/channels/*/messages"]   # what the host lets into the plugin

[settings.extra_rules]
type = "list"
default = []

[i18n]                                                 # optional, see "Localization"
name.ru = "ClearURLs"
```

---

### Licensing of plugins

PhoeCord is GPL-3.0-or-later. The plugin SDK (`phoecord-plugin-sdk`), the WIT contract (`wit/`) and the plugin templates are MIT, so a third-party WASM plugin, which only talks to PhoeCord through that contract, may use any license. A plugin that adapts Vencord code is GPL-3.0 itself.

## Dynamic plugins: choosing a runtime

| | WASM (wasmtime, Component Model) | Lua (mlua) | QuickJS (rquickjs) |
|:---|:---|:---|:---|
| Sandbox | Strong: isolated memory, only explicit imports | Weak: the stdlib must be scrubbed | Medium |
| Author languages | Rust, Go, C, AssemblyScript, JS (via componentize-js) | Lua | JS/TS, familiar to Vencord authors |
| Speed on the hot path (every Gateway frame) | Close to native | Good | Worse, GC pauses |
| Limits | Fuel and epoch interruption, memory limits built in | Instruction hooks | Memory limit and interrupt handler |
| Typed contract | WIT → generated bindings for host and guest | None | None (only `.d.ts`) |
| Size | wasmtime ≈ 10–15 MB | ≈ 0.5 MB | ≈ 1 MB |

**Decision (23.11.2026, see `docs/DECISIONS.md`):** the main runtime is **WASM (wasmtime + WIT)**. The contract is written once in `wit/phoecord.wit` and mirrors `phoecord-core::plugin`. JS authors write TypeScript and build a component with `componentize-js`, so a separate QuickJS is not needed. Lua is rejected: too weak a sandbox for code that sees every message.

How it is built (`crates/phoecord-plugin-runtime/src/wasm.rs`, `package.rs`): WIT has no recursive types, so a `Val` crosses the boundary as a `tree`, an index arena whose `nodes[0]` is the root; the host checks bounds, depth and single use of every node a guest returns. Each plugin has its own `Store` on a worker thread, and `host` imports are served over channels, so no `unsafe` is needed. A package is `plugins/<id>/plugin.toml` + `plugin.wasm`; `events` globs become `wants`, and `rights` (`kv`, `notify`, `emit_send`, `emit_dispatch`, `guilds`, `fetch`, `rest`, `plugins` for reading and switching other plugins, `data` for `append_data`; none by default) gate `Host` methods: a denied call returns an error to the guest. The chain has a built-in segment and a dynamic segment synced from the data folder's `plugins/` every 2 s: a new package loads, a removed one unloads, a changed one is swapped in between events, and a broken new version leaves the old one running. A built-in id wins over a package with the same id.

Guest limits: no WASI files or sockets (WASI imports trap); a per-event time limit (wall time minus host calls): over the 50 ms soft limit the event still completes but counts as a strike, and 3 strikes in 60 s disable the plugin; at the 1 s hard ceiling epoch interruption stops the guest and disables it at once. Measured overhead of the WASM path: about 26 µs per event per plugin on a MESSAGE_CREATE payload (Windows, release, `cargo bench -p phoecord-plugin-runtime`); a memory limit (32 MB by default); on a panic or a limit hit the plugin is disabled until restart and the event passes as `Pass`.

Built-in plugins stay native for now (`phoecord-plugins`); once the WASM runtime is stable they move to WASM packages, except those where every percent of CPU matters.

---

## UI layer: the loader

### Getting into the process (Windows)

**DLL sideloading.** Windows looks for DLLs in the application folder first. `phoecord-loader` is built as `version.dll` (or `winhttp.dll`, chosen from Phoenix's import table) and placed next to the client exe.

1. `DllMain(DLL_PROCESS_ATTACH)`: nothing heavy (loader lock). Load the real `version.dll` from `System32` by full path and start a worker thread.
2. All exports (`GetFileVersionInfoW` and so on) forward to the system DLL.
3. Worker thread: check the client version → load the signature set → install hooks → report to `phoecord-proxy` over IPC (see "Loader ↔ proxy IPC").
4. Discord's updater puts a new version into a new `app-x.y.z` folder → `ensure_loader` copies the DLL there on the next `phoecord launch` or autostart (see "Installer and releases", loader step).

macOS and Linux come later: `DYLD_INSERT_LIBRARIES` / `LD_PRELOAD` through the PhoeCord launcher.

### Loader ↔ proxy IPC

A Windows named pipe per user, `\\.\pipe\phoecord-<user SID>`. The proxy (`phoecord run` / `launch`) is the server; the loader, in the main client process only, is the client. On macOS and Linux the proxy side does nothing (the loader is Windows-only).

- **Who may connect.** The pipe's security descriptor is `D:P(A;;GA;;;<your SID>)`: a protected ACL with one entry, generic-all for the user running the proxy. No `Everyone`, no `SYSTEM`, no `Administrators`; remote clients are refused (`PIPE_REJECT_REMOTE_CLIENTS`); the proxy creates it with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a second `phoecord run` gets an error instead of sharing the name. The client opens it with an anonymous impersonation level.
- **Protocol.** JSON lines (UTF-8, newline-terminated), one message per line, each with `v` (protocol version, now 1) and `type`: `hello` (loader version, client build, `.text` hash, pid, detection time), `status` (UI layer on/off and why), `settings.get` / `settings.set` (answered by the proxy with `settings.value`, from the `[loader]` table of `config.toml`; keys are 1-64 characters of letters, digits, `_`, `-`; values are booleans, numbers or strings) and `ui.event` (name + JSON payload; logged at debug level and ignored until Phase 2). A line over 64 KiB, invalid JSON, an unknown `type` or another `v` drops the connection.
- **Where it is defined.** `crates/phoecord-loader/src/ipc.rs` holds the message types and the line codec, with no OS code; the proxy depends on `phoecord-loader` with the `dll` feature off and uses the same types, so there is one definition and the loader never depends on the proxy. The pipe itself and all its `unsafe` are in `crates/phoecord-loader/src/pipe.rs`; the proxy serves it through that safe API (`crates/phoecord-proxy/src/ipc.rs`), which keeps the rule that `unsafe` lives in the loader crate only.
- **The loader never blocks the client.** The worker connects after detecting the build and sends `hello` and `status`. Opening the pipe does not wait; it tries five times (0, 0.5, 2, 5 and 15 s apart) on its own thread and then gives up quietly, with one line in `loader.log`. A client started without a proxy runs as before.
- **The proxy logs one line per loader connection:** `loader connected: build 1.0.1355, hash …, UI layer off (no signatures) (loader 0.5.0, pid …, detected in … ms)`.

### Finding functions

**AOB signatures** (`crates/phoecord-loader/src/aob.rs`): a byte mask with `??` for addresses and offsets. A signature counts as found only if the match is **unique**.

```toml
# signatures/phoenix-canary.toml
[build."0.0.123"]              # version from exe resources + .text hash
theme_palette   = { aob = "48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B D8", rip = 3 }
settings_tabs   = { aob = "...", rip = 3 }
message_render  = { aob = "..." }
```

Process: Ghidra/IDA + the open GPUI (Zed) sources → find the function → make the signature robust (anchor on strings and on calls into GPUI crates, not on Discord's inlined code) → test on a `.text` dump of two adjacent builds. Signatures live apart from the code and update without rebuilding the loader.

### Hooks and themes

- **Hooks**: inline detours (`retour` or our own trampoline), installed with threads suspended, removed on unload.
- **Themes**: GPUI draws from a palette (`Hsla` colours in the theme struct). A hook on fetching the active theme replaces palette values from the user's `theme.toml`. If Phoenix turns out to load themes as data (as Zed does with JSON), no hooks are needed: the theme is just a file.
- **Settings tab**: a hook on building the tab list adds "PhoeCord" with GPUI elements; settings are read and written over IPC to the proxy.
- **Deleted messages**: `MessageLogger` in the proxy marks the event, the UI hook renders it struck through instead of rewriting the text.

### Degradation

| What happened | What PhoeCord does |
|:---|:---|
| Unknown client build | The UI layer stays off, network plugins work, a notice says "UI plugins are waiting for a signature update" |
| A signature is missing or ambiguous | Only the feature that depends on it is disabled |
| A panic in a hook | The hook is removed, the error is logged; the client keeps running |
| The proxy is not running | The loader does not interfere with the client and intercepts nothing |

---

## Installer and releases

- **GUI toolkit: [egui](https://github.com/emilk/egui) (`eframe`).** Pure Rust, one small static binary, mature on Windows, macOS and Linux, no runtime to install. GPUI was considered for practice before Phase 2, but it is heavier and its API still moves; GPUI knowledge comes from the research stage (1.6) instead. The GUI is a thin layer over `phoecord-installer`, so the toolkit can change later without touching install logic. Cyrillic needs an embedded font with Cyrillic glyphs (egui's default font has none). Look: a dark red theme, tabs and motion (egui's `animate_*` helpers with custom easing); the first screen is the terms of use, which must be accepted before anything is installed.
- **Install log and rollback.** Every change outside plain files goes through the `System` trait; each step is appended to `install.log` only after it succeeds, and uninstall undoes the log newest-first, so it never removes what PhoeCord did not create.
- **Loader step (1.5.6, Windows, opt-in).** `phoecord setup --loader` logs the choice (`LoaderWanted`: channel and channel folder) and copies `version.dll` from next to the `phoecord` binary into the newest `app-x.y.z` folder (`phoecord-installer::loader`). Ownership is by content: each copy is logged as `LoaderCopy` with the SHA-256 of what was written; a `version.dll` already in the target whose hash PhoeCord did not record for that file is foreign and is refused, never overwritten. Reconcile: `ensure_loader` runs first in `phoecord run` (the autostart path) and `phoecord launch`, before the client starts, and copies the DLL into a new `app-x.y.z` after a Discord update; it is idempotent, does nothing without the opt-in, and turns every problem into a logged warning instead of failing the command. Uninstall removes every logged copy (old app folders included) that still holds bytes PhoeCord wrote, and keeps a file that was replaced since. On Electron clients the loader only forwards to the system DLL: the UI layer stays off there.
- **No code signing.** PhoeCord ships unsigned, like Vencord's installer (Vencord cites a cost of over 300 € a year). Users get a SmartScreen warning on first run; releases carry `SHA256SUMS` from reproducible CI builds and a README note on the warning. Because a proxy `version.dll` next to the client is a pattern antivirus engines watch, the loader is submitted to Microsoft Defender's false-positive portal with each release.

## Localization

The app ships in **English** first; **Russian** is the first translation (ROADMAP 1.7). Docs and code comments stay English-only.

- Format: [Fluent](https://projectfluent.org/) (`fluent-bundle`), one `.ftl` file per crate and language: `crates/<crate>/i18n/{en,ru}/main.ftl`.
- Language choice: `language` in `config.toml`, defaulting to the OS language, falling back to English for any missing key.
- What is translated: CLI output, installer GUI, notifications, the in-client settings tab, slash command descriptions. Log lines stay English.
- Plugins: `Host::t(key, args) -> String` (additive, `API_VERSION` stays 1; the default returns the key) answers from the calling plugin's own bundle in the user's language (`--lang`, `language`, OS language, `en`), then from its English bundle, then with the key. `args` are string values for `{ $name }` placeholders. A WASM package ships `i18n/<lang>.ftl` next to `plugin.toml` (loaded with the package and on every hot reload; WIT `host.t`, no right needed). A built-in plugin ships `crates/phoecord-plugins/i18n/<lang>/<plugin id>.ftl`, one file per plugin, listed in `phoecord_plugins::I18N`. The proxy keeps them in `Shared::strings` (`phoecord_i18n::PluginStrings`). `/phoecord` answers are built while the `commands` plugin runs, so their keys live in `commands.ftl`.
- CI checks that every key in `en` exists in `ru` (missing keys fail the build once 1.7 lands).

---

## Security model

- A local CA per install, name-constrained to Discord's domains; the key never leaves the computer ([`SECURITY.md`](../SECURITY.md)).
- The proxy is loopback-only; only hosts on `intercept` are decrypted.
- WASM plugins run in a sandbox with explicit rights in the manifest; native plugins are only built-in ones or ones the user installed by hand.
- The loader loads the real system DLL only by full path from `System32`, so it never becomes a DLL hijacking point itself.
- No telemetry; `NoTrack` is on by default.
