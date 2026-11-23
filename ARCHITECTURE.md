# PhoeCord architecture

How PhoeCord is built, why it is built this way, and what breaks when Discord ships an update.

---

## The core problem

Vencord patches the JavaScript of Discord's web client: the code arrives as text, so it can be found with a regex and rewritten. Project Phoenix is a native Rust + GPUI binary. It has:

- no JavaScript and no source to patch;
- **no stable ABI**: struct layout, symbol names, inlining and function order change from build to build, and symbols are stripped;
- frequent updates, so anything tied to addresses lives until the next patch.

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

The base is the v0 proxy, which already works with today's Discord; its migration is in the [Phase 1 plan](docs/phase-1/README.md).

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
pub trait Host       { setting, kv_get, kv_set, self_id, notify, log }
```

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

Guest limits: no WASI files or sockets; a per-event timeout (epoch, 5 ms by default); a memory limit (32 MB by default); on a panic or a limit hit the plugin is disabled until restart and the event passes as `Pass`.

Built-in plugins stay native for now (`phoecord-plugins`); once the WASM runtime is stable they move to WASM packages, except those where every percent of CPU matters.

---

## UI layer: the loader

### Getting into the process (Windows)

**DLL sideloading.** Windows looks for DLLs in the application folder first. `phoecord-loader` is built as `version.dll` (or `winhttp.dll`, chosen from Phoenix's import table) and placed next to the client exe.

1. `DllMain(DLL_PROCESS_ATTACH)`: nothing heavy (loader lock). Load the real `version.dll` from `System32` by full path and start a worker thread.
2. All exports (`GetFileVersionInfoW` and so on) forward to the system DLL.
3. Worker thread: check the client version → load the signature set → install hooks → IPC with `phoecord-proxy` (named pipe).
4. Discord's updater puts a new version into a new `app-x.y.z` folder → the installer watches for it and reinstalls the DLL (or installs it at the launcher level).

macOS and Linux come later: `DYLD_INSERT_LIBRARIES` / `LD_PRELOAD` through the PhoeCord launcher.

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

- **GUI toolkit: [egui](https://github.com/emilk/egui) (`eframe`).** Pure Rust, one small static binary, mature on Windows, macOS and Linux, no runtime to install. GPUI was considered for practice before Phase 2, but it is heavier and its API still moves; GPUI knowledge comes from the research stage (1.6) instead. The GUI is a thin layer over `phoecord-installer`, so the toolkit can change later without touching install logic. Cyrillic needs an embedded font with Cyrillic glyphs (egui's default font has none).
- **No code signing.** PhoeCord ships unsigned, like Vencord's installer (Vencord cites a cost of over 300 € a year). Users get a SmartScreen warning on first run; releases carry `SHA256SUMS` from reproducible CI builds and a README note on the warning. Because a proxy `version.dll` next to the client is a pattern antivirus engines watch, the loader is submitted to Microsoft Defender's false-positive portal with each release.

## Localization

The app ships in **English** first; **Russian** is the first translation (ROADMAP 1.7). Docs and code comments stay English-only.

- Format: [Fluent](https://projectfluent.org/) (`fluent-bundle`), one `.ftl` file per crate and language: `crates/<crate>/i18n/{en,ru}/main.ftl`.
- Language choice: `language` in `config.toml`, defaulting to the OS language, falling back to English for any missing key.
- What is translated: CLI output, installer GUI, notifications, the in-client settings tab, slash command descriptions. Log lines stay English.
- Plugins: a plugin may ship its own `i18n/<lang>.ftl`; the host loads it next to `plugin.toml` and exposes `Host::t(key, args)` (API minor bump).
- CI checks that every key in `en` exists in `ru` (missing keys fail the build once 1.7 lands).

---

## Security model

- A local CA per install, name-constrained to Discord's domains; the key never leaves the computer ([`SECURITY.md`](SECURITY.md)).
- The proxy is loopback-only; only hosts on `intercept` are decrypted.
- WASM plugins run in a sandbox with explicit rights in the manifest; native plugins are only built-in ones or ones the user installed by hand.
- The loader loads the real system DLL only by full path from `System32`, so it never becomes a DLL hijacking point itself.
- No telemetry; `NoTrack` is on by default.
