# Phase 2. Phoenix Canary release: plan for people

The work plan from Phoenix Canary to v1.0, written 25.11.2026 at Nanda's request. Goal: PhoeCord works inside Desktop Phoenix through an in-process transport, with a theme engine, in-client settings, and full Vencord parity — network and visual plugins, panels and themes.

> This is the plan for people. The spec, the step-by-step tasks and the ledger are in [`ai/`](ai/). If this file and [`ai/spec.md`](ai/spec.md) disagree, the spec wins.

## Status

26.11.2026. PR #104 (ken) is in `main`: first results for 2.1.3 and 2.1.4, and the degradation plan `phoecord_loader::plan` (preparation for 2.2.7 and 2.3.7, not wired); 2.1.8 is decided: option 1, no hook. Nanda: 2.1.2, 2.1.5 and 2.1.6 done (findings in [`research/phoenix-recon.md`](../research/phoenix-recon.md), GPUI lifecycle notes in [`re/gpui-lifecycle.md`](../re/gpui-lifecycle.md)); 2.2.2 and 2.2.3 are reassigned to ken (he already owns 2.2.7, which calls them); 2.2.1 is done — 10 named hook targets, and the reference-build comparison is now recorded (261 accepted / 384 available / 10,655 blocked out of 11,300 correlations; `gpui::` itself is 18 / 5 / 2,111 — heavy monomorphization ambiguity, not a clean miss). Part 1 of 2.2.5 (PR #103) and 2.3.1 (PR #102) is in `main`. ken has access to the private recon notes (granted 2026-11-26) for 2.2.2/2.2.3. Next for Nanda: pick up 2.4.3-adjacent Plugin API work. Follow-up: `hooks::found::Found<T>` and `locators::Found<T>` are two separate types and should become one (2.2.2 with 2.2.5). Phase 1 open tasks running in parallel: 1.1.9, 1.2.14 (partial), 1.2.15 (partial). 1.2.17 is closed, 1.4.11 is cancelled, 1.4.10 is now 2.4.8 and 1.5.9 is now 2.2.11.

## What we get

- **A user** installs PhoeCord on Windows, starts Discord Phoenix and gets every Vencord plugin — network and visual — with custom themes, an in-client settings tab, and no extra process or CA trust required. On today's Electron client, the network layer works as before. Uninstalling is one button.
- **A plugin author** writes a WASM plugin that uses both network events and Phoenix UI hooks (message accessories, context menu entries, settings pages), following the developer docs.
- **The team** ships v1.0 with a frozen Plugin API, a security audit, signed releases and a process for updating signatures within 48 hours of a new Phoenix build.

## Decisions

Decisions from Phase 1 that carry forward (see `docs/phase-1/README.md` for quotes and dates):

- The in-client UI (settings tab, visual plugins, themes) is built only for Phoenix; on today's Electron client PhoeCord stays network-only.
- Hybrid transport: proxy today, in-process on Phoenix; the proxy as the fallback.
- Plugin runtime: WASM (wasmtime + WIT).
- No code signing (like Vencord).
- GPL-3.0-or-later; plugin SDK MIT.
- Full Vencord parity including visual plugins and panels.
- Vencord source reused with attribution.
- Dev tools (1.3.9–1.3.11) written from scratch; sunbird has no licence, so no code is copied.

New decisions for Phase 2 (made when the research answers them):

- Loading path: **launcher (APC injection)** on Phoenix, decided in 2.1.7 (ruling, 2026-11-26), live-observed via a third-party tool (`docs/ARCHITECTURE.md`, "Phoenix is different"). Sideload (`version.dll`) stays how today's Electron client is reached.
- Hook timing: synchronous under the loader lock or suspended-thread — decided in 2.1.7.
- Proxy reach on Phoenix: **option 1, no hook** (2.1.8, ruling 2026-11-26): `--proxy-server` or the system proxy plus the PhoeCord CA in the system store. `HTTPS_PROXY` is ignored.

**Not doing in Phase 2:** UI plugins through WASM (declarative elements → GPUI); the UI layer on macOS and Linux (Windows first, Linux in-process transport later); a third-party plugin catalogue with signature checks; languages other than English and Russian; Android or iOS.

## Open questions

