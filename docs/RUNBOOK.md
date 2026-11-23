# What to do when something breaks (runbook)

Checked against code: `6b3a7f5`, 23.11.2026. Whoever re-checks this document against the code updates this line (rule in `AGENTS.md`).

Short steps for when PhoeCord stops working on someone's machine: how to find out what broke and what to do. Living document: a new kind of breakage or a new diagnostic goes here, in the same commit. Commands marked *(1.1)* arrive with milestone 1.1.

Related: [`CODE_MAP.md`](CODE_MAP.md) for where things live; [`SECRETS.md`](SECRETS.md) for keys; [`SECURITY.md`](../SECURITY.md) for anything that looks like a leak.

---

## 0. The first two minutes

```bash
phoecord selftest          # (1.1) REST + Gateway against the real Discord, no login
phoecord paths             # (1.1) where config, CA and logs are
phoecord plugins           # (1.1) which plugins are on
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

## 4. Removing everything

```bash
phoecord untrust           # (1.1) remove the CA from the trust stores
```

Then delete the binary and the folders `phoecord paths` prints. The installer's "Uninstall" (1.4) does all of this.

## 5. Suspected security problem

The CA key or a token may have leaked, or the proxy accepted a non-local connection: stop the proxy, run `phoecord untrust`, delete the data folder (a new CA is created on next run), and report privately per [`SECURITY.md`](../SECURITY.md).

## 6. After a breakage

- A report from [`_template/incident.md`](_template/incident.md) into [`incidents/`](incidents/).
- A test that catches it, a new row in this runbook, or both.
