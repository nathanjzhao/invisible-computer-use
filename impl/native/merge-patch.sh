#!/usr/bin/env bash
# Three-way merge of cua-driver.patch at the source level.
#
#   impl/native/merge-patch.sh <base-rev> <ours-rev> <theirs-rev> [out]
#   impl/native/merge-patch.sh --check-merge <merge-commit> [<base>]
#   impl/native/merge-patch.sh --resume <workdir> [out]
#
# Each rev is a commit in this repo whose impl/native/cua-driver.patch is one
# side. The patches are applied to the pinned vendor commit in a private clone
# of vendor-cua (never the shared checkout), committed as base/ours/theirs, and
# merged with git. The merged patch is written to [out] (default: the tracked
# patch) only if nothing either side carries is lost: merging either side into
# the result again changes nothing, and every change present in both sides
# relative to the pin (including base-patch changes neither side touched) is
# still in the result. A side that touches files outside the patched paths (patch-paths.txt) is
# refused, because the output could not carry that change. The written patch
# is checked to apply to the pin and reproduce the merged tree.
#
# On a source-level conflict the clone is kept: resolve and commit the merge in
# <workdir>/v, then --resume <workdir> runs the same checks on that resolution
# and writes the patch. HEAD there must be that merge itself, with parents the
# ours and theirs commits the conflicting run recorded in <workdir>/sides (not
# whatever the branch names point at now), each side's tree rebuilt from the
# repo's patch, and nothing committed after it. In the conflicted files the
# resolution may change only what lies inside git's conflict markers, and each
# conflict's lines only by keeping lines of either side or writing a line in
# place of one line from each side; a line added beside a conflict is refused.
# Every other file and every change both sides carry get the checks above.
# In every mode, each file's mode in the result must be the three-way merge of
# its modes in the sides, so a chmod added or a base-patch chmod reverted is
# refused.
# A relative [out] is relative to where the script is run.
#
# --check-merge audits a merge commit that already touched the patch: it
# rebuilds the vendor tree from the commit's own patch and applies the same
# checks against its two parents and their merge base. With several merge bases
# (criss-cross history) it passes if the checks pass against any one of them;
# <base> picks one, and must be a common ancestor of both parents.
#
# A textual merge of the patch file itself is not safe: hunks can be dropped
# silently, which is how e737b7b lost two Linux fixes.
#
# Threat model: a careless or buggy resolver, not someone with write access to
# the kept clone or the workdir. Such a person can make the checks see what
# they like. --resume still refuses the cheap ways: each recorded repo rev must
# be reachable from a ref in this repo, the recorded pin must be the configured
# one, and a clone with replace refs, grafts, attribute files, merge attributes
# or a configured merge driver is refused.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PIN="${CUA_VENDOR_PIN:-00678fa8}"
# The vendor paths the patch carries: one list, impl/native/patch-paths.txt, shared with apply-and-build.sh.
PATHS=()
while IFS= read -r line; do
  case "$line" in ''|'#'*) ;; *) PATHS+=("$line") ;; esac
