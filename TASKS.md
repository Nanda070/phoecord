# Tasks

Generated from the "Tasks" tables in `docs/<plan>/README.md` by `python scripts/gen_tasks.py`; never edit by hand. Change a task in its plan's README, then rerun the script. CI fails if this file is stale.

Tasks: 51, plans with tasks: 1.

## All tasks

| Plan | Task | What | Who | Status |
|:--|:--|:--|:--|:--|
| [phase-1](docs/phase-1/README.md) | 1.0.1 🔴 | Merge the Phase 1 plan, the docs structure and the English-only docs | Nanda | 🔄 branch `docs/phase-1-plan` |
| [phase-1](docs/phase-1/README.md) | 1.0.2 🟡 | Dependabot: close #1, merge #2 and #3 after green CI | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.1 🔴 | Proxy `Host`: settings, KV, `self_id`, `notify` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.2 🔴 | `config.toml` with `[plugins.<id>]` sections and live reload | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.3 🔴 | Account state behind `Host` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.4 🔴 | Proxy: `CONNECT`, loopback only, MITM by `intercept` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.5 🔴 | REST through `Chain` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.6 🔴 | Gateway through `Chain` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.7 🔴 | CLI: `run`, `launch`, `setup`, `trust`, `untrust`, `plugins`, `enable`, `disable`, `paths` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.8 🟡 | `selftest` against the real Discord | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.9 🟡 | Gateway fixtures without personal data | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.1.10 🟡 | Chain benchmark: ≤ 1 ms p99 | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.2.1 🔴 | `silent_typing` | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.2.2 🟡 | `unindent` | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.2.3 🟡 | `game_activity_toggle` | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.2.4 🟡 | `relationship_notifier` | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.2.5 🔴 | `fake_nitro` (emoji) | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.2.6 🔴 | `message_logger` and `phoecord log` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.2.7 🔴 | `clear_urls` (new) | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.2.8 🟡 | `/phoecord` slash commands (new) | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.2.9 🟢 | `phoecord import-vencord` | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.3.1 🔴 | `wit/phoecord.wit` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.3.2 🔴 | WASM backend for `Chain` on wasmtime | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.3.3 🔴 | Limits: time, memory, disable on panic | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.3.4 🔴 | Packages `plugin.toml` + `plugin.wasm`, hot loading | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.3.5 🔴 | Rights from the manifest | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.3.6 🟡 | SDK: bindings, macro, `examples/hello-plugin` | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.3.7 🟡 | TypeScript plugin template | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.3.8 🔴 | `clear_urls` as a WASM plugin | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.4.1 🔴 | Client discovery on 3 OSes | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.4.2 🔴 | CA and proxy autostart | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.4.3 🟡 | Installer GUI | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.4.4 🔴 | Full rollback | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.4.5 🟡 | Releases: builds, `SHA256SUMS`, signing | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.5.1 🔴 | `version.dll`: export forwarding | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.5.2 🔴 | `DllMain` and the worker thread | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.5.3 🔴 | Client version detection and `signatures/*.toml` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.5.4 🔴 | Inline detours and hook removal | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.5.5 🟡 | Loader ↔ proxy IPC | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.5.6 🟡 | Reinstall after a Discord update | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.5.7 🔴 | Verify on 3 client versions | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.6.1 🔴 | A map of GPUI | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.6.2 🔴 | GPUI reference build | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.6.3 🟡 | What of GPUI is visible in a release binary | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.6.4 🟡 | Themes in Zed and what to expect from Phoenix | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.6.5 🔴 | Report `docs/research/gpui.md` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.7.1 🔴 | `phoecord-i18n` on Fluent, fallback to English | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.7.2 🔴 | `language` setting, OS default | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.7.3 🔴 | Russian translation of CLI, notifications, installer | anyone | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.7.4 🟡 | `Host::t()` and per-plugin `.ftl` | Nanda | ⏳ |
| [phase-1](docs/phase-1/README.md) | 1.7.5 🟡 | CI check: every `en` key exists in `ru` | anyone | ⏳ |

## Nanda

