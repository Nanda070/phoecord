# Porting a plugin to PhoeCord

Checked against code: `a965f75`, 23.11.2026. Whoever re-checks this guide against the code updates this line (rule in `AGENTS.md`).

A step-by-step guide to moving any plugin (a Vencord built-in, a Vencord userplugin, or your own idea) onto PhoeCord. Follow the steps in order; every step ends with something you can check.

Related: [`PLUGINS.md`](PLUGINS.md) (which plugin goes where), [`ARCHITECTURE.md`](ARCHITECTURE.md) "Plugin API v1", the templates [`_template/code/plugin.rs`](_template/code/plugin.rs) and [`_template/code/plugin.toml`](_template/code/plugin.toml).

> **Status.** Native and WASM plugins work today. `Host::rest` calls Discord's API as the user's own client session and `Host::fetch` calls outside hosts listed in the manifest; on-screen parts wait for Phase 2 UI hooks *(2)*. Write the plugin against the API that exists and leave a `// needs: …` note for the rest.

---

## 0. Licenses

PhoeCord is GPL-3.0-or-later, like Vencord, so Vencord code (and GPL or MIT userplugins) can be reused as is: translate it, copy its regexes, tables and constants. Keep the original copyright line in a comment at the top of the file, which GPL and MIT require.

**Start from Vencord's source by default.** Before writing a plugin, read its Vencord implementation (`src/plugins/<name>/` in the Vencord repository) and take what carries over: which routes and payload fields it changes, regexes, tables and constants (for example ClearURLs' parameter lists), and setting names and defaults, kept 1:1 so `phoecord import-vencord` maps them. React components, webpack patches and CSS do not carry over to GPUI; take their behaviour, not their code. The plugin SDK and templates are MIT, so third-party plugins may use any license.

---

## 1. Decide the layer

Look the plugin up in [`PLUGINS.md`](PLUGINS.md). If it is not there, decide with this table and add a row.

| The plugin… | Layer | Do it |
|:---|:---|:---|
| changes what you send (message text, files, presence, typing, voice state) | 🌐 Network | now, in the proxy |
| changes or hides what you receive (messages, profiles, flags, counts) | 🌐 Network | now, in the proxy |
| adds a command | 🌐 Network | now, as a `/phoecord` command |
| needs an external service (translation, DeArrow, Last.fm) | 🌐 Network | now, with `Host::fetch` (list the host in `fetch = [...]`) |
| shows data in a new place on screen (badge, button, panel, tooltip) | 🌐 + 🎨 | data part now, on-screen part in Phase 2 |
| only changes how the client looks or behaves locally (zoom, keybinds, menus) | 🎨 UI | Phase 2 |
| only changes colours, spacing, fonts | 🖌️ Theme | Phase 2 theme engine |
| works around web, Electron, DevTools or Vencord internals | ⛔ N/A | not ported |

Ask: *"Could a program between the client and Discord do this by changing the traffic?"* If yes, it is network, even if Vencord does it by patching the UI.

**Check:** the plugin has a row in `PLUGINS.md` with a layer and a task.

---

## 2. Map Vencord concepts to PhoeCord

