#!/usr/bin/env bash
# Fail the build if the bundle icons are still Tauri's default logo (the yellow and teal rings)
# instead of the set generated from icons/source-icon.svg, or if icon.icns lacks a size macOS uses.
#
#   app/scripts/check-icons.sh [icons dir]
#
# Regenerate after changing the design:  cd app/src-tauri && npx tauri icon icons/source-icon.svg
# Runs from beforeBuildCommand, so `tauri build` (and sign-notarize-macos.sh) stop here.
set -euo pipefail
DIR="${1:-$(cd "$(dirname "$0")/../src-tauri/icons" && pwd)}"
REGEN="regenerate: cd app/src-tauri && npx tauri icon icons/source-icon.svg"
if command -v sha256sum >/dev/null 2>&1; then
  HASH=(sha256sum)
elif command -v shasum >/dev/null 2>&1; then
  HASH=(shasum -a 256)
else
  echo "check-icons: a SHA-256 utility is required" >&2
  exit 1
fi

# sha256 of the files `tauri init` shipped in this repo (commit 18aa790).
DEFAULTS="
icon.icns 3dc10493b7de48a61de58f768f8a5708d3a44a068c148cedf0502b9b9b71ba5d
icon.png b5d93c8ec365c08b11bd006e46a46e227c681d30bb295af3d017573bfc752a83
32x32.png d151f11e325f7502de0c739a2e51697aa569fd4701ae6e11fae1a3b4c7d5f157
128x128.png 1f3689f6374b0553996fdc99743799216703835070145d7f0e6ec11e6280139e
128x128@2x.png 69194785eef4323955af73b0c03362ac0804db056c2424f9715076cabc0ed103
icon.ico e38ca88e1d5490f3dcbc3c3fa525f7fcb7b80fff3cb2f3a4eb1b2d018c0915c1
"
fail=0
while read -r name sum; do
  [ -n "$name" ] || continue
  f="$DIR/$name"
  [ -f "$f" ] || { echo "check-icons: $name is missing; $REGEN" >&2; fail=1; continue; }
  if ! digest=$("${HASH[@]}" "$f"); then
    echo "check-icons: cannot hash $name" >&2; fail=1; continue
  fi
  digest=${digest%% *}
  if [[ ! "$digest" =~ ^[0-9a-f]{64}$ ]]; then
    echo "check-icons: invalid hash for $name" >&2; fail=1; continue
  fi
  if [ "$digest" = "$sum" ]; then
    echo "check-icons: $name is Tauri's default logo, not source-icon.svg; $REGEN" >&2; fail=1
  fi
done <<< "$DEFAULTS"

# Tauri's default icns (any version) carries only a 512px entry, so this also catches a
# default from a newer template whose bytes differ. macOS wants the 1024 (512@2x) for Retina.
if [[ "$(uname -s)" == Darwin && -f "$DIR/icon.icns" ]]; then
  SET="$(mktemp -d)/icon.iconset"; trap 'rm -rf "$(dirname "$SET")"' EXIT
  iconutil -c iconset "$DIR/icon.icns" -o "$SET"
  for want in icon_16x16.png icon_32x32.png icon_128x128.png icon_256x256.png icon_512x512.png icon_512x512@2x.png; do
    [ -f "$SET/$want" ] || { echo "check-icons: icon.icns has no $want; $REGEN" >&2; fail=1; }
  done
  if [ -f "$SET/icon_512x512@2x.png" ] && [ "$(sips -g pixelWidth "$SET/icon_512x512@2x.png" | awk '/pixelWidth/{print $2}')" != 1024 ]; then
    echo "check-icons: icon.icns 512@2x entry is not 1024px; $REGEN" >&2; fail=1
  fi
fi
[ "$fail" = 0 ] && echo "check-icons: bundle icons come from source-icon.svg"
exit "$fail"
