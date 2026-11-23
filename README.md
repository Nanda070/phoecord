<p align="center">
  <img src="assets/logo.svg" alt="PhoeCord" width="72" height="72" />
</p>

<h1 align="center">PhoeCord</h1>

<p align="center">
  <strong>A next-generation native mod loader for Discord.</strong><br />
  Vencord-style plugins for the Project Phoenix client (Rust + GPUI), and for today's Discord too.
</p>

<p align="center">
  <a href="https://github.com/nanda070/phoecord/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/nanda070/phoecord/ci.yml?branch=main&style=for-the-badge&label=CI" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-3da639?style=for-the-badge" alt="MIT License" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-2024-dea584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust 2024" /></a>
  <a href="ROADMAP.md"><img src="https://img.shields.io/badge/Status-pre--alpha-ff8a1f?style=for-the-badge" alt="Status: pre-alpha" /></a>
</p>

<p align="center">
  <a href="ARCHITECTURE.md">Architecture</a> ·
  <a href="ROADMAP.md">Roadmap</a> ·
  <a href="docs/phase-1/README.md">Phase 1 plan</a> ·
  <a href="docs/README.md">Docs</a> ·
  <a href="CONTRIBUTING.md">Contributing</a> ·
  <a href="SECURITY.md">Security</a> ·
  <a href="docs/CHANGELOG.md">Changelog</a>
</p>

---

## Why PhoeCord

Vencord patches the JavaScript of Discord's web client. Project Phoenix is a native Rust and GPUI binary: there is no JavaScript to patch, and Rust has no stable ABI, so every client build lays its code out differently. PhoeCord answers this with a **hybrid architecture**: everything that can be done with data happens on the network and does not depend on the client build; we reach into the client itself only where the UI needs it.

| | |
|:---|:---|
| **Network layer** | A local Rust MITM proxy between the client and Discord: REST and Gateway (JSON/ETF, zlib/zstd). `NoTrack`, `SilentTyping`, `MessageLogger`, `FakeNitro`, `ClearURLs`, custom slash commands. Survives client updates |
| **UI layer** | A native loader: proxy DLL (`version.dll` / `winhttp.dll`), AOB scanner, GPUI hooks. Themes (palette swap), a settings tab inside the client, rendering deleted messages |
| **Plugins** | One Plugin API and an external runtime (WASM): third-party plugins install without recompiling and run in a sandbox |
| **Security** | A CA per install, generated locally, name-constrained to Discord's domains, never sent anywhere. The proxy listens on `127.0.0.1` only |
| **Compatibility** | Works with today's Discord now (network layer); Phoenix as soon as Canary ships |
| **Languages** | English first; Russian localization of the app is planned ([`ROADMAP.md`](ROADMAP.md), 1.7) |

> **Status: pre-alpha.** The codebase is being rebuilt as a Cargo workspace. Today the core, the Gateway codecs, the local CA, the plugin chain and the AOB scanner build and pass tests; the proxy is not yet wired to the new Plugin API. Modifying the client breaks Discord's Terms of Service, as Vencord does. Use at your own risk.

---

## Development status

| Component | Crate | State |
|:---|:---|:---|
| Data model (JSON/ETF), Plugin API v1 | `phoecord-core` | ✅ ported and designed |
| Gateway codecs, local CA, certificate trust | `phoecord-proxy` | ✅ ported · 🔄 proxy, REST and Gateway in progress |
| Plugin chain | `phoecord-plugin-runtime` | ✅ native · ⏳ WASM backend |
| Built-in plugins | `phoecord-plugins` | ✅ `no_track` · 🔄 6 more from v0 |
| Third-party plugin SDK | `phoecord-plugin-sdk` | ⏳ WIT contract and macro |
| Loader, AOB scanner, hooks | `phoecord-loader` | ✅ AOB scanner · ⏳ proxy DLL, hooks |
| Installer | `phoecord-installer` | ⏳ |
| `phoecord` CLI | `phoecord-cli` | 🔄 scaffold |

Milestones and checklists: [`ROADMAP.md`](ROADMAP.md). All tasks: [`TASKS.md`](TASKS.md).

---

## How it works

```text
                    ┌──────────── phoecord-loader (UI layer, targeted) ──────────┐
                    │ version.dll → AOB signatures → GPUI hooks: themes, settings│
                    └──────────────────────────────┬─────────────────────────────┘
                                                   │ inside the client process
┌──────────────────┐   CONNECT     ┌───────────────▼─────────┐   TLS    ┌──────────────┐
│ Discord / Phoenix│ ────────────► │ phoecord-proxy          │ ───────► │ discord.com  │
│ (client)         │ ◄──────────── │ REST + Gateway → plugins│ ◄─────── │ gateway      │
└──────────────────┘ 127.0.0.1     └───────────────┬─────────┘          └──────────────┘
                                                   │ Event / Verdict
                                   ┌───────────────▼─────────┐
                                   │ phoecord-plugin-runtime │ native + WASM plugins
                                   └─────────────────────────┘
```

