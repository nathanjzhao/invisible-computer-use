#!/bin/zsh
set -u; cd "$(dirname "$0")"
OUT=demo_out; rm -rf "$OUT"; mkdir -p "$OUT"
# Occluder in front, Target behind (fully covered).
open -g -n -a "$PWD/Occluder.app" --args Occluder 260 210 560 340 "$PWD/$OUT/occluder.log" 230,80,60
until grep -q geometry "$OUT/occluder.log" 2>/dev/null; do sleep 0.1; done
(./target Target 300 260 460 240 "$PWD/$OUT/target.log" 40,120,220 &)
until grep -q geometry "$OUT/target.log" 2>/dev/null; do sleep 0.1; done
sleep 0.6
read TPID BX BY WID <<< "$(python3 -c "
import json;g=[json.loads(l) for l in open('$OUT/target.log') if 'geometry' in l][0]
print(g['pid'], g['button']['x']+g['button']['w']/2, g['button']['y']+g['button']['h']/2, g['winNum'])")"
CUR0=$(python3 -c "import subprocess,json")
# snapshot cursor + front app before
cat > "$OUT/probe.swift" <<'PS'
import Cocoa
let f = NSWorkspace.shared.frontmostApplication
let p = CGEvent(source:nil)!.location
print("\(f?.localizedName ?? "?")|\(Int(f?.processIdentifier ?? -1))|\(p.x)|\(p.y)")
PS
swiftc -O "$OUT/probe.swift" -o "$OUT/probe" 2>/dev/null
BEFORE=$(./"$OUT"/probe)
# 1) per-window capture while fully occluded
screencapture -x -o -l $WID "$OUT/capture_occluded.png"
./pixel "$OUT/capture_occluded.png" center > "$OUT/cap_px.txt"
# 2) AX click the button + 3) AX/type into field
./axdump $TPID Target press > "$OUT/axdump.txt" 2>&1
./inject $TPID Target $BX $BY "typed while hidden" pidkeys > "$OUT/keys.json" 2>&1 || true
sleep 0.5
AFTER=$(./"$OUT"/probe)
kill $TPID 2>/dev/null; pkill -f "$OUT/occluder.log" 2>/dev/null
python3 - "$OUT" "$BEFORE" "$AFTER" <<'PY'
import json,sys
out,before,after=sys.argv[1],sys.argv[2],sys.argv[3]
T=[json.loads(l) for l in open(f"{out}/target.log") if l.strip()]
O=[json.loads(l) for l in open(f"{out}/occluder.log") if l.strip()]
bn,bp,bx,by=before.split("|"); an,ap,ax,ay=after.split("|")
r,g,b=map(int,open(f"{out}/cap_px.txt").read().split()[0].split(","))
res={}
res["ax_click_occluded"]=any(l.get("hit")=="buttonPressed" for l in T)
res["type_occluded"]=any(l.get("hit")=="textChanged" and "typed while hidden" in l.get("value","") for l in T) or any(l.get("hit")=="textChanged" for l in T)
res["no_activation"]=not any(l.get("active") or l.get("hit") in ("appDidBecomeActive","windowDidBecomeKey") for l in T+O)
res["cursor_unmoved"]=abs(float(bx)-float(ax))<1 and abs(float(by)-float(ay))<1
res["focus_kept"]=(bn==an and bp==ap and ap!=str(T and 0))
res["capture_faithful_occluded"]=(b>150 and r<100)
res["occluder_untouched"]=not any(str(l.get("ev","")).startswith("leftMouse") for l in O)
json.dump({"results":res,"front_before":f"{bn}({bp})","front_after":f"{an}({ap})","cursor_before":f"{float(bx):.0f},{float(by):.0f}","cursor_after":f"{float(ax):.0f},{float(ay):.0f}","capture_rgb":f"{r},{g},{b}"}, open(f"{out}/results.json","w"), indent=2)
print(json.dumps(json.load(open(f"{out}/results.json")),indent=2))
PY
