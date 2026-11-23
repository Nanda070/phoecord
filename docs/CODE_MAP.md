# Code map: which file does what

Checked against code: `6b3a7f5`, 23.11.2026. Whoever re-checks this document (or a section of it) against the code updates this line (rule in `AGENTS.md`).

A reference to the repository: which file holds what, why it exists and what is easy to break in it. Also every config key, environment variable and on-disk file the app uses. This is a living document and describes the `main` branch.

> **Mandatory for people and AI agents.** Added, renamed, moved or removed a crate, module, file, config key or environment variable: update this file in the same commit. A change after which this map is wrong is not done. The rule is in [`AGENTS.md`](AGENTS.md).

Related: [`ARCHITECTURE.md`](ARCHITECTURE.md) explains how the layers work; [`PROJECT_MAP.md`](PROJECT_MAP.md) lists what the product does for people.

**How to read.** `phoecord-core` is pure data and contracts, no I/O. `phoecord-proxy` talks to the network. `phoecord-plugins` holds plugin logic only. `phoecord-loader` is the only crate allowed to use `unsafe`.

---

## 1. Repository root

| Path | What it is |
|:---|:---|
| `Cargo.toml` | Workspace: members `crates/*`, default member `phoecord-cli`, shared `[workspace.package]` (edition 2024, MSRV 1.88, GPL-3.0-or-later), shared dependency versions, lints, release profile |
| `Cargo.lock` | Locked versions; committed because the workspace ships binaries |
| `README.md` | Product overview |
| `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md` | Process, security policy, conduct |
| `LICENSE`, `NOTICE`, `LICENSES/` | GPL-3.0 text; copyright and exceptions; MIT notices for the v0 code (`MIT-v0.txt`) and the plugin SDK (`MIT-sdk.txt`) |
| `assets/logo.svg` | Logo used in the README |
| `.github/workflows/ci.yml` | CI: rustfmt, generated docs freshness, check/clippy/test on Linux, Windows, macOS, MSRV 1.88 |
| `.github/ISSUE_TEMPLATE/` | Bug, feature and plugin forms; blank issues disabled |
| `.github/PULL_REQUEST_TEMPLATE.md` | PR form with the living-docs checklist |
| `.github/dependabot.yml` | Weekly cargo updates (grouped), monthly GitHub Actions updates; Dependabot opens the `dependabot/*` branches |
| `.editorconfig`, `.gitattributes`, `.gitignore` | LF everywhere, 4-space Rust, 2-space YAML/TOML; secrets and dumps ignored |
| `scripts/` | §4 |
| `docs/` | §5 |

## 2. Crates

| Crate | Path | Kind | Depends on (ours) |
|:---|:---|:---|:---|
| `phoecord-core` | `crates/phoecord-core/` | lib | none |
| `phoecord-proxy` | `crates/phoecord-proxy/` | lib | core |
| `phoecord-plugin-runtime` | `crates/phoecord-plugin-runtime/` | lib | core |
| `phoecord-plugin-sdk` | `crates/phoecord-plugin-sdk/` | lib (guest side) | none, by design |
| `phoecord-plugins` | `crates/phoecord-plugins/` | lib | core |
| `phoecord-loader` | `crates/phoecord-loader/` | cdylib + rlib | none |
| `phoecord-installer` | `crates/phoecord-installer/` | lib | core |
| `phoecord-cli` | `crates/phoecord-cli/` | bin `phoecord` | core, proxy, plugin-runtime, plugins |
| `phoecord-i18n` | `crates/phoecord-i18n/` | lib | none |

## 3. Files by crate

