Checked against code: Zed `f126950f9d3b62666f97437ae5d4ccac2272c4db` (2026-11-22), reference build in `research/gpui-reference` at PhoeCord `79163b7`, 2026-11-23.

# GPUI research (Phase 1, stage 1.6)

Discord's coming native client, Project Phoenix, is built on Rust and GPUI. PhoeCord's UI layer hooks Phoenix through a proxy `version.dll` and AOB signatures (`crates/phoecord-loader/src/aob.rs`; design in `ARCHITECTURE.md`, "UI layer: the loader"). This note studies GPUI *before* Phoenix ships: where a palette, a settings tab list and a message row are built, what survives in a release binary, and whether file-based themes are plausible.

**Method and limits.** Only Zed/GPUI sources and our own reference build were used. No Discord binary was downloaded, inspected or reversed. Phoenix may pin a different GPUI revision, use its own fork, or wrap GPUI (Zed's `ui` crate is a component layer on top of GPUI, and Discord will have its own). Everything below that names a type or file is therefore a **template for what to look for**, and every signature is verified against the reference build only.

Source links use the pinned Zed commit: `https://github.com/zed-industries/zed/blob/f126950f9d3b62666f97437ae5d4ccac2272c4db/<path>`, written below as `zed:<path>`. At this commit GPUI's platform code is split into `gpui_platform` plus `gpui_windows` (DirectX 11 + DirectWrite) / `gpui_macos` / `gpui_linux`.

See also: [`phoenix-desktop.md`](phoenix-desktop.md), what is known about Desktop Phoenix itself and how a mod plugs into it (learned from sunbird, task 2.1 prep).

## 1. A map of GPUI (1.6.1)

### Objects

| Object | What it is | Source |
|:---|:---|:---|
| `App` | The single application context: all entities, globals (`globals_by_type: TypeId -> Box<dyn Any>`), windows, executors, observers. Runs on the main thread. | `zed:crates/gpui/src/app.rs` (`struct App`, line 884) |
| `Global` / `cx.global::<G>()` | Typed singleton storage on `App`. Panics `no state of type {type_name::<G>()} exists` if absent. This is how Zed stores the active theme. | `zed:crates/gpui/src/app.rs` (lines 2640-2735) |
| `Entity<V>` / `Context<V>` | A reference-counted model or view owned by `App`; `Context<V>` is `&mut App` plus the entity being updated. | `zed:crates/gpui/src/app/context.rs`, `app/entity_map.rs` |
| `Window` | One OS window: platform window, layout engine (taffy), two frames (`rendered_frame`, `next_frame`), focus, input handlers, the root view. | `zed:crates/gpui/src/window.rs` (`struct Window`, line 1232) |
| `Render` | A view's `render(&mut self, &mut Window, &mut Context<Self>) -> impl IntoElement`. Called every frame the view is dirty. | `zed:crates/gpui/src/element.rs` (line 172) |
| `RenderOnce` | A stateless component (`self` by value). Zed's `ui` crate (`ListItem`, `Tab`, `TabBar`, `Button`...) is made of these. | `zed:crates/gpui/src/element.rs` (line 220) |
| `Element` | The low-level three-phase trait `request_layout` / `prepaint` / `paint`. `Div`, `UniformList`, `List`, `Text` implement it. | `zed:crates/gpui/src/element.rs` (line 52) |

### How a frame is built

GPUI is immediate-mode at the view level and retained at the platform level: every dirty window rebuilds its whole element tree each frame.

1. The platform window asks for a frame (`on_request_frame`, `zed:crates/gpui/src/window.rs` line 1908) and calls `Window::draw` (line 3837).
2. `draw` calls `draw_roots` (line 4083): the window's root view is turned into an element (`root.into_any_element()`) and **`Render::render` runs on every dirty view** while the tree is built.
3. The element tree goes through three phases: `request_layout` (builds taffy layout nodes), `prepaint` (bounds known; hitboxes, scroll offsets, lazily-rendered list items), `paint` (pushes quads, shadows, glyph runs, paths into `next_frame.scene`).
4. After `draw_roots` the frames are swapped (`mem::swap(&mut self.rendered_frame, &mut self.next_frame)`, line 3927) and `present_if_needed` (line 4037) hands the `Scene` to the platform renderer (`gpui_windows::directx_renderer`).

Consequences for PhoeCord: (a) `Render::render` bodies run once per frame, so a hook there is hot; (b) colours are read **while building elements**, so a replaced palette shows up on the next frame without any notification, provided the window is redrawn (`cx.refresh_windows()`); (c) lists are built lazily in `prepaint` (`uniform_list`, `list`), so the row closure is called only for visible rows.

### Where theme colours are read

Zed keeps the palette in a global and every component reads it through one accessor:

```rust
// zed:crates/theme/src/theme.rs
pub struct GlobalTheme { theme: Arc<Theme>, icon_theme: Arc<IconTheme> }   // line 322
impl Global for GlobalTheme {}
impl GlobalTheme { pub fn theme(cx: &App) -> &Arc<Theme> { &cx.global::<Self>().theme } }  // line 345
pub trait ActiveTheme { fn theme(&self) -> &Arc<Theme>; }                  // line 148, impl for App at 153
// Theme { id, name, appearance, styles: ThemeStyles { colors: ThemeColors, status, player, syntax, ... } }
```

Components read `cx.theme().colors().<field>` while building elements, for example `.editor_background` and `.element_background` in `zed:crates/settings_ui/src/settings_ui.rs` (lines 1348, 3049). `ThemeColors` (`zed:crates/theme/src/styles/colors.rs`) is a flat struct of `Hsla` fields (`border`, `element_background`, `text`, `surface_background`, ...). Changing the theme is `GlobalTheme::update_theme(cx, theme)` (line 335) followed by a redraw. Any GPUI app following this pattern has one `TypeId` key in `globals_by_type`, one accessor that ends in `cx.global::<T>()`, and a very large number of call sites that read fields out of the result.

### Where a settings tab list and a list row are built

GPUI itself has no "tab list" widget; the pieces live in the app layer, which is why Phoenix's exact shape is unknown. In Zed:

| What | Where it is built | Notes |
|:---|:---|:---|
| Settings window | `impl Render for SettingsWindow`, `zed:crates/settings_ui/src/settings_ui.rs:4656` | Root view of the settings window |
| Settings navigation (the "tab list") | `build_navbar` (line 2127) builds `Vec<NavBarEntry>` from `pages`; `render_nav` (line 3067) renders it; `render_page` (line 3823) renders the selected page | Data-driven: pages are static tables, entries built at runtime, rendering is a loop of `div()` / `ListItem` |
| Tab strip (editor tabs) | `TabBar` (`zed:crates/ui/src/components/tab_bar.rs:8`, `RenderOnce` at 92) and `Tab` | A different widget from settings navigation; Phoenix may use either, or neither |
| Row | `ListItem` (`zed:crates/ui/src/components/list/list_item.rs:32`, `RenderOnce` at 298) | Per-row component: `div().id(..).flex().px_*().child(..)`, colours from `cx.theme().colors()` |
| Lazy list of rows | `uniform_list` (`zed:crates/gpui/src/elements/uniform_list.rs:22`, fixed row height) and `list` (`.../list.rs:24`, variable height) | The row closure is `cx.processor(\|this, range, window, cx\| -> Vec<impl IntoElement>)` |

A chat client's message list is almost certainly a variable-height `list(...)` with a `ListState`, rows built by one render function per message. The reference app mirrors this at small scale (`research/gpui-reference/src/main.rs`): `Theme` global + `ActiveTheme::theme`, `render_tab_list`, `render_message_row`, `uniform_list`.

**Answer to "where would a palette, a settings tab list and a message row be built":**

- *Palette*: `cx.global::<ThemeType>()` behind an accessor, and at the other end `cx.set_global` / `update_global` when the theme changes (in Zed `GlobalTheme::update_theme`, called from `theme_settings::init` and `reload_theme`).
- *Settings tab list*: a view's `render` that loops over a static table of pages (label, id, page builder) and emits one clickable `div` / `ListItem` per entry; the table is a `static`/`const` array of `&str`-bearing structs.
- *Message row*: the closure given to `uniform_list` / `list` (or a `RenderOnce` component it calls), reading colours and text from the theme global per row.

## 2. Reference build (1.6.2)

`research/gpui-reference/`: its own Cargo project (empty `[workspace]` so it never joins ours) plus `exclude = ["research"]` in the root `Cargo.toml`. GPUI comes from `git = zed-industries/zed, rev = f126950f9d3b62666f97437ae5d4ccac2272c4db` (crates `gpui`, `gpui_platform`); `Cargo.lock` is committed. Release profile: `lto = "fat"`, `codegen-units = 1`, `strip = true`, `opt-level = 3`. The README has the exact commands.

**Result: GPUI builds on this Windows machine** (rustc 1.99.1, `x86_64-pc-windows-msvc`, VS 2022 build tools) with no patches; Zed's `[patch.crates-io]` entries were not needed for `gpui` + `gpui_platform` on Windows. A rebuild takes about 1 minute after the first dependency fetch. The binary is 8.7 MB, starts, and stayed running for the 8 seconds it was left up. **No one looked at the window** (no screenshot), so "the themed list and tab view render correctly" is untested. The binary is not committed.

The reference app imitates Zed's theme pattern with its own small `Theme` global instead of depending on Zed's `theme` crate, which pulls in the settings stack. The read path (`cx.global::<T>()` behind `ActiveTheme::theme`) has the same shape; the *names* differ, so the string anchors below must be re-derived for Zed's real `GlobalTheme` and for Phoenix.

## 3. GPUI in a release binary (1.6.3)

Tool: `research/aob-probe/` (PE parser, string / `Location` / xref finder, signature drafter). Every signature was run through `phoecord_loader::aob::Pattern::find_all` over `.text`, so the result is what the loader's `find_unique` would give.

### What survives `lto = "fat"` + `strip = true`

`strip = true` removes symbols, not data.

| Survives | Evidence in the reference binary | Use |
|:---|:---|:---|
| Panic `Location` structs: `{ file: &str, len, line: u32, col: u32 }` in `.rdata`, one per panicking call site | 1,281 structs with a `crates\...` path (116 for `gpui\src\window.rs`, 58 for `elements\div.rs`, 36 for `app.rs`, 55 for `gpui_windows\platform.rs`). The path is **absolute at build time**: `C:\Users\<me>\.cargo\git\checkouts\zed-<hash>\f126950\crates\gpui\src\window.rs` | Identifies GPUI and its **short commit** (`f126950`); `file:line:col` pins one call site to a line of Zed's source at that revision |
| `type_name::<T>()` strings that reach a panic or log message | `gpui_reference::Theme` (from `cx.global::<Theme>()`'s `no state of type {} exists`), `gpui_reference::Settings`, `gpui::app::SystemWindowTabController`, `gpui::view::ViewElementState`, `gpui::elements::div::InteractiveElementState` | The **name of the theme global** is among them |
| Panic and expect messages | `no state of type `, `The entity must be alive if we have a entity context`, `attempted to dereference an ArenaRef after its Arena was cleared`, `we should avoid taffy layout errors by construction if possible`, `DirectXRenderer failed to recover from lost device after multiple attempts`, `failed to initialize Windows platform` | Stable per GPUI version; anchors for the code around them |
| App string literals | `Messages`, `Plugins`, `About`, `Theme: `, `Reference message number ` | The settings tab labels and row text of the *app* |
| Dependency paths | `...\registry\src\index.crates.io-...\accesskit_windows-0.34.0\src\...`, `gif-0.14.2`, `aho-corasick-1.1.5` | The dependency version list for free |

What does **not** survive: function names, `impl` names, and type names that never reach a string. Rust string literals are **not NUL-terminated** and adjacent ones are merged: `messagesMessagesPluginsAbout` is one run in `.rdata`, so "find the string, then its `lea`" works only for a literal whose start address is referenced directly. The tab labels were referenced through a static table of `{ ptr, len, Tab }` entries (24-byte stride) copied with SSE loads (`movups xmm0, [rip+rel]`), not by `lea` to the string.

`Location` paths can be shortened with `--remap-path-prefix` (relative `crates\gpui\src\window.rs`-style paths remain) and removed only with nightly `-Zlocation-detail=none`. Zed is not built that way; assume Phoenix keeps them unless shown otherwise. If it does not, the string anchors below disappear and we fall back to code-shape signatures and `TypeId` constants, which is much harder and a 2.1 finding to record.

### Draft anchors and their checks

Method for all three: find the *data* anchor (type-name string, `Location`, static table), find the single `.text` instruction that references it rip-relatively, take bytes around it, wildcard every rel32 (`call`, `jmp`, `jcc`, `lea` / `mov` / `movups` rip-relative), and require exactly one match in `.text`.

| Target | Data anchor in the reference build | Drafted signature (matches in `.text`) | Where it lands |
|:---|:---|:---|:---|
| Palette lookup | type-name string `gpui_reference::Theme` (1 `lea` reference) | `3D FF FF 00 00 75 4E 48 89 F0 48 83 C4 40 5E C3 48 8D 05 ?? ?? ?? ?? 48 89 44 24 30 48 C7 44 24 38 15 00 00 00 48 8D` (**1**) | The cold panic tail of `<App as ActiveTheme>::theme` (`cx.global::<Theme>()` failing); the accessor's entry is just before it |
| Settings tab list | static table `[("Messages", Tab), ("Plugins", Tab), ("About", Tab)]` in `.rdata` (pointer slots at RVA 0x615518 / 0x615530, stride 24, no `lea` to the strings) | `C7 45 B0 03 00 00 00 48 8D 75 B8 0F 10 05 ?? ?? ?? ?? 0F 11 45 B8 0F 10 05 ?? ?? ?? ?? 0F 11 45 C8 0F 10 05 ?? ?? ?? ?? 0F 11 45 D8 0F 10 05 ?? ?? ?? ?? 0F 11 45 E8` (**1**) | Inside `Settings::render_tab_list`: the loop copies the 3-entry table from `.rdata` to the stack (`03` is the entry count). The stack offsets (`B0`, `B8`, `C8`...) are compiler-specific |
| Message row rendering | `Location` of `gpui\src\elements\uniform_list.rs:370:77`, referenced by one `lea` | `85 17 16 00 00 01 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 90 E9 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 90 E9 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 90` (**1**) | A cluster of cold panic call sites at the end of `UniformList::prepaint`, the code that calls the row closure for the visible range. Contains a stack displacement (`85 17 16 00 00`) that is fragile |

The row's own text (`Reference message number `) is application data, not a GPUI anchor. In Phoenix the row builder would be found by following a `uniform_list.rs` / `list.rs` `Location` into `prepaint`, then to the indirect call of the row closure.

**Stability check.** The three signatures were re-run on a second build of the app (different message count, changed string literals): exactly one match each, at the same offsets. This is a weak test because the code did not move; it shows only that data changes do not break them. They are **not** tested against another compiler version, GPUI revision, or app.

**Quality.** The tab-list and row signatures contain stack-frame constants because nothing stabler sits near the anchor. A better method for a real target is to anchor on the `Location` of a *named, stable* GPUI call site (for example a `window.rs` panic in `Window::draw`) and walk to the function start by pattern. `aob-probe locsig <exe> "gpui\src\window.rs" <line>` produces candidates for any `file:line` of Zed's source; line numbers for a given Phoenix GPUI revision come from the pinned checkout.

## 4. Themes in Zed (1.6.4)

How Zed loads a JSON theme into GPUI:

1. **Format.** A theme file is a *family*: `{ "name", "author", "themes": [ { "name", "appearance": "light"|"dark", "style": { "background", "editor.background", "element.background", "text", "border", "players": [...], "syntax": {...} } } ] }`. Keys are serde renames of `ThemeStyleContent` / `ThemeColorsContent` (`zed:crates/settings_content/src/theme.rs:528`; `element.background` at line 616, `text` at 692, `editor.background` at 852). Colours are `#rrggbb[aa]` strings.
2. **Parse.** `theme_settings::load_user_theme(registry, bytes)` (`zed:crates/theme_settings/src/theme_settings.rs:244`) calls `deserialize_user_theme` (lenient JSON through `serde_json_lenient`, line 252) into a `ThemeFamilyContent` (`zed:crates/theme_settings/src/schema.rs:25`).
3. **Refine.** `refine_theme_family` (line 273) and `refine_theme` turn each `ThemeContent` into a `Theme`: start from built-in defaults (`apply_theme_color_defaults`, `apply_status_color_defaults` in `zed:crates/theme/src/fallback_themes.rs`), apply the file's values, derive the missing ones. Colour strings go through `theme::try_parse_color` (`zed:crates/theme/src/schema.rs:17`).
4. **Register.** `ThemeRegistry::insert_theme_families` (`zed:crates/theme/src/registry.rs:149`). Bundled themes come from `assets/themes/*.json` (`load_bundled_themes`, line 218); user themes from the themes directory.
5. **Activate and reload.** `theme_settings::init` (line 71) calls `theme::init` (creates the `GlobalTheme`) then `GlobalTheme::update_theme(cx, configured_theme(cx))`; `reload_theme` (line 204) repeats it. A file watcher (`watch_themes`, `zed:crates/zed/src/main.rs:2421`) reloads when a file in the themes directory changes, so **a theme edit applies live without a restart**.

**Is a file-based theme plausible for Phoenix?** Probably not as a built-in feature, but a theme-by-file layer is plausible *if* Phoenix has a palette struct, which we do not know:

- GPUI has no theme concept. A theme is the app's choice: a `Global` holding a palette struct (Zed) or colours hard-coded at each call site. File themes exist in Zed because its theme is a data struct filled from JSON. Discord has a large web design-token system and will most likely carry it over as a palette struct or constants.
- If Phoenix has a palette **global**, a plugin does not need Phoenix's file support: replace the struct's contents in memory (section 5, H1b) and request a redraw. PhoeCord's theme file format can then be anything we choose (for example CSS-variable-style JSON mapped onto the struct), so a "file theme" is a PhoeCord feature, not a Phoenix one.
- If colours are `rgb(0x...)` constants inside render functions, a palette hook is impossible without patching each site; theming would reduce to a post-process (colour remap on the `Scene` in the renderer), which is very different work.

**What to check in Phase 2.1** (static inspection of the Phoenix file on the owner's machine, not part of this research):

1. Is there a `Global`-style palette? Look for a type name containing `Theme` / `Palette` / `Colors` / `Tokens` among the `type_name` strings near a `no state of type` / `no global` message, and for large `.rdata` tables of colour constants (consecutive f32 HSLA quadruples or `0xRRGGBBAA`).
2. Is there a JSON/serde theme path: strings like `appearance`, `background.appearance`, `.json` near a `themes` string, `serde_json` panic locations, a themes directory under the user data directory?
3. Is there a file watcher (`notify` crate paths, `ReadDirectoryChangesW`) next to such a path?
4. Does Phoenix keep the web client's token names (`--background-primary`-style) as strings? If so the palette likely maps 1:1 to them, which is also the best vocabulary for PhoeCord theme files.
5. `Location` paths: present, absolute or remapped, and which GPUI short commit do they name? This decides whether section 3's string anchors work at all.

## 5. Hook candidates, risks, and what to do first (1.6.5)

All candidates assume a GPUI app of this shape. Signatures are from the reference build and demonstrate the method only.

| # | Hook | How to find it in a release binary | Confidence it exists in Phoenix | Risks |
|:---|:---|:---|:---|:---|
| H1 | **Palette**: the accessor that reads the theme global (`ActiveTheme::theme`-like) and/or the code that sets it (`set_global` / `update_global`) | Find the theme global's `type_name` string, then its single `lea` in `.text`; take the cold panic tail; walk back to the function entry. Reference signature `3D FF FF 00 00 75 4E ... 48 8D` | **Medium.** A global palette is the dominant GPUI pattern (Zed does it); Discord may use constants | The accessor is tiny and `#[inline]`: after LTO there may be *no* single function, only many inlined copies each with its own panic tail. Then hook the setter or the global's data instead. The name is absent if the global never reaches a panic string |
| H1b | **Palette data in memory**: the entry for the theme `TypeId` in `App::globals_by_type` | Do not hook code. Obtain the `App` pointer (from a hooked function found through a `Location` in `app.rs`), look up the `TypeId` of the theme type (a 128-bit constant that appears as immediates in code calling `global::<T>`), write the colour fields | **Low-medium** | `App` and the hash map are `repr(Rust)`: field order and hashing differ per rustc version, so each build needs an offset-discovery step. Writes need a redraw (`cx.refresh_windows()` equivalent) |
| H2 | **Settings tab list**: the function that builds or renders the navigation entries | Find the tab label strings in `.rdata`, find the 24-byte-stride `{ptr,len,..}` table by scanning for the pointer slots, then the `.text` instruction that reads the table (`lea` or SSE `movups` rip-relative). Reference signature `C7 45 B0 03 00 00 00 48 8D 75 B8 0F 10 05 ?? ...` | **High that a list of labels exists, medium that it is a static table.** Discord's settings are localized, so labels may be message keys or lookups, not literals | Localized strings move the anchor to the key strings. Stack offsets in the signature are build-specific: anchor on the table, not the frame. Adding a tab means splicing into `const` data: hook the *consumer* (the render function) or the page dispatch |
| H3 | **Message row**: the function that renders one message, inside the `list` / `uniform_list` item closure | Find `Location`s for `gpui\src\elements\list.rs` / `uniform_list.rs`, locate `prepaint` / `paint`, then the indirect call through the stored closure (`call [reg+off]`) whose target is the row builder. Or find a unique message-UI string (`(edited)`, `Reply`) and its xref, which lies inside the row function. Reference: `85 17 16 00 00 01 48 8D 0D ?? ...` only shows the cluster near `uniform_list.rs:370` | **Medium-low** for the exact row function: in release builds a row builder is often inlined into its closure and LTO-merged with other closures | The closure is generic over captured state (`cx.processor`): a hook sees an opaque environment pointer and an index or `Range<usize>`, not a typed message. Text is drawn through GPUI's text system, so "modify message content" belongs at the data layer (Gateway, `phoecord-proxy`), not in the UI |

**Ranking.** For Phase 2.2: H2 (strings are visible and a tab can be added without touching drawing) and H1b (colours without code hooking) first; H1 is cheapest to try; H3 last and probably better solved on the data side.

### What to do first in Phase 2.2

1. **Fingerprint before hooking.** Extend `aob-probe` into a read-only fingerprint report for the Phoenix binary (run on the owner's machine, output not committed): GPUI short commit from `Location` paths, whether paths are remapped, the `type_name` strings (`gpui::`, theme-like names), the dependency version list, and whether the image is packed or protected (high-entropy `.text`, protector sections would end this plan early). It decides every later step.
2. **Rebuild the reference at Phoenix's GPUI commit.** Check out Zed at that exact commit, rebuild `research/gpui-reference` against it, and re-derive section 3's anchors there. If Discord uses a fork, record it and fall back to the nearest Zed commit by `Location` line numbers (line numbers shift with the revision, a cheap revision fingerprint).
3. **H1 as read-only first.** Locate the palette accessor, log the returned pointer, dump the colour struct to confirm the field layout; no write, no inline hook. This validates the chain proxy DLL -> AOB -> function -> data at low risk. Only then H1b (writing colours) and H2.
4. **Make signatures testable.** Each signature gets a unit test against a byte range dumped from the reference build (as the `aob.rs` doc comment describes), so a signature that stops matching after a GPUI bump fails CI, not the user.
5. **Integrity check.** Find out early whether Phoenix hashes its own `.text` or restarts through a launcher; inline hooks need that answer, and data-only (H1b) or import-table approaches avoid it.

### Unverified

- No Phoenix binary was examined (by rule). Discord's palette shape, tab labels, row builder, and whether `Location` paths survive are hypotheses for 2.1.
- Signatures were validated only on the reference build, with a weak variation test (same code, changed data). A different rustc, GPUI revision, `-Cpanic=abort`, PGO or `target-cpu` can move or reshape them.
- The reference app's window was never looked at; it launched and ran for 8 seconds.
- `Location` path presence depends on build flags; the reference uses defaults.
- Only Windows (`gpui_windows`, DirectX 11 + DirectWrite) was built. A renderer-level hook (theming through the `Scene`) was not studied.
- Zed's theme loading was read from source, not run; whether Phoenix keeps anything like Zed's `theme` crate is unknown.

### Roadmap items this unblocks

Phase 2.1 (checklist in section 4 plus the fingerprint step above) and 2.2 (hook order above). `ROADMAP.md` should link this note from 2.2 (see "Living docs" in the PR).