1. **DLL search.** Does Phoenix `Discord.exe` restrict DLL search to the system directory? (2.1.2)
2. **Hook timing.** ~~What is the real deadline: first frame, first gateway connect, first request?~~ Answered by live observation (2.1.7, 2026-11-26): hooks installed in 858 ms, before the client reports ready; no crash, live account traffic flowed normally afterward.
3. **Outgoing gateway.** Is there a function that sends client → server gateway frames in-process? (2.1.6)
4. **READY on Desktop.** Is READY applied outside the event registry? (2.1.6)
5. **Allocator stability.** How stable is the allocator discovery across builds? (2.1.5, 2.2.2)
6. **Prologues.** What share of hook targets have a prologue an entry hook can take? (2.2)
7. **Build cadence.** How often does Phoenix ship new builds? Our "within 48 hours" rule depends on it. (2.2)
8. **GPUI revision.** Does Phoenix pin the same GPUI revision as our reference build? (2.1.5)
9. **Store schema.** Is Phoenix's `cache.sqlite` stable enough to read? (2.1)
10. **Terms and risk.** Injecting into Phoenix is a Terms question for the owner. (not engineering)

## Who does what

| | Nanda | ken | buslan | matvee |
|:---|:---|:---|:---|:---|
| Area | core RE (binary, hooks), decisions, Plugin API, security, release, review and merging | proxy, transport, wire format, degradation | discovery, tooling, installer, signatures | RE support, locators, UI hooks, theme engine |
| Tasks | 2.1.1, 2.1.2, 2.1.5, 2.1.6, 2.1.7; 2.2.1; 2.4.3, 2.4.6, 2.4.7 | 2.1.3, 2.1.4, 2.1.8; 2.2.2, 2.2.3, 2.2.4, 2.2.7, 2.2.11; 2.3.7; 2.4.8 | 2.2.6, 2.2.8, 2.2.9, 2.2.10, 2.3.8; 2.4.5 | 2.1.9; 2.2.5; 2.3.1, 2.3.3, 2.3.4 |

Unassigned contributor tasks: 2.3.2 (themes), 2.3.5, 2.3.6; 2.4.1, 2.4.2, 2.4.4.

Plugin API changes (`phoecord-core::plugin`) are made by Nanda only.

## Tasks

Details, files and "done when" in [`ai/plan.md`](ai/plan.md). **This table is the only place task status is tracked**; `ROADMAP.md` and `ai/plan.md` carry no checkboxes.

### Stage 2.1. Phoenix reconnaissance · `0.7`