| Vencord | PhoeCord | Notes |
|:---|:---|:---|
| `definePlugin({ name, description, authors })` | `Manifest { id, name, description, authors, vencord, layers, api }` | `id` is `snake_case` of the name; set `vencord: Some("OriginalName")` |
| `definePluginSettings({ … })` | `[plugins.<id>]` in `config.toml`, read with `host.setting("key")` | defaults go in the manifest; native plugins read a missing key as its default |
| `patches: [{ find, replacement }]` | nothing in Phase 1; a UI hook in Phase 2 *(2)* | if the patch only changes data, find the request or event that carries that data instead |
| `FluxDispatcher` events (`MESSAGE_CREATE`, …) | `Event::Dispatch { name, payload, extra }` | same event names as the Gateway |
| `MessageEvents.addPreSendListener` | `Event::RestRequest` on `POST /channels/{id}/messages` | edit `body["content"]`, return `Verdict::Changed` |
| `MessageEvents.addPreEditListener` | `Event::RestRequest` on `PATCH /channels/{id}/messages/{mid}` | |
| `RestAPI` calls you make | `host.rest(method, route, body)` (route is a plain path like `/channels/{id}/messages`) | rate-limited (1/s per plugin, 5/s overall, Discord's own limits honoured); warn the user in the plugin's description if it does bulk work |
| `fetch` to an outside site | `host.fetch(request)` (HTTPS, 5 MB answer cap, no redirects followed) | the host must be listed under `fetch = [...]` in the manifest |
| Gateway sends (presence, voice state) | `Event::Send { op, payload }`, `host.emit_send(op, payload)` | op 3 presence, op 4 voice state |
| `DataStore` / `Settings.plugins.X.store` | `host.kv_get` / `host.kv_set` | small JSON values, per plugin |
| `showNotification` / `Toasts` | `host.notify(title, body)` | rate-limited by the host |
| `sendBotMessage` | `Host::emit_dispatch` with a local `MESSAGE_CREATE` (a message only the user sees) | `crates/phoecord-plugins/src/commands.rs` |
| `UserStore.getCurrentUser()` | `host.self_id()` | more account facts: `host.guild_emojis`, `host.channel_guild` |
| React components, context menus, chat bar buttons | Phase 2 UI hooks *(2)*; until then a `/phoecord` command or a setting | |
| `start()` / `stop()` | nothing: the host calls `wants` / `on_event` only while the plugin is enabled | keep state inside the plugin struct |
| User-facing strings | English in code now; `host.t("key")` with `i18n/{en,ru}.ftl` *(1.7)* | never hard-code Russian or other languages |

**Check:** for every feature of the original you can name the PhoeCord event or `Host` call that does it, or mark it *(2)*.

---

## 3. Write the plugin

### 3.1 Native (built-in) plugin

1. Copy [`_template/code/plugin.rs`](_template/code/plugin.rs) to `crates/phoecord-plugins/src/<id>.rs`.
2. Fill in the `Manifest`.
3. Put the matching rule in **one function** (`fn matches(...)`) and use it in both `wants` and `on_event`, so the cheap filter and the real handler can never disagree.
4. In `on_event`, handle only your events; everything else returns `Verdict::Pass`.
5. Add `pub mod <id>;` and `Box::new(<id>::Plugin::new())` to `builtin()` in `crates/phoecord-plugins/src/lib.rs`.

Which `Verdict` to return:

| You did | Return |
|:---|:---|
| nothing | `Pass` |
| changed the payload in place | `Changed` |
| want the event gone (the other side never sees it) | `Drop` |
| want to answer a REST request yourself | `Reply { status, body }` |

`Drop` and `Reply` stop the chain: later plugins do not see the event. Use them only when that is the point (NoTrack, SilentTyping).

### 3.2 WASM (third-party) plugin

A complete, tested example: [`examples/plugins/clear_urls/`](../examples/plugins/clear_urls/) (build and install steps in its README).

Same logic, different packaging: a crate built for `wasm32-wasip2` against `wit/phoecord.wit` (with `wit-bindgen` until the SDK and its macro land in 1.3.6), plus a `plugin.toml` from [`_template/code/plugin.toml`](_template/code/plugin.toml) listing `events`, `rights` and `fetch` hosts. The host only delivers the events and allows the `Host` calls the manifest lists. Drop the `plugin.toml` + `plugin.wasm` into the `plugins/<id>/` data folder: a running proxy loads it within 2 seconds. Switch it on with `[plugins.<id>] enabled = true` in `config.toml` (`phoecord enable` knows built-ins only for now).

**Check:** the build passes and the proxy log shows the package loaded.

---

## 4. Test it

Tests never touch the network and never need a Discord token.

1. **Matching rule:** unit tests on `matches(...)` with routes or event names that should and should not match.
2. **Behaviour:** run it through a `Chain` with `NullHost` (see the template) and assert the `Outcome` and the changed payload.
3. **Gateway plugins:** use recorded payloads from `tests/fixtures/` *(1.1.9)*, never real user data.
4. **Live check (manual, optional):** `phoecord launch`, switch the plugin on with `phoecord enable <id>`, press Ctrl+R for Gateway plugins, and try it on a test account and a test server.

**Check:** `bash scripts/check.sh` passes.

---

## 5. Finish the PR

- `docs/PLUGINS.md`: set the plugin's "Where" cell to ✅ (or ✅ data part for 🌐 + 🎨).
- `docs/CHANGELOG.md`: one line under "Unreleased".
- The task status in `docs/phase-1/README.md` if it closes one.
- PR title: `feat(plugins): port <PluginName>`.

---

## Worked example: SilentTyping

**What the original does (behaviour only):** while enabled, your typing indicator is never sent; Vencord also offers a chat bar toggle and a `/silenttype` command.

**Layer:** the typing indicator is a REST call (`POST /channels/{id}/typing`), so 🌐 Network. The chat bar button is 🎨 and waits for Phase 2; the toggle works through `phoecord toggle silent_typing` or a `/phoecord` command.

**Mapping:** "block the typing call" → `Event::RestRequest` on that route → `Verdict::Reply { status: 204, body: None }` so the client thinks it succeeded and Discord never hears about it.

```rust
//! SilentTyping: your typing indicator is never sent (Vencord: SilentTyping).

use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Verdict};

pub struct SilentTyping {
    manifest: Manifest,
}

impl SilentTyping {
    pub fn new() -> Self {
        SilentTyping {
            manifest: Manifest {
                id: "silent_typing".into(),
                name: "SilentTyping".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Your typing indicator is never sent".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("SilentTyping".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
            },
        }
    }
}

impl Default for SilentTyping {
    fn default() -> Self {
        Self::new()
    }
}

/// `POST /channels/{id}/typing` and nothing else.
fn matches(method: &str, route: &str) -> bool {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    method == "POST" && matches!(seg.as_slice(), ["channels", _, "typing"])
}

impl Plugin for SilentTyping {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn wants(&self, event: &EventKind) -> bool {
        matches!(event, EventKind::RestRequest { method, route } if matches(method, route))
    }
    fn on_event(&self, _host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::RestRequest { method, route, .. } if matches(method, route) => {
                Verdict::Reply { status: 204, body: None }
            }
            _ => Verdict::Pass,
        }
    }
}
```

Tests: `matches("POST", "/channels/1/typing")` is true; `GET` on the same route and `POST /channels/1/messages` are false; a chain run returns `Outcome::Replied { status: 204, .. }`.

---

## Common pitfalls

- **Route prefixes.** Plugins see routes without `/api/v9` (`/channels/1/messages`). Use `phoecord_core::route` helpers.
- **Snowflakes.** IDs arrive as strings in JSON and as integers in ETF. Use `Val::snowflake()` / `Val::id_at()`, never `as_str()` or `as_i64()` alone.
- **Both encodings.** Never assume JSON: `Val` covers ETF too, and `Val::str_like` keeps a rewritten string in the same shape (atom vs binary).
- **Your own actions.** Plugins that react to removals (RelationshipNotifier) must ignore what the user did themselves.
- **Bulk actions.** Anything that sends many requests goes through `host.rest` and its rate limiter, and its description says it can get an account flagged.
