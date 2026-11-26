# Phase 2: implementation plan

**Goal:** PhoeCord works inside Desktop Phoenix through an in-process transport, with a theme engine, in-client settings, full Vencord parity — network and visual plugins, panels and themes — and ships as v1.0.
**Spec:** [`spec.md`](spec.md) · **Ledger:** [`ledger.md`](ledger.md) · **For people:** [`../README.md`](../README.md)

Tasks are grouped by stage. Each has files, steps and "done when". Statuses and owners are in the "Tasks" table of [`README.md`](../README.md), the only place status is tracked. Every task PR also does the living-docs steps from `docs/AGENTS.md`; they are not repeated below.

## Areas

| Area | Owner | Files |
|:---|:---|:---|
| Core RE (binary, hooks) | **Nanda** | `crates/phoecord-loader/src/hooks/`, `crates/phoecord-loader/src/locators/` |
| Proxy, transport, wire | **ken** | `crates/phoecord-proxy/src/`, `crates/phoecord-transport/src/` |
| Discovery, installer, CLI | **buslan** | `crates/phoecord-installer/src/`, `crates/phoecord-cli/src/`, `signatures/` |
| RE support, locators, UI | **matvee** | `crates/phoecord-loader/src/locators/`, `crates/phoecord-loader/src/ui/`, `docs/research/` |
| Plugin API, runtime | **Nanda** | `crates/phoecord-core/src/plugin.rs`, `crates/phoecord-plugin-runtime/` |
| Plugins | **Contributors** | `crates/phoecord-plugins/src/` |
| Themes | **matvee** + contributors | `crates/phoecord-loader/src/theme/`, `themes/` |

## Stage 2.1. Phoenix reconnaissance

### 2.1.1. Phoenix discovery (Nanda)

**Files:** `crates/phoecord-installer/src/discover.rs`, `crates/phoecord-cli/src/main.rs`

- Find Phoenix install paths: `%LOCALAPPDATA%\DiscordPhoenix` on Windows, `~/.config/discord-phoenix` and flatpak on Linux.
- Add `Channel::Phoenix` variant to the existing `discover` module; reuse the version-reading logic.
- Wire `phoecord launch --channel phoenix` in the CLI; if no Phoenix install found, print a clear message.
- Update path: determine how Phoenix auto-updates (in-place overwrite, versioned folders, or updater binary).
- Tests: mock filesystem with the expected paths; verify discovery and version parse.

**Done when:** `phoecord launch --channel phoenix` starts Phoenix on a machine where it is installed; `phoecord paths` shows the Phoenix install directory.

### 2.1.2. Binary inspection (Nanda)

**Files:** `docs/research/phoenix-recon.md` (findings section)

- Open `Discord.exe` in CFF Explorer or `dumpbin /headers`: PE characteristics, subsystem, entry point.
- Import table: list all DLLs, check for `version.dll`, `winhttp.dll`, `winmm.dll`; note any delay-loaded DLLs.
- DLL search restrictions: check `DefaultDllDirectories` / `SetDefaultDllDirectories` calls, manifest `<dllDirectory>`, `LOAD_LIBRARY_SEARCH_*` flags.
- Dependent-load flags: does `LoadLibraryEx` with `LOAD_WITH_ALTERED_SEARCH_PATH` appear?
- Record: target machine, section layout, TLS callbacks, .NET metadata (should be none), Rust panic handler presence.

**Done when:** `phoenix-recon.md` has a "Binary inspection" section answering: can `version.dll` sideload? Which DLLs are imported? What restrictions exist?

### 2.1.3. Proxy behavior (ken)

**Files:** `docs/research/phoenix-recon.md` (findings section)

- Start Phoenix with `HTTPS_PROXY=http://127.0.0.1:8080` and PhoeCord's proxy running; observe whether traffic arrives.
- Test system proxy (Windows: Internet Options; Linux: `http_proxy` env): does Phoenix honour it?
- CA trust: does Phoenix use the system certificate store or bundle its own? Add PhoeCord's CA to the system store and check whether Phoenix accepts it.
- Certificate pinning: use mitmproxy with a custom CA; if Phoenix rejects it, pinning is confirmed.
- Record results with screenshots or logs; if proxy works, note which endpoints go through it and which don't.

**Done when:** `phoenix-recon.md` has a "Proxy behavior" section answering: does `HTTPS_PROXY` work, is the system CA store trusted, is there pinning? The answer feeds 2.1.8.

### 2.1.4. Gateway wire format (ken)

**Files:** `docs/research/phoenix-recon.md` (findings section), `tests/fixtures/phoenix/`

- Capture Phoenix's gateway traffic through the proxy (if 2.1.3 confirms it works) or via Wireshark.
- Identify encoding: JSON or ETF. Identify compression: zlib-stream, zstd, or none.
- Compare event payloads with today's Electron client: new fields, removed fields, new event types.
- Record at least HELLO, READY, MESSAGE_CREATE samples; scrub personal data.
- If encoding or compression differs from Electron, note what changes in `phoecord-proxy` codecs.

