# Phase 2: spec

Checked against code: `db479a0`, 25.11.2026.

**Plan:** [`plan.md`](plan.md) · **Ledger:** [`ledger.md`](ledger.md) · **For people:** [`../README.md`](../README.md)

## 1. Goal

At the end of Phase 2, PhoeCord v1.0 ships on Desktop Phoenix (Windows first) and on today's Electron client:

1. The **in-process transport** works on Phoenix: the plugin runtime is linked into the loader, hooks on the HTTP transport (`ReqwestTransport::send` / `poll` vtable) and the Gateway event registry feed `Engine::on_request`, `on_response` and `on_dispatch`; no CA, no port, no extra process.
2. The **proxy** is the fallback transport when signatures for a new Phoenix build are missing, or on today's Electron client. The proxy handles Phoenix's wire format (encoding and compression discovered in 2.1).
3. **Theme engine**: `theme.toml` → GPUI palette values, live switching on save. 3 built-in themes plus import of Vencord themes where they translate to a palette.
4. A **PhoeCord settings tab** inside Phoenix: enable and configure plugins, install and remove WASM packages with rights shown first, in English and Russian.
5. **MessageLogger UI**: deleted messages struck through, edit history on hover, using an element wrapper on `MessageRow::render` (not content rewriting).
6. **Full Vencord parity on Phoenix**: every 🌐 plugin (Phase 1) works; every 🎨 plugin of the top 30 and every panel (settings, plugin list, plugin settings, modals) has a Phoenix implementation. The remaining 🎨 plugins follow in 1.x releases.
7. **Plugin API v1 frozen**: `API_VERSION = 1`; changes after v1.0 are minor-only. Additive UI events for Phoenix hooks are still minor.
8. Third-party developer docs in `docs/plugins/`: quick start (Rust and TypeScript), event reference, rights, publishing, localizing a plugin.
9. **Installer 1.0 for Windows** with the Phoenix loader; macOS and Linux with the network layer.
10. **Security audit** of the CA, the proxy and the loader before release. GitHub release with binaries, `SHA256SUMS` and `docs/CHANGELOG.md`.

## 2. What exists today (Phase 1 deliverables)

| What | Where | State |
|:---|:---|:---|
| Plugin API v1 | `crates/phoecord-core/src/plugin.rs` | stable; `Host` has 18 methods including `rest`, `fetch`, `schedule`, `t` |
| Plugin chain + WASM runtime | `crates/phoecord-plugin-runtime/` | native + WASM segments, hot reload every 2 s, rights, time limits |
| Transport boundary (`Engine`) | `crates/phoecord-transport/` | four entry points, `Effects`, timers; proxy implements it |
| Proxy transport | `crates/phoecord-proxy/` | REST and Gateway through the chain, CA, trust, codecs (JSON/zlib, ETF/zstd), upstream proxy |
| 50+ built-in plugins | `crates/phoecord-plugins/` | network parts of all Vencord 🌐 plugins + owner's userplugins |
| Loader (`version.dll`) | `crates/phoecord-loader/` | export forwarding, AOB scanner, inline detours, IPC with proxy; tested on Electron Canary |
| Installer + GUI | `crates/phoecord-installer/`, `crates/phoecord-installer-gui/` | discovery, CA, autostart, loader placement, GUI with terms and plugins tab |
| CLI | `crates/phoecord-cli/` | `run`, `launch`, `setup`, `trust`, `untrust`, `plugins`, `enable`, `disable`, `paths` |
| Plugin SDK | `crates/phoecord-plugin-sdk/`, `crates/phoecord-plugin-sdk-macros/` | `Plugin` trait, `#[phoecord::plugin]` macro, `examples/hello-plugin` |
| Localization | `crates/phoecord-i18n/`, `crates/*/i18n/` | Fluent, en + ru, `Host::t`, CI check |
| GPUI research | `docs/research/gpui.md` | palette global, settings tab list, message row, reference build |
| Phoenix research | `docs/research/phoenix-desktop.md` | sunbird study: loading model, HTTP transport, gateway registry, Mana UI, locators |
| IPC | `crates/phoecord-loader/src/ipc.rs`, `pipe.rs` | named pipe, JSON lines, `hello`/`status`/`settings`/`ui.event` |

### Open Phase 1 tasks (running in parallel)

