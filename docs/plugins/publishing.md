# Publishing a plugin

Checked against code: `24e0362`, 26.11.2026.

How a plugin is packaged, what goes into `plugin.toml`, how to version it, what the v1 freeze promises, and how users get it. Back to the [index](README.md).

Sources: [`package.rs`](../../crates/phoecord-plugin-runtime/src/package.rs) (the manifest), [`store.rs`](../../crates/phoecord-catalog/src/store.rs) (what an install accepts), [`catalog/README.md`](../../catalog/README.md), [`ARCHITECTURE.md`](../ARCHITECTURE.md) "Plugin API v1" and "Licensing of plugins".

## The package

A package is a folder (or a `.zip` of one):

```
my_plugin/
├── plugin.toml            required
├── plugin.wasm            required: the component
├── README.md              optional, kept
├── LICENSE                optional, kept (also LICENSE.md, LICENSE.txt)
└── i18n/
    ├── en.ftl             optional: your strings, see Localization
    └── ru.ftl
```

- The folder name equals `id` (`plugins/<id>/`). Installing from a path or a zip names the folder for you.
- In a zip, `plugin.toml` sits at the top or inside one folder. Entries that climb out of the archive (`..`) are refused.
- **Only the files above are kept**; anything else in the zip or folder is ignored when PhoeCord installs it. A plugin cannot ship extra data files; it reads its settings with `setting` and keeps its own state with the `kv` and `data` rights.
- Limits: the download and the unpacked package are each at most **32 MB** (a TypeScript component is about 12 MB); at most **16** `i18n/*.ftl` files, each at most **256 KiB**.
- A zip for the catalog: `zip -r my_plugin.zip plugin.toml plugin.wasm README.md LICENSE i18n` (any zip tool will do). Try it with `phoecord install my_plugin.zip` before you publish it.

## `plugin.toml`

The parser refuses **unknown fields** (at the top level and inside `[[menu]]` and `[[setting]]`), so a typo is an error, not a silent no-op. The format is documented at the top of [`package.rs`](../../crates/phoecord-plugin-runtime/src/package.rs); this is the full list.

```toml
id = "clear_urls_wasm"
name = "ClearURLs (WASM example)"
version = "0.1.0"
description = "Strips tracking parameters from links you send"
authors = ["someone"]
vencord = "ClearURLs"
layers = ["network"]
api = 1

events = [
    "rest_request:POST /channels/*/messages",
    "rest_request:PATCH /channels/*/messages/*",
    "ui:menu_action",          # only needed because of [[menu]] below
]
rights = []
fetch = []

[[menu]]
id = "copy_raw"
label = "menu-copy-raw"
```

