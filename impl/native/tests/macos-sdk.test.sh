#!/usr/bin/env bash
# Hermetic guard for impl/native/macos-sdk.sh (the SDK check install.sh and
# apply-and-build.sh run before building the driver). xcrun is faked on PATH and the
# Xcode search is pointed at a scratch directory, so it runs on any host.
# Run: bash impl/native/tests/macos-sdk.test.sh
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SDK_SH="$HERE/../macos-sdk.sh"

fails=0
contains() { case "$2" in *"$3"*) echo "ok   - $1" ;; *) echo "FAIL - $1: [$2] lacks [$3]"; fails=$((fails+1)) ;; esac; }
check() { if [ "$2" = "$3" ]; then echo "ok   - $1"; else echo "FAIL - $1: expected [$3], got [$2]"; fails=$((fails+1)); fi; }

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin"
# `xcrun --sdk macosx --show-sdk-version` reads <DEVELOPER_DIR>/sdk-version; with no
# DEVELOPER_DIR it reads $FAKE_DEFAULT_SDK_FILE. "none" makes xcrun fail, as with no tools.
cat > "$TMP/bin/xcrun" <<'EOF'
#!/usr/bin/env bash
if [ -n "${DEVELOPER_DIR:-}" ]; then f="$DEVELOPER_DIR/sdk-version"; else f="$FAKE_DEFAULT_SDK_FILE"; fi
[ -f "$f" ] || exit 1
v="$(cat "$f")"
[ "$v" = none ] && exit 1
echo "$v"
EOF
chmod +x "$TMP/bin/xcrun"

# xcode <name> <sdk>: a fake /Applications/<name>.app
xcode() { mkdir -p "$TMP/apps/$1.app/Contents/Developer"; echo "$2" > "$TMP/apps/$1.app/Contents/Developer/sdk-version"; }
reset() { rm -rf "$TMP/apps"; mkdir -p "$TMP/apps"; }

# run <default-sdk> [DEVELOPER_DIR]: source the script in a subshell, call the check, print
# "<exit>|<DEVELOPER_DIR after>|<first line of output>"
run() {
  echo "$1" > "$TMP/default-sdk"
  (
    export PATH="$TMP/bin:$PATH" FAKE_DEFAULT_SDK_FILE="$TMP/default-sdk" ICU_XCODE_APPS="$TMP/apps"
    if [ -n "${2:-}" ]; then export DEVELOPER_DIR="$2"; else unset DEVELOPER_DIR; fi
    . "$SDK_SH"
    out="$(icu_check_macos_sdk 2>&1)"; rc=$?
    # the function exports DEVELOPER_DIR into this shell; capture it from a non-subshell call
    icu_check_macos_sdk >/dev/null 2>&1
    echo "$rc|${DEVELOPER_DIR:-}|$(echo "$out" | head -1)"
  )
}

# --- version comparison -------------------------------------------------------
cmp_() { ( . "$SDK_SH"; icu_sdk_at_least "$1" "$2" && echo yes || echo no ); }
check "15.2 meets 15.2" "$(cmp_ 15.2 15.2)" yes
check "15.10 is newer than 15.2" "$(cmp_ 15.10 15.2)" yes
check "15.1 is older than 15.2" "$(cmp_ 15.1 15.2)" no
check "16 meets 15.2" "$(cmp_ 16 15.2)" yes
check "14.99 is older than 15.2" "$(cmp_ 14.99 15.2)" no
check "26.0 meets 15.2" "$(cmp_ 26.0 15.2)" yes
check "an empty version is older" "$(cmp_ '' 15.2)" no

# --- the selected Xcode is new enough: nothing changes -----------------------
reset; xcode Xcode_16.4 15.5
check "a new enough default is left alone" "$(run 15.4)" "0||"
check "26.x default is left alone" "$(run 26.0)" "0||"

# --- the default is too old: the newest sufficient Xcode is selected ----------
reset; xcode Xcode_16.0 15.0; xcode Xcode_16.1 15.1; xcode Xcode_16.4 15.5; xcode Xcode_16.2 15.2
got="$(run 15.0)"
check "the newest sufficient Xcode is selected" "${got%%|*}|$(echo "$got" | cut -d'|' -f2 | sed "s#^$TMP#T#")" "0|T/apps/Xcode_16.4.app/Contents/Developer"
contains "it says which Xcode it chose" "$(echo "$got" | cut -d'|' -f3)" "note: the selected Xcode has macOS SDK 15.0; building with"

reset; xcode Xcode_16.2 15.2; xcode Xcode_16.1 15.1
got="$(run 15.0)"
check "an Xcode with exactly the minimum SDK is accepted" "$(echo "$got" | cut -d'|' -f2 | sed "s#^$TMP#T#")" "T/apps/Xcode_16.2.app/Contents/Developer"

# --- no Xcode is new enough: refuse, naming the SDK -------------------------
reset; xcode Xcode_16.0 15.0; xcode Xcode_16.1 15.1
got="$(run 15.0)"
check "no sufficient Xcode fails" "${got%%|*}" 1
contains "and says what it needs" "$(echo "$got" | cut -d'|' -f3)" "needs the macOS 15.2 SDK or later"
check "and leaves DEVELOPER_DIR unset" "$(echo "$got" | cut -d'|' -f2)" ""

# --- an explicit DEVELOPER_DIR is the user's choice --------------------------
reset; xcode Xcode_16.4 15.5; xcode Old 15.0
got="$(run 15.4 "$TMP/apps/Old.app/Contents/Developer")"
check "an explicit old DEVELOPER_DIR is checked, not replaced" "${got%%|*}" 1
check "and kept" "$(echo "$got" | cut -d'|' -f2 | sed "s#^$TMP#T#")" "T/apps/Old.app/Contents/Developer"
got="$(run 15.0 "$TMP/apps/Xcode_16.4.app/Contents/Developer")"
check "an explicit new DEVELOPER_DIR passes" "${got%%|*}" 0

# --- no developer tools at all ------------------------------------------------
reset; xcode Xcode_16.4 15.5
got="$(run none)"
check "with xcrun failing, a sufficient Xcode is still found" "${got%%|*}" 0
reset
got="$(run none)"
check "with xcrun failing and no Xcode, it fails" "${got%%|*}" 1

[ "$fails" = 0 ] && echo "all macos-sdk checks passed" || { echo "$fails check(s) failed"; exit 1; }
