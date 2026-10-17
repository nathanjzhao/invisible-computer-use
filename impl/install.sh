#!/usr/bin/env bash
# Build and install invisible-computer-use with one command.
#
#   impl/install.sh [--prefix DIR] [--no-native] [--linux-docker]
#
# What it does:
#   1. builds the `icu` workspace in release mode (impl/target/release/icu);
#   2. builds the patched cua-driver for THIS host (impl/native/cua-driver.patch on the vendored
#      tree) natively with cargo (macOS, Linux, or Windows when run on a Windows host); `--no-native`
#      skips this and expects a `cua-driver` on PATH (unpatched: no host readiness, no native
#      diff lineage, no occlusion clip; the layer still runs its own lineage and packs);
#   3. copies `icu` (and the patched `cua-driver`) into the prefix (default ~/.local/bin);
#   4. prints the MCP client config for `icu mcp`.
#   `--linux-docker` additionally builds the Linux driver AND `icu` inside Docker for containers
#   (impl/native/linux-drive/build.sh; outputs under vendor-cua/.../target/docker-linux and
#   impl/target/docker-linux) and prints their paths; it never installs them into the prefix.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX="$HOME/.local/bin"; NATIVE=1; LINUX_DOCKER=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) PREFIX=$2; shift 2 ;;
    --no-native) NATIVE=0; shift ;;
    --linux-docker) LINUX_DOCKER=1; shift ;;
    *) echo "usage: impl/install.sh [--prefix DIR] [--no-native] [--linux-docker]" >&2; exit 2 ;;
  esac
done
mkdir -p "$PREFIX"

# macOS: the driver needs the macOS 15.2 SDK or later; pick or refuse before building.
if [[ $NATIVE == 1 && "$(uname -s)" == Darwin ]]; then
  # shellcheck source=impl/native/macos-sdk.sh
  . "$ROOT/impl/native/macos-sdk.sh"
  icu_check_macos_sdk
fi

echo "== icu (release)"
cargo build -q --release --manifest-path "$ROOT/impl/Cargo.toml" -p icu
install -m 0755 "$ROOT/impl/target/release/icu" "$PREFIX/icu"

DRIVER="cua-driver"
if [[ $NATIVE == 1 ]]; then
  echo "== patched cua-driver"
  RUST="$ROOT/vendor-cua/libs/cua-driver/rust"
  [[ -d "$RUST" ]] || { echo "vendor-cua is not checked out at $ROOT/vendor-cua" >&2; exit 1; }
  ( cd "$ROOT/vendor-cua" && git apply --check "$ROOT/impl/native/cua-driver.patch" 2>/dev/null && git apply "$ROOT/impl/native/cua-driver.patch" ) || true
  ( cd "$ROOT/vendor-cua" && git apply --check --reverse "$ROOT/impl/native/cua-driver.patch" ) || { echo "impl/native/cua-driver.patch does not apply to vendor-cua" >&2; exit 1; }
  case "$(uname -s)" in
    Darwin|Linux) ( cd "$RUST" && cargo build -q --release -p cua-driver ); BIN="$RUST/target/release/cua-driver" ;;
    MINGW*|MSYS*|CYGWIN*) ( cd "$RUST" && cargo build -q --release -p cua-driver ); BIN="$RUST/target/release/cua-driver.exe" ;;
    *) echo "unsupported host $(uname -s)" >&2; exit 1 ;;
  esac
  install -m 0755 "$BIN" "$PREFIX/$(basename "$BIN")"
  DRIVER="$PREFIX/$(basename "$BIN")"
  # macOS: give the rebuilt driver a stable code-signing identity so Accessibility
  # and Screen Recording grants survive rebuilds (ICU_SIGN_IDENTITY /
  # ICU_REQUIRE_STABLE_SIGNING; see impl/DISTRIBUTION.md). No-op off macOS.
  if [[ "$(uname -s)" == Darwin ]]; then
    # shellcheck source=impl/native/macos-sign.sh
    . "$ROOT/impl/native/macos-sign.sh"
    icu_sign_macos_binary "$DRIVER"
  fi
else
  command -v cua-driver >/dev/null || echo "note: no cua-driver on PATH; set ICU_DRIVER before running icu" >&2
fi

if [[ $LINUX_DOCKER == 1 ]]; then
  echo "== Linux binaries for containers (Docker)"
  "$ROOT/impl/native/linux-drive/build.sh"
  echo "  driver: $ROOT/vendor-cua/libs/cua-driver/rust/target/docker-linux/release/cua-driver"
  echo "  icu:    $ROOT/impl/target/docker-linux/release/icu"
fi

echo "== installed"
echo "  $PREFIX/icu"
[[ $NATIVE == 1 ]] && echo "  $DRIVER (patched)"
echo
echo "== MCP client config (icu mcp-config)"
ICU_DRIVER="$DRIVER" "$PREFIX/icu" mcp-config
cat <<EOF

Next:
  $PREFIX/icu doctor            # driver health, patch status, packs, paths
  $PREFIX/icu call list_apps    # one call through the same code path as \`icu mcp\`
On macOS the driver needs Accessibility + Screen Recording: \`$DRIVER permissions grant\`.
Without ICU_SOCKET, icu starts \`$DRIVER serve --no-overlay\` on ~/.icu/state/cua-driver.sock and reuses it; that daemon holds the grants.
Run the daemon once per login for the fastest calls: \`$DRIVER serve\` (then ICU_SOCKET if you chose a socket).
EOF
