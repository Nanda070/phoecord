# PhoeCord terms of use

Version: 1

Please read this before you install PhoeCord. It is short. You must accept it once; if it changes, you will be asked again.

## 1. What PhoeCord is

PhoeCord is an unofficial, open-source modification for the Discord client. It is **not affiliated with, endorsed by or supported by Discord Inc.** "Discord" is a trademark of its owner.

## 2. It breaks Discord's rules, and your account is at risk

Modifying the Discord client is against Discord's Terms of Service. Discord may limit, suspend or ban an account that uses a client modification. **You accept that risk.** Plugins that do many actions at once (mass delete, mass send, mass join and the like) make it higher. Nobody on the PhoeCord team can restore a banned account.

## 3. No warranty, no liability

PhoeCord is provided "as is". The developers are not liable for any loss or damage from using it. The exact wording is in sections 15 and 16 of the GPL-3.0-or-later in the [`LICENSE`](LICENSE) file.

## 4. What PhoeCord does on your computer

- It creates a **local certificate authority (CA)** that exists only on your computer. It can sign certificates only for `discord.com`, `discordapp.com`, `discord.gg` and their subdomains.
- It **adds that CA to your trust store**.
- It runs a **local proxy on `127.0.0.1`** that reads Discord API and Gateway traffic on this machine only. Other traffic is passed through untouched.
- It can **start at login**, if you leave that on.
- There is **no telemetry**. Nothing is sent to the developers or anyone else.

To remove all of it, run `phoecord uninstall`. It undoes everything `phoecord setup` did.

## 5. Plugins from other people

Installing a third-party plugin is your choice and your responsibility. WASM plugins run in a sandbox and have only the rights they show. Native plugins run with full rights on your computer: install them only from sources you trust.

## 6. Unsigned releases

Releases are not code-signed, so your system may warn you when you run them. Check the file against `SHA256SUMS` from the release page.

## 7. Where to report

- Any problem, bug or question: [GitHub Issues](https://github.com/Nanda070/phoecord/issues).
- A leak of personal data or secrets, or a vulnerability: **privately** to `nandak070` on Discord, as described in [`SECURITY.md`](SECURITY.md). Never in a public issue.

## 8. Changes

These terms can change. Every version has a number. When the number goes up, PhoeCord shows the terms again and you must accept them again.
