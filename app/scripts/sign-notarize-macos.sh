#!/usr/bin/env bash
# Build Copilot.app signed with a Developer ID, notarize it, staple the ticket and check Gatekeeper.
#
#   ICU_NOTARY_ISSUER=<issuer uuid> app/scripts/sign-notarize-macos.sh
#
# Inputs (defaults are the owner's layout; override any of them):
#   ICU_SIGN_KEY      PEM private key of the Developer ID Application certificate
#                     (default ~/.tauri/signing-request/developerID_application.key)
#   ICU_SIGN_CERT     that certificate, DER or PEM (default ~/Downloads/developerID_application.cer)
#   ICU_NOTARY_KEY    App Store Connect API key (.p8)
#                     (default ~/.appstoreconnect/private_keys/AuthKey_$ICU_NOTARY_KEY_ID.p8)
#   ICU_NOTARY_KEY_ID key id (default G63MAFDGA5, a team key, so the issuer is required)
#   ICU_NOTARY_ISSUER App Store Connect issuer id (required)
#
# The key and certificate are bundled with Apple's Developer ID G2 intermediate into a
# throwaway p12 that Tauri imports into its own temporary keychain (APPLE_CERTIFICATE), so
# the login keychain is never changed. Without the intermediate, codesign fails with
# errSecInternalComponent ("unable to build chain to self-signed root"). Notarization passes
# the key on the command line, so no notarytool keychain profile is stored either.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
KEY="${ICU_SIGN_KEY:-$HOME/.tauri/signing-request/developerID_application.key}"
CERT="${ICU_SIGN_CERT:-$HOME/Downloads/developerID_application.cer}"
KEY_ID="${ICU_NOTARY_KEY_ID:-G63MAFDGA5}"
P8="${ICU_NOTARY_KEY:-$HOME/.appstoreconnect/private_keys/AuthKey_$KEY_ID.p8}"
ISSUER="${ICU_NOTARY_ISSUER:?set ICU_NOTARY_ISSUER to the App Store Connect issuer id}"
IDENTITY="${APPLE_SIGNING_IDENTITY:-Developer ID Application: Nathan Zhao (G7P6939437)}"

TMP="$(mktemp -d)"; chmod 700 "$TMP"; trap 'rm -rf "$TMP"' EXIT
umask 077

# Certificate and key must match, or codesign fails much later with a vaguer error.
if openssl x509 -inform der -in "$CERT" -out "$TMP/cert.pem" 2>/dev/null; then :; else cp "$CERT" "$TMP/cert.pem"; fi
# Each side is read on its own: with an unreadable key and certificate both pipelines used to print
# nothing, and the digest of nothing is equal on both sides.
CERT_PUB="$(openssl x509 -in "$TMP/cert.pem" -noout -pubkey 2>/dev/null)" || CERT_PUB=""
KEY_PUB="$(openssl pkey -in "$KEY" -pubout 2>/dev/null)" || KEY_PUB=""
[ -n "$CERT_PUB" ] || { echo "cannot read a certificate from $CERT" >&2; exit 1; }
[ -n "$KEY_PUB" ] || { echo "cannot read a private key from $KEY (is it encrypted or the wrong file?)" >&2; exit 1; }
[ "$(printf %s "$CERT_PUB" | openssl sha256)" = "$(printf %s "$KEY_PUB" | openssl sha256)" ] \
  || { echo "certificate does not match the key" >&2; exit 1; }

curl -fsSL -o "$TMP/g2.der" https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer
openssl x509 -inform der -in "$TMP/g2.der" -out "$TMP/g2.pem"
PW="$(openssl rand -hex 16)"
# The password goes in through the environment, not argv, so `ps` does not show it.
PW="$PW" openssl pkcs12 -export -inkey "$KEY" -in "$TMP/cert.pem" -certfile "$TMP/g2.pem" -name "$IDENTITY" \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1 -passout env:PW -out "$TMP/id.p12"

cd "$ROOT/app"
# Cross-compile support: when ICU_BUILD_TARGET is set (e.g. building the x86_64 app
# on an arm64 runner), pass --target and read the bundle from target/<triple>/release.
# Left empty for a native local build, which keeps target/release unchanged.
TARGET_FLAGS=""; BUNDLE_SUB="release"
if [ -n "${ICU_BUILD_TARGET:-}" ]; then TARGET_FLAGS="--target $ICU_BUILD_TARGET"; BUNDLE_SUB="$ICU_BUILD_TARGET/release"; fi
APPLE_CERTIFICATE="$(base64 -i "$TMP/id.p12")" APPLE_CERTIFICATE_PASSWORD="$PW" APPLE_SIGNING_IDENTITY="$IDENTITY" \
  npm run tauri build -- --bundles app $TARGET_FLAGS --config '{"bundle":{"createUpdaterArtifacts":false}}'

APP="$ROOT/app/src-tauri/target/$BUNDLE_SUB/bundle/macos/Copilot.app"
codesign --verify --deep --strict "$APP"
ditto -c -k --keepParent "$APP" "$TMP/notarize.zip"
xcrun notarytool submit "$TMP/notarize.zip" --key "$P8" --key-id "$KEY_ID" --issuer "$ISSUER" --wait --timeout 30m
xcrun stapler staple "$APP"
spctl -a -vv -t exec "$APP"
echo "signed, notarized and stapled: $APP"