**Done when:** `phoenix-recon.md` has a "Gateway wire format" section with encoding, compression, and a diff of event shapes. Test fixtures for Phoenix gateway frames exist in `tests/fixtures/phoenix/`.

### 2.1.5. Locate HTTP transport (Nanda)

**Files:** `docs/research/phoenix-recon.md` (findings section)

- In Ghidra/IDA, find `ReqwestTransport::send`: search for the string `"ReqwestTransport"`, or trace from `reqwest::Client::execute`.
- Identify the vtable in `.rdata`; note offsets of `send` and `poll` slots.
- Find the type layout: `self` pointer, URL field, headers field, body field. Determine body ownership and allocator.
- Locate `__rust_alloc` and `__rust_dealloc` exports or symbols; test whether they match `#[global_allocator]`.
- Check the prologue of `send`: enough instructions for an inline entry hook? Note bytes available before the first branch.
- Compare with GPUI reference build symbols where possible.

**Done when:** `phoenix-recon.md` has an "HTTP transport" section with the address of `send`, the vtable layout, the allocator shim addresses, and the prologue assessment.

### 2.1.6. Locate Gateway registry (Nanda)

**Files:** `docs/research/phoenix-recon.md` (findings section)

- Search for known event names as strings: `"READY"`, `"MESSAGE_CREATE"`, `"GUILD_CREATE"`.
- Trace string references to the registration code; identify the descriptor table structure: `{name, handler_fn}` or more complex.
- Note how many domains (tables) exist and what distinguishes them.
- Check READY handling: is it dispatched through the registry like other events, or handled separately?
- Search for an outgoing gateway send function: trace from `tungstenite::write_message` or `WebSocket::send`.
- Note handler ABI: `fn(ctx: &mut Context, payload: &Value)` or similar; are payloads pre-decoded JSON/struct?

**Done when:** `phoenix-recon.md` has a "Gateway registry" section with the table address, entry layout, handler ABI, READY handling, and whether outgoing send was found.

### 2.1.7. Loading path decision and prototype (Nanda)

**Files:** `crates/phoecord-loader/src/lib.rs` (prototype), `docs/research/phoenix-recon.md`

- Based on 2.1.2 findings, decide: sideload (`version.dll`) or launcher (APC injection from `phoecord launch`).
- If sideload: test that placing `version.dll` next to `Discord.exe` loads it; verify export forwarding works.
- If launcher: implement `CreateProcess` with `CREATE_SUSPENDED`, `QueueUserAPC` to load the DLL, `ResumeThread`. The launcher lives in `phoecord-cli`.
- Hook timing test: instrument the loader entry to record when it runs relative to the first HTTP request and first gateway connect. Answer: is `DllMain` early enough, or must we hold the main thread?
- If `DllMain` is too late: prototype the suspended-thread approach (hold the main thread until hooks are installed).
- Record the decision as a `Ruling:` in the ledger.

**Done when:** a prototype loader runs inside Phoenix, hooks fire before the first network activity, and `phoenix-recon.md` has a "Loading path" section with the decision and timing data.

### 2.1.8. Proxy reach decision and fallback (ken)

**Files:** `crates/phoecord-proxy/src/`, `docs/research/phoenix-recon.md`

- Based on 2.1.3 findings, decide which of the three options from `ARCHITECTURE.md` applies:
  - Option 1: `HTTPS_PROXY` works → no code changes, the proxy is the fallback out of the box.
  - Option 2: hook HTTP client creation in-process to point at the proxy → add a hook in the loader that sets the proxy on `reqwest::Client`.
  - Option 3: add PhoeCord's CA to the process certificate roots → add a hook that calls `rustls` or native APIs to trust our CA.
- Implement the chosen path; if option 2 or 3, the hook is a thin shim in the loader that runs at load time.
- Test: start Phoenix with the proxy, confirm REST and Gateway traffic flows through it.
- Record the decision as a `Ruling:` in the ledger.

**Done when:** Phoenix's traffic goes through PhoeCord's proxy on a fresh install with no manual setup. The fallback path is documented in `ARCHITECTURE.md`.

### 2.1.9. Recon report (matvee)

**Files:** `docs/research/phoenix-recon.md` (assembly and editing), `ARCHITECTURE.md`

- Collect all findings from 2.1.1–2.1.8 into a coherent `phoenix-recon.md`: binary layout, loading path, proxy behavior, wire format, HTTP transport, gateway registry.
- Write a "Decisions for 2.2–2.4" section summarizing what the reconnaissance confirmed and what changed from the original plan.
- Update `ARCHITECTURE.md`: the "Phoenix and the proxy" section with the confirmed option; the "Two transports" section with any new details about the in-process side.
- List remaining open questions and risks discovered during reconnaissance.

