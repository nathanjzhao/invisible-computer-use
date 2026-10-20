#!/usr/bin/env bash
# Build + Developer ID sign + notarize the macOS app, then produce the updater
# artifact (a clean tar of the NOTARIZED .app, signed with the minisign updater
# key). Stages everything into dist-release/$PLATFORM. When the signing secrets
# are absent, builds an unsigned .app so CI still validates the build.
#
# Expects (from the workflow): RUST_TARGET, PLATFORM, and when signing:
#   APPLE_DEV_ID_KEY_PEM, APPLE_DEV_ID_CERT_BASE64, APPLE_NOTARY_API_KEY_P8,
#   APPLE_NOTARY_KEY_ID, APPLE_NOTARY_ISSUER_ID, APPLE_SIGNING_IDENTITY,
#   TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

ARCH_SUFFIX=aarch64
[ "${RUST_TARGET:-}" = "x86_64-apple-darwin" ] && ARCH_SUFFIX=x64
VERSION="$(node -p "require('./app/src-tauri/tauri.conf.json').version")"
OUT="$ROOT/dist-release/${PLATFORM:?}"
mkdir -p "$OUT"

# Cross-compile when the requested target is not the host arch (keeps this script
# usable from an arm64 runner). The release workflow builds the Intel (x86_64) app
# natively on macos-15-intel after selecting Xcode 16, whose linker tolerates a
# duplicate CoreMedia.swift object the apple_cf + screencapturekit vendored crates
# both embed (the image's default Xcode 26 linker rejects it), so this path runs
# native there. tauri writes a cross build under target/<triple>/release; native
# builds use target/release.
TGT="${RUST_TARGET:?}"
case "$TGT" in x86_64-*) TGT_ARCH=x86_64 ;; arm64-*|aarch64-*) TGT_ARCH=arm64 ;; *) TGT_ARCH="$(uname -m)" ;; esac
if [ "$TGT_ARCH" = "$(uname -m)" ]; then
  export ICU_BUILD_TARGET=""
  APP="$ROOT/app/src-tauri/target/release/bundle/macos/Copilot.app"
else
  export ICU_BUILD_TARGET="$TGT"
  APP="$ROOT/app/src-tauri/target/$TGT/release/bundle/macos/Copilot.app"
fi

if [ -n "${APPLE_DEV_ID_KEY_PEM:-}" ] && [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
  printf '%s' "$APPLE_DEV_ID_KEY_PEM" > "$TMP/devid.key"
  printf '%s' "$APPLE_DEV_ID_CERT_BASE64" | base64 -d > "$TMP/devid.cer"
  printf '%s' "$APPLE_NOTARY_API_KEY_P8" > "$TMP/notary.p8"

  # The repo's own script builds, Developer ID signs, notarizes and staples the .app
  # (it builds --bundles app with updater artifacts off).
  ICU_SIGN_KEY="$TMP/devid.key" \
  ICU_SIGN_CERT="$TMP/devid.cer" \
  ICU_NOTARY_KEY="$TMP/notary.p8" \
  ICU_NOTARY_KEY_ID="${APPLE_NOTARY_KEY_ID:?}" \
  ICU_NOTARY_ISSUER="${APPLE_NOTARY_ISSUER_ID:?}" \
    bash app/scripts/sign-notarize-macos.sh

  # Downloadable installer: the notarized, stapled .app zipped (ditto keeps the staple).
  ditto -c -k --keepParent "$APP" "$OUT/Copilot_${VERSION}_${ARCH_SUFFIX}.app.zip"

  # Updater artifact: a clean tar (no AppleDouble ._* files) of the notarized .app,
  # signed with the minisign updater key and bound to this version.
  TARBALL="$OUT/Copilot_${VERSION}_${ARCH_SUFFIX}.app.tar.gz"
  ( cd "$(dirname "$APP")" && COPYFILE_DISABLE=1 tar czf "$TARBALL" "$(basename "$APP")" )
  ( cd app && npx tauri signer sign \
      --private-key "$TAURI_SIGNING_PRIVATE_KEY" \
      --password "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" \
      --app-version "$VERSION" \
      "$TARBALL" )
  echo "signed + notarized macOS build staged in $OUT"
else
  echo "::warning::macOS signing/updater secrets absent — building an unsigned .app (validation only)."
  ( cd app && npm run tauri build -- --bundles app ${ICU_BUILD_TARGET:+--target "$ICU_BUILD_TARGET"} )
  ditto -c -k --keepParent "$APP" "$OUT/Copilot_${VERSION}_${ARCH_SUFFIX}.app.zip"
fi

ls -la "$OUT"
