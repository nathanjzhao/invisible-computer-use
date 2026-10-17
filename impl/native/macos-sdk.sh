#!/usr/bin/env bash
# Sourced by impl/install.sh and impl/native/apply-and-build.sh on macOS.
#
# The patched cua-driver's ScreenCaptureKit bindings use APIs first shipped in the
# macOS 15.2 SDK (SCContentFilter.includedWindows); an older SDK fails deep in a
# Swift build step. Check before building. With no DEVELOPER_DIR set, pick the
# newest /Applications/Xcode*.app whose SDK is new enough; an explicit
# DEVELOPER_DIR is the user's choice and is only checked.
ICU_MIN_MACOS_SDK=15.2
icu_sdk_at_least() {  # icu_sdk_at_least <version> <minimum>
  local IFS=.; local -a v=($1) m=($2)
  (( ${v[0]:-0} > ${m[0]:-0} || (${v[0]:-0} == ${m[0]:-0} && ${v[1]:-0} >= ${m[1]:-0}) ))
}
icu_check_macos_sdk() {
  local sdk best="" best_sdk="" dir v
  sdk=$(xcrun --sdk macosx --show-sdk-version 2>/dev/null || echo none)
  [[ $sdk != none ]] && icu_sdk_at_least "$sdk" "$ICU_MIN_MACOS_SDK" && return 0
  if [[ -z "${DEVELOPER_DIR:-}" ]]; then
    # ICU_XCODE_APPS only moves the search for tests/macos-sdk.test.sh.
    for dir in "${ICU_XCODE_APPS:-/Applications}"/Xcode*.app/Contents/Developer; do
      [[ -d $dir ]] || continue
      v=$(DEVELOPER_DIR=$dir xcrun --sdk macosx --show-sdk-version 2>/dev/null) || continue
      if icu_sdk_at_least "$v" "$ICU_MIN_MACOS_SDK" && { [[ -z $best ]] || icu_sdk_at_least "$v" "$best_sdk"; }; then best=$dir; best_sdk=$v; fi
    done
    if [[ -n $best ]]; then
      export DEVELOPER_DIR=$best
      echo "note: the selected Xcode has macOS SDK $sdk; building with DEVELOPER_DIR=$best (SDK $best_sdk)"
      return 0
    fi
  fi
  echo "error: building cua-driver needs the macOS $ICU_MIN_MACOS_SDK SDK or later (Xcode 16.2+); xcrun reports $sdk${DEVELOPER_DIR:+ for DEVELOPER_DIR=$DEVELOPER_DIR}." >&2
  echo "Install Xcode 16.2 or later and select it (xcode-select -s, or DEVELOPER_DIR)." >&2
  return 1
}