**Done when:** `phoenix-recon.md` is a complete, reviewable document. `ARCHITECTURE.md` reflects the confirmed decisions. The ledger has a summary line.

### 2.1.10. Gateway fixtures (matvee)

Was task 1.1.9 (moved when Phase 1 closed, ruling 2026-11-26). Coordinate with 2.1.4 (ken), which needs fixtures of Phoenix's JSON + zstd-stream gateway too.

**Files:** `tests/fixtures/**`, `tests/fixtures/README.md`, `scripts/scrub_fixture.py`

- Record READY, GUILD_CREATE, MESSAGE_CREATE/UPDATE/DELETE, PRESENCE_UPDATE, TYPING_START from a throwaway account, both encodings.
- Scrub tokens, emails, phone numbers, real user ids and names (a script in `scripts/scrub_fixture.py`, `-I` safe).
- README: how each fixture was recorded and scrubbed.

**Done when:** fixtures load in tests and contain no personal data (reviewed by Nanda).

## Stage 2.2. Reverse engineering and AOB signatures

### 2.2.1. Ghidra / IDA project (Nanda)

**Files:** local only (the project file is never committed); `docs/research/phoenix-recon.md` (symbol import notes)

- Create a Ghidra or IDA project on the Phoenix Canary binary.
- Import GPUI symbols from the reference build (`research/gpui-reference/`): `App`, `Window`, `Entity`, `Render`, `Element`, `GlobalTheme`, `ThemeColors`, `Hsla`.
- Apply Rust demangling; tag known crate boundaries (`discord_desktop_core::`, `gpui::`, `reqwest::`, `tungstenite::`).
- Document which reference-build symbols matched and which did not (GPUI revision delta).

**Done when:** the project has named symbols for the hook targets identified in 2.1.5–2.1.6. The reference build comparison is recorded.

### 2.2.2. HTTP transport hook (ken)

**Files:** `crates/phoecord-loader/src/hooks/http.rs`, `crates/phoecord-loader/src/hooks/mod.rs`

- Inline entry hook on `ReqwestTransport::send`: save registers, extract method/URL/headers/body from `self`, call `Engine::on_request`, apply verdict (Pass/Changed/Reply).
- Vtable swap for `poll`: replace the `poll` slot in `.rdata` to intercept the response future. Extract status, headers, body; call `Engine::on_response`; apply verdict.
- Allocator-safe body replacement: use `__rust_alloc` / `__rust_dealloc` from the host process (addresses from 2.1.5). If allocator discovery fails, fall back to `Verdict::Reply` only.
- Panic guard: `catch_unwind` at every entry; a panic disables the hook and logs, never crashes Discord.
- Tests: a mock binary layout with known addresses; verify hook installation and callback flow.

**Done when:** REST traffic through Phoenix is intercepted, modified by a test plugin, and Discord still works. Allocator mismatch is handled gracefully.

### 2.2.3. Gateway registry detours (ken)

**Files:** `crates/phoecord-loader/src/hooks/gateway.rs`

- For each handler in the registry (from 2.1.6), replace the pointer with a detour that: decodes the event name, calls `Engine::on_dispatch`, applies the verdict, then calls the original handler (or not, for `Dropped`).
- Batch installation: all handlers replaced in one pass before resuming the main thread (if using the suspended approach from 2.1.7).
- If the handler ABI is `fn(&mut Ctx, &serde_json::Value)`, pass the JSON directly; if it's a typed struct, serialize to JSON for the engine.
- Panic guard per handler.

**Done when:** gateway events flow through `Engine::on_dispatch`; a test plugin can intercept and modify `MESSAGE_CREATE` on Phoenix.

### 2.2.4. Outgoing gateway hook (ken)

**Files:** `crates/phoecord-loader/src/hooks/gateway_send.rs`

- If 2.1.6 found an outgoing send function: hook it, extract the frame, call `Engine::on_send`, apply verdict.
- If no send function was found: document the gap; `Engine::on_send` works only through the proxy fallback.
- Panic guard.

**Done when:** outgoing gateway frames are interceptable, or the gap is documented and the proxy fallback covers it.

### 2.2.5. Typed locators (matvee)

**Files:** `crates/phoecord-loader/src/locators/mod.rs`, per-target modules

- Define the `Found<T>` pattern: a locator function that scans the binary for a specific structure and returns `Found<T>` (typed result with addresses and offsets) or `LocatorError`.
- Implement locators for: `ReqwestTransport` (struct size, `send` offset, vtable address), gateway registry (table address, entry size, handler offset), allocator shims (`__rust_alloc`, `__rust_dealloc`).
- Each locator uses a combination of string anchors, code patterns and cross-references; not just AOB.
- Locators share a call index (see `phoenix-desktop.md` §5) for efficiency.
- Unit tests with mock binary sections.