1.1.9, 1.2.14 (USRBG/Decor wait for a Phase 2 UI hook), 1.2.15 (FixImagesQuality partial). 1.4.10 and 1.5.9 moved into Phase 2 as 2.4.8 and 2.2.11. These do not block Phase 2.

## 3. What Phase 2 adds

### 3.1 In-process transport

The design is in [`ARCHITECTURE.md`](../../ARCHITECTURE.md), "Two transports". The in-process transport lives inside the loader (`version.dll`) and calls `Engine` directly, without sockets or TLS.

**HTTP hooks.** Desktop Phoenix routes all Discord REST traffic through one `dyn Transport` trait object (`ReqwestTransport::send`). An inline entry hook on `send` intercepts requests above TLS (method, URL, headers, body as Rust types). The response arrives through the future's `poll`, reached via vtable slots in `.rdata`. Replacing a body requires Discord's own allocator (`__rust_alloc` / `__rust_dealloc`), because the client uses a custom `#[global_allocator]`. Hooked requests and responses feed `Engine::on_request` and `Engine::on_response`.

**Gateway hooks.** Desktop Phoenix dispatches decoded gateway events through a static registry of `{name, handler}` entries, organized as per-domain descriptor tables. The registry is read once at start-up and not consulted again, so hooks must be in place before the first frames. Each handler pointer is replaced with a detour that feeds `Engine::on_dispatch`. Outgoing gateway (client → server) is an open question from 2.1; if no send function is found, `Engine::on_send` is a documented gap on the in-process transport and works only through the proxy fallback.

**Loading.** Desktop Phoenix may restrict DLL search to the system directory (sunbird's finding; to be confirmed in 2.1.2). If so, `version.dll` sideloading does not work and a launcher approach is needed: `phoecord launch` starts Phoenix suspended and injects the loader before the client's code runs. Hook timing is critical: all hooks must be in before the first registry read and vtable lookup. This may require synchronous hooking from the loader entry point (under the OS loader lock) or a suspended-thread approach where the launcher holds the main thread until hooks are ready.

### 3.2 Signatures and locators

Phase 1's `signatures/*.toml` (AOB masks with optional `rip`, keyed by version + `.text` hash) is **kept** for simple entry points and hot-fixes. Phase 2 adds **typed code locators** in `phoecord-loader` for structures that need decoding: struct sizes, field offsets, allocator functions, handler ABI. Each locator returns a typed `Found<T>` (unique match or failure). Both follow the existing "unique or off" rule.

A `phoecord check <exe>` command runs every locator against a given binary and exits non-zero on a miss. CI runs it on `.text` dumps kept outside the repository.

### 3.3 UI hooks and theme engine

The in-client UI exists only on Phoenix. PhoeCord calls Discord's own GPUI/Mana builders to construct native elements (see [`phoenix-desktop.md`](../../research/phoenix-desktop.md) §4).

**Theme engine.** A hook on the active theme global replaces palette values (`Hsla` fields in `ThemeColors`) from the user's `theme.toml`. If Phoenix loads themes as data (as Zed does with JSON), no hook is needed. Live switching: change `theme.toml` → palette updates on the next frame.

**Settings tab.** An existing or added settings sidebar page renders a PhoeCord UI: plugin list with on/off switches, per-plugin settings, package install/remove with rights shown. Built with Discord's Mana components.

**Message hooks.** `MessageRow::render` is wrapped to add accessories (deleted-message indicators, edit history) and an optional tint. `MessageContextMenu` is hooked to add plugin action entries. `Store::delete_message` (the client's own optimistic delete) is hooked so MessageLogger sees local deletes.

### 3.4 Installer and release changes

- The installer discovers Phoenix at `%LOCALAPPDATA%\DiscordPhoenix` on Windows, `discord-phoenix` on Linux.
- `phoecord launch --channel phoenix` starts Phoenix; the loader placement depends on the loading path chosen in 2.1.7.
- The installer handles Phoenix updates (new build → loader reinstall or re-injection).
- Installer 1.0 for Windows; macOS and Linux with the network layer only.

## 4. What is not in Phase 2

- UI plugins through WASM (declarative elements the loader turns into GPUI); plugins call Discord's builders through the host.
- The UI layer on macOS and Linux (loader is Windows and Linux x86-64; UI hooks are Windows first).
- A third-party plugin catalogue with signature checks (the Phase 1 catalog task 1.3.11 covers CLI/installer listing only).
- Languages other than English and Russian.
- Android or iOS.
