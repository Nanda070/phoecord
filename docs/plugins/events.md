# Events

Checked against code: `24e0362`, 26.11.2026.

Everything a plugin reacts to is an **event**: REST calls, gateway frames and, on Phoenix, things the user does in the UI. This page lists each one, when it fires, what it carries, what your answer (the **verdict**) does, and how to subscribe in `plugin.toml`. Back to the [index](README.md).

Sources: [`wit/phoecord.wit`](../../wit/phoecord.wit) (`event`, `event-kind`, `verdict`, `outcome`), [`crates/phoecord-core/src/plugin.rs`](../../crates/phoecord-core/src/plugin.rs) (`Event`, `UiEvent`, `Verdict`, the dispatch names), [`crates/phoecord-plugin-runtime/src/package.rs`](../../crates/phoecord-plugin-runtime/src/package.rs) (filters), [`crates/phoecord-plugin-runtime/src/lib.rs`](../../crates/phoecord-plugin-runtime/src/lib.rs) (`Chain`).

## How an event reaches you

For every event the host walks the plugin chain in load order (built-in plugins first, then packages in folder-name order). A WASM plugin sees it only if all of these hold:

1. the plugin is **enabled** (`[plugins.<id>] enabled = true` in `config.toml`, or installed with `phoecord install`);
2. the event matches a pattern of **`events`** in `plugin.toml` (an empty `events` delivers nothing);
3. your `wants(kind)` returns true (a cheap second filter; the SDK's default says yes);
4. the plugin is not disabled after a fault (a trap, a memory limit, repeated overruns).

Then `on-event(event)` runs and returns an **outcome**: a verdict, an optional replacement payload and extra gateway frames. The chain stops at the first `Drop` or `Reply`; `Changed` from any plugin means the event is re-encoded with the changed payload. A plugin that panics or traps is disabled until restart and counts as `Pass`.

Payloads are **copies** in a WIT `tree` (an arena of nodes, root at `nodes[0]`; see `tree` in the WIT, and `Tree::at`, `str_at` and `id_at` in the Rust SDK). To change an event, put the new tree in `outcome.payload` and answer `changed`. The tree is read only when the verdict is `changed`.

The gateway is only decoded when some enabled plugin may want it. A plugin whose `events` has no `dispatch:` or `send:` pattern never makes PhoeCord decode gateway frames for it (and cannot send from a timer: see [`PHOECORD_TIMER`](#phoecord_timer)).

## The events

| `plugin.toml` kind | WIT case / Rust `Event` | When it fires | Fields | Read-only? |
|:---|:---|:---|:---|:---|
| `rest_request` | `rest-request` / `Event::RestRequest` | The client sent a REST call to Discord, before it leaves | `method`, `route`, `body: option<tree>` | no |
| `rest_response` | `rest-response` / `Event::RestResponse` | Discord answered a REST call, before the client gets it | `method`, `route`, `status: u16`, `body: tree` | no (`Changed` rewrites the body) |
| `dispatch` | `dispatch` / `Event::Dispatch` | A server → client gateway frame with `op 0` arrived; also the synthetic `PHOECORD_TIMER` and `PHOECORD_LOCAL_DELETE` | `name`, `payload: tree` | no |
| `send` | `send` / `Event::Send` | The client sent a gateway frame (identify, presence update, typing …), or a plugin queued one with `emit_send` | `op: s64`, `payload: tree` | no |
| `ui` | `ui(ui-event)` / `Event::Ui(UiEvent)` | The user did something in the Phoenix UI | `name`, `payload: tree` | **yes**: every verdict counts as `Pass` |

`event-kind` (what `wants` receives) is the same without the data: `rest-request(rest-kind { method, route })`, `rest-response(rest-kind)`, `dispatch(string)`, `send(s64)`, `ui(string)`.

The Rust enums `Event`, `EventKind`, `UiEvent`, `Verdict` and `Layer` are `#[non_exhaustive]` in `phoecord-core`. In the WIT they are closed: the five cases above are all there will be in v1, and later UI events are new *names* under `ui`, never new cases. Ignore names you do not know.

### REST requests and responses

- `method` is the HTTP method as the client sent it (`GET`, `POST`, `PATCH`, `DELETE` …); `route` is the path with **no `/api/vN` prefix and no query string**, with ids as they are, e.g. `/channels/123/messages`. Match it with a filter glob or by splitting on `/`.
- The query (`?limit=50&q=…`) is not in `route`. Read it with `request-query` (Rust `host::request_query()`, always allowed) while you handle the event. It can hold what the user typed: do not log it.
- `body` of a request is the JSON body decoded into a tree, `None` when there is none.
- `body` of a response is the JSON answer.

### Gateway: `dispatch` and `send`

- `dispatch`: `name` is the event name from the frame's `t` (`MESSAGE_CREATE`, `READY`, `GUILD_CREATE` …). `payload` is the **whole frame**, `{"op": 0, "t": …, "s": …, "d": {…}}`: read the data under `d`. Keys match in both gateway encodings: `Tree::at(&["d", "author", "id"])` works for JSON and ETF frames alike, and `id_at` reads an id that is a string in JSON and an integer in ETF.
- `send`: `op` is the gateway opcode (3 presence update, 4 voice state update, …) and `payload` the whole frame `{"op": …, "d": …}`. Frames a plugin queued with `emit_send` pass the Send chain once (so other plugins can change or drop them); frames queued while that chain runs are not chained again.
- `outcome.extra` (WIT `list<tree>`): extra frames to emit **after** this dispatch, read only for `dispatch` events. Each is a whole server → client frame.

## Verdicts

| Verdict | Meaning | `rest_request` | `rest_response` | `dispatch` | `send` | `ui` |
|:---|:---|:---|:---|:---|:---|:---|
| `pass` | Not interested, nothing changed | forwarded as is | unchanged | unchanged | unchanged | – |
| `changed` | `outcome.payload` replaces the payload | the request goes out with the new body | the client gets the new body | the client gets the new frame | Discord gets the new frame | ignored |
| `drop` | Swallow it; stops the chain | the call never reaches Discord; the client gets an empty `204` | **no effect**: the chain stops and the client gets Discord's answer unchanged | the client never sees the frame | Discord never gets the frame | ignored |
| `reply(status, body)` | Answer locally; stops the chain | the client gets this `status` and `body` at once; nothing reaches Discord | **no effect**, as `drop` | acts as `drop` | acts as `drop` | ignored |

Notes:

- **`reply` is for REST requests.** It is how a local slash command answers without calling Discord.
- A host that meets a verdict it does not know treats it as `pass`, so a future verdict cannot break a plugin built today.
- `Drop` and `Reply` stop the chain: plugins after you do not see the event. Use them rarely, and only when you really own the outcome.
- To change a response, use `changed`. `drop` does not hide a response from the client.

## `events` filters in `plugin.toml`

```toml
events = [
    "rest_request:POST /channels/*/messages",   # kind:glob
    "dispatch:MESSAGE_*",
    "send:3",
    "ui:menu_action",
    "rest_response",                            # kind alone: every event of the kind
]
```

- Each entry is `kind` or `kind:glob`. Kinds: `rest_request`, `rest_response`, `dispatch`, `send`, `ui`. Another kind is an error and the package does not load.
- The glob alphabet is `*` (any run of characters, also none) and `?` (exactly one character). Matching is case-sensitive and covers the whole subject.
- The subject the glob is matched against:

| Kind | Subject | Example |
|:---|:---|:---|
| `rest_request`, `rest_response` | `"METHOD route"`, with **one space** | `rest_request:POST /channels/*/messages`, `rest_response:GET /users/@me` |
| `dispatch` | the event name | `dispatch:MESSAGE_CREATE`, `dispatch:MESSAGE_*`, `dispatch:PHOECORD_TIMER` |
| `send` | the opcode as a decimal number | `send:3`, `send:*` |
| `ui` | the UI event name | `ui:menu_action` |

- Only the first `:` splits kind from glob, so a glob may contain more colons.
- **A REST filter written `POST:/channels/*/messages` (colon instead of a space) loads without an error and matches nothing**: the subject has a space, so the plugin silently gets no events. If a REST plugin sees nothing, check this first.
- `*` inside a route matches across `/` as well (`/channels/*` also matches `/channels/1/messages`). Anchor with the fixed parts of the path.
- The filter is what the user is asked about at install time ("It will see …"), so make it as narrow as the plugin allows. The host does not hand you events your filter does not list, and it does not decode the gateway at all for a plugin with no `dispatch` or `send` pattern.

## Special dispatch names

Discord never sends these; PhoeCord injects them as ordinary `dispatch` events, so you subscribe with `dispatch:<name>` and `wants(dispatch(name))`.

### `PHOECORD_TIMER`

`TIMER_EVENT = "PHOECORD_TIMER"`. You ask for one with `schedule(after_ms, tag)` (always allowed); when due, **your plugin and no other** gets

```json
{"op": 0, "t": "PHOECORD_TIMER", "d": {"tag": "<the tag you gave>"}}
```

- Delivered only while the plugin is enabled, and only if both `events` lists `dispatch:PHOECORD_TIMER` (or a glob that matches it) and `wants` says yes.
- A delay under 50 ms is raised to 50 ms; more than 24 h is an error; at most 64 timers may be pending per plugin (the 65th is an error). The same tag twice gives two timers. `cancel-schedule(tag)` drops your pending timers with that tag.
- Timers live in memory: a restart forgets them. They are for delays (a pause between deletes, a heartbeat), not for schedules.
- Anything you queue while handling a timer (`emit_send`, `emit_dispatch`) goes out at once. A plugin that sends from a timer should list a `dispatch:` pattern (the timer one counts), otherwise the gateway connection can be a plain passthrough and the send is dropped with a log line.

### `PHOECORD_LOCAL_DELETE`

`LOCAL_DELETE_EVENT = "PHOECORD_LOCAL_DELETE"`. The Phoenix loader sends it when the user deletes a message, **before** Discord confirms (the client's optimistic delete, task 2.3.6; the hook that emits it is tracked in [`docs/phase-2/README.md`](../phase-2/README.md)). The payload is shaped like Discord's `MESSAGE_DELETE`:

```json
{"op": 0, "t": "PHOECORD_LOCAL_DELETE", "d": {"id": "<message>", "channel_id": "<channel>", "guild_id": "<guild>"}}
```

(`guild_id` only in a server.) It is an **observation**: dropping it does not stop the delete, and Discord's own `MESSAGE_DELETE` still follows when the server confirms. A message logger uses it to keep the text of a message the user removed before the server told anyone.

## UI events

UI events come from the Phoenix loader's hooks and are **read-only**: whatever you return counts as `Pass`, so no plugin can stop one reaching the next. They come from the Phoenix loader, so they do not fire where the loader's UI layer is off (on Electron clients it is always off). In Rust they are `Event::Ui(UiEvent { name, payload })`; in TypeScript `{ tag: "ui", val: { name, payload } }`. Always match by `name` and ignore names you do not know: more will be added without a new WIT case.

### `menu_action`

The user clicked one of **your** entries in a message's context menu (task 2.3.5; the hook that emits it is tracked in [`docs/phase-2/README.md`](../phase-2/README.md)).

- Declare the entries in `plugin.toml` with `[[menu]]` (`id`, `label`) and list `"ui:menu_action"` in `events`, otherwise you do not get the clicks. The label is a key of your own `i18n/<lang>.ftl` (see [Localization](localization.md)).
- Payload: `{"entry": "<your menu id>", "channel_id": "<id>", "message_id": "<id>"}`, all strings (ids as strings, as in Discord's JSON). `id_at(&["channel_id"])` parses them to `u64`.
- **Delivered only to the plugin that declared the entry**, never broadcast: no other plugin sees your clicks, and you see no one else's.

```rust
if let Event::Ui(u) = event
    && u.name == "menu_action"
    && u.payload.str_at(&["entry"]) == Some("copy_raw")
{
    let (channel, message) = (u.payload.id_at(&["channel_id"]), u.payload.id_at(&["message_id"]));
    // …
}
```

## Quick reference: which verdict for what

| I want to … | Event | Verdict |
|:---|:---|:---|
| rewrite the text of a message the user sends | `rest_request` `POST /channels/*/messages` | `changed` with the new body |
| answer a command myself, without calling Discord | `rest_request` | `reply(200, body)` |
| stop a call | `rest_request` | `drop` (the client gets `204`) |
| rewrite what Discord sent (profiles, messages, flags) | `rest_response` | `changed` |
| hide or rewrite a live event | `dispatch` | `drop` / `changed` |
| add an event the client sees | `dispatch` | `pass` and `outcome.extra`, or `emit_dispatch` (needs the right) |
| change or suppress presence, typing, voice state | `send` | `changed` / `drop` |
| only observe | any | `pass` |
