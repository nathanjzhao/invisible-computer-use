#!/usr/bin/env bash
# One-command verification of the invisible-computer-use parity delta.
#   impl/verify.sh              deterministic: workspace tests + build + conformance (our through `icu`, cua, native if built)
#   impl/verify.sh --live       also the live suites (needs CONF_PID/CONF_WID/CONF_PACK_* and, for native, a daemon on NATIVE_CUA_SOCKET)
#   impl/verify.sh --native     also rebuild the patched driver first (impl/native/apply-and-build.sh) and run its unit tests
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
LIVE=0; NATIVE=0
for a in "$@"; do case "$a" in --live) LIVE=1;; --native) NATIVE=1;; esac; done
fail=0
step() { printf '\n=== %s\n' "$*"; }
step "fresh workspace binaries (including required AX-diff guard)"
# Explicit workspace target directory: inherited CARGO_TARGET_DIR and stale per-crate
# artifacts must not change which source/build this verification accepts.
export CARGO_TARGET_DIR="$ROOT/impl/target"
cargo build --locked --manifest-path impl/Cargo.toml
export ICU_BIN="$CARGO_TARGET_DIR/debug/icu"
export AXDIFF_CUA_BIN="$CARGO_TARGET_DIR/debug/axdiff-cua"
for binary in "$ICU_BIN" "$AXDIFF_CUA_BIN"; do
  [[ -x "$binary" ]] || { echo "required binary missing: $binary" >&2; exit 1; }
done
python3 impl/verification_manifest.py "$ICU_BIN" "$AXDIFF_CUA_BIN"
step "owned crates (workspace impl/Cargo.toml): tests"
for c in axdiff app_instructions window2 confirmations skysight icu; do
  printf '%-18s ' "$c"
  if out=$(cargo test -q --manifest-path impl/Cargo.toml -p "$c" 2>&1); then echo "$out" | grep -E 'test result' | awk '{p+=$4; f+=$6} END {printf "passed %d failed %d\n", p, f}'; else echo "FAILED"; echo "$out" | tail -20; fail=1; fi
done
if [[ $NATIVE == 1 ]]; then
  step "native patch: apply + build + unit tests"
  env -u CARGO_TARGET_DIR impl/native/apply-and-build.sh
  ( cd vendor-cua/libs/cua-driver/rust && unset CARGO_TARGET_DIR && cargo test -q -p cua-driver-core --lib && cargo test -q -p platform-macos --lib ) || fail=1
fi
step "conformance (deterministic)"
conf() {
  local label=$1; shift
  local result=0
  echo "$label"
  env "$@" python3 -m pytest conformance/ -q || result=$?
  [[ $result == 0 ]] || fail=1
}
conf "our:   " CONFORMANCE_TARGET=our
conf "cua:   " CONFORMANCE_TARGET=cua
NB=vendor-cua/libs/cua-driver/rust/target/release/cua-driver
if [[ $NATIVE == 1 ]]; then
  [[ -x $NB ]] || { echo "required native binary missing: $NB" >&2; exit 1; }
  python3 impl/verification_manifest.py "$NB"
fi
echo "native daemon conformance is live-only; native unit tests run with --native"
if [[ $LIVE == 1 ]]; then
  step "conformance (live) — windows must be on their display's current Space (window2 placement --id N tells you)"
  : "${CONF_PID:?set CONF_PID/CONF_WID (no-pack window, e.g. Calculator) and CONF_PACK_PID/CONF_PACK_WID/CONF_PACK_BUNDLE_ID (e.g. Clock)}"
  export CONFORMANCE_LIVE=1
  AXDIFF_STATE_DIR=$(mktemp -d) CONFORMANCE_TARGET=our python3 -m pytest conformance/ -q 2>&1 | tail -1 | sed 's/^/our live:    /'
  CONFORMANCE_TARGET=cua python3 -m pytest conformance/ -q 2>&1 | tail -1 | sed 's/^/cua live:    /'
  [[ -x $NB ]] && NATIVE_CUA_SOCKET="${NATIVE_CUA_SOCKET:-/tmp/icu-native.sock}" CONFORMANCE_TARGET=native python3 -m pytest conformance/ -q 2>&1 | tail -1 | sed 's/^/native live: /'
fi
step "done (fail=$fail)"; exit $fail