**Done when:** every hook target from 2.2.2–2.2.4 can be found by a typed locator. Each locator has a test with a mock section.

### 2.2.6. `phoecord check <exe>` (buslan)

**Files:** `crates/phoecord-cli/src/check.rs`, `crates/phoecord-loader/src/locators/`

- CLI command: takes a path to `Discord.exe`, runs every locator and every AOB signature against it.
- Output: a table of locator name, status (found / missed), address. Exit code 0 if all found, 1 if any missed.
- CI integration: a job that runs `phoecord check` on `.text` dumps kept outside the repository (a private artifact or a developer's machine).

**Done when:** `phoecord check Discord.exe` reports all locators and signatures. CI can run it.

### 2.2.7. In-process transport wired to Engine (ken)

**Files:** `crates/phoecord-loader/src/transport.rs`, `crates/phoecord-transport/src/`

- Implement `InProcessTransport` that satisfies the `Transport` trait (or the same entry points as the proxy transport).
- Wire the hooks from 2.2.2–2.2.4 to `Engine::on_request`, `on_response`, `on_dispatch`, `on_send`.
- `Effects` delivery: timers and scheduled events from the plugin chain are dispatched on the loader's thread pool.
- Fallback: the transport is chosen per launch, not per entry point (ruling, 2026-11-26; see 2.2.8): if any entry point's locator fails, the whole launch goes through the proxy and the in-process transport stays off; no handoff of single entry points between the loader and the proxy is needed.

**Done when:** plugins receive events through the in-process transport on Phoenix. With a locator missing, the in-process transport stays off and the launch uses the proxy.

### 2.2.8. Phoenix installer and transport choice (Nanda)

**Files:** `crates/phoecord-installer/src/` (`launch.rs`, `shortcut.rs`, `install.rs`, `uninstall.rs`), `crates/phoecord-installer-gui/src/`, `crates/phoecord-cli/src/`

The target on Windows Phoenix is **no proxy**: the loader is injected and runs the plugin chain in-process. The proxy stays for Electron clients, for Phoenix on macOS and Linux, and as the fallback when a Phoenix build has no working signatures. All seven questions below were decided in advance (rulings, 2026-11-26, in `ledger.md`); follow them, don't reopen them in the PR.

1. **Who picks the transport, and when.** `--proxy-server` is a command-line flag, so the choice is made before the client starts. `phoecord launch --channel phoenix` checks `Discord.exe` on disk with the same locators and `phoecord_loader::plan` that `phoecord check` uses (2.2.6), and caches the result in the data folder under a key of two hashes: the client's `.text` hash and a hash of the signature folder (`%LOCALAPPDATA%\PhoeCord\signatures`: every `*.toml` file's name and contents, in the order the loader reads them). A full check runs only after a Phoenix update or a change to the signatures (a hot-fix from 2.2.10, a file added or removed by hand).
2. **Whole-launch mode, no mixing.** All Engine entry points found → in-process, no proxy. Any one missing → the whole launch goes through the proxy and the loader does the UI only. The same traffic never passes the plugin chain twice. If 2.2.4 finds no send function, Phoenix always uses the proxy; that is accepted.
3. **No idle proxy.** In in-process mode the proxy is not started at all. The loader reads its settings from `config.toml` itself (the `Engine` lives inside it), with no named pipe.
4. **A failure during a session.** A hook that fails to install or panics after launch cannot be switched to the proxy live. Network plugins stay off for the rest of that session, the client keeps running, and the user gets a notice. The build is marked failed in the cache, so the next launch goes straight to the proxy; a signature hot-fix (2.2.10) changes the signature-folder hash, so the mark no longer applies and the next launch checks again.
5. **The CA is installed up front**, as today, even if Phoenix will mostly run in-process: the fallback needs it at any moment, and asking for trust in the middle of a launch is not acceptable. Audited in 2.4.6.
6. **How Phoenix is started, and updates.** The existing pattern: a separate "Discord Phoenix (PhoeCord)" shortcut that runs `phoecord launch --channel phoenix`. Discord's own shortcuts are not touched; no IFEO or other launch interception. Phoenix updates in place in `app\` and the loader is injected on every launch, so reconciling after an update needs no step of its own: the new `.text` hash misses the cache from item 1; nothing is copied. Uninstall removes the shortcut and the cache. Proxy autostart at login is needed only for Electron clients.
7. **Manual override and other OSes.** `phoecord launch --transport auto|proxy|in-process` and `[phoenix] transport = "auto"` in `config.toml`; default `auto`, the manual values are for debugging. On macOS and Linux Phoenix gets the proxy and a plain launch. The mode is per client: an Electron client on the proxy and Phoenix in-process can run at the same time.

