# Rights and limits

Checked against code: `24e0362`, 26.11.2026.

A WASM plugin runs in a sandbox: no WASI, no files, no sockets, no clock. Its only door to the outside is the `host` interface of [`wit/phoecord.wit`](../../wit/phoecord.wit), and most of that door is locked until `plugin.toml` lists the **right** for it. This page lists every right, the `Host` methods behind it, the limits that apply, and what the user sees. Back to the [index](README.md).

Sources: the table at the top of [`package.rs`](../../crates/phoecord-plugin-runtime/src/package.rs), `Right` and `WasmLimits` in [`wasm.rs`](../../crates/phoecord-plugin-runtime/src/wasm.rs), `ProxyHost` in [`crates/phoecord-transport/src/host.rs`](../../crates/phoecord-transport/src/host.rs), `Net` in [`crates/phoecord-proxy/src/hostnet.rs`](../../crates/phoecord-proxy/src/hostnet.rs).

## How rights work

```toml
rights = ["kv", "notify"]
fetch = ["api.example.com"]     # hosts for the `fetch` right; see below
```

- Rights are plain names in `rights`. **None are granted by default.** A name PhoeCord does not know makes the package fail to load (`unknown right "x" (known: kv, notify, …)`).
- A call without its right **returns an error to the plugin** (WIT `result<_, string>`; in TypeScript the call throws) and writes one log line per right: ``this plugin has no `kv` right (add it to `rights` in plugin.toml)``. The plugin keeps running.
- The error text of any failing `Host` call is for logs and people, **not a contract**: do not match on it. A plugin cannot tell a refusal from a failure except by reading the text.
- Which events you receive is a separate list, `events` ([Events](events.md)). Rights are about what you may *do*; events are about what you may *see*.

## The rights

There are nine, in this order in `Right::ALL`:

| Right | Host methods (WIT / Rust) | What it grants |
|:---|:---|:---|
| `kv` | `kv-get`, `kv-set` | A small key/value store of your own that survives restarts |
| `notify` | `notify` | Desktop notifications |
| `emit_send` | `emit-send` | Queue a **client → server** gateway frame: act on the user's gateway session (presence, voice state …) |
| `emit_dispatch` | `emit-dispatch` | Queue a **server → client** frame that only this client sees (a local answer to a slash command) |
| `guilds` | `guild-emojis`, `channel-guild` | Read which custom emoji a server has and which server a channel belongs to |
| `fetch` | `fetch`, `fetch-with` | HTTPS to the outside hosts you list in `fetch = [...]` |
| `rest` | `rest`, `rest-files` | Call Discord's API **as the user** |
| `plugins` | `plugin-enabled`, `set-plugin-enabled` | See and switch the user's other plugins |
| `data` | `append-data` | Append lines to a file in your own folder |

Always allowed, no right needed: `setting`, `self-id`, `log`, `t`, `schedule`, `cancel-schedule`, `request-query`.

In Rust these are `phoecord::host::<name>` with `-` as `_` (`host::kv_get`, `host::emit_send`, `host::set_plugin_enabled` …); in TypeScript they are imports from `phoecord:plugin/host@1.0.0` in camelCase (`kvGet`, `emitSend` …).

### `kv`

- `kv-get(key) -> result<option<tree>, string>`, `kv-set(key, value: tree) -> result<_, string>`.
- Stored per plugin in `plugins/<id>/kv.json` in the data folder, cached in memory, written atomically. Plugins do not share a store. Updating a package keeps it; removing the package deletes it.
- Small JSON-like values; not a database. If saving fails, the host logs it and the call still returns `ok`.
- The user shown: "Remember its own settings and data".

### `notify`

- `notify(title, body)`: a desktop notification.
- **At most 5 per 60 s per plugin**; further ones are dropped silently (the call still returns `ok`). `PHOECORD_NO_NOTIFY` in the environment turns notifications off.
- The user shown: "Show notifications".

### `emit_send`

