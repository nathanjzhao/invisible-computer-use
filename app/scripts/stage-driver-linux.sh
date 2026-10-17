#!/usr/bin/env bash
# Packaged x86_64 Linux driver: portal input and PipeWire capture for GNOME/KDE.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
test "$(uname -s)" = Linux
test "$(rustc -vV | sed -n 's/host: //p')" = x86_64-unknown-linux-gnu
VENDOR="$ROOT/vendor-cua"
PATCH="$ROOT/impl/native/cua-driver.patch"
if git -C "$VENDOR" -c core.autocrlf=false apply --check "$PATCH" 2>/dev/null; then
  git -C "$VENDOR" -c core.autocrlf=false apply "$PATCH"
else
  git -C "$VENDOR" -c core.autocrlf=false apply --reverse --check "$PATCH"
fi
cd "$VENDOR/libs/cua-driver/rust"
cargo build --locked --release -p cua-driver --features portal-libei
mkdir -p "$ROOT/app/src-tauri/binaries"
install -m 755 "${CARGO_TARGET_DIR:-target}/release/cua-driver" \
  "$ROOT/app/src-tauri/binaries/cua-driver-x86_64-unknown-linux-gnu"
sha256sum "$ROOT/app/src-tauri/binaries/cua-driver-x86_64-unknown-linux-gnu"