Also:
- Injection itself is `phoecord_loader::inject` (2.1.7: `CreateProcess` suspended + `QueueUserAPC` + `LoadLibraryW`); `phoecord launch` calls it in in-process mode and in proxy mode too (the loader is needed for the UI either way).
- GUI: a "Phoenix" toggle next to the existing Electron options.
- Depends on 2.2.6 (the check code), 2.2.7 (the in-process transport) and 2.2.10 (signatures from disk). Items 1, 6 and 7 can be built before those land, against fake locator results.

**Done when:** on a Windows machine with Phoenix, setup makes the shortcut; `phoecord launch --channel phoenix` starts in-process with no proxy when the build's signatures are all found, and through the proxy when one is missing or the build is marked failed; a Phoenix update or a signature hot-fix only re-runs the check (the cache key changes); `--transport` overrides the choice; uninstall is clean. Tested with fake locator results; one live run on the owner's PC recorded in the ledger.

### 2.2.9. Signatures verified on two builds (buslan)

**Files:** `signatures/*.toml`

- Collect two adjacent Phoenix Canary builds (before and after an update).
- Run `phoecord check` on both; every signature and locator must match both.
- Signatures that match only one build are rewritten or converted to typed locators.
- Document the two builds (version, date, `.text` hash) in the signature TOML comments.

**Done when:** every entry in `signatures/` and every locator passes `phoecord check` on two distinct Phoenix builds.

### 2.2.10. Signature hot-fix path (buslan)

**Files:** `signatures/*.toml`, `crates/phoecord-loader/src/aob.rs`

- Ensure TOML AOB entries can be updated without rebuilding the loader: the loader reads `signatures/` at startup (or from an embedded resource that the installer replaces).
- A hot-fix workflow: a new Phoenix build drops → update TOML → push to a release → users get the fix on next restart.
- If the loader currently embeds signatures at compile time, change it to load from disk with a compile-time fallback.

**Done when:** changing a `.toml` file and restarting Discord picks up the new signature without a new loader binary. The hot-fix workflow is documented.

### 2.2.11. Loader check inside Discord Canary (ken)

Was task 1.5.9. On Windows with Discord Canary, from the next release or a `main` build: put `version.dll` in the newest `app-x.y.z` (`phoecord setup --loader --channel canary`), start `phoecord launch --channel canary`, use the client for 10 minutes, quit. Check: `loader.log` has one `client build` line per start (no worker in child processes, 1.5.8), the `.text` hash equals the file's, detection takes under 20 ms, and the proxy log shows `loader connected: build …`; no crash; `phoecord uninstall` removes the DLL.

**Done when:** every check passes, written in the ledger with the Canary build.

## Stage 2.3. UI hooks and theme engine

### 2.3.1. Theme engine (matvee)

**Files:** `crates/phoecord-loader/src/theme/mod.rs`, `crates/phoecord-loader/src/theme/toml.rs`, `themes/`

- Parse `theme.toml`: a flat map of `ThemeColors` field names to `Hsla` values (see `gpui.md` for the palette structure).
- Hook the active theme global (`GlobalTheme` or equivalent) to replace palette values.
- If Phoenix loads themes as JSON data files (like Zed), intercept the file read instead of hooking the global.
- Live switching: watch `theme.toml` for changes (mtime poll every 2s); on change, reload and update the palette on the next frame.
- Ship a default `theme.toml` that matches Discord's default palette (a no-op theme for testing).

**Done when:** editing `theme.toml` changes Discord Phoenix's colors within 2 seconds. The default theme causes no visual change.

### 2.3.2. Built-in themes and Vencord import (contributor)

**Files:** `themes/*.toml`

- 3 built-in themes: a dark variant, a light variant, and a high-contrast variant.
- Vencord theme import: a script or CLI command that reads a Vencord CSS theme and extracts color values into a `theme.toml` where they map to palette fields. Colors that don't map are listed as unsupported.
- Document theme authoring in `docs/plugins/themes.md`.

**Done when:** 3 themes ship in `themes/`; `phoecord theme import <vencord-theme.css>` produces a `.toml`.

### 2.3.3. Settings tab (ken)

**Files:** `crates/phoecord-loader/src/ui/settings.rs`, `crates/phoecord-loader/src/ui/mod.rs`

- Add a PhoeCord page to Phoenix's settings sidebar (hook the navigation list or register a custom page via Mana).
- Plugin list: on/off switches per plugin, with the plugin's `manifest().name` and description.
- Per-plugin settings: render the plugin's config fields as UI controls (toggles, text fields, dropdowns) based on the config schema.
- Package management: install/remove WASM packages from a local path, with rights shown before install.
- Localization: en + ru through `.ftl` files.

**Done when:** a PhoeCord tab appears in Phoenix's settings; plugins can be toggled; a WASM package can be installed with rights displayed.

