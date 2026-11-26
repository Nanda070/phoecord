# Quick start

Checked against code: `24e0362`, 26.11.2026.

From an empty folder to a plugin that PhoeCord loads, in Rust or TypeScript. Both build the same plugin as the repository's examples: it logs the message id and channel id of every `MESSAGE_CREATE` with a running count, and never the text. The commands below are the ones the repository's CI runs (`SDK example (wasm32-wasip2)` and `TypeScript template (componentize-js)` in [`ci.yml`](../../.github/workflows/ci.yml)), and the Rust and TypeScript steps were run outside the repository, from a clean folder, when these pages were written.

Back to the [index](README.md).

## What you need

- PhoeCord installed (`phoecord setup`, or the installer window), so that `phoecord paths` works and a proxy or loader runs when Discord does.
- **Rust:** a stable toolchain, 1.88 or newer, and the target: `rustup target add wasm32-wasip2`.
- **TypeScript:** [Node.js](https://nodejs.org/) (LTS) and npm. No Rust needed.

`phoecord paths` prints where the config file, the CA and the **plugins** folder are (and the client folders it knows); you need the plugins one (`<data folder>/plugins`).

---

## Rust

### 1. Create the crate

```sh
cargo new --lib my_plugin
cd my_plugin
```

Replace `Cargo.toml` with:

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2024"
license = "MIT"

[lib]
crate-type = ["cdylib"]

[dependencies]
# The SDK must be named `phoecord`: `#[phoecord::plugin]` expands to paths under that name.
phoecord = { package = "phoecord-plugin-sdk", git = "https://github.com/Nanda070/phoecord" }

[profile.release]
opt-level = "s"
lto = true
strip = true
codegen-units = 1
```

The SDK is not on crates.io (`publish = false`), so you depend on it by `git`. For a reproducible build add `rev = "<commit>"` to the dependency; from the v1.0 release on, a `tag` works too. Inside a clone of the PhoeCord repository you can use `path = "../../crates/phoecord-plugin-sdk"` instead, as [`examples/hello-plugin/Cargo.toml`](../../examples/hello-plugin/Cargo.toml) does.

### 2. Write the plugin

`src/lib.rs` is [`examples/hello-plugin/src/lib.rs`](../../examples/hello-plugin/src/lib.rs) with your own id:

```rust
use phoecord::{Event, Manifest, Outcome, Plugin, host};

struct Hello {
    seen: u64,
}

#[phoecord::plugin]
impl Plugin for Hello {
    fn new() -> Self {
        Hello { seen: 0 }
    }

    fn manifest(&mut self) -> Manifest {
        let mut m = phoecord::manifest(
            "my_plugin",
            "My plugin",
            "Logs the ids of every new message",
        );
        m.authors = vec!["you".into()];
        m
    }

    fn on_event(&mut self, event: Event) -> Outcome {
        if let Event::Dispatch(d) = event
            && d.name == "MESSAGE_CREATE"
        {
            // The payload is the whole gateway frame; the message is under `d`.
            let id = |key| d.payload.id_at(&["d", key]).unwrap_or(0);
            self.seen += 1;
            host::log(&format!(
                "MESSAGE_CREATE #{}: message {} in channel {}",
                self.seen,
                id("id"),
                id("channel_id")
            ));
        }
        Outcome::pass()
    }
}
```

What the SDK gives you (all in [`crates/phoecord-plugin-sdk/src/lib.rs`](../../crates/phoecord-plugin-sdk/src/lib.rs)):

- `trait Plugin` with `new()`, `manifest()`, `on_event(event)` and an optional `wants(kind)` (default: yes). One instance lives as long as the package is loaded, so fields keep state between events.
- `Event`, `Outcome` (`Outcome::pass()`, `changed(tree)`, `swallow()`, `reply(status, body)`), `Tree` with `at`, `str_at` and `id_at` to read payloads by path.
- `phoecord::host::*`: the host calls (`log`, `setting`, `kv_get`, `notify`, `fetch`, `rest`, `t`, `schedule`, …), snake_case. See [Rights](rights.md).
- `phoecord::manifest(id, name, description)`: a network plugin's manifest for API v1.

The `id` you return from `manifest()` must be `snake_case` and equal the folder name in `plugins/`. PhoeCord takes the rest of the manifest (name, version, events, rights) from `plugin.toml`, which wins over what the guest returns.

### 3. Write `plugin.toml`

```toml
id = "my_plugin"
name = "My plugin"
version = "0.1.0"
description = "Logs the ids of every new message"
authors = ["you"]
layers = ["network"]
api = 1

# Only these events reach the plugin.
events = ["dispatch:MESSAGE_CREATE"]
# `log` needs no right, and the plugin calls nothing else.
rights = []
```

This is [`examples/hello-plugin/plugin.toml`](../../examples/hello-plugin/plugin.toml). Every field is in [Publishing](publishing.md); the events in [Events](events.md).

### 4. Build

```sh
cargo build --release --target wasm32-wasip2
```

The component is `target/wasm32-wasip2/release/my_plugin.wasm` (the crate name with `-` turned into `_`). The example is about 60 KB.

### 5. Install and enable

Put `plugin.toml` and the build, renamed to `plugin.wasm`, into one folder and hand it to PhoeCord:

```sh
mkdir package
cp plugin.toml package/
cp target/wasm32-wasip2/release/my_plugin.wasm package/plugin.wasm
phoecord install package
```

(PowerShell: `New-Item -ItemType Directory package`, then `Copy-Item plugin.toml package/` and `Copy-Item target/wasm32-wasip2/release/my_plugin.wasm package/plugin.wasm`.)

`phoecord install` checks the package (the manifest parses, the component loads), lists what the plugin will see and can do, asks, copies it to `plugins/my_plugin/` and **switches it on**. It warns that the package is not from the catalog; that is expected for your own build. `--yes` skips the question (required when there is no terminal).

By hand it is the same: create `plugins/my_plugin/`, put `plugin.toml` and `plugin.wasm` in it, and switch it on in `config.toml` (`phoecord paths` shows where it is):

```toml
[plugins.my_plugin]
enabled = true
```

A plugin the config does not list is off. (`phoecord enable` and `phoecord disable` know only the built-in plugins; for a package use the config, `toggle my_plugin` in `phoecord dev console`, or the installer window's Plugins tab.)

A running PhoeCord loads the package within a moment of the files appearing (file-system notifications with a 50 ms debounce; a 2 s scan takes over if notifications fail), no restart. **Press Ctrl+R in Discord** afterwards: the gateway connection that was open before the plugin existed may be a plain passthrough, and reconnecting makes PhoeCord read it for your plugin.

### 6. See it work

```sh
phoecord dev console --plugin my_plugin
```

attaches to the running proxy and prints the plugin's `log` lines and every event it ran, with its verdict and time. Send yourself a message; you should see `MESSAGE_CREATE #1: message … in channel …`.

### The edit loop

```sh
phoecord dev watch .
```

(run in the crate folder) builds with `cargo build --target wasm32-wasip2 --release` on every source change and copies `plugin.toml`, `plugin.wasm` and `i18n/*.ftl` into `plugins/<id>/`; the running PhoeCord reloads the package in about a tenth of a second and keeps the old version if the new one is broken. `--once` builds and installs once; `--plugins-dir <dir>` installs somewhere else. It does not enable the plugin: do that once, as in step 5.

---

## TypeScript

The template is [`examples/hello-plugin-ts`](../../examples/hello-plugin-ts). It builds a component with [componentize-js](https://github.com/bytecodealliance/ComponentizeJS) against `wit/phoecord.wit`.

### 1. Get the template and the WIT

```sh
git clone https://github.com/Nanda070/phoecord
mkdir my-plugin-ts
cp phoecord/examples/hello-plugin-ts/{package.json,package-lock.json,build.mjs,tsconfig.json,plugin.toml} my-plugin-ts/
cp -r phoecord/examples/hello-plugin-ts/src my-plugin-ts/
mkdir my-plugin-ts/wit
cp phoecord/wit/phoecord.wit my-plugin-ts/wit/
cd my-plugin-ts
```

The template reaches the WIT through `../../wit`, which only exists inside the repository. Outside it, point both places at your copy: in `package.json` the `types` script (`jco guest-types ./wit --world-name plugin -o types`) and in `build.mjs` the `witPath` (`"./wit"`). One `sed` does both:

```sh
sed -i 's#\.\./\.\./wit#./wit#' package.json build.mjs
```

(PowerShell: edit the two lines by hand.) Inside the repository, skip all of this and work in `examples/hello-plugin-ts` directly.

Then set your own id in `plugin.toml` (`id = "my_plugin_ts"`) and in `src/plugin.ts` (`manifest()`).

### 2. The plugin

[`src/plugin.ts`](../../examples/hello-plugin-ts/src/plugin.ts) exports the three functions of the world (WIT names in camelCase):

```ts
import { log } from "phoecord:plugin/host@1.0.0";
import type { ManifestInfo, Kind, PluginEvent, PluginOutcome } from "phoecord:plugin/plugin@1.0.0";
import { idAt } from "./tree";

let seen = 0;

const pass: PluginOutcome = { verdict: { tag: "pass" }, payload: undefined, extra: [] };

export function manifest(): ManifestInfo {
  return {
    id: "my_plugin_ts",
    name: "My plugin",
    version: "0.1.0",
    description: "Logs the ids of every new message",
    authors: ["you"],
    vencord: undefined,
    layers: ["network"],
    api: 1,
  };
}

export function wants(kind: Kind): boolean {
  return kind.tag === "dispatch" && kind.val === "MESSAGE_CREATE";
}

export function onEvent(event: PluginEvent): PluginOutcome {
  if (event.tag !== "dispatch" || event.val.name !== "MESSAGE_CREATE") return pass;
  // The payload is the whole gateway frame; the message is under `d`.
  const id = (key: string) => idAt(event.val.payload, ["d", key]) ?? 0n;
  seen += 1;
  log(`MESSAGE_CREATE #${seen}: message ${id("id")} in channel ${id("channel_id")}`);
  return pass;
}
```

Rules specific to the TypeScript build (they come from [`build.mjs`](../../examples/hello-plugin-ts/build.mjs) and the [template README](../../examples/hello-plugin-ts/README.md)):

- Host calls are imports from `phoecord:plugin/host@1.0.0`. A call that can fail returns its value or throws.
- Payloads are WIT `tree`s (an arena of nodes, root at `nodes[0]`); [`src/tree.ts`](../../examples/hello-plugin-ts/src/tree.ts) reads (`at`, `text`, `idAt`) and converts (`toJs`, `fromJs`). 64-bit integers are `bigint`.
- **No WASI.** `build.mjs` turns off `random`, `clocks`, `stdio`, `http` and `fetch-event`, so there is no `Math.random`, `Date.now`, `console.log` or `fetch()`. Use the host's `log`, `fetch` and `schedule`.
- The component embeds the JavaScript engine: about 12 MB, and about 17 s to compile each time PhoeCord loads it (release build, desktop PC). Hot reload is slower than for Rust.

### 3. Build

```sh
npm ci
npm run build
```

`npm run build` runs `jco guest-types` (TypeScript types for the WIT world into `types/`), `tsc` (type check) and `build.mjs` (esbuild bundle, then `componentize-js`). The result is `plugin.wasm` next to `plugin.toml`.

### 4. Install and test

```sh
mkdir package
cp plugin.toml plugin.wasm package/
phoecord install package
phoecord dev console --plugin my_plugin_ts
```

Then press Ctrl+R in Discord and send a message, as in the Rust steps 5 and 6. `phoecord dev watch` is for Rust crates only (it runs `cargo`); with TypeScript, rebuild and copy `plugin.toml` and `plugin.wasm` into `plugins/<id>/` yourself, or run `phoecord install package --yes` again (an update keeps the plugin's data and whether it was on).

The repository checks the template with `cargo test --release -p phoecord-plugin-runtime --test hello_plugin_ts_wasm -- --ignored --nocapture`, which builds it, loads it as a package and checks the log lines (release, because loading the 12 MB component in a debug build takes minutes).

---

## Testing a plugin

- **Live:** `phoecord dev console --plugin <id>`. Commands (type `help`): `plugins`, `toggle <plugin>`, `reload <plugin>`, `settings <plugin>`, `kv <plugin>`, `events` (the last event of each kind the proxy saw), `replay <event> <plugin>` (the last such event into your plugin, a dry run: what it queues is not delivered), `filter <text>`, `plugin <id>`. The console attaches to a running proxy.
- **Logic:** keep matching and parsing in plain functions that take and return strings, and test them with `cargo test` on your own machine; the SDK builds for the host target, only the `host::*` calls cannot run there. [`examples/plugins/clear_urls`](../../examples/plugins/clear_urls) shows how much can live in such functions.
- **In the repository's tests:** [`crates/phoecord-plugin-runtime/tests/hello_plugin_wasm.rs`](../../crates/phoecord-plugin-runtime/tests/hello_plugin_wasm.rs) builds the Rust example, loads it as a package and feeds it a message; copy its shape if you contribute a plugin to this repository.
- **No network, no token:** tests must never touch the network or need a Discord token.

## When it does not load

| You see | Cause |
|:---|:---|
| `plugin.toml: unknown field …` | the manifest has a field PhoeCord does not know (see the field table in [Publishing](publishing.md); `[settings]` and `[i18n]` tables are not part of it) |
| `plugin.toml: the plugin targets Plugin API v…, this host speaks v1` | wrong `api` |
| `plugin.toml: id … must equal the folder name` | `plugins/<folder>/` is named differently from `id` |
| `the plugin was not built against Plugin API v1 (wit/phoecord.wit); rebuild it with the current phoecord-plugin-sdk` | the component was built for PhoeCord 0.5 or earlier, or against a different WIT: rebuild it |
| `unknown right "…" (known: …)` | a typo in `rights` |
| it loads, but nothing arrives | `events` does not list what you expect (see the filter syntax in [Events](events.md)), the plugin is not enabled, or the gateway connection predates it: Ctrl+R in Discord |
| the log says `this plugin has no `x` right` | add `x` to `rights` |
| the plugin stopped | it was disabled after a trap, a memory limit or repeated time overruns ([Rights](rights.md), "Limits"); fix it and reload the package, or restart |
