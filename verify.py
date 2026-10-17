import json, sys
out, mode, occ = sys.argv[1], sys.argv[2], sys.argv[3]
def lines(p): return [json.loads(l) for l in open(p) if l.strip()]
T, O = lines(f"{out}/target.log"), lines(f"{out}/occluder.log")
inj = json.load(open(f"{out}/inject.json")); ov = json.load(open(f"{out}/overlay.json"))
r,g,b = map(int, open(f"{out}/capture_pixel.txt").read().split()[0].split(","))
near = int(open(f"{out}/orange_near_point.txt").read()); strip = int(open(f"{out}/orange_in_occluded_strip.txt").read())
tpid, opid = inj["pid"], [l for l in O if l.get("hit")=="geometry"][0]["pid"]
checks = []
def chk(name, ok, detail): checks.append((name, bool(ok), detail))
ev = lambda L: [l.get("ev") or l.get("hit") for l in L if l.get("hit")!="geometry"]
bf, af = inj["before"]["front"], inj["after"]["front"]
chk("focus_invariant", bf["pid"]==af["pid"] and af["pid"] not in (tpid,opid), f"front before={bf['name']}({bf['pid']}) after={af['name']}({af['pid']})")
bc, ac = inj["before"]["cursor"], inj["after"]["cursor"]
chk("real_cursor_unmoved", abs(bc["x"]-ac["x"])<0.5 and abs(bc["y"]-ac["y"])<0.5, f"before=({bc['x']:.0f},{bc['y']:.0f}) after=({ac['x']:.0f},{ac['y']:.0f})")
chk("apps_never_activated", not any(l.get("hit") in ("appDidBecomeActive","windowDidBecomeKey") or l.get("active") for l in T+O), f"target events={ev(T)[:8]} occluder events={ev(O)[:6]}")
top = inj["topmostAtPoint"]
downs = [l for l in T if l.get("ev")=="leftMouseDown"]
chk("target_got_click", any(l.get("hit")=="buttonPressed" for l in T), f"buttonPressed={'buttonPressed' in ev(T)}; topmost window at point={top.get('name')}; " + (f"target saw down with window={downs[0].get('hasWindow')} f91={downs[0].get('f91')} f92={downs[0].get('f92')} (target win={inj['targetWindow']['id']})" if downs else f"no mouseDown reached target; ax={inj.get('ax')}"))
if downs and mode != "ax": chk("click_landed_at_point", abs(downs[0]["sx"]-inj["point"]["x"])<1.5 and abs(downs[0]["sy"]-inj["point"]["y"])<1.5, f"down at ({downs[0]['sx']:.0f},{downs[0]['sy']:.0f}) vs sent ({inj['point']['x']:.0f},{inj['point']['y']:.0f})")
if occ != "none": chk("occluder_untouched", not any(str(l.get("ev","")).startswith("leftMouse") for l in O), f"occluder events={ev(O)}")
vals = [l["value"] for l in T if l.get("hit")=="textChanged"]
chk("typing_landed", bool(vals) and vals[-1]=="hello from agent", f"field={vals[-1] if vals else None!r} keyDowns={sum(1 for l in T if l.get('ev')=='keyDown')} hasKeyWindow={set(l.get('hasKeyWindow') for l in T if l.get('ev')=='keyDown')}")
chk("occluded_capture_faithful", b>150 and r<100, f"per-window capture center rgb=({r},{g},{b}); target blue=(40,120,220) occluder red=(230,80,60)")
chk("overlay_never_focused", not ov["overlayEverKey"] and not ov["overlayEverActive"] and ov["ignoresMouse"], f"everKey={ov['overlayEverKey']} everActive={ov['overlayEverActive']}")
if occ == "full":
    chk("overlay_hidden_when_covered", ov["framesPointVisible"]==0 and ov["framesPointHidden"]>0 and near==0, f"visibleFrames={ov['framesPointVisible']} hiddenFrames={ov['framesPointHidden']} orangePixelsNearPoint={near}")
else:
    chk("overlay_drawn_when_visible", ov["framesPointVisible"]>0 and near>0, f"visibleFrames={ov['framesPointVisible']} orangePixelsNearPoint={near}")
if occ == "partial":
    chk("overlay_clipped_at_occluder", strip==0, f"orangePixels in covered strip x440-560={strip} (label would extend to x~480 unclipped)")
print(f"\n== verify mode={mode} occlusion={occ} targetOnActiveSpace={inj.get('targetOnScreen')} ==")
for n,ok,d in checks: print(f"{'PASS' if ok else 'FAIL':4} {n:28} {d}")
print(f"{sum(ok for _,ok,_ in checks)}/{len(checks)} passed")
