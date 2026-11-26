<p align="center">
  <img src="assets/logo.svg" alt="PhoeCord" width="88" height="88" />
</p>

<h1 align="center">PhoeCord</h1>

<p align="center">
  <strong>Plugins and themes for Discord's native client.</strong><br />
  Built for Project Phoenix (Rust + GPUI), and it works with today's Discord too.
</p>

<p align="center">
  <a href="https://phoecord.dev"><img src="https://img.shields.io/badge/website-phoecord.dev-ff4d1a?style=for-the-badge" alt="phoecord.dev" /></a>
  <a href="https://discord.gg/cheterin"><img src="https://img.shields.io/badge/discord-join-5865F2?style=for-the-badge&logo=discord&logoColor=white" alt="Discord server" /></a>
  <a href="https://github.com/Nanda070/phoecord/releases/latest"><img src="https://img.shields.io/github/v/release/Nanda070/phoecord?style=for-the-badge&color=ff8a1f&label=release" alt="Latest release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0-3da639?style=for-the-badge" alt="GPL-3.0" /></a>
</p>

<p align="center">
  <a href="https://phoecord.dev">Website</a> ·
  <a href="https://phoecord.dev/download">Download</a> ·
  <a href="https://phoecord.dev/plugins">Plugins</a> ·
  <a href="https://discord.gg/cheterin">Discord</a> ·
  <a href="docs/README.md">Docs</a> ·
  <a href="CONTRIBUTING.md">Contributing</a> ·
  <a href="TERMS.md">Terms</a> ·
  <a href="SECURITY.md">Security</a>
</p>

---

## Why

Phoenix is a compiled Rust binary: there is no JavaScript to patch and no stable ABI between builds. So PhoeCord does the job in two layers, and an update can break at most the smaller one.

| | |
|:---|:---|
| 🌐 **Network** | A local proxy rewrites Discord's REST and Gateway traffic. Most plugins live here and keep working after client updates |
| 🎨 **UI** | A small in-process loader for themes, a settings tab and on-screen extras. If it misses after an update, only the UI turns off |
| 🧩 **Plugins** | One API and sandboxed WASM packages: install third-party plugins without rebuilding anything |
| 🔒 **Private** | A certificate generated on your machine, limited to Discord's domains and never sent anywhere. No telemetry |
| 🔄 **Self-updating** | Installed copies pick up signed releases on their own and check every file before installing |

## Install

Grab the installer for your system from **[phoecord.dev/download](https://phoecord.dev/download)** or the [latest release](https://github.com/Nanda070/phoecord/releases/latest): `.exe` for Windows, `.dmg` for macOS (Apple Silicon and Intel), `.deb` and `.rpm` for Linux. Pick the Discord you use, press **Install PhoeCord**, and start Discord through PhoeCord. `phoecord uninstall` (or Installed apps on Windows) removes everything again.

> **Status: early.** The network layer runs on today's Discord with 60+ plugins, WASM plugins load without a restart, and releases install and update themselves on Windows, macOS and Linux. The UI layer on Phoenix is in progress ([Phase 2](docs/phase-2/README.md)). Client mods break Discord's Terms, as Vencord does: use at your own risk.

## Plugins

Plugin names match Vencord's, so settings carry over 1:1 (`phoecord import-vencord`) and Vencord themes convert with `phoecord theme import`.

| | | Status |
|:---|:---|:---:|
| `NoTrack` | Blocks Discord's analytics and crash reports (on by default) | ✅ |
| `FakeNitro` · `MessageLogger` · `SilentTyping` · `ClearURLs` · `TextReplace` | The everyday favourites | ✅ |
| `PlatformIndicators` · `BetterSessions` · `ViewRaw` · `RelationshipNotifier` | Data part via the proxy and `/phoecord`, on-screen part next | ✅ / ⏳ |
| Themes · settings tab · on-screen plugins | UI layer on Phoenix | ⏳ Phase 2 |

The full list with search: **[phoecord.dev/plugins](https://phoecord.dev/plugins)**. Every Vencord plugin is mapped one by one in [`docs/PLUGINS.md`](docs/PLUGINS.md). Languages: English and Russian.

## Community

Questions, themes, plugin ideas and release news: **[discord.gg/cheterin](https://discord.gg/cheterin)**. Bugs and feature requests go to [Issues](https://github.com/Nanda070/phoecord/issues); vulnerabilities and leaks privately, see [`SECURITY.md`](SECURITY.md).

## Build

```bash
git clone https://github.com/nanda070/phoecord && cd phoecord
cargo run                       # Rust 1.88+
bash scripts/check.sh           # all checks (Windows: powershell -File scripts/check.ps1)
```

Branches, commits and PRs: [`CONTRIBUTING.md`](CONTRIBUTING.md). Rules for people and AI agents: [`AGENTS.md`](docs/AGENTS.md). Where every file lives: [`docs/CODE_MAP.md`](docs/CODE_MAP.md). Porting a plugin into PhoeCord: [`docs/PORTING_PLUGINS.md`](docs/PORTING_PLUGINS.md). Writing your own WASM plugin (Rust or TypeScript): [`docs/plugins/`](docs/plugins/README.md).

## Contributors

<!-- contributors:start -->
<p>
  <a href="https://github.com/Nanda070" title="Nanda070"><img src="https://github.com/Nanda070.png?size=96" width="64" height="64" alt="Nanda070" /></a>
  <a href="https://github.com/Kennurken" title="Kennurken"><img src="https://github.com/Kennurken.png?size=96" width="64" height="64" alt="Kennurken" /></a>
  <a href="https://github.com/matveee" title="matveee"><img src="https://github.com/matveee.png?size=96" width="64" height="64" alt="matveee" /></a>
  <a href="https://github.com/buslan07" title="buslan07"><img src="https://github.com/buslan07.png?size=96" width="64" height="64" alt="buslan07" /></a>
</p>
<!-- contributors:end -->

Everyone with a merged commit, from GitHub's contributors list ([graph](https://github.com/Nanda070/phoecord/graphs/contributors)); refreshed with `python scripts/update_contributors.py` when a new contributor's PR is merged. Want to join? See [`CONTRIBUTING.md`](CONTRIBUTING.md).

Thanks to [Vencord](https://github.com/Vendicated/Vencord) for the plugin ideas and to [Zed](https://github.com/zed-industries/zed) for open-sourcing GPUI. Not affiliated with Discord Inc., Vencord or Zed Industries.

---

<p align="center">
  <sub>© 2026 Cheterin Group · <a href="LICENSE">GPL-3.0</a> · <a href="SECURITY.md">report a leak privately</a></sub>
</p>
