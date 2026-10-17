#!/usr/bin/env bash
# Gather a platform's release assets (installers + updater artifacts + .sig) into
# dist-release/<platform>/ so the publish job can upload them and read the .sig
# files for latest.json.
set -euo pipefail
PLATFORM="${1:?usage: collect-artifacts.sh <platform>}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/dist-release/$PLATFORM"
mkdir -p "$OUT"
BUNDLE="$ROOT/app/src-tauri/target/release/bundle"

case "$PLATFORM" in
  windows-*)
    # NSIS + MSI installers and the updater signatures tauri writes next to them.
    for d in nsis msi; do
      [ -d "$BUNDLE/$d" ] || continue
      find "$BUNDLE/$d" -maxdepth 1 -type f \
        \( -name '*.exe' -o -name '*.msi' -o -name '*.zip' -o -name '*.sig' \) \
        -exec cp -f {} "$OUT/" \;
    done
    ;;
  darwin-*)
    # build-macos.sh already staged the notarized .app.zip + the signed
    # .app.tar.gz updater artifact + its .sig into $OUT.
    : ;;
  *)
    echo "unknown platform: $PLATFORM" >&2; exit 1 ;;
esac

echo "staged for $PLATFORM:"
ls -la "$OUT"