done < "$ROOT/impl/native/patch-paths.txt"
[[ ${#PATHS[@]} -gt 0 ]] || { echo "$ROOT/impl/native/patch-paths.txt lists no paths" >&2; exit 2; }
usage() { sed -n '2,48p' "$0"; exit 2; }
g() { git -c user.email=merge@local -c user.name=merge-patch "$@"; }

CHECK="" RESUME="" OUT=""
if [[ "${1:-}" == "--check-merge" ]]; then
  [[ $# -eq 2 || $# -eq 3 ]] || usage
  CHECK="$2"
  P1=$(git -C "$ROOT" rev-parse "$CHECK^1"); P2=$(git -C "$ROOT" rev-parse "$CHECK^2")
  if [[ $# -eq 2 ]]; then
    BASES=$(git -C "$ROOT" merge-base --all "$P1" "$P2")
    if [[ $(wc -l <<<"$BASES") -gt 1 ]]; then
      # Criss-cross history: every merge base is a real common ancestor, and the
      # checks against any one of them are complete (changes after it survive, and
      # its own content survives unless a side changed it), so one passing suffices.
      for b in $BASES; do
        echo "merge base $b:"
        if "$0" --check-merge "$CHECK" "$b"; then exit 0; fi
      done
      echo "merge $CHECK is lossy against every merge base" >&2; exit 1
    fi
    set -- "$BASES" "$P1" "$P2"
  else
    git -C "$ROOT" merge-base --is-ancestor "$3" "$P1" && git -C "$ROOT" merge-base --is-ancestor "$3" "$P2" \
      || { echo "$3 is not a common ancestor of $CHECK's parents" >&2; exit 2; }
    set -- "$(git -C "$ROOT" rev-parse "$3")" "$P1" "$P2"
  fi
elif [[ "${1:-}" == "--resume" ]]; then
  [[ $# -ge 2 && $# -le 3 ]] || usage
  RESUME="$2"; OUT="${3:-}"
else
  OUT="${4:-}"
fi
# Before any cd: a relative path would otherwise land in the temporary clone.
[[ -z "$OUT" || "$OUT" == /* ]] || OUT="$PWD/$OUT"

# The tree the pin plus one repo rev's patch gives, built in a scratch index.
patched_tree() {
  local idx="$WORK/side.index"; rm -f "$idx"
  GIT_INDEX_FILE="$idx" git read-tree "$PIN"
  git -C "$ROOT" show "$1:impl/native/cua-driver.patch" | GIT_INDEX_FILE="$idx" git apply --cached - || { rm -f "$idx"; return 1; }
  GIT_INDEX_FILE="$idx" git write-tree; rm -f "$idx"
}

if [[ -n "$RESUME" ]]; then
  WORK="$(cd "$RESUME" && pwd)"
  cd "$WORK/v"
  [[ -z "$(git ls-files -u)" ]] || { echo "unmerged paths remain in $WORK/v" >&2; exit 1; }
  [[ -z "$(git status --porcelain --untracked-files=no)" ]] || { echo "commit the resolution in $WORK/v first" >&2; exit 1; }
  # The sides as the conflicting run built them. The branch names in the clone
  # can be moved (commit onto theirs, merge again), so HEAD's parents are
  # compared with these SHAs, and each side's tree is rebuilt from the repo's
  # own patch at the recorded rev, so an edited record cannot vouch for it.
  [[ -f "$WORK/sides" ]] || { echo "$WORK/sides is missing; not a workdir left by a conflicting run" >&2; exit 1; }
  read -r RPIN SB SO ST BASE OURS THEIRS < "$WORK/sides" || true
  [[ -n "${THEIRS:-}" ]] || { echo "$WORK/sides is malformed; not a record left by a conflicting run" >&2; exit 1; }
  trap 'echo "kept $WORK" >&2' EXIT
  # The checks read objects, attributes and merge drivers through this clone's
  # own setup, so a clone that changes any of them is refused.
  export GIT_NO_REPLACE_OBJECTS=1
  gp() { git rev-parse --git-path "$1"; }
  setup=""
  [[ -z "$(git for-each-ref refs/replace)" ]] || setup+=" replace refs;"
  [[ ! -s "$(gp info/grafts)" ]] || setup+=" info/grafts;"
  [[ ! -s "$(gp info/attributes)" ]] || setup+=" info/attributes;"
  af=$(git config --path --get core.attributesFile || true)
  [[ -z "$af" ]] || ! grep -qsw merge "$af" || setup+=" merge attributes in core.attributesFile;"
  [[ -z "$(git config --get-regexp '^merge\..*\.driver$' || true)" ]] || setup+=" a configured merge driver;"
  for t in "$SB" "$SO" "$ST" HEAD; do
    while IFS= read -r f; do
      git show "$t:$f" | grep -qw merge && { setup+=" merge attributes in $t:$f;"; break; }
    done < <(git ls-tree -r --name-only "$t" 2>/dev/null | grep -E '(^|/)\.gitattributes$' || true)
    # The checks above name the files this clone holds. Git also reads the
    # global ($XDG_CONFIG_HOME/git/attributes or ~/.config/git/attributes) and
    # system attributes files, so ask git what merge attribute each path gets.
    # check-attr --source needs git 2.40; an older git fails this, which refuses.
    ca=$(mktemp)
    git ls-tree -r -z --name-only "$t" | git check-attr -z --source "$t" --stdin merge > "$ca" \
      || setup+=" attributes that cannot be read for $t (git 2.40 or later is needed);"
    while IFS= read -r -d '' p && IFS= read -r -d '' _ && IFS= read -r -d '' val; do
      [[ "$val" == unspecified ]] || { setup+=" merge=$val on $t:$p;"; break; }
    done < "$ca"
    rm -f "$ca"
  done
  [[ -z "$setup" ]] || { echo "$WORK/v changes how git reads or merges it:$setup the checks would read through that" >&2; exit 1; }
  [[ "$(git rev-parse -q --verify "$RPIN^{commit}")" == "$(git rev-parse -q --verify "$PIN^{commit}")" ]] \
    || { echo "the recorded pin $RPIN is not the configured pin $PIN" >&2; exit 1; }
  # The rebuild below proves a clone side matches its recorded rev, not that the
  # rev is one the user named, so each must be a commit this repo has a ref to.
  for r in "base $BASE" "ours $OURS" "theirs $THEIRS"; do
    set -- $r
    [[ -n "$(git -C "$ROOT" for-each-ref --contains "$2" 2>/dev/null)" ]] \
      || { echo "the recorded $1 rev $2 is reachable from no ref in $ROOT" >&2; exit 1; }
    echo "$1: $(git -C "$ROOT" log -1 --format='%h %s' "$2")"
  done
  for s in "base $SB $BASE" "ours $SO $OURS" "theirs $ST $THEIRS"; do
    set -- $s
    [[ "$(git rev-parse -q --verify "$2^{tree}")" == "$(patched_tree "$3")" ]] \
      || { echo "$1 in $WORK/v is not the tree of $3's patch" >&2; exit 1; }
  done
  [[ "$(git rev-parse -q --verify "$SO^1")" == "$SB" && "$(git rev-parse -q --verify "$ST^1")" == "$SB" ]] \
    || { echo "the recorded sides in $WORK/v do not share the recorded base" >&2; exit 1; }
  # The merge itself: an ancestry check would also pass a commit stacked on it.
  [[ "$(git rev-parse -q --verify 'HEAD^1')" == "$SO" && "$(git rev-parse -q --verify 'HEAD^2')" == "$ST" \
     && -z "$(git rev-parse -q --verify 'HEAD^3' || true)" ]] \
    || { echo "HEAD in $WORK/v is not the merge of ours and theirs (parents ours then theirs as recorded, nothing committed after it)" >&2; exit 1; }
  # Every check below reads the refs, so point them back at the recorded sides.
  g checkout -q --detach HEAD
  for r in "base $SB" "ours $SO" "theirs $ST" "merged $(git rev-parse HEAD)"; do set -- $r; git update-ref "refs/heads/$1" "$2"; done
else
  [[ $# -ge 3 ]] || usage
  BASE="$1" OURS="$2" THEIRS="$3"
  WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
  git clone -q --no-checkout "$ROOT/vendor-cua" "$WORK/v"
  cd "$WORK/v"
  side() { # branch parent rev
    g checkout -q -B "$1" "$2"
    g read-tree -u --reset "$PIN"
    git -C "$ROOT" show "$3:impl/native/cua-driver.patch" | g apply --index -
    g commit -q --allow-empty -m "$1 from $3"
  }
  g checkout -q -B root "$PIN"
  side base root "$BASE"
  side ours base "$OURS"
  side theirs base "$THEIRS"
fi

outside() { git diff --name-only "$PIN" "$1" -- . "${PATHS[@]/#/:!}"; }
for s in ours theirs; do
  o=$(outside "$s")
  [[ -z "$o" ]] || { echo "$s changes files outside the patched paths: $o" >&2; exit 1; }
done

if [[ -n "$CHECK" ]]; then
  side merged base "$CHECK"
elif [[ -z "$RESUME" ]]; then
  g checkout -q -B merged ours
  # MERGE_PATCH_STRATEGY=ours reproduces a lossy resolution; the check below must refuse it.
  if ! g merge -q --no-edit ${MERGE_PATCH_STRATEGY:+-s "$MERGE_PATCH_STRATEGY"} theirs; then
    echo "$PIN $(git rev-parse base ours theirs | tr '\n' ' ')$(git -C "$ROOT" rev-parse "$BASE^{commit}" "$OURS^{commit}" "$THEIRS^{commit}" | tr '\n' ' ')" > "$WORK/sides"
    # Quoted for the shell, so a path with a space can be pasted as it is.
    hint=$(printf '%q --resume %q' "$0" "$WORK"; [[ -z "$OUT" ]] || printf ' %q' "$OUT")
    echo "source-level conflict; resolve it in $WORK/v and commit the merge there (git add, git commit), then run: $hint" >&2
    trap - EXIT; exit 1
  fi
fi
o=$(outside merged)
[[ -z "$o" ]] || { echo "merged tree changes files outside the patched paths: $o" >&2; exit 1; }

# A merged tree keeps a side's changes exactly when merging that side into it
# again is a no-op. The merged tree is re-parented onto the base so git cannot
# shortcut the merge via ancestry, then each side is merged into it with
# merge-tree. A dropped hunk would come back, a kept deletion would go away,
# a duplicated line dropped at one call site would reappear.
MT=$(git rev-parse "merged^{tree}")
DETACHED=$(g commit-tree "$MT" -p base -m "merged tree on base")
fail=0
# A hand resolution (--resume) necessarily rewrites the conflicted regions, so
# re-merging a side conflicts there by construction. For those files the rule
# is instead that the resolution changed nothing outside the conflict markers
# git left, checked below; every other file gets the re-merge check.
CONFLICTED=""
if [[ -n "$RESUME" ]]; then
  CT_OUT=$(g merge-tree --write-tree --name-only --no-messages ours theirs || true)
  CT=$(printf '%s\n' "$CT_OUT" | head -1)
  CONFLICTED=$(printf '%s\n' "$CT_OUT" | tail -n +2 | sed '/^$/d' | sort -u)
fi
not_conflicted() { if [[ -n "$CONFLICTED" ]]; then comm -23 - <(printf '%s\n' "$CONFLICTED"); else cat; fi; }
for s in ours theirs; do
  if out=$(g merge-tree --write-tree --name-only --no-messages "$DETACHED" "$s"); then
    lost=$(git diff --name-only "$MT" "$(printf '%s\n' "$out" | head -1)" | sort -u | not_conflicted)
    what="merge dropped changes in"
  else
    # The names git reports conflicting, and every file the re-merge changes
    # cleanly: a resolution that reverts a side in an unconflicted file (or
    # re-adds one both sides deleted) conflicts nowhere but still comes back.
    lost=$({ printf '%s\n' "$out" | tail -n +2 | sed '/^$/d'; git diff --name-only "$MT" "$(printf '%s\n' "$out" | head -1)"; } | sort -u | not_conflicted)
    what="re-merging conflicts with the merged tree in"
  fi
  if [[ -z "$lost" ]]; then
    echo "$s: every change survives the merge${CONFLICTED:+ (outside the conflicted files)}"
  else
    echo "$s: $what: $(printf '%s' "$lost" | tr '\n' ' ')" >&2; fail=1
  fi
done
if [[ -n "$CONFLICTED" ]]; then
  # shellcheck disable=SC2086 # file names in the vendor tree have no spaces
  if python3 - "$CT" $CONFLICTED <<'PY'
import subprocess, sys
ct = sys.argv[1]
def lines(spec):
    r = subprocess.run(["git", "show", spec], capture_output=True)
    return None if r.returncode else r.stdout.decode("utf-8", "surrogateescape").splitlines(keepends=True)
bad = []
from collections import Counter
for f in sys.argv[2:]:
    m, r = lines(f"{ct}:{f}"), lines(f"merged:{f}")
    if m is None or r is None:
        bad.append(f"{f} (added or deleted in the conflict)"); continue
    # The non-conflict text between marker blocks, in order, and each block's
    # ours, base (diff3 styles only) and theirs lines.
    segs, blocks, cur, part = [], [], [], None
    for l in m:
        if l.startswith("<<<<<<< ") and part is None: segs.append(cur); cur, part = [], "ours"; blocks.append({"ours": [], "base": [], "theirs": []})
        elif l.startswith("||||||| ") and part == "ours": part = "base"
        elif l.rstrip("\r\n") == "=======" and part in ("ours", "base"): part = "theirs"
        elif l.startswith(">>>>>>> ") and part == "theirs": part = None
        elif part is None: cur.append(l)
        else: blocks[-1][part].append(l)
    segs.append(cur)
    if any(l.startswith(("<<<<<<< ", ">>>>>>> ")) for l in r):
        bad.append(f"{f} (conflict markers left)"); continue
    # r must be segs[0] <any> segs[1] <any> ... segs[-1]; placing each middle
    # segment at its leftmost match is optimal for this kind of pattern.
    regions = []
    if len(segs) == 1:
        ok = r == segs[0]
    else:
        ok, pos = r[:len(segs[0])] == segs[0], len(segs[0])
        for sg in segs[1:-1]:
            k = next((i for i in range(pos, len(r) - len(sg) + 1) if r[i:i + len(sg)] == sg), None)
            if k is None:
                ok = False; break
            regions.append(r[pos:k]); pos = k + len(sg)
        ok = ok and len(r) - len(segs[-1]) >= pos and r[len(r) - len(segs[-1]):] == segs[-1]
        if ok: regions.append(r[pos:len(r) - len(segs[-1])])
    if not ok:
        bad.append(f"{f} (changed outside the conflict markers)"); continue
    # What stands in a block's place may keep lines of either side (or the base),
    # and may write a new line only in place of one conflicting line from each
    # side, as when both edits go on one line. Anything more is a line added
    # beside the conflict, which the markers cannot tell from the resolution.
    for i, (reg, b) in enumerate(zip(regions, blocks), 1):
        o, t, base = Counter(b["ours"]), Counter(b["theirs"]), Counter(b["base"])
        written = []
        for l in reg:
            if o[l]: o[l] -= 1
            elif t[l]: t[l] -= 1
            elif base[l]: base[l] -= 1
            else: written.append(l)
        new = len(written)
        room = min(sum(o.values()), sum(t.values()))
        if new > room:
            bad.append(f"{f} (conflict {i}: {new} new lines where {room} could replace a line from each side)")
        else:
            # What a combined line says is the resolver's judgement, which no check here
            # can make; print it so it is read, not buried in the written patch.
            for l in written:
                print(f"resolution: {f} conflict {i} writes a line in neither side: {l.rstrip(chr(10)).rstrip(chr(13))!r}")
if bad:
    print("resolution: " + "; ".join(bad), file=sys.stderr); sys.exit(1)
PY
  then echo "resolution: only the conflicted regions of $(printf '%s\n' "$CONFLICTED" | wc -l | tr -d ' ') files changed"
  else fail=1
  fi
fi

# File modes. The re-merge above compares contents only: a mode git's merge
# would keep can be reverted (a base-patch chmod both sides carry) or a new
# one added in a hand resolution, and either is written into the patch. So
# every file's mode in the result must be the three-way merge of its modes in
# base, ours and theirs; where the sides changed it differently, either side's.
if ! python3 - "${PATHS[@]}" <<'PY'
import subprocess, sys
paths = sys.argv[1:]
def modes(rev):
    out = subprocess.run(["git", "ls-tree", "-r", "-z", rev, "--", *paths], check=True, capture_output=True).stdout
    return {e.split(b"\t", 1)[1].decode("utf-8", "surrogateescape"): e.split(b" ", 1)[0].decode()
            for e in out.split(b"\0") if e}
b, o, t, m = (modes(r) for r in ("base", "ours", "theirs", "merged"))
bad = []
for f, mm in sorted(m.items()):
    sides = [x[f] for x in (o, t) if f in x]
    if not sides:
        continue                       # in neither side: the content checks own it
    if len(sides) == 2 and sides[0] != sides[1]:
        want = {sides[0]} if sides[1] == b.get(f) else {sides[1]} if sides[0] == b.get(f) else set(sides)
    else:
        want = {sides[0]}
    if mm not in want:
        bad.append(f"{f} ({mm}, expected {' or '.join(sorted(want))})")
if bad:
    print("modes: the result changes a file mode neither side has: " + "; ".join(bad), file=sys.stderr); sys.exit(1)
print("modes: every file mode is one a side carries")
PY
then fail=1
fi

# The check above only sees what a side changed relative to the base, so a
# base-patch change both sides kept can be dropped by a hand resolution with
# every re-merge still a no-op. So: wherever the result differs from the base,
# at a line the base patch added or next to a place it deleted, some side must
# have changed the base at that same place. A reverted base-patch fix neither
# side touched fails, however close a side's own edit is.
# Changes the merge makes in untouched vendor code are not base-patch changes
# and are left alone.
if ! python3 - "$PIN" "${PATHS[@]}" <<'PY'
import re, subprocess, sys
pin, paths, SLACK = sys.argv[1], sys.argv[2:], 0
HUNK = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@", re.M)

def diff(a, b, *extra):
    return subprocess.run(["git", "diff", "--no-renames", "-U0", "--no-color", "--no-ext-diff",
                           *extra, a, b, "--", *paths], check=True, capture_output=True
                          ).stdout.decode("utf-8", "surrogateescape")

def per_file(text):
    out = {}
    for sec in re.split(r"^(?=diff --git )", text, flags=re.M):
        if sec:
            name = sec.split("\n", 1)[0].split(" b/", 1)[1]
            out[name] = [tuple(int(x) if x is not None else 1 for x in m.groups()) for m in HUNK.finditer(sec)]
    return out

def old_positions(hunks):
    # Base-side positions a hunk touches: its lines, or the gap after line a (a + 0.5).
    return [x for a, b, _, _ in hunks for x in (range(a, a + b) if b else [a + 0.5])]

def new_positions(hunks):
    return [x for _, _, c, d in hunks for x in (range(c, c + d) if d else [c + 0.5])]

patch = per_file(diff(pin, "base"))
sides = [per_file(diff("base", s)) for s in ("ours", "theirs")]
merged = per_file(diff("base", "merged"))
binary = {l.split("\t")[-1] for l in diff("base", "merged", "--numstat").splitlines() if l.startswith("-\t-\t")}
lost = []
for name, hunks in merged.items():
    if name not in patch:
        continue
    touched = [x for side in sides for x in old_positions(side.get(name, []))]
    if name in binary:
        if not touched:
            lost.append(name)
        continue
    base_patch = new_positions(patch[name])
    for m in old_positions(hunks):
        if any(abs(m - p) <= 0.5 for p in base_patch) and not any(abs(m - t) <= SLACK for t in touched):
            lost.append(f"{name}:{int(m)}")
            break
if lost:
    print("both: merge dropped base-patch changes neither side touched in: " + " ".join(lost), file=sys.stderr)
    sys.exit(1)
print("both: every base-patch change both sides kept survives")
PY
then fail=1
fi

[[ "$fail" == 0 ]] || { if [[ -n "$CHECK" ]]; then echo "merge $CHECK is lossy" >&2; else echo "merge lost changes; not writing" >&2; fi; exit 1; }
[[ -n "$CHECK" ]] && { echo "merge $CHECK keeps both sides"; exit 0; }
[[ -n "$OUT" ]] || OUT="$ROOT/impl/native/cua-driver.patch"
g diff --binary "$PIN" merged -- "${PATHS[@]}" > "$WORK/merged.patch"
# The written patch must apply to the pin and give back exactly the merged tree.
VIDX="$WORK/verify.index"
GIT_INDEX_FILE="$VIDX" git read-tree "$PIN"
GIT_INDEX_FILE="$VIDX" git apply --cached "$WORK/merged.patch" \
  || { echo "merged patch does not apply to $PIN; not writing" >&2; exit 1; }
[[ "$(GIT_INDEX_FILE="$VIDX" git write-tree)" == "$MT" ]] \
  || { echo "merged patch does not reproduce the merged tree; not writing" >&2; exit 1; }
cp "$WORK/merged.patch" "$OUT"
[[ -z "$RESUME" ]] || { trap - EXIT; rm -rf "$WORK"; }
echo "wrote $OUT ($(wc -l < "$OUT") lines, sha256 $(shasum -a 256 "$OUT" | cut -c1-16))"
