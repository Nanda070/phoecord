# Hello: the smallest SDK plugin

Logs the message and channel id of every `MESSAGE_CREATE`, with a running count. It never logs the text.

## Build

```bash
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
```

## Install

Copy `plugin.toml` and `target/wasm32-wasip2/release/hello_plugin.wasm` (renamed to `plugin.wasm`) into the `plugins/hello_plugin/` data folder (`phoecord paths` shows it), then add to `config.toml`:

```toml
[plugins.hello_plugin]
enabled = true
```

A running proxy loads it within 2 seconds. Press Ctrl+R in Discord so the Gateway is read for it.

## Your own plugin

Copy this folder, change the `id` in `plugin.toml` and in `manifest()`, list the `events` you need and the `rights` for the `Host` calls you make. Outside the PhoeCord repository, point the `phoecord` dependency at the SDK by `git` instead of `path`.
