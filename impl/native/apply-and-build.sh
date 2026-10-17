#!/usr/bin/env bash
# Apply the invisible-computer-use native patches to the vendored cua-driver and build it.
#
#   impl/native/apply-and-build.sh          # apply patch (idempotent) + release build
#   impl/native/apply-and-build.sh --diff   # regenerate cua-driver.patch from the vendor working tree
#   impl/native/apply-and-build.sh --regen  # regenerate contract/manifest.json and the SDK bindings, then --diff
#   impl/native/apply-and-build.sh --check-generated   # fail when the manifest or bindings are stale
#
# vendor-cua/ is its own git checkout and is gitignored here, so the patch file
# is the tracked artefact, including the native dependency lockfile. Build on
# the target platform with its native libraries installed.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VENDOR="$ROOT/vendor-cua"
RUST="$VENDOR/libs/cua-driver/rust"
PATCH="$ROOT/impl/native/cua-driver.patch"

# The vendor paths the patch carries (impl/native/patch-paths.txt), shared with merge-patch.sh.
PATCH_PATHS=()
while IFS= read -r line; do
  case "$line" in ''|'#'*) ;; *) PATCH_PATHS+=("$line") ;; esac
done < "$ROOT/impl/native/patch-paths.txt"
[[ ${#PATCH_PATHS[@]} -gt 0 ]] || { echo "impl/native/patch-paths.txt lists no paths" >&2; exit 1; }

# The generated files the patch carries (the tool contract manifest and the SDK bindings) must match
# the source in the same patch. The manifest check compiles one crate; the bindings check also builds
# the SDK library and needs node and `npm ci` in libs/cua-driver/typescript.
generated() {
  local mode="$1"
  ( cd "$RUST" && cargo run -q --locked -p cua-driver-contract --bin cua-contract-gen -- manifest ${mode:+"$mode"} ) || return 1
  if [[ "${2:-}" == "bindings" ]]; then
    ( cd "$VENDOR/libs/cua-driver" && node scripts/generate-uniffi-bindings.mjs ${mode:+"$mode"} ) || return 1
  fi
}

if [[ "${1:-}" == "--check-generated" ]]; then
  generated --check bindings
  echo "manifest and bindings match the source"
  exit 0
fi

if [[ "${1:-}" == "--regen" ]]; then
  # Regenerate the manifest and the bindings from the patched source, then rewrite the patch.
  generated "" bindings
  exec "$0" --diff
fi

if [[ "${1:-}" == "--diff" ]]; then
  # A patch with a stale manifest is the bug this refuses: the source gained an argument the
  # checked-in contract does not list.
  generated --check || { echo "contract/manifest.json is stale against the crates; run impl/native/apply-and-build.sh --regen" >&2; exit 1; }
  ( cd "$VENDOR" && git ls-files --others --exclude-standard -- "${PATCH_PATHS[@]}" | xargs -r git add -N
    git diff -- "${PATCH_PATHS[@]}" > "$PATCH" )
  echo "files in patch:"; grep '^diff --git' "$PATCH" | sed 's/.* b\///'
  grep -q 'cua-driver-core/src/element_revision.rs' "$PATCH" || { echo "BUG: core element_revision.rs missing from patch" >&2; exit 1; }
  echo "wrote $PATCH ($(wc -l < "$PATCH") lines)"
  exit 0
fi

# Git Bash prepends its Unix tools, including an unrelated link.exe. Bind
# Rust's MSVC linker to the compiler selected by the developer shell.
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    HOST="$(rustc -vV | sed -n 's/host: //p')"
    case "$HOST" in
      *-pc-windows-msvc)
        LINKER_KEY="CARGO_TARGET_$(printf '%s' "$HOST" | tr '[:lower:]-' '[:upper:]_')_LINKER"
        if [[ -z "${!LINKER_KEY:-}" ]]; then
          CL="$(command -v cl.exe)" || { echo "Run from an MSVC developer shell (cl.exe is missing)." >&2; exit 1; }
          MSVC_LINKER="$(dirname "$CL")/link.exe"
          [[ -f "$MSVC_LINKER" ]] || { echo "MSVC linker missing beside cl.exe." >&2; exit 1; }
          export "$LINKER_KEY=$(cygpath -w "$MSVC_LINKER")"
        fi
        # One source built at two paths gave two drivers. Cargo hashes a path
        # dependency outside the workspace (../impl/axdiff) by its absolute
        # location into the metadata of every crate that uses it, which moves
        # symbols and layout; so build through one fixed junction to this tree.
        # rustc also embeds absolute paths (panic locations in registry crates
        # and in axdiff), so map the junction and CARGO_HOME away. Arrays
        # merge, so the workspace's crt-static rustflags stay; a RUSTFLAGS
        # variable would replace both. One build at a time per machine.
        [[ -z "${RUSTFLAGS:-}" ]] || { echo "Unset RUSTFLAGS: it replaces the path remapping and crt-static." >&2; exit 1; }
        BUILD_ROOT="${ICU_DRIVER_BUILD_ROOT:-C:\\icu-driver-src}"
        if [[ -e "$(cygpath -u "$BUILD_ROOT")" || -L "$(cygpath -u "$BUILD_ROOT")" ]]; then
          # rmdir removes a junction, not its target, and refuses a directory with contents.
          MSYS_NO_PATHCONV=1 cmd /c rmdir "$BUILD_ROOT" >/dev/null || { echo "$BUILD_ROOT exists and is not a junction." >&2; exit 1; }
        fi
        MSYS_NO_PATHCONV=1 cmd /c mklink /J "$BUILD_ROOT" "$(cygpath -w "$ROOT")" >/dev/null || { echo "Could not create $BUILD_ROOT." >&2; exit 1; }
        trap 'MSYS_NO_PATHCONV=1 cmd /c rmdir "$BUILD_ROOT" >/dev/null' EXIT
        # Name the junction to cargo in Windows form: a Git Bash cwd under the
        # junction reaches cargo resolved to the real tree (measured: axdiff's
        # absolute paths then came back).
        CARGO_ARGS=(--manifest-path "$BUILD_ROOT\\vendor-cua\\libs\\cua-driver\\rust\\Cargo.toml")
        TARGET_DIR="$BUILD_ROOT\\vendor-cua\\libs\\cua-driver\\rust\\target"
        CARGO_PREFIX="$(cygpath -w "${CARGO_HOME:-$HOME/.cargo}")"
        [[ "$BUILD_ROOT$CARGO_PREFIX" != *"'"* ]] || { echo "A path with a quote cannot be remapped." >&2; exit 1; }
        CARGO_CONFIG=(--config "target.$HOST.rustflags=['--remap-path-prefix=$BUILD_ROOT=/src','--remap-path-prefix=$CARGO_PREFIX=/cargo']")
        ;;
    esac
    ;;
esac

# core.autocrlf=false: Git for Windows defaults to true, and `git apply` then writes CRLF into the vendor tree (127 of
# 419 Rust files and contract/manifest.json measured on a host), so a tree built there is not byte-identical to one
# applied on macOS or Linux.
if ! ( cd "$VENDOR" && git -c core.autocrlf=false apply --check "$PATCH" 2>/dev/null ); then
  if ( cd "$VENDOR" && git -c core.autocrlf=false apply --check --reverse "$PATCH" 2>/dev/null ); then
    echo "patch already applied"
  else
    echo "patch does not apply cleanly to vendor-cua at $(git -C "$VENDOR" rev-parse --short HEAD)" >&2
    exit 1
  fi
else
  ( cd "$VENDOR" && git -c core.autocrlf=false apply "$PATCH" ) && echo "patch applied"
fi
if [[ "$(uname -s)" == Darwin ]]; then
  # shellcheck source=impl/native/macos-sdk.sh
  . "$ROOT/impl/native/macos-sdk.sh"
  icu_check_macos_sdk
fi
( cd "$RUST" && CARGO_TARGET_DIR="${TARGET_DIR:-$RUST/target}" cargo build --locked -p cua-driver --release ${CARGO_ARGS[@]+"${CARGO_ARGS[@]}"} ${CARGO_CONFIG[@]+"${CARGO_CONFIG[@]}"} )
BIN="$RUST/target/release/cua-driver"
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    BIN="$BIN.exe"
    # The UIAccess worker ships beside the driver in the Windows app. Its own
    # cargo invocation: building both packages in one would unify their
    # features and could change the driver's bytes.
    ( cd "$RUST" && CARGO_TARGET_DIR="${TARGET_DIR:-$RUST/target}" cargo build --locked -p cua-driver-uia --release ${CARGO_ARGS[@]+"${CARGO_ARGS[@]}"} ${CARGO_CONFIG[@]+"${CARGO_CONFIG[@]}"} )
    echo "built: $RUST/target/release/cua-driver-uia.exe"
    ;;
esac
echo "built: $BIN"
"$BIN" --version
