#!/usr/bin/env bash
# Build our patched cua-driver, sign it with a stable Developer ID, and stage it
# as the Tauri externalBin sidecar so `tauri build` bundles it into Copilot.app.
#
#   app/scripts/stage-driver.sh
#
# Output: app/src-tauri/binaries/cua-driver-<host-triple> (gitignored, ~31MB).
# The bundled driver runs with the fast-path env (ICU_NO_WINDOW_OBSERVE) that the
# app sets when it launches it; see app/src-tauri/src/cua.rs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TRIPLE="$(rustc -vV | sed -n 's/host: //p')"
BUILT="$ROOT/vendor-cua/libs/cua-driver/rust/target/release/cua-driver"
DEST="$ROOT/app/src-tauri/binaries/cua-driver-$TRIPLE"
case "$TRIPLE" in
  *-windows-*) BUILT="$BUILT.exe"; DEST="$DEST.exe" ;;
esac

echo "== build patched cua-driver"
bash "$ROOT/impl/native/apply-and-build.sh"

echo "== sign with a stable Developer ID (grants survive rebuilds)"
# ICU_SIGN_IDENTITY overrides; otherwise the first Developer ID Application id.
. "$ROOT/impl/native/macos-sign.sh"
mkdir -p "$(dirname "$DEST")"
cp "$BUILT" "$DEST"
icu_sign_macos_binary "$DEST"

case "$TRIPLE" in
  *-windows-*)
    # The UIAccess worker (manifest uiAccess="true") is the second Windows
    # sidecar; tauri.windows.conf.json lists it. Authenticode signing happens
    # at bundle time (scripts/sign-windows.ps1), not here.
    UIA="$(dirname "$BUILT")/cua-driver-uia.exe"
    [ -f "$UIA" ] || { echo "UIAccess worker not built: $UIA" >&2; exit 1; }
    cp "$UIA" "$(dirname "$DEST")/cua-driver-uia-$TRIPLE.exe"
    echo "== staged $(dirname "$DEST")/cua-driver-uia-$TRIPLE.exe"
    ;;
esac

echo "== staged $DEST"
if [[ "$(uname -s)" == Darwin ]]; then
  codesign --verify --strict "$DEST"
fi
echo "Driver staged for the native $TRIPLE app build."