| Task | What | Who | Status |
|:--|:--|:--|:--|
| **2.1.1** 🔴 | Phoenix discovery: install paths on Windows and Linux, update `discover` module, `phoecord launch --channel phoenix` | Nanda | ✅ Windows ([recon](../research/phoenix-recon.md)): `discover` finds `%LOCALAPPDATA%\DiscordPhoenix\app` and reads the build from `Discord.exe`; `launch --channel phoenix` starts it through the proxy and `paths` shows the folder (run on the owner's PC: Phoenix 0.1.0.2208 started, TLS refused until the CA is trusted). Plain launch only, injection is 2.1.7. Update scheme: in-place `app\`, inferred, no update observed. Linux and macOS: no Phoenix location known, nothing looked for |
| **2.1.2** 🔴 | Binary inspection: PE headers, import table, DLL search restrictions, dependent-load flags, update scheme | Nanda | ✅ |
| **2.1.3** 🔴 | Proxy behavior: does Phoenix honour `HTTPS_PROXY`, the system proxy, the system CA store; does it pin | ken | ⏳ in progress ([recon](../research/phoenix-recon.md)): Windows results written up (`--proxy-server` and system proxy honoured, `HTTPS_PROXY` not, trusted PhoeCord CA accepted, no pin seen on `discord.com` / the gateway); open: endpoints beyond the account screen, Linux |
| **2.1.4** 🔴 | Gateway wire format for the proxy fallback: encoding (JSON / ETF), compression (zlib / zstd), new events and fields | ken | ⏳ in progress ([recon](../research/phoenix-recon.md)): gateway is JSON + zstd-stream and decodes with the current codec; open: fixtures and event comparison (need an account the client accepts) |
| **2.1.5** 🔴 | Locate the HTTP transport in the binary: `ReqwestTransport::send`, the vtable, type layouts, allocator shims | Nanda | ✅ static analysis only, not run in Phoenix ([recon](../research/phoenix-recon.md) summary; addresses kept privately): `send`, the vtable, the request layout and the allocator shims found. Open for 2.2.2: the response layout |
| **2.1.6** 🔴 | Locate the Gateway registry: per-domain tables, seed events, handler ABI, READY handling; search for outgoing send | Nanda | ✅ static analysis only, not run in Phoenix ([recon](../research/phoenix-recon.md) summary; addresses kept privately): handler ABI, the outgoing send path and `READY`'s special handling were found (as before); the registry itself turns out to have no fixed table address — it is a per-instance tree-shaped structure built at runtime, reached through the context, not a static array. That answers the task's "done when" without one |
| **2.1.7** 🔴 | Loading path decision and prototype: sideload vs launcher; hook timing test | Nanda | ✅ both decisions live-confirmed (ruling, 2026-11-26) **with PhoeCord's own loader**, not a third-party tool: `phoecord dev inject` (launcher injection: `CreateProcess` suspended + `QueueUserAPC` + `LoadLibraryW`) ran `phoecord-loader` inside a real Phoenix process; `DllMain` and the worker fired, detected the build (RVA-free: a new `.text` sha256 for `0.1.0.2208`) and degraded cleanly (no signature table yet, UI layer off), no crash. First validated on a throwaway `cmd.exe` target, then the real client |
| **2.1.8** 🔴 | Proxy reach decision and fallback: option 1 / 2 / 3 from `ARCHITECTURE.md`; implement the chosen path | ken | ⏳ decided: option 1, no hook (ruling, 2026-11-26). Open: the check on a fresh install with no manual setup (`phoecord launch --channel phoenix` exists since 2.1.1) |
| **2.1.9** 🟡 | Recon report: `docs/research/phoenix-recon.md` with all findings, decisions for 2.2–2.4; update `ARCHITECTURE.md` | matvee | ⏳ |

### Stage 2.2. Reverse engineering and AOB signatures · `0.8`

| Task | What | Who | Status |
|:--|:--|:--|:--|
| **2.2.1** 🔴 | Ghidra / IDA project on Phoenix Canary: GPUI symbols imported from the reference build (1.6) | Nanda | ✅ 10 of the 2.1.5/2.1.6 hook targets are named symbols (`http_send`, `http_send_poll`, `rust_alloc`, `rust_dealloc`, `gateway_registry_build`, `gateway_registry_insert`, `gateway_send_encode`, `gateway_send_budget`, `gateway_connection_loop`, `rust_realloc_candidate`). The Version Tracking session against the GPUI reference build is now read (fixed the headless-scripting blocker from last round): 11,300 correlations, 261 accepted / 384 available / 10,655 blocked; narrowed to `gpui::` itself, 18 accepted / 5 available / 2,111 blocked — the revision delta is "heavily ambiguous from monomorphization", not a clean miss (see [recon](../research/phoenix-recon.md)). Open for the GUI: reviewing the 384 available candidates and the 10,655 blocked associations |
| **2.2.2** 🔴 | HTTP transport hook: inline entry on `send`, vtable swap for `poll`, allocator-safe body replacement | ken | 🚧 structure only (`hooks/`: `HookPlan`, `HookRuntime` → `Engine::on_request` / `on_response`, panic guard; nothing calls it yet; moved from Nanda to ken). 2.1.5 and 2.1.7 are done; access to the private recon notes granted 2026-11-26; writing the real detour is next |
| **2.2.3** 🔴 | Gateway registry detours: per-event handler swap, `Engine::on_dispatch` wiring | ken | 🚧 structure only (`hooks/gateway.rs`: `Engine::on_dispatch` bridge, panic guard; moved from Nanda to ken). 2.1.7 is done; signatures for 2.1.6 anchors are not in `signatures/` yet; access to the private recon notes granted 2026-11-26; writing the real detour is next |
| **2.2.4** 🟡 | Outgoing gateway hook (if found in 2.1.6): `Engine::on_send` wiring | ken | ⏳ |
| **2.2.5** 🔴 | Typed locators (`Found<T>`) for decoded structures: struct sizes, field offsets, handler ABI | matvee | ⏳ part 1 merged, PR #103 (`Found<T>`, errors, `Image`, `CallIndex`, string anchors; the target locators wait for 2.1.5 / 2.1.6) |
| **2.2.6** 🔴 | `phoecord check <exe>`: run all locators against a binary, CI on `.text` dumps | buslan | 🚧 part 1: AOB signatures (`phoecord check`, synthetic tests only); open: typed locators (wait for 2.2.5 part 2) and the CI job on private dumps |
| **2.2.7** 🔴 | In-process transport wired to `Engine`: REST and Gateway through the hook layer, `Effects` delivery | ken | ⏳ planner (PR #104) and the transport around `HookRuntime`: effects delivery, timer clock, per-entry-point fallback; open: calls from the detours (2.2.2, 2.2.3), in-process `Network`, a way to send (2.2.4); the fallback is now per launch, not per entry point (ruling, 2026-11-26, see 2.2.8), so no handoff to the proxy is needed |
| **2.2.8** 🔴 | Phoenix installer and transport choice: `phoecord launch` picks in-process (no proxy) or proxy per launch from a cached signature check, shortcut, reconcile after Phoenix updates, uninstall | buslan | ⏳ all design questions answered in advance (rulings, 2026-11-26; see the plan) |
| **2.2.9** 🔴 | Every signature verified unique on two adjacent builds | buslan | ⏳ |
| **2.2.10** 🟡 | Signature updates without rebuilding the loader; hot-fix path for TOML AOB entries | buslan | ⏳ |
| **2.2.11** 🔴 | Loader check inside Discord Canary (was 1.5.9): one worker per start, in-memory hash equals the file's, under 20 ms, `loader connected` in the proxy log, clean uninstall | ken | ✅ checked on Discord Canary 1.0.1364 (Windows, a release build from `main`, sandboxed `PHOECORD_CONFIG`/`PHOECORD_DATA`): one `client build` line per start (none from the `--type=` children), the `.text` hash matched across 3 restarts, `loader connected` in the proxy log, no crash over three starts plus a 4-minute run, `phoecord uninstall` removed the DLL, shortcut, Run key and CA. **Not met: detection took ~130 ms, not under 20 ms** (consistent across 3 runs; `DiscordCanary.exe` is 224 MiB, so a `.text`-section SHA-256 this slow is plausible — open question for whoever set the 20 ms figure) |

### Stage 2.3. UI hooks and theme engine · `0.9`

| Task | What | Who | Status |
|:--|:--|:--|:--|
| **2.3.1** 🔴 | Theme engine: `theme.toml` → GPUI palette, live switching | matvee | ⏳ part 1 merged, PR #102 (format, colour parsing, 2 s file watcher; the palette hook and default theme wait for 2.1) |
| **2.3.2** 🟡 | 3 built-in themes + import of Vencord themes where they translate to a palette | — | ⏳ |
| **2.3.3** 🔴 | PhoeCord settings tab in Phoenix: plugins, settings, install / remove packages with rights shown, en + ru | matvee | ⏳ |
| **2.3.4** 🔴 | MessageLogger UI: deleted messages struck through, edit history on hover | matvee | ⏳ |
| **2.3.5** 🟡 | Message context menu: plugin action entries in right-click and `...` menus | — | ⏳ |
| **2.3.6** 🟡 | Optimistic delete hook: `Store::delete_message` visible to plugins (MessageLogger sees local deletes) | — | ⏳ |
| **2.3.7** 🔴 | Degradation verified: unknown build → network only, no crashes; missing signature → that feature off only | ken | ⏳ preparation merged, PR #104: scenarios 1 and 2 covered for the plan with fake locator results; nothing verified inside a client |
| **2.3.8** 🟡 | CDN picture hook for USRBG and Decor (was 1.2.14's blocker): native avatar/banner loading by hash, not a REST route | buslan | ⏳ |

### Stage 2.4. Release v1.0 · `1.0`

| Task | What | Who | Status |
|:--|:--|:--|:--|
| **2.4.1** 🔴 | Top 10 plugins on Phoenix (network + UI parts): MessageLogger, FakeNitro, NoTrack, ClearURLs, SilentTyping, ShowHiddenChannels, Translate, PlatformIndicators, PermissionsViewer, ViewIcons | — | ⏳ |
| **2.4.2** 🔴 | Remaining v1.0 scope: every 🌐 and 🌐 + 🎨 plugin complete, the 30 most-used 🎨 plugins, all panels | — | ⏳ |
| **2.4.3** 🔴 | Plugin API stabilization: `API_VERSION = 1` frozen, changes minor-only; additive UI events | Nanda | 🚧 in review, PR #114: v1 frozen with the owner's rulings (Q1–Q8, 2026-11-26): `#[non_exhaustive]` types with `new` constructors, `ChainHost`, `Event::Ui(UiEvent::MenuAction)` + `Manifest.menu` / `[[menu]]`, `LOCAL_DELETE_EVENT`, WIT `ui(ui-event { name, payload })`; `wit/frozen/` and `tests/frozen_wit.rs` guard the WIT. WASM packages built for 0.5 must be rebuilt |
| **2.4.4** 🔴 | Developer docs `docs/plugins/`: quick start (Rust + TS), events, rights, publishing, localization | — | ⏳ |
| **2.4.5** 🔴 | Installer 1.0 for Windows; macOS and Linux with the network layer | buslan | ⏳ |
| **2.4.6** 🔴 | Security audit of the CA, the proxy and the loader | Nanda | ⏳ |
| **2.4.7** 🔴 | v1.0 release: binaries, `SHA256SUMS`, `docs/CHANGELOG.md` | Nanda | ⏳ |
| **2.4.8** 🔴 | macOS check of the 0.5.1 fixes on a real Mac (was 1.4.10): no "damaged", Discord starts and updates through PhoeCord, clean uninstall | ken | ⏳ |

Order: 2.1 → 2.2 → 2.3 → 2.4. Stage 2.1 must finish before 2.2 starts (the loading path, proxy reach and binary findings feed every hook). Stages 2.2 and 2.3 can overlap once the first hooks land (2.2.2–2.2.3). Stage 2.4 starts after 2.3.7 (degradation verified).

## Risks

- **Phoenix restricts DLL search.** The sideload plan changes to a launcher; `version.dll` becomes a fallback for clients that allow it. 2.1.2 answers this first.
- **Hook timing: too late from a worker thread.** Hooks must be in before the first registry read. The fallback is synchronous hooking under the loader lock (acceptable cost: bounded scan, ~1 s on a ~500 MB executable). 2.1.7 tests this.
- **Allocator mismatch.** Replacing a body with the wrong allocator corrupts the heap. The safe fallback is a local reply (`Verdict::Reply`) instead of body replacement, at the cost of losing `Verdict::Changed` for responses. 2.2.2 handles this.
- **Build cadence faster than 48 hours.** If Phoenix ships faster than signatures can be updated, the in-process transport is off more often than on. Mitigated by the proxy fallback and by moving locators from AOB to code (more resilient to layout changes).
- **A panic in a hook kills Discord.** Panic guard at every entry; a failed locator disables only its feature; the "degrade to network only" rule (2.3.7) is tested before release.
- **Antivirus flags the injected DLL.** Same as Phase 1: unsigned, `SHA256SUMS`, Defender false-positive submissions.
- **Account bans.** Client modification breaks Discord's Terms. The risk is the same as Phase 1 and is stated in `TERMS.md`.

## Living docs and publications

- [`docs/CHANGELOG.md`](../CHANGELOG.md): an "Unreleased" line in every PR; versions `0.7`–`1.0` at the end of stages.
- [`ARCHITECTURE.md`](../ARCHITECTURE.md): on transport changes, loader changes, UI hook design.
- [`docs/CODE_MAP.md`](../CODE_MAP.md): every new file, config key and env variable.
- [`docs/PLUGINS.md`](../PLUGINS.md): the plugin's "Where" cell for UI parts.
- README: pre-alpha note updated per stage.
- GitHub Releases: one per stage starting at `0.7`.

## How to start

Read [`AGENTS.md`](AGENTS.md), then [`ai/spec.md`](ai/spec.md) and your task in [`ai/plan.md`](ai/plan.md). Branch from a fresh `main`.

## Where things are

| What | Where |
|:--|:--|
| Plugin API | `crates/phoecord-core/src/plugin.rs` |
| Transport boundary (`Engine`) | `crates/phoecord-transport/src/lib.rs` |
| Plugin chain + WASM runtime | `crates/phoecord-plugin-runtime/src/` |
| Built-in plugins | `crates/phoecord-plugins/src/` |
| Proxy transport | `crates/phoecord-proxy/src/` |
| Loader | `crates/phoecord-loader/src/` |
| AOB scanner | `crates/phoecord-loader/src/aob.rs` |
| IPC | `crates/phoecord-loader/src/ipc.rs`, `pipe.rs` |
| Installer | `crates/phoecord-installer/src/` |
| Installer GUI | `crates/phoecord-installer-gui/src/` |
| CLI | `crates/phoecord-cli/src/main.rs` |
| Plugin SDK | `crates/phoecord-plugin-sdk/src/` |
| Signatures | `signatures/` |
| GPUI research | `docs/research/gpui.md` |
| Phoenix research | `docs/research/phoenix-desktop.md` |
| GPUI reference build | `research/gpui-reference/` |