| Field | Required | Meaning |
|:---|:---|:---|
| `id` | yes | `snake_case`: lower-case letters, digits and `_`, starting with a letter. Unique, equal to the folder name, and the section name `[plugins.<id>]` in the user's `config.toml`. At most 64 characters in the catalog. **A built-in plugin with the same id wins over your package**, so pick a distinct one (the WASM example is `clear_urls_wasm` for that reason) |
| `name` | yes | Display name |
| `version` | yes | Your plugin's version. The catalog requires [semver](https://semver.org/) and the same version in `plugin.toml` as in its entry |
| `description` | no | One English sentence |
| `authors` | no | List of names |
| `vencord` | no | The Vencord plugin this one mirrors, if any |
| `layers` | no, default `["network"]` | `network` (REST and gateway traffic), `theme`, `ui` (Phoenix UI hooks). Anything else fails to load |
| `api` | yes | The Plugin API version you built against. **Must be `1`**: another value is refused |
| `events` | no, default none | What you may see; filter syntax in [Events](events.md). With none, nothing is delivered |
| `rights` | no, default none | Which `Host` calls you may make; [Rights](rights.md). An unknown name fails to load |
| `fetch` | no | Outside hosts for the `fetch` right: `"api.example.com"` or `"*.example.com"` (exactly one more label). Needs `rights = ["fetch", …]` too |
| `[[menu]]` | no | One table per message context-menu entry (Phoenix, task 2.3.5): `id` (`snake_case`, unique in the plugin) and `label` (a key of your `i18n/<lang>.ftl`, not empty). Also list `"ui:menu_action"` in `events` to receive the clicks ([Events](events.md#menu_action)) |

Things that are **not** in `plugin.toml`:

- **No `[i18n]` table.** Strings are `i18n/<lang>.ftl` files next to the manifest ([Localization](localization.md)). `name` and `description` are English; they are not translated.
- **No `[settings]` table.** A plugin cannot declare settings today. The user sets them under `[plugins.<id>]` in `config.toml` and you read them with `setting(key)` (`None` when unset, so keep your defaults in code and describe the keys in your README).
- **No WIT `manifest` fields for `fetch` or `menu`.** The WIT `manifest` record is frozen (`id`, `name`, `version`, `description`, `authors`, `vencord`, `layers`, `api`); the rest comes from `plugin.toml`, which also wins over what your `manifest()` returns.

## Versioning

Two version numbers are in play, and only one is yours:

- **`api`** is the Plugin API the plugin targets. It is `1`, it is frozen, and it changes only on a new major API.
- **`version`** is your plugin's own. Use semver: bump it in `plugin.toml` for every release. `phoecord update` offers an update when the catalog lists a higher semver than the installed one; an update keeps the plugin's data (`kv.json` and any other file it wrote in its folder) and whether it was switched on.

### What API v1 promises

The Plugin API was frozen at v1 on 26.11.2026 (task 2.4.3). **A plugin built against v1 keeps loading and running in every later 1.x PhoeCord.** What that rests on:

- `wit/phoecord.wit` is the contract. wasmtime type-checks a component's imports and exports when it loads, so any `variant`, `enum` or `record` with one case or field more or less than the host's would refuse every plugin built before it. The frozen copy is [`wit/frozen/phoecord-1.0.0.wit`](../../wit/frozen/phoecord-1.0.0.wit), and `crates/phoecord-plugin-runtime/tests/frozen_wit.rs` fails when `interface types` or the world drift from it, when a host function changes, or when a guest built against the frozen file no longer loads.
- **Frozen, never changing in 1.x:** the five `event` cases, the `event-kind` cases, the four `verdict` cases, the fields of `manifest`, `outcome`, `tree` / `node`, `host-response`, `file-part`, `rest-kind` and the others, and the signature of every existing host function.
- **May still grow in 1.x, without breaking what you built:**
  - a **new host function** at the end of `interface host` (you only have it if you build against the newer WIT);
  - a **new UI event name** under the generic `ui(ui-event { name, payload })` case;
  - a **new dispatch name**, like `PHOECORD_TIMER`;
  - the Rust side of PhoeCord itself (new fields of `Manifest`, `HostResponse`; new `#[non_exhaustive]` variants), which a WASM plugin never sees.
- **Anything else is a new major version** (`phoecord:plugin@2.x`, `api = 2`); the host then keeps an adapter for the previous major for one cycle.

What it means for you:

- **Ignore what you do not know.** Match `ui` events by name and dispatches by name, and return `pass` for the rest; do not assume the list of names is closed.
- **Do not depend on error text.** Fallible host calls return `result<_, string>`; the text is for logs and people.
- **A newer host function needs a newer PhoeCord.** If you build against a WIT that has a function an older PhoeCord lacks, calling it on that older PhoeCord does not work. Say in your README which PhoeCord version you need.
- **Packages built before the freeze must be rebuilt.** A WASM package built for PhoeCord 0.5 or earlier (before the `ui` case was added to the WIT) is refused with `the plugin was not built against Plugin API v1 (wit/phoecord.wit); rebuild it with the current phoecord-plugin-sdk`. Update the SDK dependency and run the build again; the source needs no change unless you match `Event` exhaustively: the `Ui` case that the freeze added needs an arm (`Event::Ui(_) => …`).

## Releasing

1. Build in release mode (`cargo build --release --target wasm32-wasip2` or `npm run build`) and check the component with `phoecord install <folder>` on a clean profile.
2. Bump `version`, write a changelog line in your README.
3. Zip `plugin.toml`, `plugin.wasm`, `README.md`, `LICENSE` and `i18n/` as above.
4. Publish the zip as a **fixed release file** (a GitHub release asset, for example), never a link to a branch: a hash must stay true.
5. Hash it (`sha256sum my_plugin.zip`; PowerShell: `Get-FileHash my_plugin.zip -Algorithm SHA256`, lower-case hex).

## How users install it

| Way | Command | Notes |
|:---|:---|:---|
| From the catalog | `phoecord catalog`, then `phoecord install <id>` | Reviewed; the download is checked against the listed SHA-256 **before it is unpacked**; the package must have the entry's `id` and `version` and may not ask for a right the entry does not list |
| From a link | `phoecord install https://…/my_plugin.zip` | Not reviewed, hash not checked: PhoeCord warns |
| From a file or folder | `phoecord install ./my_plugin.zip` or `./my_plugin/` | Same warning |
| By hand | copy `plugin.toml` and `plugin.wasm` to `plugins/<id>/` (`phoecord paths`) and set `enabled = true` under `[plugins.<id>]` | Hot-loaded, no restart |
| From the installer window | Plugins tab: the Catalog list, and a zip or folder picker | The same checks and the same rights dialog |

Every install shows what the plugin sees and can do and asks first ([Rights](rights.md), "What the user sees"); `--yes` skips the question. `phoecord update [id]` installs newer catalog versions of what came from the catalog; `phoecord remove <id>` deletes the package and its data. A first install switches the plugin on; an update keeps it as it was.

### Getting into the catalog

[`catalog/plugins.json`](../../catalog/plugins.json) lists community plugins. PhoeCord reads it from the `main` branch at run time, so a merged entry is live at once. It ships empty today. To add yours, open a pull request that changes only that file, with an entry like:

```json
{
  "id": "my_plugin",
  "name": "My plugin",
  "author": "Your name",
  "description": "One sentence on what it does.",
  "version": "1.0.0",
  "download": "https://github.com/you/my_plugin/releases/download/v1.0.0/my_plugin.zip",
  "sha256": "64 lower-case hex digits",
  "rights": ["notify"]
}
```

Entries are sorted by `id`; `rights` are exactly the names in your `plugin.toml`. Put the source link and the build steps in the PR text: a maintainer rebuilds the package, compares the hash, reads the code and the rights, and merges. Unneeded rights, `rest` and `emit_send`, and downloads from places the author may change later are the usual reasons to ask for changes. To release an update, raise `version`, `download` and `sha256` of your entry. Details and the check CI runs: [`catalog/README.md`](../../catalog/README.md).

## Licensing

PhoeCord is GPL-3.0-or-later. The pieces you build against are **MIT**: `crates/phoecord-plugin-sdk`, `crates/phoecord-plugin-sdk-macros`, the WIT contract in `wit/` and the plugin templates (`docs/_template/code/`; the notice is [`LICENSES/MIT-sdk.txt`](../../LICENSES/MIT-sdk.txt), listed in [`NOTICE`](../../NOTICE)). Your plugin only talks to PhoeCord through that contract, so **it may use any license**: ship a `LICENSE` file with it.

If your plugin adapts Vencord code (GPL-3.0-or-later) or another GPL project, the plugin is a derivative work under GPL-3.0 as well: keep the original copyright line in a comment at the top of the file ([`PORTING_PLUGINS.md`](../PORTING_PLUGINS.md) §0). Do not adapt code whose license is not GPL-3.0-compatible.

## Before you publish

- [ ] `id` is `snake_case`, equals the folder name and does not clash with a built-in plugin (see [`PLUGINS.md`](../PLUGINS.md))
- [ ] `api = 1`, a semver `version`, a one-sentence English `description`
- [ ] `events` is as narrow as the plugin allows; `rights` lists only what it calls; `fetch` lists only the hosts it needs
- [ ] Nothing logs message text, tokens or `request_query()`
- [ ] The plugin works with a setting missing (defaults in code) and with a host call failing (every `result` handled)
- [ ] It loads from a zip (`phoecord install my_plugin.zip`) and behaves after Ctrl+R in Discord
- [ ] The README says what it does, which settings it reads under `[plugins.<id>]`, and which PhoeCord version it needs
