#!/bin/zsh
# usage: run.sh <outdir> <mode: pid|pidnofields|ax|global> <occlusion: full|partial|none>
set -u
cd "$(dirname "$0")"
OUT=${1:-out}; MODE=${2:-pid}; OCC=${3:-partial}
rm -rf "$OUT"; mkdir -p "$OUT"
launch() {  # name cgx cgy w h log color -> pid
  open -g -n -a "$PWD/$1.app" --args "$1" $2 $3 $4 $5 "$PWD/$6" $7
  until grep -q geometry "$6" 2>/dev/null; do sleep 0.1; done
  python3 -c "import json;print([json.loads(l) for l in open('$6') if 'geometry' in l][0]['pid'])"
}
TPID=$(launch Target 300 260 460 240 "$OUT/target.log" 40,120,220)
case $OCC in
  full)    OPID=$(launch Occluder 240 200 600 360 "$OUT/occluder.log" 230,80,60);;
  partial) OPID=$(launch Occluder 440 200 460 360 "$OUT/occluder.log" 230,80,60);;
  none)    OPID=""; echo '{"hit":"geometry","pid":-1}' > "$OUT/occluder.log";;
esac
sleep 0.7
read BX BY WID <<< "$(python3 -c "
import json;g=[json.loads(l) for l in open('$OUT/target.log') if 'geometry' in l][0]
print(g['button']['x']+g['button']['w']/2, g['button']['y']+g['button']['h']/2, g['winNum'])")"
echo "run: mode=$MODE occlusion=$OCC target=$TPID occluder=$OPID window=$WID click=($BX,$BY)"
./overlay $WID $BX $BY 3 "$OUT/overlay.json" & OVPID=$!
sleep 0.75
screencapture -x -R 200,150,760,460 "$OUT/shot_overlay.png"
./inject $TPID Target $BX $BY "hello from agent" $MODE > "$OUT/inject.json"
sleep 0.3
screencapture -x -o -l $WID "$OUT/capture_target.png"
./pixel "$OUT/capture_target.png" center > "$OUT/capture_pixel.txt"
./pixel "$OUT/shot_overlay.png" orange 200 150 760 $((BX-10)) $((BY-40)) 80 90 > "$OUT/orange_near_point.txt"
./pixel "$OUT/shot_overlay.png" orange 200 150 760 440 240 120 120 > "$OUT/orange_in_occluded_strip.txt"
wait $OVPID
if [ "$MODE" = global ]; then FN=$(python3 -c "import json;print(json.load(open('$OUT/inject.json'))['before']['front']['name'])"); osascript -e "tell application \"$FN\" to activate"; fi
kill $TPID $OPID 2>/dev/null
python3 verify.py "$OUT" "$MODE" "$OCC"
