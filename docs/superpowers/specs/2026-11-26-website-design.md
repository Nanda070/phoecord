# Website (phoecord.dev) and README refresh: design

Approved by the owner on 2026-11-26.

## Intent

PhoeCord needs a public face: a site at `phoecord.dev` where people learn what it is, download it and browse its plugins, and a README that matches it. Both point to the community server, `discord.gg/cheterin`.

## Goals

- A static site in the spirit of vencord.dev (hero with name and tagline, download button, feature cards, plugin list with search), in PhoeCord's own look: dark, "Orange Blood" palette from `assets/logo.svg` (#ff4d1a → #ff8a1f → #ffd24a on #1e1f22-like surfaces).
- Pages: home, plugins, download; English at `/`, Russian at `/ru/`.
- Discord link in the site header, footer and a "Join the community" block; a Discord badge in the README.
- README keeps its structure (Why, Plugins, Build, Contributors), with current facts (v0.6.0, installers, plugin counts), a website link and the Discord badge. Tone closer to Vencord's README, no copied text.

## Non-goals and anti-goals

- No copied Vencord text, artwork, logo or palette; no "Vencord" branding beyond attribution.
- No Node toolchain outside `site/`; the Rust checks (`scripts/check.sh`) stay untouched.
- No analytics or third-party trackers on the site (matches the "no telemetry" promise).

## Approach (owner's choices)

- **Astro** in `site/` (its own `package.json`), static output.
- **GitHub Pages** via `.github/workflows/pages.yml` (`actions/deploy-pages`), on pushes to `main` touching `site/`, `docs/PLUGINS.md` or `catalog/`, on a published release, and by hand. `site/public/CNAME` = `phoecord.dev`.
- **DNS** is done (name.com, 2026-11-26): four `A` and four `AAAA` records to GitHub Pages, `www` CNAME to `nanda070.github.io`. The owner sets Pages source to "GitHub Actions", the custom domain and "Enforce HTTPS".

Alternatives considered: plain HTML (no build step, but a hand-kept plugin list) and Cloudflare Pages (another account for no gain). Astro won on the owner's call.

## Builder's calls

- **Plugin data.** The owner chose `catalog/plugins.json`, but that file is the third-party WASM catalog and is empty. The plugins page therefore reads the tables of `docs/PLUGINS.md` at build time (name, description, layer, v1.0 target, status) and shows `catalog/plugins.json` entries as a "Community plugins" section when there are any. One source of truth, always in sync.
- **Download data.** At build time the latest release is read from the GitHub API (assets by OS); if the call fails, buttons fall back to `releases/latest`.
- Plugin descriptions stay in English on the Russian page (they come from `PLUGINS.md`); the page chrome is translated.
