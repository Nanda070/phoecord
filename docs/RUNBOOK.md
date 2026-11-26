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
| `phoecord dev console` cannot attach | "No dev token": the proxy is not running (start `phoecord run`). "refused the dev token": the proxy was restarted after the console opened; open it again. The console shows only events a plugin runs, and `replay` only events seen while it was attached |
| A WASM plugin is disabled | (1.3) It hit a time or memory limit or panicked: the log names the plugin |

## 3. After a Discord update

| Symptom | What to do |
|:---|:---|
| Network plugins broke | Run `phoecord selftest`; a Gateway encoding/compression change shows up there. Fix in `phoecord-proxy::codec`, not in plugins |
| UI plugins are off, notice "waiting for a signature update" | Expected degradation: the client build is unknown. New signatures go into `signatures/*.toml` (Phase 2, 2.2) |
| Loader missing after a Discord update (Windows, `setup --loader`) | Discord updated into a new `app-x.y.z` folder. Start Discord with `phoecord launch` (or let autostart run): it copies `version.dll` into the newest folder first. If it does not, read the warning `run`/`launch` print: `version.dll` missing next to `phoecord` (reinstall PhoeCord), no `app-x.y.z` build, or the copy failed (quit Discord fully: a running client keeps the old DLL locked) |
| "belongs to another program": a foreign `version.dll` is in the way | Another tool put its own `version.dll` in the `app-x.y.z` folder; PhoeCord never overwrites a file whose hash it did not record. Decide which one you want: remove or rename the other file, then `phoecord launch` installs PhoeCord's. `uninstall` likewise leaves a `version.dll` that is no longer the one PhoeCord wrote ("changed by another program") |
| The loader cannot reach the proxy (Windows) | `%LOCALAPPDATA%\PhoeCord\loader.log` says `ipc: no proxy reachable on \\.\pipe\phoecord-S-1-5-… (…); giving up`: the client was started without `phoecord run` / `launch` running (or the proxy runs as another Windows user). Nothing is broken: network plugins need the proxy anyway, and the loader retries five times in about 22 s. Start the client with `phoecord launch`; the proxy log then shows `loader connected: build …`. `ipc: reported to the proxy (attempt N)` in `loader.log` is the success line. If the proxy log says `loader IPC is off: cannot create …`, another `phoecord run` already owns the pipe (or another user took the name): stop it and start again |
| Client crashes on start with the loader installed | Remove the loader (`phoecord-installer`, "Uninstall") and file a bug with the client version: a hook is wrong |

## 3b. Starting Discord through PhoeCord

- Discord restarts in a loop or never finishes "Starting" after `setup`: the CA PhoeCord signs with is not trusted (`run` and `launch` print a warning). Run `phoecord trust`, then restart Discord.
- `phoecord launch --channel canary` (or `stable`, `ptb`). With several clients installed and no `--channel`, `launch` lists them and stops: it never picks one silently.
- If the proxy already runs (autostart), `launch` only starts the client with the proxy flag.
- "address used by another program, not PhoeCord": something else holds the port; change `listen` in `config.toml`.
- `phoecord setup --channel canary` adds a Start-menu shortcut "Discord Canary (PhoeCord)"; `phoecord uninstall` removes it.

macOS 13 (Ventura) or later is required (the client starts with `open --env`).

### macOS: "Operation not permitted" / `installer.db` / "Installation paused"

`launch.log` (`~/Library/Application Support/phoecord/`) shows `Reading .../discord/settings.json: Operation not permitted` or `unable to open database file: .../installer.db`, and Discord's updater says "Installation paused". macOS (TCC) denied a client that PhoeCord started as its own child. Fixed in 0.5.1: the client is started with `open -n -a`. On 0.5.0: update PhoeCord; or allow PhoeCord under System Settings, Privacy and Security, App Management / Full Disk Access, then start Discord again.

### macOS: "PhoeCord is damaged and can't be opened"

The `.app` had no valid signature (0.5.0). From 0.5.1 it is ad-hoc signed and macOS shows "unidentified developer": System Settings, Privacy and Security, **Open Anyway**. On 0.5.0 run `xattr -dr com.apple.quarantine /Applications/PhoeCord.app`, or install 0.5.1.

## 3c. VPN, zapret, a blocked network

Discord works without PhoeCord but not through it: the proxy connects to Discord itself, outside your VPN app's or zapret's per-app rules. Set `upstream = "socks5://127.0.0.1:1080"` (or `http://host:port`) in `config.toml` to send PhoeCord's connections through the same tunnel; `phoecord selftest` checks it. Credentials in the upstream address are not supported.

## 3a. Running checks from inside another app

If you start `phoecord` from a packaged (MSIX / Store) app, for example the Claude desktop app's terminal, Windows redirects its AppData and HKCU writes into that app's private cache: setup seems to work but nothing lands in your real profile. Run PhoeCord from a normal terminal (Windows Terminal, PowerShell from the Start menu).

## 3d. Antivirus or SmartScreen blocks PhoeCord

