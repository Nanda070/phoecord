#!/usr/bin/env bash
# Give the installers their release names. cargo-packager names the NSIS .exe and the .deb after the
# main binary (phoecord-installer_<version>_x64-setup.exe, phoecord-installer_<version>_amd64.deb)
# and cargo-generate-rpm writes phoecord-<version>-1.x86_64.rpm; users get PhoeCord_<version>_...
# like the .dmg, which already uses the product name. The updater looks for exactly these names
# (`phoecord_installer::update::asset_name`). Run after packaging; idempotent.
# Usage: bash scripts/package/name-files.sh [dir]   (default: target/packages)
set -euo pipefail
cd "${1:-target/packages}"
shopt -s nullglob
for f in phoecord-installer_*.exe phoecord-installer_*.dmg phoecord-installer_*.deb; do
  mv -f -- "$f" "PhoeCord_${f#phoecord-installer_}"
  echo "PhoeCord_${f#phoecord-installer_}"
done
for f in phoecord-*-*.x86_64.rpm; do
  v=${f#phoecord-}
  v=${v%-*.x86_64.rpm}
  mv -f -- "$f" "PhoeCord_${v}_x86_64.rpm"
  echo "PhoeCord_${v}_x86_64.rpm"
done
