# Writing PhoeCord plugins

Checked against code: `24e0362`, 26.11.2026. Whoever re-checks these pages against the code updates this line (rule in [`AGENTS.md`](../AGENTS.md)).

Developer documentation for **third-party plugin authors**: people who build a WebAssembly (WASM) plugin that PhoeCord loads from a folder, in Rust, TypeScript or any language that can produce a WASM component. The Plugin API is **frozen at v1** (task 2.4.3): a plugin built today keeps loading in every later 1.x PhoeCord.

If you are porting a Vencord plugin into PhoeCord itself as a native, built-in plugin (Rust code in `crates/phoecord-plugins/`), you want [`PORTING_PLUGINS.md`](../PORTING_PLUGINS.md) instead. The two guides share the same API: the types behind `Event`, `Verdict` and `Host` are the same, a WASM plugin only sees them through the WIT contract.

## Pages

| Page | Read it to |
|:---|:---|
| [Quick start](quick-start.md) | Go from an empty folder to a `.wasm` in `plugins/<id>/` that PhoeCord loads, in Rust or TypeScript; test it with `phoecord dev` |
| [Events](events.md) | See every event, when it fires, its fields, what each verdict does, how `events` filters in `plugin.toml` work, and the dispatch names `PHOECORD_TIMER` and `PHOECORD_LOCAL_DELETE` |
| [Rights](rights.md) | Know what each right in `plugin.toml` grants, which `Host` methods are behind it, the limits (time, memory, rate, fetch hosts) and what the user sees at install time |
| [Publishing](publishing.md) | Lay out a package, fill in `plugin.toml`, version it, understand what the v1 freeze guarantees, and get it into the catalog or into users' hands |
| [Localization](localization.md) | Ship `i18n/<lang>.ftl`, call `Host::t`, label menu entries |
| [Themes](themes.md) | Write a `theme.toml`, the built-in themes, `phoecord theme import` for a Vencord CSS theme |

## The model in one minute

- A plugin is **one WASM component** built against [`wit/phoecord.wit`](../../wit/phoecord.wit) (world `plugin`, package `phoecord:plugin@1.0.0`), plus a **`plugin.toml`** next to it. PhoeCord reads both from `plugins/<id>/` in its data folder (`phoecord paths` shows it).
- The component exports three functions: `manifest()`, `wants(kind)` and `on-event(event)`. PhoeCord calls `on-event` for the traffic you subscribed to in `plugin.toml` (`events`), and you answer with a **verdict**: `Pass`, `Changed`, `Drop` or `Reply`.
- The component has **no sockets, no files, no clock and no WASI at all**. Everything it does to the outside world goes through the `host` interface, and each call needs a **right** listed in `plugin.toml` (`rights`). A call without its right returns an error to the plugin.
- The user sees what you ask for (events and rights) and says yes before the plugin is installed.
- PhoeCord loads, reloads and unloads packages **without a restart**; a broken new version leaves the old one running.

```
my_plugin/
├── plugin.toml        # id, api = 1, events, rights, fetch hosts, [[menu]], [[setting]]
├── plugin.wasm        # the component
└── i18n/              # optional
    ├── en.ftl
    └── ru.ftl
```

## Which language

| | Rust | TypeScript |
|:---|:---|:---|
| Tooling | the SDK (`phoecord-plugin-sdk`) and `#[phoecord::plugin]`, target `wasm32-wasip2` | [componentize-js](https://github.com/bytecodealliance/ComponentizeJS) with `jco`, `tsc`, esbuild |
| Size | tens of kilobytes (the SDK example is under 100 KB) | about 12 MB: a whole JavaScript engine is inside |
| Load time | well under a second | about 17 s per load in a release build on a desktop PC; PhoeCord does not cache compiled plugins yet |
| Good for | plugins that run on every gateway event | plugins that run on a few events, authors who know Vencord's TypeScript |
| Template | [`examples/hello-plugin`](../../examples/hello-plugin) | [`examples/hello-plugin-ts`](../../examples/hello-plugin-ts) |

Any other language works if it can produce a component for the same WIT (Go, C, AssemblyScript): there is no SDK for them, so you generate the bindings yourself from `wit/phoecord.wit`.

## Where things are

| What | Where |
|:---|:---|
| The contract (MIT) | [`wit/phoecord.wit`](../../wit/phoecord.wit); the frozen copy is [`wit/frozen/phoecord-1.0.0.wit`](../../wit/frozen/phoecord-1.0.0.wit) |
| The Rust SDK (MIT) | [`crates/phoecord-plugin-sdk/`](../../crates/phoecord-plugin-sdk/), the macro in [`crates/phoecord-plugin-sdk-macros/`](../../crates/phoecord-plugin-sdk-macros/) |
| The `plugin.toml` parser, the rights table and the format, documented at the top of the file | [`crates/phoecord-plugin-runtime/src/package.rs`](../../crates/phoecord-plugin-runtime/src/package.rs) |
| Time and memory limits | [`crates/phoecord-plugin-runtime/src/wasm.rs`](../../crates/phoecord-plugin-runtime/src/wasm.rs) (`WasmLimits`) |
| Examples | [`examples/hello-plugin`](../../examples/hello-plugin) (Rust), [`examples/hello-plugin-ts`](../../examples/hello-plugin-ts) (TypeScript), [`examples/plugins/clear_urls`](../../examples/plugins/clear_urls) (rewrites message text, uses a setting) |
| The built-in side of the same API | [`crates/phoecord-core/src/plugin.rs`](../../crates/phoecord-core/src/plugin.rs), [`ARCHITECTURE.md`](../ARCHITECTURE.md) "Plugin API v1" |
| Community catalog | [`catalog/README.md`](../../catalog/README.md) |

## Ground rules

- Plugins act on the user's own Discord client, and client mods are against Discord's Terms of Service, as with Vencord. Say so in your plugin's description if it does something that could get an account flagged (`rest` and `emit_send` act as the user).
- Never log message text or what the user typed, and do not log `request_query()`: it can hold search terms. The examples log ids and counts only.
- Ask for the least. A plugin that lists rights it does not use is a reason for the catalog review to ask for changes.
