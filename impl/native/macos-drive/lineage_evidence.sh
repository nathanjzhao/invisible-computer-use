#!/usr/bin/env bash
# Poll for Space 1; the moment it appears, run the native diff evidence + conformance and save output.
cd "$(dirname "$0")/../../.."
N=${NATIVE_CUA_BIN:-$PWD/vendor-cua/libs/cua-driver/rust/target/release/cua-driver}; S=${NATIVE_CUA_SOCKET:-/tmp/icu-native.sock}
SP=${OCCL_OUT:-/tmp/icu-occlusion}; mkdir -p $SP; HERE="$(cd "$(dirname "$0")" && pwd)"
OUT=$SP/native-evidence.txt
gws() { $N --socket $S call get_window_state "{\"pid\":$1,\"window_id\":$2,\"include_screenshot\":false,\"include_text\":true$3}"; }
summ() { python3 -c '
import json,sys; d=json.load(sys.stdin); x=d.get("diff")
print("  elements=",len(d["elements"]),"degraded=",d.get("degraded"),"| diff:", x and (x["kind"], x["accessibility_revision"], x["accessibility_snapshot_count"]))
print("  text:", x and x["text"][:220].replace("\n","|"))
print("  stable:", [(e["element_index"], e.get("stable_id")) for e in d["elements"][:6]])'; }
for i in 1; do
  sp=1
  if [ "$sp" = "1" ]; then
    { echo "=== $(date) Space 1 detected"
      CP=${CONF_PACK_PID:?}; CW=${CONF_PACK_WID:?}
      python3 $HERE/ensure_visible.py ${CONF_PID:?} ${CONF_WID:?} | tail -1; python3 $HERE/ensure_visible.py $CP $CW | tail -1
      echo "--- turn 1"; gws $CP $CW "" | summ
      echo "--- turn 2"; gws $CP $CW "" | summ
      echo "--- click the Alarms tab (AXRadioButton) via my daemon to force a real change"
      TOK=$(gws $CP $CW "" | python3 -c 'import json,sys; d=json.load(sys.stdin); t=[e for e in d["elements"] if e["role"]=="AXRadioButton" and not e.get("selected")]; print(t[0]["element_token"] if t else "")')
      echo "  clicking element_token=$TOK"
      [ -n "$TOK" ] && $N --socket $S call click "{\"pid\":$CP,\"window_id\":$CW,\"element_token\":\"$TOK\"}" | python3 -c 'import json,sys; d=json.load(sys.stdin); print("  click:", d.get("effect"), d.get("route"), d.get("status"))'; echo
      sleep 1
      echo "--- turn 3 (after click)"; gws $CP $CW "" | summ
      echo "--- turn 4 disableDiff"; gws $CP $CW ',"disableDiff":true' | summ
      echo "--- turn 5 cumulative_diff (vs initial)"; gws $CP $CW ',"cumulative_diff":true' | summ
      echo "--- conformance native (live)"
      CONFORMANCE_LIVE=1 CONF_PID=$CONF_PID CONF_WID=$CONF_WID CONF_PACK_PID=$CP CONF_PACK_WID=$CW CONF_PACK_BUNDLE_ID=com.apple.clock NATIVE_CUA_SOCKET=$S CONFORMANCE_TARGET=native python3 -m pytest conformance/ -q -rx 2>&1 | grep -E 'passed|failed|FAILED|XFAIL.*native' | sed 's/ - .*//'
      echo "=== done $(date)"
    } > $OUT 2>&1
    echo "EVIDENCE_WRITTEN"; exit 0
  fi
  sleep 5
done
echo "NO_SPACE_1_IN_WINDOW"
