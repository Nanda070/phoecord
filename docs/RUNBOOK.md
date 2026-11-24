# What to do when something breaks (runbook)

Checked against code: `a965f75`, 23.11.2026. Whoever re-checks this document against the code updates this line (rule in `AGENTS.md`).

Short steps for when PhoeCord stops working on someone's machine: how to find out what broke and what to do. Living document: a new kind of breakage or a new diagnostic goes here, in the same commit.

Related: [`CODE_MAP.md`](CODE_MAP.md) for where things live; [`GOVERNANCE.md`](GOVERNANCE.md#secrets) for keys; [`SECURITY.md`](../SECURITY.md) for anything that looks like a leak.

---

## 0. The first two minutes

```bash
phoecord selftest          # REST + Gateway against the real Discord, no login
phoecord paths             # where config, CA and logs are
phoecord plugins           # which plugins are on
```

Run with more logging: `PHOECORD_LOG=phoecord=debug phoecord run`.

What to look at in the log: `refused a connection`, `gateway:`, `config not reloaded`, `no certificate for`, TLS errors (`UnknownIssuer`, `BadCertificate`), panics.

Write down right away: when it started (UTC), Discord client and version, PhoeCord version, what the user sees, what the log says. Then an incident report (§6) if it affects more than one person.

## 1. Discord does not connect at all

| Symptom | What to do |
|:---|:---|
| Discord shows "connecting" forever | Is the proxy running (`phoecord run`)? Is the port free (`listen` in `config.toml`)? |
| TLS / certificate errors in the client | CA not trusted: `phoecord trust`. On Linux, NSS `certutil` must be installed; Flatpak and Snap have their own NSS databases |
| Works in the browser, not in the app | The app was started without the proxy: quit Discord fully and use `phoecord launch` |
| Breaks after enabling a plugin | Disable it (`phoecord disable <id>`), reproduce, attach the debug log to an issue |

## 2. Plugins do nothing

| Symptom | What to do |
|:---|:---|
| A Gateway plugin does nothing | Gateway plugins start on reconnect: press Ctrl+R in Discord |
| Config change ignored | Look for `config not reloaded` in the log: the TOML has an error |
| A WASM plugin is disabled | (1.3) It hit a time or memory limit or panicked: the log names the plugin |

## 3. After a Discord update

| Symptom | What to do |
|:---|:---|
| Network plugins broke | Run `phoecord selftest`; a Gateway encoding/compression change shows up there. Fix in `phoecord-proxy::codec`, not in plugins |
| UI plugins are off, notice "waiting for a signature update" | Expected degradation: the client build is unknown. New signatures go into `signatures/*.toml` (Phase 2, 2.2) |
| Client crashes on start with the loader installed | Remove the loader (`phoecord-installer`, "Uninstall") and file a bug with the client version: a hook is wrong |

## 3b. Starting Discord through PhoeCord

- Discord restarts in a loop or never finishes "Starting" after `setup`: the CA PhoeCord signs with is not trusted (`run` and `launch` print a warning). Run `phoecord trust`, then restart Discord.
- `phoecord launch --channel canary` (or `stable`, `ptb`). With several clients installed and no `--channel`, `launch` lists them and stops: it never picks one silently.
- If the proxy already runs (autostart), `launch` only starts the client with the proxy flag.
- "address used by another program, not PhoeCord": something else holds the port; change `listen` in `config.toml`.
- `phoecord setup --channel canary` adds a Start-menu shortcut "Discord Canary (PhoeCord)"; `phoecord uninstall` removes it.

## 3c. VPN, zapret, a blocked network

Discord works without PhoeCord but not through it: the proxy connects to Discord itself, outside your VPN app's or zapret's per-app rules. Set `upstream = "socks5://127.0.0.1:1080"` (or `http://host:port`) in `config.toml` to send PhoeCord's connections through the same tunnel; `phoecord selftest` checks it. Credentials in the upstream address are not supported.

## 3a. Running checks from inside another app

If you start `phoecord` from a packaged (MSIX / Store) app, for example the Claude desktop app's terminal, Windows redirects its AppData and HKCU writes into that app's private cache: setup seems to work but nothing lands in your real profile. Run PhoeCord from a normal terminal (Windows Terminal, PowerShell from the Start menu).

## 4. Removing everything

```bash
phoecord uninstall         # undo autostart, CA trust and the files setup created
```

Your `config.toml` stays; delete it and the binary by hand if you want nothing left. If `uninstall` reports a failed step, run it again: the step stays in `install.log` until it succeeds.

## 5. Suspected security problem

The CA key or a token may have leaked, or the proxy accepted a non-local connection: stop the proxy, run `phoecord untrust`, delete the data folder (a new CA is created on next run), and report privately per [`SECURITY.md`](../SECURITY.md).

## 6. After a breakage

- A test that catches it, a new row in this runbook, or both.
