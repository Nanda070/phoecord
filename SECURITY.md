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

Status: in development (ROADMAP, 1.5). Requirements the loader must meet: `phoecord-loader` is placed next to the client as a proxy DLL; it loads only plugins installed in PhoeCord's own folder, never downloads code from the network and has no function that reads the token. The installer puts the DLL in place only on an explicit user command and can remove it.

## Third-party plugins

Status: the WASM runtime is in development (ROADMAP, 1.3). How it must work: WASM plugins run in a sandbox with no access to files, network or the process, only through the host API. A plugin sees only the events its manifest subscribes to. Native plugins are code with full rights: install them only from sources you trust.

## Reporting a vulnerability

**Do not open a public issue and do not post about a vulnerability in public channels.**

Use [GitHub Security Advisories](https://github.com/nanda070/phoecord/security/advisories/new) or write privately:

| Who | Discord | Telegram |
|:---|:---|:---|
| Nanda, project lead | `nandak070` | [@nanda070](https://t.me/nanda070) |

Describe what you found, how to reproduce it and what it affects. We reply within 72 hours and agree on a fix timeline separately. Until it is fixed, do not disclose it.

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