### `phoecord-core`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | Re-exports `Val` and the Plugin API types | Anything re-exported here is public API for plugins |
| `src/val.rs` | `Val`: one value type for JSON and ETF; keeps atoms, big ints, binaries, so ETF round-trips unchanged | Changing a variant changes how every Gateway frame re-encodes |
| `src/etf.rs` | Erlang External Term Format decode/encode (the desktop client's Gateway encoding) | Compressed terms (tag 80) need `flate2` |
| `src/route.rs` | `route_of` (`/api/v9/x` → `/x`), `message_route_channel` | Plugins match on the normalized route |
| `src/plugin.rs` | Plugin API v1: `API_VERSION`, `Layer`, `Manifest`, `Event`, `EventKind`, `Verdict`, `Host`, `Plugin` | **The contract.** Changes need an `api` PR and the owner's review |

### `phoecord-proxy`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | Module list and status | |
| `src/codec.rs` | `Codec` from the Gateway query (`encoding`, `compress`), `Inflater` / `Deflater` for zlib-stream and zstd-stream | Compression context is per connection: once a frame changes, everything after must use our context |
| `src/ca.rs` | `Authority`: local CA load-or-create, name constraint (`CONSTRAINED_TO`), leaf certificates per SNI, rustls `ServerConfig` | `CA_NAME` is what `untrust` looks for; tests check the constraint blocks other domains |
| `src/trust.rs` | Install/remove the CA: Windows `certutil -user`, macOS keychain, Linux NSS (incl. Flatpak, Snap) | Platform code behind `cfg`; build on all 3 OSes before merging |

Not yet ported (Phase 1, 1.1): `proxy.rs`, `rest.rs`, `gateway.rs`, `state.rs`, `config.rs`, `selftest.rs`, `launch.rs`, `notify.rs` (see [`phase-1/ai/spec.md`](phase-1/ai/spec.md) §2).

### `phoecord-plugin-runtime`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | `Chain` (plugins in run order; first `Drop`/`Reply` stops), `Outcome`, `NullHost` for tests | The stop rule is part of the API semantics |

### `phoecord-plugin-sdk`

| File | What |
|:---|:---|
| `src/lib.rs` | `API_VERSION` only; bindings and macro come in 1.3. MIT-licensed (`license = "MIT"`), unlike the rest of the workspace |

### `phoecord-plugins`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | `builtin()`: every built-in plugin in default order; uniqueness test for ids | Order is run order |
| `src/no_track.rs` | NoTrack: answers analytics and crash-report routes locally with 204 | `wants` must stay as narrow as `on_event` |

### `phoecord-loader`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | Module list and design notes | |
| `src/aob.rs` | `Pattern`: parse `48 8B ?? …`, `find_all`, `find_unique` (ambiguous = miss) | All-wildcard patterns are rejected on purpose |

### `phoecord-installer`

| File | What |
|:---|:---|
| `src/lib.rs` | `Channel` (Stable, PTB, Canary, Phoenix); logic comes in 1.4 |

### `phoecord-i18n`

| File | What | Easy to break |
|:---|:---|:---|
| `src/lib.rs` | `Localizer` (`new`, `t`, `t_args`, `language`), `normalize`, `detect_os_language`, `SUPPORTED` | Bundles are `include_str!`-ed: a new language needs a `source()` arm |
| `i18n/en/main.ftl`, `i18n/ru/main.ftl` | Shared strings; English is the source | Every `en` key needs a `ru` twin with the same variables (`check_i18n.py`) |

### `phoecord-cli`

| File | What |
|:---|:---|
| `src/main.rs` | Scaffold: prints version, built-in plugin ids and the CA name |

## 4. Scripts

| Script | What |
|:---|:---|
| `scripts/check.sh`, `scripts/check.ps1` | All checks in order, stop at first failure (see `AGENTS.md`) |
| `scripts/gen_tasks.py` | Builds `docs/TASKS.md` from the "Tasks" tables of `docs/<plan>/README.md`; `--check` exits 1 if stale |
| `scripts/check_i18n.py` | Every key of `crates/*/i18n/en/*.ftl` exists in the `ru` file with the same variables; exit 1 otherwise |
| `scripts/gen_decisions.py` | Builds `docs/DECISIONS.md` from `Ruling:` lines of `docs/<plan>/ai/ledger.md`; `--check` exits 1 if stale |

## 5. Docs

See [`README.md`](README.md) in this folder. Project-level documents live here too: [`ARCHITECTURE.md`](ARCHITECTURE.md), [`ROADMAP.md`](ROADMAP.md), [`AGENTS.md`](AGENTS.md) + [`CLAUDE.md`](CLAUDE.md) (rules for people and AI agents) and the generated [`TASKS.md`](TASKS.md). [`PORTING_PLUGINS.md`](PORTING_PLUGINS.md) is the porting guide; [`PLUGINS.md`](PLUGINS.md) is the plugin catalogue (every Vencord plugin and userplugin mapped to a layer and a task).

## 6. Config, environment and files on disk

Planned in 1.1; the table is filled as each item lands.

| Name | Kind | Default | Used by |
|:---|:---|:---|:---|
| `listen` | `config.toml` | `127.0.0.1:18463` | proxy |
| `intercept` | `config.toml` | Discord API and Gateway hosts | proxy |
| `language` | `config.toml` | OS language, fallback `en` | all user-facing text (1.7) |
| `[plugins.<id>]` | `config.toml` | per plugin | `Host::setting` |
| `PHOECORD_CONFIG`, `PHOECORD_DATA` | env | OS config/data dirs | paths |
| `PHOECORD_LOG` | env | `phoecord=info` | logging |
| `PHOECORD_NO_NOTIFY` | env | unset | `Host::notify` |
| `ca.pem`, `ca.key` | data folder | created on first run | `phoecord-proxy::ca` |
