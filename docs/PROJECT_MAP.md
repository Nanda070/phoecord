# Project map: what PhoeCord does

Checked against code: `6b3a7f5`, 23.11.2026.

What the product can do, for people rather than for the code. Where each piece lives in the repository: [`CODE_MAP.md`](CODE_MAP.md). How it works: [`ARCHITECTURE.md`](../ARCHITECTURE.md).

Statuses: ✅ works · 🔄 in progress · ⏳ planned.

---

## 1. For users

| Feature | What the user gets | Status |
|:---|:---|:---|
| Install | One command (`phoecord setup`) now, one button in the GUI installer later | 🔄 CLI scaffold, ⏳ GUI (1.4) |
| Launch | `phoecord launch` starts Discord through the proxy | ⏳ 1.1 |
| Plugins on/off | `phoecord enable/disable <id>`, later a settings tab inside the client | ⏳ 1.1, 2.3 |
| Themes | Custom palette for the Phoenix client | ⏳ 2.3 |
| Languages | English; Russian next | ⏳ 1.7 |
| Uninstall | Removes the CA, the loader, autostart and folders | ⏳ 1.4 |

## 2. Plugins

The full map of every Vencord plugin (and the owner's userplugins) to PhoeCord is in [`PLUGINS.md`](PLUGINS.md). This table lists what is already built or next.

| Plugin | Layer | What it does | Status |
|:---|:---|:---|:---|
| `no_track` | Network | Blocks Discord's analytics and crash reports. On by default | ✅ |
| `silent_typing` | Network | Your typing indicator is never sent | ⏳ 1.2 |
| `unindent` | Network | Strips shared indentation from code blocks you send | ⏳ 1.2 |
| `game_activity_toggle` | Network | Hides the game you are playing from your status | ⏳ 1.2 |
| `relationship_notifier` | Network | Notifies you when someone unfriends you or removes you from a server | ⏳ 1.2 |
| `fake_nitro` | Network | Sends emoji you cannot use as small linked images | ⏳ 1.2 |
| `message_logger` | Network (+ UI in Phase 2) | Keeps deleted and edited messages visible | ⏳ 1.2 |
| `clear_urls` | Network | Removes tracking parameters from links you send | ⏳ 1.2, 1.3 (as WASM) |
| `/phoecord` commands | Network | Slash commands to list and toggle plugins | ⏳ 1.2 |

## 3. For plugin authors

| Feature | Status |
|:---|:---|
| Plugin API v1 (`Manifest`, `Event`, `Verdict`, `Host`) | ✅ designed, used by `no_track` |
| WASM plugins without rebuilding PhoeCord | ⏳ 1.3 |
| Rust SDK and macro | ⏳ 1.3 |
| TypeScript template | ⏳ 1.3 |
| Plugin localization (`.ftl`) | ⏳ 1.7 |
| Developer docs in `docs/plugins/` | ⏳ 2.4 |

## 4. Privacy and safety

- Local CA per install, limited to Discord's domains, never leaves the computer.
- Proxy only accepts connections from this computer.
- No telemetry.

Details: [`SECURITY.md`](../SECURITY.md).
