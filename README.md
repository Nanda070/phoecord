<p align="center">
  <img src="assets/logo.svg" alt="PhoeCord" width="88" height="88" />
</p>

<h1 align="center">PhoeCord</h1>

<p align="center">
  <strong>Vencord-style plugins for Discord's native client.</strong><br />
  Built for Project Phoenix (Rust + GPUI), working with today's Discord too.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0-3da639?style=for-the-badge" alt="GPL-3.0" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-2024-dea584?style=for-the-badge&logo=rust&logoColor=white" alt="Rust 2024" /></a>
  <a href="docs/ROADMAP.md"><img src="https://img.shields.io/badge/status-pre--alpha-ff8a1f?style=for-the-badge" alt="pre-alpha" /></a>
</p>

<p align="center">
  <a href="docs/ARCHITECTURE.md">Architecture</a> ·
  <a href="docs/ROADMAP.md">Roadmap</a> ·
  <a href="docs/README.md">Docs</a> ·
  <a href="CONTRIBUTING.md">Contributing</a> ·
  <a href="TERMS.md">Terms</a> ·
  <a href="https://github.com/Nanda070/phoecord/issues">Issues</a> ·
  <a href="SECURITY.md">Security</a>
</p>

---

## Why

Phoenix is a compiled Rust binary: no JavaScript to patch, no stable ABI between builds. So PhoeCord splits the job in two.

| | |
|:---|:---|
| 🌐 **Network** | A local proxy rewrites REST and Gateway traffic. Data plugins live here and survive every client update |
| 🎨 **UI** | A small in-process loader, used only for themes, a settings tab and on-screen extras. If it misses after an update, only the UI turns off |
| 🧩 **Plugins** | One API, sandboxed WASM packages: install third-party plugins without rebuilding |
| 🔒 **Private** | A CA generated on your machine, limited to Discord's domains, never sent anywhere. No telemetry |

> **Pre-alpha.** The proxy runs on today's Discord with 8 built-in plugins, WASM plugins load from a folder without a restart, `phoecord setup` / `uninstall` install and remove everything, and the loader prototype is in. Next: the rest of Vencord's network plugins, the GUI installer and Russian ([Phase 1](docs/phase-1/README.md)). Client mods break Discord's Terms, as Vencord does.

## Plugins

| Plugin | | Status |
|:---|:---|:---:|
| `no_track` | Blocks Discord's analytics and crash reports | ✅ |
| `silent_typing` · `game_activity_toggle` · `relationship_notifier` | Quality-of-life network plugins | ✅ |
| `fake_nitro` (emoji) · `message_logger` · `/phoecord` | The headline features | ✅ |
| `unindent` · `clear_urls` · the rest of Vencord's network plugins | Next | ⏳ |
| Themes · settings tab · deleted-message view | UI layer on Phoenix | ⏳ Phase 2 |

Goal: every Vencord plugin that works on a native client, mapped one by one in [`docs/PLUGINS.md`](docs/PLUGINS.md). Languages: English, Russian planned.

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
