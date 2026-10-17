#!/usr/bin/env bash
# Guard for LICENSING.md: the doc's factual anchors must stay true against the
# repo. Fails if cua-driver stops being MIT, if a tracked instruction pack stops
# being byte-identical to OpenAI's corpus (so "verbatim, redraw before
# distribution" would be wrong), or if the Codex art / OpenAI binaries stop being
# gitignored. Run: bash tests/licensing-claims.test.sh
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
fails=0
ok()   { echo "ok   - $1"; }
bad()  { echo "FAIL - $1"; fails=$((fails+1)); }

# 1. cua-driver is MIT (bucket 2). Skip only if the vendored tree is absent.
if [ -f vendor-cua/LICENSE.md ]; then
  if grep -qi 'MIT License' vendor-cua/LICENSE.md; then ok "cua-driver LICENSE.md is MIT"; else bad "cua-driver LICENSE.md is not MIT"; fi
else
  echo "skip - cua-driver LICENSE (vendor-cua not checked out)"
fi

# 2. Each tracked pack is verbatim OpenAI (sha256 present in the corpus list).
SUMS="reference/openai-sky/app-bundle-packs/SHA256SUMS.txt"
if [ -f "$SUMS" ]; then
  n=0; matched=0
  for f in impl/app-instructions/packs/*.md; do
    n=$((n+1))
    h="$(shasum -a 256 "$f" | awk '{print $1}')"
    if grep -q "$h" "$SUMS"; then matched=$((matched+1)); else bad "pack not verbatim in corpus: $f ($h)"; fi
  done
  [ "$n" -eq 7 ] && ok "seven instruction packs present" || bad "expected 7 packs, found $n"
  [ "$matched" -eq "$n" ] && ok "all $n packs are byte-identical to OpenAI corpus" || bad "only $matched/$n packs verbatim"
else
  bad "corpus SHA256SUMS.txt missing"
fi

# 3. Confirmations policy text is tracked (its licensing bucket is bucket 1).
for p in confirmations-policy.md confirmations-policy-macos.md windows-hard-deny.md; do
  git ls-files --error-unmatch "impl/confirmations/policy/$p" >/dev/null 2>&1 \
    && ok "policy text tracked: $p" || bad "policy text missing: $p"
done

# 4. Codex art and OpenAI binaries are gitignored (bucket 1, kept out of the tree).
check_ignored() { if git check-ignore -q "$1"; then ok "gitignored: $1"; else bad "NOT gitignored: $1"; fi; }
check_ignored "study/cursor-graphics/probe.png"
check_ignored "reference/openai-sky/bin/SkyComputerUseService"
check_ignored "reference/openai-windows/codex-computer-use.exe"
check_ignored "vendor-cua/anything"

echo "----"
if [ "$fails" -eq 0 ]; then echo "PASS (licensing claims hold)"; exit 0; else echo "FAIL ($fails)"; exit 1; fi