### 2.3.4. MessageLogger UI (matvee)

**Files:** `crates/phoecord-loader/src/ui/message_logger.rs`

- Wrap `MessageRow::render` (or the equivalent found in 2.2.1) to add accessories to message elements.
- Deleted messages: a strikethrough tint and a "deleted" indicator, using data from the MessageLogger plugin's store.
- Edit history: on hover, show previous versions in a tooltip or expandable section.
- Do not rewrite message content; use element wrapping only.

**Done when:** a deleted message shows struck-through in Phoenix; hovering an edited message shows its history.

### 2.3.5. Message context menu (contributor)

**Files:** `crates/phoecord-loader/src/ui/context_menu.rs`

- Hook `MessageContextMenu` (or the right-click handler on messages) to append plugin action entries.
- Each plugin declares menu entries in its manifest; the hook reads them and builds Mana `ListItem` elements.
- Clicking an entry calls the plugin's `on_event` with a `MenuAction` event.

**Done when:** a test plugin adds a "Copy Raw" entry to the message right-click menu on Phoenix.

### 2.3.6. Optimistic delete hook (contributor)

**Files:** `crates/phoecord-loader/src/hooks/store.rs`

- Hook `Store::delete_message` (the client's own optimistic delete, before the server confirms).
- Emit a `LocalDelete` event through `Engine::on_dispatch` so MessageLogger sees it.
- The hook must not block the original delete; it's an observation point only.

**Done when:** MessageLogger records a local delete immediately when the user deletes a message in Phoenix.

### 2.3.7. Degradation verified (ken)

**Files:** `tests/`, `docs/research/phoenix-recon.md` (degradation section)

- Test scenario 1: unknown Phoenix build (no matching signatures) → PhoeCord falls back to proxy-only mode, no crashes, all network plugins work.
- Test scenario 2: one signature missing (e.g. gateway registry) → that feature is off, everything else works.
- Test scenario 3: loader present but no Phoenix (Electron client) → loader detects non-Phoenix binary, does nothing, Electron runs normally.
- Automate where possible: a test harness that patches locator results to simulate missing signatures.

**Done when:** all three scenarios pass without crashes. Results documented.

### 2.3.8. CDN picture hook for USRBG and Decor (buslan)

Was blocked since Phase 1 (1.2.14, ruling 2026-11-26: a UI hook, not CDN interception — the proxy only intercepts the API and Gateway hosts, `docs/CODE_MAP.md`'s `intercept` key; adding every CDN host there would mean decrypting all image/attachment traffic for every install, not just these two plugins).

**Files:** `crates/phoecord-loader/src/hooks/cdn.rs` (new), `crates/phoecord-plugins/src/usrbg.rs`, `crates/phoecord-plugins/src/decor.rs` (new, off by default)

- Find, by the same method as 2.2.1/2.2.5 (typed locator, not a guess), the native function that turns a user id and an avatar/banner/decoration hash into the CDN URL or the loaded image for the client's own UI (not the REST client `send` that 2.2.2 already hooks — this is the asset-loading path GPUI calls directly).
- Hook it: USRBG (banner) and Decor (avatar decoration) data parts already fetch their own picture through `Host::fetch` (Phase 1, network only so far); this hook has to let that fetched image (or its URL) reach the native loader in place of the real CDN one, for exactly these two plugins' hashes — not a blanket substitution.
- Panic guard, same as every other hook (`docs/phase-2/AGENTS.md`, "Hook targets are checked, not assumed"): a locator miss turns this feature off, nothing else.

**Done when:** a banner set by USRBG, and a decoration set by Decor, show on Phoenix without the real Nitro entitlement; no change to how any other picture loads.

### 2.3.9. FixImagesQuality's default mode on Phoenix (buslan)

Was the open part of task 1.2.15 (moved when Phase 1 closed, ruling 2026-11-26). `fix_images_quality` already does `originalImagesInChat` for image attachments on the network layer; the default mode, which asks the client's media proxy for the original size of every image in chat, changes how the client builds image URLs, so it needs the picture-loading path found and hooked in 2.3.8.

**Files:** `crates/phoecord-plugins/src/fix_images_quality.rs`, `crates/phoecord-loader/src/hooks/cdn.rs`

- Reuse 2.3.8's locator and hook; add the size/quality rewrite for this plugin only, when its default mode is on.
- Panic guard and a locator miss turning only this feature off, as for every hook.

**Done when:** with the default mode on, images in chat load at original resolution on Phoenix; nothing changes with the plugin off, and the proxy-side behaviour on Electron stays as it is.

### 2.3.10. Generic view-render wrap and message-row accessories (matvee)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/` (wrap, message row), `crates/phoecord-loader/src/locators/` (the render anchors)

- Scope: the one wrap around a GPUI view's `render` that every on-screen part below builds on, then the message row: accessories under, beside and inside a message (MessageLogger's struck-through and history from 2.3.4's `Accessory`, Translate's result, ReplyTimestamp, ShowTimeoutDuration, UserMessagesPronouns, ShikiCodeblocks, MessageClickActions, AntiRickroll's warning).
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin adds an element to a message row through the frozen API, and a locator miss turns off only the wrap it guards on Phoenix.

### 2.3.11. Every context menu (Nanda)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/menu.rs`

- Scope: plugin entries in all context menus, not only the message menu of 2.3.5: user, channel, server, role (BetterRoleContext), image, link, account panel (CopyUserURLs, ReverseImageSearch, ViewIcons).
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** each menu kind takes plugin entries, and a click reaches the plugin as a UI event on Phoenix.

### 2.3.12. Modals (Nanda)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/modal.rs`

- Scope: plugin modals and the modal stack: a plugin opens a modal with text, buttons and fields (PermissionsViewer, ViewRaw, ReviewDB's list).
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin opens and closes a modal; Escape and a click outside close it on Phoenix.

### 2.3.13. User names (matvee)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/name.rs`

- Scope: how a user's name renders everywhere it appears: ShowMeYourName, RoleColorEverywhere, ForceOwnerCrown, PlatformIndicators' icons.
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin changes the text, colour and trailing icons of a name in chat, the member list and mentions on Phoenix.

### 2.3.14. Profiles (Nanda)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/profile.rs`

- Scope: sections and buttons in profile popouts and full profiles: ShowConnections, ReviewDB, PermissionsViewer, ViewIcons.
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin adds a section to the profile popout and the full profile on Phoenix.

### 2.3.15. Chat input (buslan)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/input.rs`

- Scope: buttons, counters and text changes in the chat input: QuickReply, TypingTweaks, Translate's button, FavoriteEmojiFirst and MoreQuickReactions in the emoji picker, ExpressionManager.
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin adds a button to the chat bar and reads or changes the draft before send on Phoenix.

### 2.3.16. Voice and audio (buslan)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/voice.rs`

- Scope: the voice panel and per-user audio: CallTimer, VolumeBooster (gain above 200 %), VoiceDownload (save voice messages).
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin shows an element in the voice panel and sets a user's volume above the client's limit on Phoenix.

### 2.3.17. Account panel (buslan)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/account.rs`

- Scope: entries and elements on the account panel at the bottom left: SpotifyControls' player, plugin quick toggles.
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin adds an element above the account panel on Phoenix.

### 2.3.18. Media (buslan)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/media.rs`

- Scope: images, video and streams: ImageZoom, PictureInPicture, BlurNSFW, BiggerStreamPreview.
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin wraps an image or video element (blur, zoom lens, pop-out) and the stream preview size on Phoenix.

### 2.3.19. Channel and server lists (buslan)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/lists.rs`

- Scope: the server list and the channel list: BetterFolders, PinDMs.
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin adds, reorders and decorates entries in both lists on Phoenix.

### 2.3.20. Client behaviour patches (Nanda)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/hooks/behaviour.rs`

- Scope: switches on the client's own logic rather than its views: AlwaysTrust (no untrusted-link and suspicious-file prompts), NoUnblockToJump, ShowAllMessageButtons.
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** each patch is one located function with its own on/off, and a miss turns off only that patch on Phoenix.

### 2.3.21. Overlay (matvee)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/overlay.rs`

- Scope: PhoeCord's own overlay drawn above the client (the global overlay injection point of `docs/re/gpui-lifecycle.md`): toasts and notices from plugins.
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a plugin notification shows as a toast inside Phoenix on Phoenix.

### 2.3.22. Themes picker (matvee)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/themes.rs`

- Scope: the themes page in the PhoeCord settings tab: list, preview, switch, import (2.3.1, 2.3.2).
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** a theme is picked and applied without a restart on Phoenix.

### 2.3.23. Toolbox (Nanda)

Added by the owner's ruling of 2026-11-26 (UI surfaces v1.0 needs that no task covered). Blocked on the detours (2.2.2/2.2.3) and on an account that gets past Phoenix's access gate.

**Files:** `crates/phoecord-loader/src/ui/toolbox.rs`

- Scope: the PhoeCord toolbox button in the title bar with plugin actions (Vencord's VencordToolbox).
- Every anchor is a typed locator (2.2.5), with the panic guard; a miss turns off only this surface.

**Done when:** plugins add actions to the toolbox, and it opens the settings tab on Phoenix.

## Stage 2.4. Release v1.0

### 2.4.1. Top 10 plugins on Phoenix (ken)

**Files:** `crates/phoecord-plugins/src/`

- For each of the top plugins (MessageLogger, FakeNitro, NoTrack, ClearURLs, SilentTyping, Translate, PlatformIndicators, PermissionsViewer, ViewIcons; ShowHiddenChannels was removed by the owner's ruling of 2026-11-26): add Phoenix UI parts where the plugin has visual elements.
- Network parts already work from Phase 1; this task adds the Phoenix-specific rendering using the UI hooks from 2.3.
- Each plugin follows `docs/PORTING_PLUGINS.md` with Vencord attribution.

**Done when:** all 10 plugins work on Phoenix with both network and UI features.

### 2.4.2. Remaining v1.0 scope (Nanda)

**Files:** `crates/phoecord-plugins/src/`

- Complete every remaining 🌐 and 🌐 + 🎨 plugin.
- Port the 30 🎨 plugins of v1.0 (the list in `docs/PLUGINS.md`, ruling 2026-11-26) to Phoenix.
- Implement all panels: settings, plugin list, plugin settings, modals.

**Done when:** the plugin table in `docs/PLUGINS.md` shows all v1.0 plugins complete on Phoenix.

### 2.4.3. Plugin API stabilization (Nanda)

**Files:** `crates/phoecord-core/src/plugin.rs`, `crates/phoecord-plugin-sdk/`

- Freeze `API_VERSION = 1`; from this point, changes are minor-only (additive events, new `Host` methods).
- Review every public type and method in the Plugin API for stability: naming, lifetime, error types.
- Additive UI events for Phoenix hooks are still minor: `UiEvent::MenuAction`, `UiEvent::SettingsRender`, etc.
- Update the SDK to match; ensure `examples/hello-plugin` compiles against the frozen API.

**Done when:** `API_VERSION = 1` is set; no breaking changes are planned; the SDK examples compile.

### 2.4.4. Developer docs (Nanda)

**Files:** `docs/plugins/`

- Quick start: Rust + TypeScript (via the SDK's proc-macro and the WASM target).
- Event reference: every event type, when it fires, what fields it carries.
- Rights: what each right grants, how to request it, what the user sees.
- Publishing: how to distribute a `.wasm` package, versioning, the manifest format.
- Localization: how to add `.ftl` files, how `Host::t` works.

**Done when:** a new developer can follow the quick start to build, test and publish a plugin.

### 2.4.5. Installer 1.0 (Nanda)

Moved from buslan (ruling, 2026-11-26). The Phoenix part (loader injection, the transport choice, the Phoenix shortcut) is 2.2.8 (also Nanda's since the same day); 2.4.5 packages it.

**Files:** `crates/phoecord-installer/`, `crates/phoecord-installer-gui/`

- Windows installer with both Electron and Phoenix support.
- macOS and Linux with the network layer (proxy) only; Phoenix in-process transport is Windows-first.
- NSIS or WiX for Windows; `.deb` and `.rpm` for Linux; `.dmg` for macOS.
- Auto-update mechanism or clear instructions for manual update.

**Done when:** a user on each platform can install PhoeCord from a release binary.

### 2.4.6. Security audit (Nanda)

**Files:** `docs/SECURITY.md` (audit results), `ARCHITECTURE.md`

- Audit the CA: key generation, storage, trust scope, revocation on uninstall.
- Audit the proxy: no data leaks, hop-by-hop headers stripped, no open relay.
- Audit the loader: `unsafe` blocks reviewed, panic guards verified, no arbitrary code execution paths.
- Review the WASM sandbox: rights enforcement, time limits, memory limits.
- Document findings and mitigations.

**Done when:** `SECURITY.md` has an audit section with findings and mitigations. No critical issues remain open.

### 2.4.7. v1.0 release (Nanda)

**Files:** `docs/CHANGELOG.md`, `README.md`, release artifacts

- Tag `v1.0.0` on `main` after all tasks complete.
- Build release binaries for Windows (x86-64), Linux (x86-64), macOS (x86-64, aarch64).
- Generate `SHA256SUMS` for all binaries.
- Write the `CHANGELOG.md` entry for v1.0.
- Update `README.md`: remove pre-alpha warnings, add install instructions.
- GitHub Release with binaries and changelog.

**Done when:** `v1.0.0` is tagged, release binaries are published, and `CHANGELOG.md` and `README.md` are updated.

### 2.4.8. macOS check of the 0.5.1 fixes (ken)

> **Closed without the check** (owner's ruling, 2026-11-26). The 0.5.1 macOS fixes stay unverified on a real Mac.

Was task 1.4.10. On a real Mac (Apple Silicon, macOS 13+), with the `.dmg` of the next release or of the `Package` workflow: the app opens with "unidentified developer" and Open Anyway (not "damaged"); install from the window; "Open Discord" starts Discord, its updater finishes, and `~/Library/Application Support/phoecord/launch.log` has no `Operation not permitted` or `installer.db` line; the client works through the proxy; Uninstall in the window, then removing the app, leaves no CA in the keychain and no launchd entry.

**Done when:** every step above passes, written in the ledger with the macOS version and the build.
