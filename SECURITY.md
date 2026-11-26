# Security

PhoeCord sits between your Discord client and Discord's servers and decrypts that traffic on your computer. That is why the security bar here is higher than usual. One version is supported: the latest release and the `main` branch.

## The local CA certificate

To read API and Gateway traffic, PhoeCord terminates TLS itself and shows the client a certificate from its own certificate authority (CA). How it works:

| | |
|:---|:---|
| **Where it is created** | On your computer, on first run (`phoecord setup`). There are no prebuilt keys in the repository or in releases |
| **Where it is sent** | Nowhere. Neither the key nor the certificate ever leaves your machine: no network, no telemetry, no developers. PhoeCord has no telemetry |
| **Uniqueness** | Every install has its own CA. A leak of one affects no one else |
| **What it can sign** | Only `discord.com`, `discordapp.com`, `discord.gg` and their subdomains, enforced by an X.509 name constraint. Even a leaked key cannot impersonate your bank, your mail or any other site |
| **Where it lives** | `ca.pem` and `ca.key` in PhoeCord's data folder. `ca.key` is written readable by its owner only |
| **Lifetime** | 10 years; delete and recreate it at any time |
| **How to remove it** | `phoecord untrust` removes the CA from the trust stores, then delete the data folder |

All other traffic (CDN, media, voice, other sites) goes through a plain tunnel: PhoeCord does not decrypt it. The proxy accepts connections from `127.0.0.1` only and refuses everything else.

## The loader (UI layer)

Status: prototype merged (stages 1.5.1–1.5.5), not yet placed by the installer; checked with a test host only, not inside Discord. Requirements the loader must meet: `phoecord-loader` is placed next to the client as a proxy DLL; it loads only plugins installed in PhoeCord's own folder, never downloads code from the network and has no function that reads the token. The installer puts the DLL in place only on an explicit user command and can remove it.

