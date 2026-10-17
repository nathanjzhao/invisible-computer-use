#!/usr/bin/env bash
# Hermetic guard for impl/native/macos-sign.sh. Runs on any host (no real cert
# needed): codesign, security, and uname are faked on PATH so the policy
# branches are exercised deterministically. Run: bash impl/native/tests/macos-sign-policy.test.sh
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SIGN_SH="$HERE/../macos-sign.sh"

fails=0
check() { if [ "$2" = "$3" ]; then echo "ok   - $1"; else echo "FAIL - $1: expected [$3], got [$2]"; fails=$((fails+1)); fi; }

# --- fakes on PATH -----------------------------------------------------------
SHIM="$(mktemp -d)"
cat > "$SHIM/codesign" <<'EOF'
#!/usr/bin/env bash
mode="sign"; sign=""; target=""; i=1
args=("$@")
while [ $i -le $# ]; do
  a="${args[$((i-1))]}"
  case "$a" in
    -d) mode="display" ;;
    --sign) i=$((i+1)); sign="${args[$((i-1))]}" ;;
    --identifier) i=$((i+1)) ;;
    -r-|--force|--deep) : ;;
    -*) : ;;
    *) target="$a" ;;
  esac
  i=$((i+1))
done
if [ "$mode" = "display" ]; then
  if [ "$(cat "$target.sig" 2>/dev/null)" = "cert" ]; then
    echo 'designated => identifier "com.trycua.driver" and anchor apple generic and certificate leaf[subject.OU] = TEAMID'
  else
    echo 'designated => cdhash H"deadbeef"'
  fi
  exit 0
fi
# refuse an unresolvable named identity, like real codesign
if [ "$sign" != "-" ] && [ "${FAKE_SIGN_FAIL:-0}" = "1" ]; then exit 1; fi
[ "$sign" = "-" ] && echo adhoc > "$target.sig" || echo cert > "$target.sig"
exit 0
EOF
cat > "$SHIM/security" <<'EOF'
#!/usr/bin/env bash
# only find-identity -p codesigning is used
[ "${FAKE_HAS_DEVID:-0}" = "1" ] && \
  echo '  1) ABCDEF0123456789ABCDEF0123456789ABCDEF01 "Developer ID Application: Test (TEAMID)"'
exit 0
EOF
cat > "$SHIM/uname" <<'EOF'
#!/usr/bin/env bash
[ "${1:-}" = "-s" ] && { echo Darwin; exit 0; }
exec /usr/bin/uname "$@"
EOF
chmod +x "$SHIM/codesign" "$SHIM/security" "$SHIM/uname"
export PATH="$SHIM:$PATH"

# reset every knob the source reads, then load it
unset ICU_SIGN_IDENTITY ICU_REQUIRE_STABLE_SIGNING FAKE_HAS_DEVID FAKE_SIGN_FAIL
# shellcheck source=../macos-sign.sh
. "$SIGN_SH"

# --- direct classifier units -------------------------------------------------
check "certificate leaf classifies as certificate-backed" \
  "$(icu_classify_requirement 'identifier x and certificate leaf[subject.OU] = T')" "certificate-backed"
check "cdhash classifies as ad-hoc" \
  "$(icu_classify_requirement 'cdhash H"deadbeef"')" "ad-hoc"

# --- A: a Developer ID identity is auto-detected and used --------------------
binA="$SHIM/a"; cp "$SHIM/uname" "$binA"
out="$(FAKE_HAS_DEVID=1 icu_sign_macos_binary "$binA" 2>&1)"; rc=$?
check "A exit 0" "$rc" "0"
check "A signed cert-backed" "$(cat "$binA.sig")" "cert"
case "$out" in *"stable identity"*) check "A reports stable identity" yes yes ;; *) check "A reports stable identity" no yes ;; esac

# --- A2: cert designated requirement is byte-stable across a rebuild ---------
drA="$(icu_designated_requirement "$binA")"
cp "$SHIM/security" "$binA"   # different bytes = a rebuild
FAKE_HAS_DEVID=1 icu_sign_macos_binary "$binA" >/dev/null 2>&1
check "A2 designated requirement unchanged after rebuild" "$(icu_designated_requirement "$binA")" "$drA"

# --- B: require-stable with no usable identity refuses (no signature) --------
binB="$SHIM/b"; cp "$SHIM/uname" "$binB"
FAKE_HAS_DEVID=0 ICU_REQUIRE_STABLE_SIGNING=1 icu_sign_macos_binary "$binB" >/dev/null 2>&1; rc=$?
check "B exit non-zero" "$([ $rc -ne 0 ] && echo nonzero || echo zero)" "nonzero"
check "B left the binary unsigned" "$([ -f "$binB.sig" ] && echo signed || echo unsigned)" "unsigned"

# --- C: no identity, not require-stable -> ad-hoc + warning ------------------
binC="$SHIM/c"; cp "$SHIM/uname" "$binC"
out="$(FAKE_HAS_DEVID=0 icu_sign_macos_binary "$binC" 2>&1)"; rc=$?
check "C exit 0" "$rc" "0"
check "C signed ad-hoc" "$(cat "$binC.sig")" "adhoc"
case "$out" in *"ad-hoc signed"*) check "C warns about ad-hoc" yes yes ;; *) check "C warns about ad-hoc" no yes ;; esac

# --- D: off macOS the policy is a no-op -------------------------------------
binD="$SHIM/d"; cp "$SHIM/uname" "$binD"
rm -f "$SHIM/uname"   # now `uname -s` is the real one (not Darwin on Linux CI; Darwin here)
if [ "$(/usr/bin/uname -s)" != "Darwin" ]; then
  FAKE_HAS_DEVID=1 icu_sign_macos_binary "$binD" >/dev/null 2>&1; rc=$?
  check "D no-op exit 0 off macOS" "$rc" "0"
  check "D unsigned off macOS" "$([ -f "$binD.sig" ] && echo signed || echo unsigned)" "unsigned"
else
  echo "skip - D (host is macOS; off-macOS no-op path is Linux/Windows CI only)"
fi

rm -rf "$SHIM"
echo "----"
if [ "$fails" -eq 0 ]; then echo "PASS (all policy branches)"; exit 0; else echo "FAIL ($fails)"; exit 1; fi
