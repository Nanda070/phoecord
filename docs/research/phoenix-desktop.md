Learned from reading sunbird-tools/sunbird (no license, no code taken), commit `b1189d797fdf73be3c4e6da0ee8ff2c7510c44fe`, read on 2026-11-25.

Checked against code: not applicable (no PhoeCord code touches Phoenix yet); against sunbird `b1189d797fdf73be3c4e6da0ee8ff2c7510c44fe`, 2026-11-25.

# Desktop Phoenix and how a mod plugs into it (task 2.1 prep)

Desktop Phoenix is Discord's native client written in Rust on GPUI: `%LOCALAPPDATA%\DiscordPhoenix\app\Discord.exe` on Windows, `discord-phoenix` on Linux. This note records what sunbird's public repository teaches about it, so that PhoeCord's Phase 2 (stages 2.1 to 2.4 in [`ROADMAP.md`](../ROADMAP.md)) starts from facts instead of guesses. It complements [`gpui.md`](gpui.md), which studied GPUI in general before any Phoenix material was available.

## 0. Ground rules for this note

- **Source and licence.** sunbird (by mantikafasi and Amaan Qureshi) has no licence file, so all rights are reserved. Nothing here is copied: no code, no type definitions, no byte patterns, no prologue encodings, no file excerpts beyond identifiers. Everything is described in our own words. Names that come from Discord's binary (`ReqwestTransport::send`, `MessageRow::render`, `Workspace::render_overlays`) and the names of sunbird's own modules (used only to say where something lives) are the only quoted identifiers. Our implementation must be written independently from this description.
- **The access gate.** Desktop Phoenix is gated per account: it shows a "not available for your account" screen and does not let the login continue. sunbird bypasses that gate. **PhoeCord will not.** Phase 2 starts once Phoenix is available to users officially (the owner's plan). This note says nothing about how the gate works or how it is bypassed.
- **What was not done.** No Discord Phoenix build, no sunbird binary, installer or script was downloaded or run. Everything below is reading sunbird's source and its `AGENTS.md`. Sunbird's own claims are not verified by us; each section marks what a PhoeCord check on an official build still has to confirm (collected in section 9).
- **Scope of sunbird.** It also mods Phoenix on Android and iOS. Those parts (`runtime-mobile`, `ui-mobile`) are out of scope here, except where sunbird's shared notes explain something about the desktop build.

## 1. What sunbird is, in one paragraph

A single loader library (`phoenix.dll` on Windows, loaded through `LD_PRELOAD` on Linux) starts a Rust runtime inside the client. The runtime hooks the client's gateway handlers, its HTTP transport, and the render functions of a few GPUI views, and runs TypeScript plugins in an embedded QuickJS engine. Plugins see a small typed API (`phoenix.d.ts`): gateway hooks with `before` and `after` stages, a settings schema, per-plugin storage, `fetch`, a call into Discord's API with the app's own session, message accessories, message context-menu entries and modal pages. The crates that matter for Desktop are `loader` (entry), `inject` (the Windows launcher), `runtime` (shared engine and API), `runtime-desktop` (all Desktop hooks), `ui-desktop` (tray, hotkeys, dashboard), `sigs` and `sigscan` (locators and their checker).

## 2. Process and loading model

### 2.1 Getting into the process

| Step | What sunbird does | Why |
|:---|:---|:---|
| Why not a sideloaded DLL | sunbird's launcher notes that `Discord.exe` resolves its imports from the system directory only (it sets restricted dependent-load flags), so a DLL placed beside it is never loaded | This is the opposite of our current plan (ARCHITECTURE.md, "Getting into the process (Windows)": `version.dll` next to the exe). It matters for Phoenix and **must be checked on an official build** (section 9) |
| What it does instead (Windows) | The launcher, `sunbird.exe` (module `inject`), starts `Discord.exe` suspended and queues a `LoadLibraryW` call for the loader as an APC on the main thread. The APC runs when the OS loader finishes, before the client's own code | The loader is in the process before the first instruction of Discord's `main` |
| Single instance | The launcher asks the user to quit Discord first, because a second Discord instance hands over to the running one and exits, which would drop the suspended child | A launch-time constraint any launcher of ours has too |
| Launcher self-install | Without arguments it installs itself under `%LOCALAPPDATA%\Sunbird`, adds Start Menu and Desktop shortcuts, and fetches the newest loader DLL from its site before every launch (https only, using the system `curl.exe`). A failed update keeps the DLL already there. It never touches Discord's own files, so Discord's updates do not undo it | "Discord's files untouched" is a good property. Fetching code that is then loaded into Discord needs integrity checks we would add (signatures, section 8) |
| Linux | `LD_PRELOAD` through the launcher. `AppRun` is a shell script, so the preload reaches the client and every process it starts; the loader checks whether this process is the client, and if so removes itself from `LD_PRELOAD` so the helper children (media process, CEF helper, `xdg-open`) never load it | Same idea as our "macOS and Linux later" note, with a per-child cleanup |

### 2.2 Why hooks must be in place before the first frames

The one hard timing fact in sunbird's notes: Desktop Phoenix **reads its gateway event registry and its HTTP vtables during start-up and does not look at them again**. A pointer swapped later, from a worker thread after the client is running, is never consulted. So sunbird splits start-up into two phases (module `runtime-desktop`, functions `prepare` and `start`):

1. `prepare`, called synchronously from the loader's entry point (`DllMain` on Windows, the `.init_array` constructor on Linux): finds every function it needs and installs all hooks (HTTP, registry, render functions). It logs how long it took.
2. `start`, on a separate thread named for the runtime: picks up what `prepare` hooked, starts the engine, loads plugins, starts the tray and the local debug shell.

### 2.3 The loader lock

`DllMain` runs under the OS loader lock, where loading other DLLs, waiting on threads and doing slow work is dangerous. sunbird's answer is to do **only the minimum synchronously** (locate and hook) and push everything else (QuickJS, plugin I/O, the tray, update checks) onto its own thread. It also installs its panic guard: unwinding out of a constructor aborts the host, so the thread entry catches panics and logs them. "A panic inside Discord's process kills the app" is rule one in their style guide; untrusted input read from the client returns an error and never panics.

What this tells us: sunbird accepts a scan and a set of in-place hooks under the lock, because the hooks must exist before Discord's own code runs. Their code builds an index of every call and every RIP-relative `lea` in the executable (tens of MB, roughly a second per full `.text` scan on a file of about 515 MB) once, lets all locators share it, and frees it when the locators are done. Our design note says "nothing heavy in `DllMain`" (ARCHITECTURE.md). For Phoenix that is a conflict to resolve on purpose, not a detail (section 8, "Loader").

### 2.4 Where things live on disk

- The runtime's home sits directly under Phoenix's own data directory (`%LOCALAPPDATA%\DiscordPhoenix\sunbird` on Windows; `<data dir>/discord-phoenix/sunbird` on Linux, where the data dir comes from `LIBDISCORD_DATA_DIR`, then `XDG_DATA_HOME`, then `~/.local/share`). The comment in their loader says the *parent* is meant to equal Phoenix's `store` root, because the store code looks for `store/<account>/cache.sqlite` relative to it. Plugins are in `plugins/` under that home, plugin settings and the debug-shell token beside them.
- Phoenix keeps per-account state in `store/<account>/cache.sqlite`. sunbird's mobile notes describe its tables as a key (`k`) plus a JSON blob (`v`), with message keys ending in the message id padded to 20 digits. That is an Android observation; **Desktop's schema is not documented in what we read**. The plugin API exposes it read-only: SQL against that file, at most 10,000 rows, arguments bound as text.
- No credential is written to disk: sunbird's rules forbid logging or saving request headers, and session headers live in memory only.

## 3. Where the network lives

### 3.1 The HTTP transport

Desktop Phoenix sends **all Discord REST traffic through one trait object**, `dyn Transport`, with one method. The implementation is `ReqwestTransport::send(&self, req)`, which returns a boxed future (`SendFuture`); the response arrives through that future's `poll`. The crate path in the binary is `libdiscord/crates/http`. That makes the client's HTTP layer plain Rust types (method, URL, header map, body bytes) **above TLS**: nothing about certificates, pinning or the system proxy matters to an in-process hook.

How sunbird intercepts it (module `runtime-desktop::discord`, locator in `sigs::http`):

- **Locating, without fixed addresses.** `send` is found from a unique error string inside it (the one for being handed a mobile host-file body). Its future's vtable is found from the small callee that boxes the future (or from `send` itself), accepted only if the vtable passes Rust trait-vtable shape checks (drop glue, size, alignment, method slot). All `.rdata` slots holding `send` and `poll` are collected, because the linker can emit several copies of a vtable across codegen units.
- **Two kinds of interception.** The API client calls `send` *directly*, not through the vtable, so `send` gets an inline entry hook with a trampoline back to the original. `poll` is only reached through vtables, so its `.rdata` slots are rewritten (a pointer-word swap, no inline patch).
- **Reading and rewriting.** The detours decode the client's own `String`, `Vec<u8>`, `http::Method` and `http::HeaderMap` layouts. For header names they carry a table of the `http` crate's standard header names in discriminant order, matching the compiled crate. **Replacing a body must use the client's own allocator**: Discord links a custom `#[global_allocator]`, so the runtime locates the host's `__rust_alloc` and `__rust_dealloc` (by following `send` to the helper that boxes the request, by the shape of the allocator functions, and by the allocator shims' pattern) and frees and allocates through them. A body built with our allocator would corrupt Discord's heap.
- **Plugin hooks on top.** Request and response filters (by method and path), then the same chain. There is **no composer hook on Desktop**, so the `onSend` plugin hook runs on the request that posts a message (`POST /channels/<id>/messages`), before the generic request hooks.
- **Plugin-originated HTTP** does not use Discord's transport. `phoenix.fetch` goes through WinHTTP on Windows (system proxy, certificates, VPN) and a `dlopen`ed libcurl on Linux, so no TLS library is bundled. `phoenix.discord` sends requests to Discord's API through the app's own transport with the app's captured session headers, and rejects until the app has made a request of its own (that is how it learns the session).

### 3.2 The gateway event registry

The gateway is **not read from the socket**. Phoenix decodes frames itself, then dispatches each event by name through a static registry of `{name, handler}` entries. sunbird hooks that registry. This matters for PhoeCord: we do not need to care about the wire encoding or compression, because the plugin sees the **already-decoded dispatch JSON** (`name` and `data` as borrowed UTF-8).

- **How handlers are found.** The registry is walked at load time by event name, starting from a seed event every build has. The set of events is never compiled into sunbird: a new build brings its own. On Desktop the registry is not one flat table. It is a table of per-domain descriptors (a name, a domain id and a slice of event entries each), and some session-level reducers (`READY_SUPPLEMENTAL`, `RESUMED` and similar) sit in a separate slice found from a second seed. Linkers can emit duplicate copies of the table across codegen units, so an event may have several slots, all swapped.
- **How handlers are swapped.** Every registry entry's handler pointer is replaced with one detour. Phoenix looks handlers up in the registry on each dispatch, so a pointer swap catches every event without code patching, and an event no plugin listens to costs one atomic load. The original handler is kept per event.
- **The handler ABI.** A handler takes `(store, event, ctx)` and returns a Rust `Result` through a hidden out-pointer; on Desktop that result is 12 words (96 bytes) and its `Ok` variant is recognised by a niche value in the first word. This differs between Desktop, mobile 64-bit and 32-bit builds, and sunbird keeps it as per-platform constants checked by the locators.
- **Hook semantics.** A plugin's `before` runs ahead of Phoenix's handler and may return a replacement payload or drop the event; `after` runs afterwards and learns whether the handler succeeded. A replacement is re-serialized with `JSON.stringify`, so integers over 2^53 lose precision (a known limit they document, and one our `Engine` avoids by design if payloads stay as raw JSON text).
- **Feeding events back.** A plugin can run a payload through Phoenix's own handler without running other plugins (`dispatch`, only while a gateway hook is running), or queue one (`enqueue`) that is applied on the store thread before the next event Phoenix handles. This only works inside a handler call, because only a handler call carries the `store` pointer.
- **READY is applied outside the registry.** sunbird's plugin API documents `READY` as applied by the session, not through the registry: `after` hooks run before the app applies it, and a hook cannot drop it. (The sentence comes from the shared plugin API and from the mobile notes; we have not confirmed it for the Desktop build. See section 9.)
- **Typed handles.** Plugins get opaque handles for events (`phoenix.gateway.messageCreate`), not event-name strings. The list of events is read from the running binary, never from a hand-written list.

What we did not see: any hook on **client → server** gateway frames. The API has hooks for server → client dispatch and for REST, plus `enqueue` to feed the client. Whether Phoenix has a comparable outgoing registry or send function is open (section 9). `Engine::on_send` in PhoeCord has no source on Phoenix until we find one.

## 4. The UI: GPUI and the Mana design system, compiled into the exe

### 4.1 The approach

sunbird **does not link its own GPUI and does not use a webview for in-client UI**. Discord's GPUI and its Mana component layer (crates named `mana_gpui` and `mana_html::hosted` inside the binary) are compiled into `Discord.exe`. The runtime finds Discord's own builders for `Div`, `Switch`, `Button`, `TextBlock`, `ActionRow`, `TextInput`, `TextArea`, `Modal` and `MenuItem`, and **calls them** from its hooks to construct native elements that are indistinguishable from Discord's. Visually it inherits the Mana look for free; the cost is that every struct layout and every builder signature is a property of one build.

The techniques that make this tractable, in sunbird's own order of importance:

- **Anchors are strings.** Panic `Location` data, `type_name` strings, element ids (`message-content-`, `account-loading`, `modal-layer-scrim`, `nav-overlay-popout-layer`, `layer-trap-` plus `settings-layer`, `backdrop`) and tracing callsite names (`media_fetch`) lead to unique functions; call graphs and call counts then separate the right function from its look-alikes (for example `Modal::size` is "the byte most often set right after `Modal::new`", and `Modal::subtitle` is "the small setter with by far the most callers").
- **Layouts are read from the binary.** Field offsets of `StyleRefinement` come from its derived `Debug` implementation (field names are printed there, so each name has a pointer in the binary). Sizes of `Div`, `Switch`, `Button`, `AnyElement`, `Entity<T>` and the `Modal` struct are decoded, and each constructor is **checked against a fixed scratch buffer** (a few constants in `sigs`: the maximum sizes of a `Div`, of other Mana structs, of an `AnyElement`). A layout that does not decode turns the feature off.
- **Ownership across the boundary.** A `SharedString` is built in its static variant, so no allocator or reference count is involved; dynamic strings go through a process-lifetime interner (bounded, leaked on purpose). Click handlers are `Rc<dyn Fn>` objects allocated on the host's heap so Discord's own `Rc::drop` frees them. Callbacks are written in the Rust ABI, not `extern "C"`, because Linux's C ABI assumes the caller widened small arguments and Rust callers do not.
- **Redrawing from another thread.** GPUI redraws a window only on input or when notified. sunbird forces a frame: on Windows by posting GPUI's own forced-update window message (`WM_GPUI_FORCE_UPDATE_WINDOW`) with the validation token GPUI keeps in the window state (found from the window procedure), on Linux Wayland by moving the window's frame-request status and pinging the event loop. A hook on the per-frame path sets the window's force-render flag so every view, cached ones included, re-renders.

### 4.2 What it builds, and where it hooks

| Feature | Where sunbird hooks | Notes |
|:---|:---|:---|
| **Settings page** | One of Discord's existing, normally hidden settings pages in the sidebar is relabeled and forced visible; `host::section`, which renders a page, is hooked so that when that page is active it returns a native element tree into the scroll viewport (category tabs, rounded bordered cards, switches, collapsible plugin settings, native selects) | Reuses an existing sidebar slot instead of adding one. Everything it needs (label strings, the visibility check, the function that renders pages, the field holding the active page) is found by string and shape |
| **Modal overlay** (pages, choose dialogs, code viewer and editor) | `WindowRoot::render` (found through `modal-layer-scrim`) and `Workspace::render_overlays` (`nav-overlay-popout-layer`) are hooked and Discord's own `Modal` is composed on top; the Settings layer is a separate render (`settings-layer`) so a modal opened from Settings is drawn inside it | GPUI renders the workspace and the settings layers independently, each replaying its last frame until notified, so deciding per frame which layer owns the modal made it flicker: they pin it for as long as Settings is open. Click-outside-to-close copies how Discord's own backdrop wires `on_mouse_down` (a press goes front to back in capture, then back to front in bubble, so the card sees it first). `Div::id` is required for wheel scrolling |
| **Text input and text area** | Mana `FieldState` creation and the hosted `TextInput`/`TextArea` constructors, found by their shared tail (a function ending in three `None` stores) | Enough for a URL box and a plugin code editor |
| **Message accessories and tint** | `MessageRow::render` is hooked. After Discord renders the row, the returned element is wrapped in a native `Div` with extra text lines under it and an optional tinted left bar. `build_message_body`, called once per row, is hooked to recolour the text | Never modifies the message content. Needs the message id from the row object (its offset is decoded from the code just before the `message-content-` format string is used). A cache miss queues a lookup and a repaint |
| **Message context menu** | The `MessageContextMenu` item-list builder, called by both the right-click menu and the `...` action popover. The hook reads message and channel ids from its input before Discord consumes it, then appends a separator and `MenuItem::Action` entries with native click callbacks | GPUI rebuilds an open menu every frame, so the message JSON is read once (from the client's memory) and cached for a short time. Icons map to Discord's Mana icon enum values seen in this menu |
| **Message JSON** | `serde_json::to_value::<Message>` (the function Phoenix runs to store a message, found by the field names it writes) is called on a live message, and the returned `Value` is read in place; object keys keep insertion order, so the JSON keeps Phoenix's field order | Gives plugins the message even when Phoenix has not cached it |
| **Optimistic delete** | `Store::delete_message` (the client's own delete, which skips the `MESSAGE_DELETE` path) is hooked so plugins' `before` hooks run for deletes the user makes in this client | The reason a logger-style plugin sees local deletes |
| **Custom badge icons** | `libdiscord_media::fetch` is hooked so a made-up badge URL is unwrapped to a real image | Needed because images do not pass through the REST transport |

### 4.3 Tray, dashboard and the rest of the host side

`ui-desktop` is deliberately outside Discord's UI. It provides a tray icon with a menu (open settings, reload plugins, check for updates), a global hotkey, native toasts and notifications, a native file picker for a `.js` plugin, and a **self-contained HTML dashboard served from a loopback HTTP server** (the same server as the debug shell) for plugin management, catalog installs and a JS console. The dashboard opens in a Chromium `--app` window when a Chromium-family browser exists, else the system browser. A subclass on Discord's main GPUI window handles `Ctrl+Shift+I` while Discord is focused, which avoids registering a global hotkey that would steal the key from browsers and editors. On Linux the tray is a small helper process (AppIndicator through the system Python's GTK) so the client's process loads no GTK. Dialogs that need native look (`choose`, `showSheet`) are routed from the dashboard back into the in-client modal.

The loopback server is an exposure: `/eval` runs JavaScript with the user's Discord session. sunbird's desktop build keeps it on and protects it with a token file written at start; on mobile it is off by default. We should not copy that default (section 8).

## 5. How functions are located without fixed addresses

sunbird's rule: **a locator is code, it returns a typed result, and `check` runs every locator against each new build in CI.** Concretely (crate `sigs`):

1. **Locators return `Found<T>`**: the single match or the count it found. Zero and several are both failures, and only the feature that needed the locator is disabled; the runtime never falls back to an offset seen on one build ("when it does not decode, leave that feature off").
2. **Anchors**: strings (including Rust panic messages, format templates and element ids), tracing metadata, `type_name` strings, vtable shape, `.pdata` function bounds on Windows.
3. **Call graph**: one index of "who calls whom" and "who loads which address" is built once and shared. Locators intersect callers of two helpers (that is how `MessageRow::render` is the single function calling both helpers that format `message-content-{id}`), follow a small number of calls from an anchor, or pick the target with the most callers or most post-constructor stores by a clear margin (a winner must beat the runner-up by a factor).
4. **Instruction shapes**: small decoders for the few instruction forms needed (a RIP-relative `lea` of a string, a `mov` of a small constant into an argument register, a store of a field offset, a call target). Decoders handle both Windows x64 and System V, because both builds compile the same Rust; what differs (argument registers, prologues, GOT calls on ELF, packed string literals on ELF) is isolated behind an `Abi` value.
5. **Hook sites are checked, not assumed.** An inline entry hook needs a prologue made of whole instructions with no RIP-relative operand and enough bytes for a jump; sunbird classifies the prologues of the functions it hooks (a few families, such as "pushes plus stack-frame reservation") and refuses a function that does not fit. The three families are written as their own types, not as copied byte strings. (We deliberately do not record their patterns.)
6. **`check`.** The `sigscan` tool runs every locator for a platform against a given binary and exits non-zero on a miss; a CI workflow runs it against current Discord builds when `sigs`, the scanner or the lock file changes. The same tool dumps a registry, searches strings, lists references and disassembles. The loop for a new build is: run `check`, find which locators missed, look up the anchor string or neighbouring function, repair the locator.
7. **Interactive aid.** A debug console (not available to plugins) can read memory, search strings and references, and install a logging hook at an address that logs its arguments and caller. This is how new anchors get found.

Compared with PhoeCord's `signatures/*.toml` (see [`../../signatures/README.md`](../../signatures/README.md)): ours is **data**, an AOB mask with optional `rip`, keyed by version and `.text` hash, unique-match-only, no rebuild to update. sunbird's is **code**, so it survives layout changes that a byte mask cannot (an instruction reordered, a register changed) and can decode structure sizes, but needs a rebuild per breakage. Their experience says a mask is a bad fit for the many things that need *decoding* (struct layouts, which field is which, the allocator), while a mask is cheap to hot-fix. The practical shape for us is probably both (section 8).

## 6. Plugin runtime

TypeScript plugins in QuickJS (no `URLSearchParams`, no other web globals promised). The runtime is a shared crate with a `Platform` trait the Desktop crate implements (HTTP, the SQL read, the Discord call, UI patch API, console extras, plugin unload, tray). A bundled plugin is a single `.js` with leading comment lines for metadata. Plugins get: opaque event handles, `before` and `after` hooks, a settings schema (switch, text, secret, select, number, button) whose `secret` type is scrubbed from log lines, per-plugin JSON storage, `onUnload` for cleanup, a local `fetch`, Discord's API with the app's session, message accessories, message actions, profile actions, pages, toasts, slash commands (mobile). Hot reload takes about 200 ms in their dev shell. Their API reference is a `.d.ts` kept in sync with the Rust.

The point that carries over to our design: **the plugin API is thin and typed; opaque handles instead of strings; all UI and patch targets live in Rust**.

## 7. Comparison

| | sunbird has | PhoeCord has | PhoeCord plan (and the gap) |
|:---|:---|:---|:---|
| Getting into the process | Launcher starts the client suspended and queues the loader as an APC; no files beside Discord; Linux preload that strips itself from child processes | Proxy `version.dll` design with forwarding exports (ARCHITECTURE.md); not tested on Phoenix | **Gap.** If the client restricts DLL search, the sideload does not work. Add a launcher path (2.1). `phoecord launch` already exists for the proxy; it can start Phoenix |
| Timing | All hooks installed synchronously from the loader entry, before Discord's code; engine started on a thread | "Nothing heavy in DllMain"; worker thread installs hooks (IPC doc) | **Gap.** Phoenix reads registry and vtables once. A worker-thread install is too late. We need an agreed way to have hooks in before the first frames (section 8, "Loader") |
| Transport boundary | One runtime inside the client; plugins call Rust through QuickJS | `phoecord-transport::Engine` with four entry points; proxy implements it (1.1.12) | **Fit.** In-process transport calls `on_request`, `on_response`, `on_dispatch`. Missing source for `on_send` |
| REST interception | Entry hook on `ReqwestTransport::send` plus `poll` vtable swaps; host allocator for replaced bodies; Discord API with the app's session | None on Phoenix | Locator and hook (2.2). Prefer the same split (inline for direct calls, vtable words for poll). Allocator discovery is the risky part |
| Gateway interception | Registry walk by name, per-domain tables, duplicates, all handlers swapped to one detour; typed handles | Proxy sees frames on the wire (zlib-stream, ETF/JSON) | Different layer: registry hands decoded JSON, so **no wire decoding needed in-process**. `on_dispatch` fits as is |
| Outgoing gateway | None seen | `on_send` | Unknown. Needs 2.1 research |
| UI building | Calls Discord's own Mana/GPUI builders; layouts decoded from the binary; buffers checked | Reference GPUI build and notes (gpui.md); no Phoenix material | We will use Discord's builders too (we do not link GPUI). The decoding machinery is the main cost (2.2/2.3) |
| Settings page | Reuses a hidden sidebar page, hooks `host::section` | Plan: a "PhoeCord" tab (2.3) | Same approach is likely; evaluate adding a page versus taking over a slot |
| Message row | `MessageRow::render` wrapper; `build_message_body`; message id decoded | Plan: MessageLogger UI (2.3) | Same hook; need a diff-free way to render "deleted" state (wrap element, not content) |
| Context menu | `MessageContextMenu` builder hook | Not planned in ROADMAP | Add to 2.3 for plugin actions |
| Optimistic delete | Hook on `Store::delete_message` | MessageLogger network part (1.2.6) | **Gap.** Local deletes bypass `MESSAGE_DELETE`; MessageLogger on Phoenix needs this hook |
| Theme | None (no palette hook found) | Theme engine plan, palette research | Nothing to learn from sunbird; stays our research (gpui.md section 4) |
| Locators | Code, typed `Found`, shared call index, ABI abstraction, per-build `check` in CI | Data: TOML AOB with `rip`, version plus `.text` hash, unique-only; loader logs build and hash | Data stays for hot-fixes; add code locators for decoded structure; add a `check` step and CI on dumps kept outside the repo (2.2) |
| Both OS | Same Rust on Windows and Linux; ABI value | Windows first; macOS and Linux later | Keep the `Abi` idea; do not hard-code Win64 |
| Tray and dashboard | Native tray, hotkeys, localhost HTML dashboard, `Ctrl+Shift+I` subclass | CLI and installer GUI (Phase 1) | Our GUI exists; a tray is optional. Dashboard server is a hazard (section 8) |
| Store | Read-only SQL over Phoenix's `cache.sqlite`; layout assumed to be the mobile one | Own KV store in `Engine` | Do not depend on their store; read-only access to Phoenix's cache is an option for MessageLogger history |
| Updating the mod | Site-served loader fetched by the launcher; plugin updates in the client | Release plan with signing (Phase 1) | Add signature verification of everything loaded into Discord |
| Debug shell | Loopback `/eval`, token file on desktop | None | Our rule: off by default, per-start token (section 8) |

## 8. What this changes in our Phase 2 plan

Proposals, not edits. They are for the owner and the coordinator to accept, reorder or reject.

**Stage 2.1, reconnaissance** (static inspection of the official build on the owner's machine, no Discord files in the repo)
- Replace the question "does Phoenix honour `HTTPS_PROXY` and trust the system store" by a smaller one for the in-process case: **the HTTP layer is a Rust trait object above TLS, so TLS and pinning are irrelevant in-process.** Keep the proxy questions only for the fallback.
- "Gateway encoding and compression" also stops being an in-process question, because the registry hands out decoded JSON. It remains a question for the proxy fallback.
- Add: **does the exe restrict its DLL search path?** (Check the import and manifest data of the official build.) If yes, drop `version.dll` as the primary route, keep it only if still possible, and add a launcher route (below).
- Add: is there a gateway registry of the same shape (per-domain descriptors, seed events), how many copies, and whether `READY` is applied outside it on Desktop.
- Add: is there any function that sends gateway frames to the server (for `Engine::on_send`)? If none, `on_send` is a documented gap on Phoenix.
- Add: where does the client keep its account store, and is its schema stable enough to read.
- `phoecord launch` and the installer find Phoenix at `%LOCALAPPDATA%\DiscordPhoenix\app\Discord.exe`; also record the Linux path.

**Stage 2.2, signatures and in-process transport**
- Adopt the **`Found<T>`, checked-at-build-time** model for locators that need decoding (struct sizes, field offsets, allocator functions, handler ABI) as Rust code in `phoecord-loader`, next to `aob.rs`. Keep the TOML AOB for simple entry points and for hot-fixes. Both go through the same "unique or off" rule.
- Add a `phoecord check <exe>` command (or `phoecord-loader` test runner) that runs every locator against a given exe and exits non-zero on a miss, and use it in the existing plan for "CI check of signatures on `.text` dumps (dumps kept outside the repository)". This is sunbird's main process idea and it fits our stage text.
- In-process transport recipe to evaluate: **inline entry hook on the transport `send` plus vtable-word swap for `poll`, host allocator for replaced bodies, `Engine::on_request` and `on_response`**; registry detours that call `Engine::on_dispatch`. Check each against two adjacent builds (existing rule).
- Make the prologue check an explicit step: a function is hookable only when its first instructions are whole, position-independent and long enough. Fail loudly otherwise.
- Cost warning: a full call-index over a ~500 MB executable is about a second per scan. Budget for it (build once, share, free) and decide where it runs (section "Loader").

**Stage 2.3, UI hooks and the theme engine**
- Plan the UI as "call Discord's builders" (as sunbird does), starting with the PhoeCord settings page and the modal layer, because the other panels (plugin list, plugin settings, the editor) are all built on these two.
- Add the **message context menu** hook (plugin actions in right-click and `...` menus) and the **optimistic delete** hook (so MessageLogger sees local deletes) to the 2.3 list.
- MessageLogger UI: wrap the rendered row in a new element rather than changing message content (sunbird's approach), and recolour through the body builder rather than patching the palette.
- Theme engine stays ours: sunbird has no palette hook, so nothing in it replaces gpui.md section 4. Check for a palette global during 2.1 reading.
- Thread discipline: build UI only on GPUI's thread; trigger redraws through a forced-update path found by the same locators; pin a modal to the Settings layer while Settings is open.

**Stage 2.4, release**
- The release set can include "plugin actions in message menus" and "message accessories" as the first visual API for plugins. Keep the plugin API typed, with opaque event handles and a secret-type setting that is scrubbed from logs (we already have secrets in `Engine`'s config).

**The loader**
- **Launching.** Add a Windows launcher path: start the client suspended, queue the loader as an APC (or inject another way before the entry point), resume. `phoecord launch` is the natural place. Keep `version.dll` sideload as a second route only if the exe allows it.
- **Timing.** Decide between two designs and test them: (a) sunbird's "locate and hook inside the loader entry" (works, costs time under the loader lock, needs a bounded scan), or (b) the loader entry only spawns a worker and signals the launcher, which keeps the main thread suspended until the hooks are in, then resumes it. (b) keeps our "nothing heavy under the loader lock" rule and uses the launcher we need anyway. It is a proposal that sunbird does not demonstrate. It needs a test on a real build.
- **Crash safety.** Panic guard at every thread entry; no unwinding out of the entry point; an error in one locator disables only its feature (we already have the "degrade to network only" rule in 2.3).
- **Hot-fix path.** Keep `signatures/*.toml` for AOB entries and add a signed override for code locators' tunables if needed; never ship a locator that falls back to an old offset.
- **IPC.** The existing loader-to-proxy pipe stays for the proxy mode; in in-process mode the loader talks to the same `Engine` directly.
- **Security of the loaded code.** The launcher may update the loader; if it does, it verifies a signature, not only https. No loopback shell by default; if a dev shell exists, it is off, bound to loopback and token-gated per start.
- **Data location.** Do not put our data inside Phoenix's own directory unless a feature needs it (sunbird does so to share its `store` root). Use `%LOCALAPPDATA%\PhoeCord`.

## 9. Open questions

1. **DLL search.** Does the official Phoenix `Discord.exe` really restrict its DLL search to the system directory? If so, our `version.dll` plan changes (above).
2. **Hook timing.** What is the real deadline: first frame, first gateway connect, first request? sunbird says the registry and vtables are read once at start-up; we need the actual order on an official build to size the budget.
3. **Outgoing gateway.** Is there a way to see or change client → server gateway frames in-process?
4. **READY on Desktop.** Is it applied outside the registry on Desktop, as the shared plugin API says?
5. **Store.** Desktop's `cache.sqlite` schema (the key and blob layout read about is a mobile note). Is it versioned and stable? Is reading it while the client runs safe?
6. **Allocator.** How stable is the allocator discovery across builds, and is there a safer way to replace a body (for example by returning a local reply instead of editing in place)?
7. **Prologues.** What share of the hook targets have a prologue an entry hook can take, on both Windows and Linux builds? A target that fails needs another hook site.
8. **Build cadence.** How often does Phoenix ship, and how often did sunbird's locators miss? (Their `check` workflow history would answer this without any Discord file.) Our "within 48 hours" rule depends on it.
9. **GPUI revision.** Does Phoenix pin the GPUI revision our reference build uses? sunbird's decoding of `StyleRefinement` shows the layout is read from the binary, so a different revision is survivable, but our `gpui.md` anchors assume the pinned one.
10. **Platforms.** macOS Phoenix: unknown to sunbird's desktop crates (they cover Windows and Linux x86-64 only). Windows on Arm: not covered either.
11. **Terms and risk.** Injecting into the official Phoenix client, once it is available, is a Terms-of-Service question for the owner, exactly as it is for today's client. Not an engineering question, but it decides how loud PhoeCord should be about Phoenix support.
12. **Release timing.** Our Phase 2 starts only when Phoenix is officially available to users. Until then every item above stays on paper.