**The IPC pipe is a local attack surface.** `phoecord run` / `launch` open the named pipe `\\.\pipe\phoecord-<user SID>`. Who can connect: only the Windows user who runs the proxy (a protected ACL with a single entry for that user's SID; no `Everyone`, no `SYSTEM`, no `Administrators`) and only from this computer (`PIPE_REJECT_REMOTE_CLIENTS`). What a connected process can do: tell the proxy about the client build (logged), read and write scalar keys of the `[loader]` table in `config.toml` (nothing else in the file), and send UI events (logged, ignored). It cannot read the token, plugin settings or traffic. Every other process of the same user can do this too, as it could edit `config.toml` directly. Lines over 64 KiB, bad JSON and unknown messages drop the connection; at most 8 connections are served. Known limits: another user who creates the same pipe name first can make the loader report to them (build and hash only; the loader never trusts what a pipe answers beyond its own settings); the proxy then reports that it could not create the pipe.

## Automatic updates

An installed PhoeCord updates itself from this repository's GitHub Releases (task 2.4.5):

| | |
|:---|:---|
| **What it contacts** | Once a day: `api.github.com` (the release list) and, when a newer release is out, GitHub's download servers for one installer, `SHA256SUMS` and its signature `SHA256SUMS.minisig`. Nothing about you or your Discord account is sent; GitHub sees your IP address, as with any download |
| **What it accepts** | Only a published release (no drafts, no prereleases, no `-` tags) newer than the running one, only the installer for your kind of install, and only when `SHA256SUMS` carries the owner's minisign signature for that version (checked with the public key compiled into PhoeCord, `crates/phoecord-installer/update-key.pub`) and the installer's SHA-256 matches its line in it. Anything missing, unsigned or different is refused and nothing is installed |
| **When it installs** | At the next start of the PhoeCord window or of its background proxy, never in the middle of a session. The file is hashed again just before it runs |
| **Who can ship an update** | Only whoever holds the owner's offline signing key. CI never sees it: a release is built as a draft and published by the owner after signing (`scripts/sign-release.ps1`). A compromised GitHub account or release cannot ship an update without the key (finding A1 of the audit below) |
| **What it does not protect against** | A stolen signing key together with its password. The installers themselves have no publisher certificate, so a file downloaded by hand is checked by you: against `SHA256SUMS`, and `SHA256SUMS` with `minisign -Vm SHA256SUMS -p update-key.pub` |
| **How to turn it off** | Settings in the PhoeCord window, or `[update] auto = false` in `config.toml`. `phoecord upgrade` still checks when you ask |

## Your Discord session

To let plugins call Discord's API the way the client does (`Host::rest`), the proxy keeps the `Authorization` header of your own client's latest API request **in memory only**: never written to disk or logs, never handed to a plugin (plugins get only the answer), dropped after 30 minutes without client traffic or when Discord answers 401. Calls go only to the Discord API host your client uses. Outside calls (`Host::fetch`) never carry it.

## Third-party plugins

Status: the WASM runtime works (stage 1.3). WASM plugins run in a sandbox with no access to files, network or the process, only through the host API, and only with the `rights` their `plugin.toml` lists (none by default). A plugin sees only the events its manifest subscribes to. Each event has a time limit (50 ms soft with strikes, 1 s hard) and a 32 MiB memory limit. Native plugins are code with full rights: install them only from sources you trust.


## Audit (task 2.4.6)

Checked against code: `main` after PR #132, 2026-11-26. Scope: the CA, the proxy, the loader, the WASM sandbox, the updater. Each claim of this file was read against the code; the table lists what did not hold or deserves a note. Severity is for a single-user Windows or macOS desktop, PhoeCord's target.

| # | Area | Finding | Severity | Status |
|:--|:--|:--|:--|:--|
| A1 | Updates | An update is trusted if it is in a release of this repository and matches that release's `SHA256SUMS`. Both come from GitHub, so whoever controls the repository or one maintainer's account can ship code to every install with automatic updates on. | High | **Fixed** (owner's ruling, 2026-11-26): `SHA256SUMS` is signed with the owner's offline minisign (Ed25519) key, trusted comment `phoecord <version>`; the public key is compiled in (`update-key.pub`) and an update without a valid signature for its own version is refused (`update::verify_sums`). `release.yml` leaves a draft; the owner checks every file against `SHA256SUMS`, signs and publishes with `scripts/sign-release.ps1`. The key was made on the owner's computer on 2026-11-26 (key ID `CC88851881F300F5`); the public key is committed |
| A2 | CA | `ca.key` and `dev-token` are written owner-only (`0600`) on Unix. On Windows they get the ACL of the data folder in the user's profile: that user, `SYSTEM` and `Administrators`, which can read anything of the user anyway. | Medium | Accepted; documented here. Tightening the ACL needs Win32 calls outside the loader (`unsafe`), against the rules |
| A3 | CA | On Unix, `0600` was only applied when the file was created; a `ca.key` or `dev-token` already there kept its old bits. | Low | Fixed: the bits are set on every write |
| A4 | Proxy | `Host::rest` refused `..` and `%2e%2e` in a route but not `.%2e`, `%2e.` or `%2e`, so a plugin could send the user's session to a non-API path on the same Discord host. | Low | Fixed: a dot segment in any spelling is refused; tests added |
| A5 | Loader | Two `unsafe { mem::zeroed() }` in `inject.rs` had no `// SAFETY:` comment (repository rule). Both were sound. | Info | Fixed |
| A6 | Proxy | Any process on this computer can use the proxy as a `CONNECT` tunnel to any host and port, including other local services. | Low | Accepted: only loopback peers are served, and such a process can open those connections itself. Nothing is decrypted outside `intercept` |
| A7 | Proxy | JSON request bodies and `/interactions` multipart bodies are read whole without a size cap (message bodies with files: 64 MiB, then streamed). | Low | Accepted: only the local client sends them |
| A8 | Plugins | `Host::fetch` does not look at what a host resolves to, so a plugin's own listed host could resolve to `127.0.0.1`. | Info | Accepted: only `https://` on port 443, no redirects, no credentials or cookies, 5 MB answers; a local service would also need TLS on 443 |

Verified and holding:

- **CA**: key made locally with `rcgen`; the CA is `pathlen:0` with an X.509 name constraint to `discord.com`, `discordapp.com`, `discord.gg`; leaves are made only for hosts the proxy intercepts (`CONNECT` to port 443 of an `intercept` host) and live 90 days in memory; `uninstall` removes the trust entry and both files (tests in `phoecord-installer::uninstall`).
- **Proxy**: loopback peers only (test); everything outside `intercept` is a byte-for-byte tunnel (test); hop-by-hop headers are stripped both ways; the dev console needs a 256-bit random token from a file only the user can read, compared in constant time; the client's `Authorization` stays in memory, goes only to the client's own API host, never to `fetch`.
- **Loader**: the real `version.dll` is loaded by its full system path (no search-order hijack); every `unsafe` block has a `// SAFETY:` comment; the hook entry points run under the panic guard; the `send` detour reads client memory through `ReadProcessMemory`, so a wrong layout is a refused read, not a crash; `unsafe` appears in no other crate.
- **WASM**: no WASI imports are linked, only PhoeCord's own host interface; every host call checks its right; events are filtered by the manifest; 50 ms soft (strikes) and 1 s hard limits by epoch interruption; 32 MiB of memory per plugin.

2.4.6 stays open until the secret key has a password (`minisign -C`) and an offline backup.

## Where to report

| What | Where |
|:---|:---|
| Any problem: a bug, a crash, a broken plugin, a question, an idea | [Issues](https://github.com/Nanda070/phoecord/issues) of this repository |
| A **leak of personal data or other secret information** (tokens, `ca.key`, messages, IDs, traffic), or a vulnerability that could cause one | **Privately only:** [DM `nandak070` on Discord](https://discord.com/users/1219803151180370021) (user ID `1219803151180370021`) |

**Never put a leak or a vulnerability into a public issue, a pull request or a public channel.** If you already did, tell Nanda in a DM at once so it can be removed and the secret rotated.

In a private report, describe what you found, how to reproduce it and what it affects. We reply within 72 hours and agree on a fix timeline separately. Until it is fixed, do not disclose it.

## What counts as a vulnerability

- The CA signs anything outside Discord's domains, or the name constraint can be bypassed.
- The CA key, a Discord token or traffic contents leave the computer or are readable by other users of the system.
- The proxy accepts connections from anywhere but `127.0.0.1`, or decrypts hosts outside the `intercept` list.
- A WASM plugin escapes the sandbox or receives events it did not subscribe to.
- The loader loads code the user did not install (DLL hijacking through PhoeCord).

## Team rules

- Never commit tokens, `ca.key`, `ca.pem`, traffic dumps or `.env` files. Where secrets live: [`docs/GOVERNANCE.md`](docs/GOVERNANCE.md#secrets).
- Changes to `crates/phoecord-proxy/src/ca.rs`, `trust.rs`, the proxy and `phoecord-loader` are reviewed by the owners.
- A secret leaked? Tell Nanda right away.