Releases are unsigned, so Windows SmartScreen shows "Windows protected your PC" (click **More info**, **Run anyway**) and macOS Gatekeeper blocks the first start (right-click, **Open**). First check the file against `SHA256SUMS` from the same release (`Get-FileHash <file> -Algorithm SHA256` on Windows, `shasum -a 256 <file>` on macOS, `sha256sum -c SHA256SUMS --ignore-missing` on Linux). If the hash matches and Defender still quarantines `version.dll`, it is a false positive: tell Nanda the release and the detection name, so the file is (re)submitted to Microsoft ([`GOVERNANCE.md`](GOVERNANCE.md#releases)). If the hash does not match, do not run it and report it privately (§5).

## 3e. A release run failed

- `tag matches Cargo.toml` failed: the tag and `[workspace.package] version` differ. Delete the tag, merge a version bump, tag again.
- A build or installer job failed: nothing was published (publishing waits for every job). Fix on `main`, delete the tag and push it again.
- `publish the release` failed after the release was created: delete the half-made release on GitHub, then re-run the failed job.

## 3f. Installing and updating PhoeCord

Which file to install: `PhoeCord_<version>_x64-setup.exe` (Windows), `PhoeCord_<version>_aarch64.dmg` / `_x64.dmg` (Mac), `PhoeCord_<version>_amd64.deb` (`sudo apt install ./<file>`, Debian/Ubuntu/Mint) or `PhoeCord_<version>_x86_64.rpm` (`sudo dnf install ./<file>`, Fedora/openSUSE). Other Linux distributions use the `phoecord-*-linux-x86_64.tar.gz` archive, which has the CLI only.

An installed copy updates itself (2.4.5): it checks GitHub once a day, downloads and verifies the new installer, and installs it at the next start of the window or of `phoecord run`. The window's Settings shows the version and the last update error; `phoecord upgrade` checks now and says what it found. `updates/state.json` in the data folder holds the state (`last_check`, `pending`, `applied`, `error`); deleting the `updates` folder makes the next start check again from scratch.

| Symptom | What to do |
|:---|:---|
| "the update was refused: … does not match its line in SHA256SUMS" | The download is not what the release lists: a broken download, a proxy or antivirus rewriting it, or a release that was changed. Nothing was installed. Run `phoecord upgrade` again; if it still fails, report it privately (§5) with the release and the file name |
| "the release has no …" / "the release has no SHA256SUMS" / "… is not listed in SHA256SUMS" | The release is missing that file (a failed or half-finished release run, §3e). Nothing was installed; the next daily check tries again |
| "the update check failed: …" | GitHub could not be reached (offline, blocked, rate limit). Nothing to do; it tries again the next day. Behind a VPN or zapret, note that the update check does not use `upstream` |
| Linux: "Open the PhoeCord window to install it" in the proxy log | `phoecord run` cannot ask for the administrator password. Open the PhoeCord window: it runs `pkexec dpkg -i` / `pkexec rpm -U` and asks for it. If you cancel, the window opens anyway and asks again next time |
| Windows: the setup says a file cannot be written | A `phoecord.exe` is still running from the install folder. The setup runs `phoecord stop` and then ends `phoecord.exe` processes; if one survives (another user's, or an elevated one), quit Discord and PhoeCord and run the setup again |
| macOS: the update did not go in, "moving … aside" or "putting the new … in place" | PhoeCord could not write next to `PhoeCord.app` (an account without rights to `/Applications`). Install the new `.dmg` by hand, or move the app to a folder you can write |
| Updates should not happen | Turn off "Install updates automatically" in Settings, or set `[update] auto = false` in `config.toml`. A downloaded update then waits; `phoecord upgrade` still checks when asked |
| Updating by hand | Install the new file over the old one: run the new setup (it keeps your settings), drag the new app into Applications, or install the new `.deb` / `.rpm`. Settings, the CA and plugins are in the data folder and stay |

## 4. Removing everything

```bash
phoecord uninstall         # undo autostart, CA trust, loader DLL copies and the files setup created
```

Your `config.toml` stays; delete it and the binary by hand if you want nothing left.

Removing the program itself: on Windows, removing PhoeCord in Settings, Installed apps runs `phoecord uninstall` first, so the steps above happen on their own. macOS and Linux have no such hook: press **Uninstall** in the PhoeCord window (or run `phoecord uninstall`) first, then move `PhoeCord.app` to the Bin or run `sudo apt remove phoecord` / `sudo dnf remove phoecord`. If the program is already gone, the CA stays trusted and the autostart entry points at a missing file: remove the CA with your OS's certificate tool (its name is "PhoeCord local CA (this computer only)"), and delete `~/Library/LaunchAgents/dev.phoecord.proxy.plist` (macOS) or `~/.config/systemd/user/phoecord.service` (Linux). If `uninstall` reports a failed step, run it again: the step stays in `install.log` until it succeeds.

## 5. Suspected security problem

The CA key or a token may have leaked, or the proxy accepted a non-local connection: stop the proxy, run `phoecord untrust`, delete the data folder (a new CA is created on next run), and report privately per [`SECURITY.md`](../SECURITY.md).

## 6. After a breakage

- A test that catches it, a new row in this runbook, or both.