1. The client reaches the network through PhoeCord as its HTTPS proxy.
2. For Discord's API and Gateway hosts the proxy terminates TLS itself, with a certificate from the **local** CA, and opens its own TLS connection to the real Discord. CDN, media, voice and every other site go through an untouched tunnel.
3. REST requests and Gateway events are decoded into one `Val` type and pass through the plugin chain: each plugin returns `Pass`, `Changed`, `Drop` or `Reply`.
4. The in-process loader is needed only by UI plugins. If a signature is not found after a Discord update, the UI layer turns off and the network keeps working.

Details: [`ARCHITECTURE.md`](ARCHITECTURE.md).

---

## Repository layout

| Path | Purpose |
|:---|:---|
| `crates/phoecord-core/` | `Val` (JSON/ETF), Plugin API: `Manifest`, `Event`, `Verdict`, `Host` |
| `crates/phoecord-proxy/` | MITM proxy, Gateway codecs (zlib/zstd), local CA, trust stores |
| `crates/phoecord-plugin-runtime/` | Plugin chain, later the WASM runtime (wasmtime) |
| `crates/phoecord-plugin-sdk/` | SDK for third-party plugin authors |
| `crates/phoecord-plugins/` | Built-in plugins |
| `crates/phoecord-loader/` | Proxy DLL, AOB scanner, GPUI hooks (`cdylib`) |
| `crates/phoecord-installer/` | Client discovery, CA and loader install, rollback |
| `crates/phoecord-cli/` | The `phoecord` binary |
| `docs/` | Developer docs: [`README.md`](docs/README.md) is the index, [`CODE_MAP.md`](docs/CODE_MAP.md) maps files, [`phase-1/`](docs/phase-1/) is the current plan |
| `scripts/` | `check.sh` / `check.ps1` (all checks), `gen_tasks.py`, `gen_decisions.py` |
| `.github/` | Issue and PR templates, CI (`fmt`, `clippy`, `test` on Linux, Windows, macOS) |

Full file map: [`docs/CODE_MAP.md`](docs/CODE_MAP.md).

---

## Local development

You need Rust **1.88+** (edition 2024) and Python 3.10+ for the doc scripts. On Linux, trusting the CA needs NSS `certutil` (Arch: `nss`, Debian/Ubuntu: `libnss3-tools`, Fedora: `nss-tools`).

```bash
git clone https://github.com/nanda070/phoecord
cd phoecord
cargo build
cargo run            # the phoecord binary (scaffold for now)
```

All checks, the same as CI:

```bash
bash scripts/check.sh            # Windows: powershell -File scripts/check.ps1
```

Tests never touch the network and never need a Discord account. Code rules: [`CONTRIBUTING.md`](CONTRIBUTING.md).

---

## How we work

- Changes go through a branch and a Pull Request only: prefixes `feat/`, `fix/`, `refactor/`, `chore/`, `docs/`; commits as `type(scope): description`, where scope is the crate name without `phoecord-` (`feat(proxy): …`). Steps: [`CONTRIBUTING.md`](CONTRIBUTING.md); rules for people and AI agents: [`AGENTS.md`](AGENTS.md).
- Done means: CI is green, docs and [`docs/CHANGELOG.md`](docs/CHANGELOG.md) are updated, the box in [`ROADMAP.md`](ROADMAP.md) is ticked.
- Anything touching TLS, the CA, trust stores or the loader is reviewed by the owners: [`SECURITY.md`](SECURITY.md).
- Never commit Discord tokens, `ca.key`, traffic dumps or client binaries.

Docs and code comments are in **English**.

---

## Team

| Who | Role | Discord | Telegram |
|:---|:---|:---|:---|
| **Nanda** | Project lead | `nandak070` | [@nanda070](https://t.me/nanda070) |

Project documents: [license](LICENSE) (MIT) · [contributing](CONTRIBUTING.md) · [governance](docs/GOVERNANCE.md) · [conduct](CODE_OF_CONDUCT.md) · [security](SECURITY.md) · [support](docs/SUPPORT.md) · [architecture](ARCHITECTURE.md) · [roadmap](ROADMAP.md) · [changelog](docs/CHANGELOG.md)

---

## Acknowledgments

- **[Vencord](https://github.com/Vendicated/Vencord)** for plugin ideas and names. No Vencord code is used in PhoeCord.
- **[Zed / GPUI](https://github.com/zed-industries/zed)** for the open GPUI sources we study to understand Phoenix's UI layer.

PhoeCord is not affiliated with Discord Inc., Vencord or Zed Industries.

---

## Links

| | |
|:---|:---|
| **Repository** | [github.com/nanda070/phoecord](https://github.com/nanda070/phoecord) |
| **Issues** | [bug](https://github.com/nanda070/phoecord/issues/new?template=bug_report.yml) · [feature](https://github.com/nanda070/phoecord/issues/new?template=feature_request.yml) · [plugin](https://github.com/nanda070/phoecord/issues/new?template=plugin_request.yml) |
| **Vulnerabilities** | privately only, see [`SECURITY.md`](SECURITY.md) |

---

<p align="center">
  <sub>© 2026 Cheterin Group · MIT · not affiliated with Discord Inc.</sub>
</p>