- `emit-send(op: s64, payload: tree)`: queue a client → server gateway payload (a re-sent presence, a voice state). From a gateway frame it is sent right after that frame; from a REST event or a [timer](events.md#phoecord_timer) it goes to the newest live gateway connection at once.
- Every queued send passes the Send chain once, so other plugins can change or drop it; sends queued while that chain runs are not chained again.
- It acts as the user on Discord: the catalog review looks hard at this right. The user shown: "Send live events as you, such as changing your status".

### `emit_dispatch`

- `emit-dispatch(payload: tree)`: queue a whole server → client frame (`{"op":0,"t":"MESSAGE_CREATE","d":{…}}`) that only this client sees. From a REST event it goes to the open gateway connections at once.
- Nothing reaches Discord. The user shown: "Show made-up events in your Discord, on this computer only".

### `guilds`

- `guild-emojis(guild-id: u64) -> result<list<u64>, string>`: the custom emoji ids of a server the user is in (empty when unknown).
- `channel-guild(channel-id: u64) -> result<option<u64>, string>`: the server a channel belongs to (`none` for DMs and unknown channels).
- Both answer from what the client has already shown PhoeCord (READY, `GUILD_CREATE` …). The user shown: "Read server emoji and which server a channel is in".

### `fetch`

- `fetch(method, url, body: option<tree>) -> result<host-response, string>` and `fetch-with(method, url, headers: list<tuple<string,string>>, body)` with extra request headers.
- Needs the right **and** the host in `fetch = [...]` of `plugin.toml`. Rules:

| Rule | Detail |
|:---|:---|
| Allowed hosts | Exact bare host names, lower case, no scheme, port or path: `"api.example.com"`. A pattern `"*.example.com"` matches **exactly one more label** (`123.example.com`; not `example.com`, not `a.b.example.com`). At least two labels |
| URL | `https://` only, port 443 only, no `user:pass@`, no spaces or control characters |
| Redirects | **Not followed.** A 3xx comes back as the answer; the `location` header is available |
| Credentials | No Discord credentials, no cookies. Refused request headers: `Host`, `Authorization`, `Cookie`, `Proxy-*`, `Connection`, `Content-Length`, `Transfer-Encoding`, `TE`, `Upgrade`, `Keep-Alive`, `Trailer`, and anything with CR or LF |
| Size | Request body at most 1 MB; the answer is capped at 5 MB |
| Time | 15 s for the whole call, connect to last byte |
| Through the user's proxy | When the user set an upstream proxy for PhoeCord (VPN, zapret …), plugin calls take it too |

- The answer, `host-response`, has `status`, `body` (the JSON, if there was one), `text` (when it was not JSON, UTF-8, lossy), `headers` (lower-case names; only `content-type`, `retry-after`, `x-ratelimit-*`, `link`, `location`, never cookies) and `bytes` (the raw body when it was not JSON, e.g. an image).
- The host sets **no rate limit** on `fetch` of its own. Be considerate: a call that waits for a slow server holds your event (the wait does not count toward your time limit, but the user's client waits for your verdict).
- The user shown: "Connect to the sites listed below", and the host list.

### `rest`

- `rest(method, route, body: option<tree>)` and `rest-files(method, route, payload, files: list<file-part { filename, content-type, data }>)`.
- Calls Discord's API **as the user's own client session.** The proxy keeps the client's `Authorization` and client headers in memory only (never on disk, in logs or in a plugin's hands), drops them after 30 minutes without client traffic or on a 401, and sends only to the client's own API host.
- **The route is a plain path** such as `/channels/123/messages?limit=5`, optionally with `/api/v9` in front. Absolute URLs, other hosts, `..`, `//`, backslashes, `#` and whitespace are refused.
- **The client must be signed in** first: until Discord has made an authenticated API request through PhoeCord, calls return an error ("client not signed in yet"). Retry after the first event you see.
- **Limits:** 5 calls per 5 s per plugin (a short burst is fine, 1 a second on average), 5 per second over all plugins, and Discord's own rate-limit buckets (`X-RateLimit-*`, `Retry-After`) are remembered per route. A refusal says `retry in X s`; treat it like a 429.
- Size and time: request body at most 1 MB, answer at most 5 MB, 15 s per call. `rest-files` sends `multipart/form-data` the way the client uploads attachments (`payload_json` + `files[n]`): **at most 10 files and 25 MB** in total.
- **This right acts as the user.** A plugin that sends, edits or deletes messages in bulk can get an account rate-limited or flagged: say so in the description. The user shown: "Use the Discord API as you: send, edit and delete on your behalf".

### `plugins`

- `plugin-enabled(id) -> result<option<bool>, string>`: whether a plugin is switched on in the config (`none` for an unknown id).
- `set-plugin-enabled(id, on) -> result<_, string>`: switch another plugin on or off in `config.toml`; PhoeCord applies it within seconds.
- The user shown: "See and switch your other plugins".

### `data`

- `append-data(file, line) -> result<_, string>`: append one line (plus a newline) to `plugins/<id>/<file>`. `file` is a bare file name: no `/` or `\`, not `..`. It is the only way a plugin writes a file; there is no read-back, and the host enforces no size cap, so rotate or limit what you write.
- The user shown: "Save files in its own folder".

## Limits on the guest

Defaults of `WasmLimits` in `wasm.rs`; they apply to every WASM plugin.

| Limit | Value | What happens |
|:---|:---|:---|
| Soft time limit per event | 50 ms of **guest** time (wall time minus time spent waiting in host calls) | The event still completes and its verdict counts, but it is logged with the measured time and counts as a **strike** |
| Strikes | 3 within 60 s | The plugin is **disabled** |
| Hard ceiling per call | 1 s of guest time (enforced by epoch interruption, about a millisecond accurate) | The guest is stopped and the plugin is **disabled at once** |
| Memory | 32 MB of linear memory per plugin | A failed growth traps, which disables the plugin |
| Trap, panic, bad answer | – | The plugin is disabled and the event counts as `Pass` |
| WASI | none | Any WASI call traps: no files, sockets, clocks, randomness or stdio. The Rust standard library links, but do not use what needs the OS |

A disabled plugin stays off until PhoeCord restarts or a new version of its package is loaded (a changed `plugin.wasm` gets a fresh start). Measured overhead of the WASM path: about 26 µs per event per plugin on a `MESSAGE_CREATE` payload (Windows, release, `cargo bench -p phoecord-plugin-runtime`); a Rust plugin that does little stays far below the 50 ms limit. A JavaScript plugin is slower per call and about 17 s slower to load ([Quick start](quick-start.md)).

Other limits worth knowing: `schedule` (50 ms minimum, 24 h maximum, 64 pending timers per plugin, [details](events.md#phoecord_timer)); payload trees are checked for bounds, depth (at most 256) and single use of every node when you return them, and a malformed tree counts as a bad answer.

## What the user sees

At install time (`phoecord install`, and the installer window's Plugins tab) the user gets a dialog or prompt built from your `plugin.toml`:

```
My plugin 0.1.0 by <author> asks for:
  It will see:
    - Live events from Discord: MESSAGE_CREATE
  It can:
    - Remember its own settings and data (kv)
  It can connect to:
    - api.example.com
Install My plugin? [y/N]
```

- **"It will see"** lists your `events` entries: "Requests your Discord sends", "Answers from Discord", "Live events from Discord", "Live events your Discord sends", each with your glob (or "all of them"). A `ui:` entry is shown as `ui: <name>`.
- **"It can"** lists your `rights`, each with the sentence quoted above and the right's name; "Nothing beyond reading the events above" for none.
- **"It can connect to"** lists `fetch`.
- A package that is not from the catalog gets a warning that nobody reviewed it and its hash is not checked. With no terminal, `phoecord install` refuses unless `--yes` is given.
- For a catalog install, `plugin.toml` may not ask for a right the catalog entry does not list, and the entry's `sha256` must match the download.

The user can turn any plugin off at any time, and `phoecord remove <id>` deletes the package and its data.

Ask for the least: every right is on that dialog, `rest` and `emit_send` (which act as the user) get the closest review, and a plugin that lists rights it never calls is a reason for the review to ask for changes ([`catalog/README.md`](../../catalog/README.md)).
