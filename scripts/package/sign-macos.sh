#!/usr/bin/env bash
# Ad-hoc sign PhoeCord.app made by `cargo packager` and put the signed copy into the .dmg.
# cargo-packager leaves the bundle without a valid signature, which macOS reports as "PhoeCord is
# damaged and can't be opened". An ad-hoc signature (no certificate) makes it the ordinary
# "unidentified developer" case, with Open Anyway in Privacy & Security.
# The .dmg is converted to read-write, its app replaced, and converted back, so the window layout and
# background stay. Run on macOS after `cargo packager`, before name-files.sh.
# Usage: bash scripts/package/sign-macos.sh [dir]   (default: target/packages)
set -euo pipefail
cd "${1:-target/packages}"
app=PhoeCord.app
[ -d "$app" ] || { echo "no $app in $PWD" >&2; exit 1; }

codesign --force --deep --sign - "$app"
codesign --verify --deep --strict --verbose=2 "$app"

shopt -s nullglob
for dmg in *.dmg; do
  rw="rw-$$.dmg"
  mnt="$(mktemp -d)"
  hdiutil convert "$dmg" -format UDRW -o "$rw" -quiet
  hdiutil attach "$rw" -mountpoint "$mnt" -nobrowse -noverify -quiet
  rm -rf "$mnt/$app"
  ditto "$app" "$mnt/$app"
  codesign --verify --deep --strict --verbose=2 "$mnt/$app"
  sync
  hdiutil detach "$mnt" -quiet
  rm -f "$dmg"
  hdiutil convert "$rw" -format UDZO -o "$dmg" -quiet
  rm -f "$rw"
  echo "re-signed app in $dmg"
done