| Plan | Task | What | Status |
|:--|:--|:--|:--|
| phase-1 | 1.0.1 🔴 | Merge the Phase 1 plan, the docs structure and the English-only docs | 🔄 branch `docs/phase-1-plan` |
| phase-1 | 1.0.2 🟡 | Dependabot: close #1, merge #2 and #3 after green CI | ⏳ |
| phase-1 | 1.1.1 🔴 | Proxy `Host`: settings, KV, `self_id`, `notify` | ⏳ |
| phase-1 | 1.1.2 🔴 | `config.toml` with `[plugins.<id>]` sections and live reload | ⏳ |
| phase-1 | 1.1.3 🔴 | Account state behind `Host` | ⏳ |
| phase-1 | 1.1.4 🔴 | Proxy: `CONNECT`, loopback only, MITM by `intercept` | ⏳ |
| phase-1 | 1.1.5 🔴 | REST through `Chain` | ⏳ |
| phase-1 | 1.1.6 🔴 | Gateway through `Chain` | ⏳ |
| phase-1 | 1.1.7 🔴 | CLI: `run`, `launch`, `setup`, `trust`, `untrust`, `plugins`, `enable`, `disable`, `paths` | ⏳ |
| phase-1 | 1.1.8 🟡 | `selftest` against the real Discord | ⏳ |
| phase-1 | 1.1.10 🟡 | Chain benchmark: ≤ 1 ms p99 | ⏳ |
| phase-1 | 1.2.5 🔴 | `fake_nitro` (emoji) | ⏳ |
| phase-1 | 1.2.6 🔴 | `message_logger` and `phoecord log` | ⏳ |
| phase-1 | 1.2.8 🟡 | `/phoecord` slash commands (new) | ⏳ |
| phase-1 | 1.3.1 🔴 | `wit/phoecord.wit` | ⏳ |
| phase-1 | 1.3.2 🔴 | WASM backend for `Chain` on wasmtime | ⏳ |
| phase-1 | 1.3.3 🔴 | Limits: time, memory, disable on panic | ⏳ |
| phase-1 | 1.3.4 🔴 | Packages `plugin.toml` + `plugin.wasm`, hot loading | ⏳ |
| phase-1 | 1.3.5 🔴 | Rights from the manifest | ⏳ |
| phase-1 | 1.3.8 🔴 | `clear_urls` as a WASM plugin | ⏳ |
| phase-1 | 1.4.1 🔴 | Client discovery on 3 OSes | ⏳ |
| phase-1 | 1.4.2 🔴 | CA and proxy autostart | ⏳ |
| phase-1 | 1.4.3 🟡 | Installer GUI | ⏳ |
| phase-1 | 1.4.4 🔴 | Full rollback | ⏳ |
| phase-1 | 1.4.5 🟡 | Releases: builds, `SHA256SUMS`, signing | ⏳ |
| phase-1 | 1.5.1 🔴 | `version.dll`: export forwarding | ⏳ |
| phase-1 | 1.5.2 🔴 | `DllMain` and the worker thread | ⏳ |
| phase-1 | 1.5.3 🔴 | Client version detection and `signatures/*.toml` | ⏳ |
| phase-1 | 1.5.4 🔴 | Inline detours and hook removal | ⏳ |
| phase-1 | 1.5.5 🟡 | Loader ↔ proxy IPC | ⏳ |
| phase-1 | 1.5.6 🟡 | Reinstall after a Discord update | ⏳ |
| phase-1 | 1.5.7 🔴 | Verify on 3 client versions | ⏳ |
| phase-1 | 1.6.1 🔴 | A map of GPUI | ⏳ |
| phase-1 | 1.6.3 🟡 | What of GPUI is visible in a release binary | ⏳ |
| phase-1 | 1.6.5 🔴 | Report `docs/research/gpui.md` | ⏳ |
| phase-1 | 1.7.1 🔴 | `phoecord-i18n` on Fluent, fallback to English | ⏳ |
| phase-1 | 1.7.2 🔴 | `language` setting, OS default | ⏳ |
| phase-1 | 1.7.4 🟡 | `Host::t()` and per-plugin `.ftl` | ⏳ |

## anyone

| Plan | Task | What | Status |
|:--|:--|:--|:--|
| phase-1 | 1.1.9 🟡 | Gateway fixtures without personal data | ⏳ |
| phase-1 | 1.2.1 🔴 | `silent_typing` | ⏳ |
| phase-1 | 1.2.2 🟡 | `unindent` | ⏳ |
| phase-1 | 1.2.3 🟡 | `game_activity_toggle` | ⏳ |
| phase-1 | 1.2.4 🟡 | `relationship_notifier` | ⏳ |
| phase-1 | 1.2.7 🔴 | `clear_urls` (new) | ⏳ |
| phase-1 | 1.2.9 🟢 | `phoecord import-vencord` | ⏳ |
| phase-1 | 1.3.6 🟡 | SDK: bindings, macro, `examples/hello-plugin` | ⏳ |
| phase-1 | 1.3.7 🟡 | TypeScript plugin template | ⏳ |
| phase-1 | 1.6.2 🔴 | GPUI reference build | ⏳ |
| phase-1 | 1.6.4 🟡 | Themes in Zed and what to expect from Phoenix | ⏳ |
| phase-1 | 1.7.3 🔴 | Russian translation of CLI, notifications, installer | ⏳ |
| phase-1 | 1.7.5 🟡 | CI check: every `en` key exists in `ru` | ⏳ |
