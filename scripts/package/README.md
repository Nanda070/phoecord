# Native installers

The installer window ships as a real installer on each OS, built by
[`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager) from the
`[package.metadata.packager]` table in `crates/phoecord-installer-gui/Cargo.toml` (the `.rpm`:
[`cargo-generate-rpm`](https://github.com/cat-in-136/cargo-generate-rpm) from
`[package.metadata.generate-rpm]` in the same file):

| OS | Files | Notes |
|:---|:---|:---|
| Windows | `PhoeCord_<version>_x64-setup.exe` (NSIS, PhoeCord's own template `assets/installer/nsis/installer.nsi`) | Per-user install, no administrator prompt; Start menu entry and uninstaller; the loader in `loader\version.dll`; stops the proxy before copying; removing PhoeCord runs `phoecord uninstall` first |
| macOS | `PhoeCord_<version>_<arch>.dmg` with `PhoeCord.app` | Drag into Applications; the DMG shows the GPL first |
| Linux | `PhoeCord_<version>_amd64.deb`, `PhoeCord_<version>_x86_64.rpm` | Both binaries in `/usr/bin`, a menu entry for the window; built on Ubuntu 22.04 (glibc 2.35) |

The file names are a contract: installed copies look for exactly these names when they update
themselves (`phoecord_installer::update::asset_name`).

Both binaries go in: the window (`phoecord-installer`) and the proxy (`phoecord`), side by side,
because the window looks for `phoecord` next to itself.

## Build locally

```bash
cargo install cargo-packager --locked --version 0.11.8
cd crates/phoecord-installer-gui
cargo packager --release -f app -f dmg      # macOS
cargo packager --release -f nsis            # Windows (downloads NSIS on first run)
cargo packager --release -f deb             # Linux; then, from the repository root:
#   cargo install cargo-generate-rpm --locked --version 0.21.1
#   cargo generate-rpm -p crates/phoecord-installer-gui -o target/packages/
cd ../.. && bash scripts/package/name-files.sh
```

Results land in `target/packages/`. cargo-packager names the `.exe` and `.deb` after the main
binary (`phoecord-installer_…`) and cargo-generate-rpm writes `phoecord-<version>-1.x86_64.rpm`;
`name-files.sh` renames them to `PhoeCord_…` like the `.dmg`. The NSIS template is a marked copy of
cargo-packager's (every change says `PhoeCord:`): compare it with the upstream file when bumping
cargo-packager. The
Russian strings of cargo-packager's own NSIS pages come from
`crates/phoecord-installer-gui/assets/installer/nsis/Russian.nsh` (UTF-8 with BOM, same keys as
cargo-packager's `English.nsh`; check them when bumping cargo-packager). CI builds the same on every PR that touches the installer and
on `v*` tags (`.github/workflows/package.yml`) and keeps them as workflow artifacts.

## Artwork

`python scripts/package/make_art.py` renders the app icons (`assets/icon/`) and the installer
pictures (`assets/installer/`: NSIS header and sidebar, WiX banner and dialog, DMG background) from
`assets/icon/icon.svg`. It needs `rsvg-convert` and Pillow (`iconutil` on macOS for the `.icns`).
Commit what it writes; packaging only reads those files.

## Updates

Installed copies update themselves from GitHub Releases (task 2.4.5; `docs/ARCHITECTURE.md`,
"Installer and releases"). The Windows setup understands `/S` (silent), `/R` (start the window
afterwards) and `/LAUNCHPROXY` (start `phoecord run` afterwards); an uninstall with `/UPDATE` keeps
what `phoecord setup` did. CLI archives are never updated automatically.

## Not signed

Releases are not code-signed (rulings, 1.4.5 and 2.4.5 Q8). Windows SmartScreen and macOS Gatekeeper
warn on first start: on macOS, right-click PhoeCord.app → Open. Check downloads against `SHA256SUMS`.
