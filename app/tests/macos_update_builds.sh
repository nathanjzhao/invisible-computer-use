#!/bin/bash
# Build signed-update test bundles on a disposable Mac. Nothing is published and
# the release key is never used: a throwaway minisign key signs every archive,
# and the bundles point their updater at https://localhost:8443/latest.json,
# served with a throwaway CA that this script trusts system-wide.
#
#   app/tests/macos_update_builds.sh <out-dir>
# Produces <out>/<version>/Copilot.app, Copilot.app.tar.gz(.sig) for 0.1.0 and
# 0.2.0, a second throwaway key for wrong-key signatures, and the TLS files.
set -euxo pipefail
OUT=$(cd "$(dirname "$1")" && pwd)/$(basename "$1"); mkdir -p "$OUT"
APP=$(cd "$(dirname "$0")/.." && pwd)
cd "$APP"
for k in throwaway other; do
  [ -f "$OUT/$k.key" ] || npx tauri signer generate -w "$OUT/$k.key" -p "" --ci -f
done
if [ ! -f "$OUT/ca.pem" ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj "/CN=ICU throwaway update CA" \
    -keyout "$OUT/ca.key" -out "$OUT/ca.pem" -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign
  openssl req -newkey rsa:2048 -nodes -subj "/CN=localhost" -keyout "$OUT/server.key" -out "$OUT/server.csr"
  printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\n' > "$OUT/server.ext"
  openssl x509 -req -in "$OUT/server.csr" -CA "$OUT/ca.pem" -CAkey "$OUT/ca.key" -CAcreateserial -days 2 -extfile "$OUT/server.ext" -out "$OUT/server.pem"
  sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain "$OUT/ca.pem"
fi
PUB=$(cat "$OUT/throwaway.key.pub")
for V in 0.1.0 0.2.0; do
  CONFIG=$(python3 -c 'import json,sys; print(json.dumps({"version": sys.argv[1], "bundle": {"createUpdaterArtifacts": True, "macOS": {"signingIdentity": "-"}},
    "plugins": {"updater": {"pubkey": sys.argv[2], "endpoints": ["https://localhost:8443/latest.json"]}}}))' "$V" "$PUB")
  TAURI_SIGNING_PRIVATE_KEY="$(cat "$OUT/throwaway.key")" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
    npx tauri build --bundles app --config "$CONFIG"
  rm -rf "$OUT/$V"; mkdir -p "$OUT/$V"
  ditto src-tauri/target/release/bundle/macos/Copilot.app "$OUT/$V/Copilot.app"
  cp src-tauri/target/release/bundle/macos/Copilot.app.tar.gz src-tauri/target/release/bundle/macos/Copilot.app.tar.gz.sig "$OUT/$V/"
done
# The 0.2.0 archive signed by a key the clients do not trust.
cp "$OUT/0.2.0/Copilot.app.tar.gz" "$OUT/wrong-key.app.tar.gz"
TAURI_SIGNING_PRIVATE_KEY="$(cat "$OUT/other.key")" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" npx tauri signer sign "$OUT/wrong-key.app.tar.gz"
shasum -a 256 "$OUT"/*/Copilot.app/Contents/MacOS/* "$OUT"/*/Copilot.app.tar.gz "$OUT"/wrong-key.app.tar.gz
echo UPDATE_BUILDS_DONE
