# clear_urls: ClearURLs as a WASM plugin

The first example of a third-party PhoeCord plugin: a WebAssembly component that strips tracking
parameters (`utm_*`, `fbclid`, `gclid`, YouTube's `si`, ...) from the links in the messages you send
and edit. It does the same as the built-in `clear_urls` plugin, but you add it **without rebuilding
PhoeCord**, and PhoeCord picks it up **without a restart**.

| File | What it is |
|:---|:---|
| `plugin.toml` | The package manifest: id `clear_urls_wasm`, the two message routes it listens to, no rights |
| `src/lib.rs` | The plugin. Matching logic copied from `crates/phoecord-plugins/src/clear_urls.rs` (by matvee) |
| `../../../wit/phoecord.wit` | The Plugin API the plugin is built against (MIT) |

The id is `clear_urls_wasm` so it does not collide with the built-in `clear_urls` (a built-in always
wins over a package with the same id).

## Build

You need Rust 1.88 or newer.

```sh
rustup target add wasm32-wasip2
cd examples/plugins/clear_urls
cargo build --release --target wasm32-wasip2
```

The result is `target/wasm32-wasip2/release/clear_urls_wasm.wasm` (about 80 KB).

## Install

1. Make the folder `plugins/clear_urls_wasm/` inside PhoeCord's data folder.
2. Copy two files into it, the second one **renamed** to `plugin.wasm`:
   - `plugin.toml`
   - `target/wasm32-wasip2/release/clear_urls_wasm.wasm` as `plugin.wasm`
3. Switch it on in `config.toml`:

   ```toml
   [plugins.clear_urls_wasm]
   enabled = true
   # Optional: more parameters to strip. "ref" (any site), "ref_*" (prefix), "shop.com:tag" (one site).
   extra_rules = ["ref", "shop.com:tag"]
   ```

A running PhoeCord notices the new folder on its next scan. Replacing `plugin.wasm` reloads the
plugin between two events.

## Check it

Send `https://example.com/a?utm_source=x&id=5` in a channel: the message goes out as
`https://example.com/a?id=5`.

The test `crates/phoecord-plugin-runtime/tests/clear_urls_wasm.rs` builds this example, installs it
into a temporary `plugins/` folder and checks it against a `Chain`, including loading it into a
chain that is already running. Run it with `cargo test -p phoecord-plugin-runtime --test clear_urls_wasm`.

## Write your own

Copy this folder, change `id`, `name` and `events` in `plugin.toml`, and edit `src/lib.rs`: it
implements the three exports of `wit/phoecord.wit` (`manifest`, `wants`, `on-event`). A call to a
`Host` method needs the matching entry in `rights` (the table is at the top of
`crates/phoecord-plugin-runtime/src/package.rs`); `setting`, `self-id` and `log` need none.
