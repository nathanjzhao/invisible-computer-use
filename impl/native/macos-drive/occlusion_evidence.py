"""Live occlusion check for the patched overlay (slice 6, macOS).
Usage: TARGET_PID=.. TARGET_WID=.. OCCLUDER_PID=.. OCCLUDER_WID=.. python3 occlusion_evidence.py
(the patched daemon must be serving on /tmp/icu-native.sock; both windows on the current Space; see impl/native/README.md)
"""

import json, subprocess, sys, time, os, struct, zlib
N = os.environ.get("NATIVE_CUA_BIN", os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../vendor-cua/libs/cua-driver/rust/target/release/cua-driver"))
OLD = os.environ.get("CUA_DRIVER", os.path.expanduser("~/.local/bin/cua-driver"))
S = "/tmp/icu-native.sock"
SP = os.environ.get("OCCL_OUT", "/tmp/icu-occlusion")
os.makedirs(SP, exist_ok=True)
CLOCK = (int(os.environ["TARGET_PID"]), int(os.environ["TARGET_WID"])); AM = (int(os.environ["OCCLUDER_PID"]), int(os.environ["OCCLUDER_WID"]))  # target: Clock; occluder: Calculator (static content; Activity Monitor repaints every second)
def old(tool, args):
    r = subprocess.run([OLD, "call", tool, json.dumps(args)], capture_output=True, text=True)
    try: return json.loads(r.stdout)
    except Exception: return {"_raw": (r.stdout + r.stderr)[:300]}
def new(tool, args): return json.loads(subprocess.run([N, "--socket", S, "call", tool, json.dumps(args)], capture_output=True, text=True).stdout or "{}")
def png_rgb(path):
    d = open(path, "rb").read(); assert d[:8] == b"\x89PNG\r\n\x1a\n"
    pos = 8; w = h = None; idat = b""; ct = None
    while pos < len(d):
        ln = struct.unpack(">I", d[pos:pos+4])[0]; typ = d[pos+4:pos+8]; body = d[pos+8:pos+8+ln]; pos += 12 + ln
        if typ == b"IHDR": w, h, bd, ct = struct.unpack(">IIBB", body[:10]); assert bd == 8, bd
        elif typ == b"IDAT": idat += body
    raw = zlib.decompress(idat); bpp = {2: 3, 6: 4}[ct]; stride = w * bpp; out = bytearray(); prev = bytearray(stride); p = 0
    for _ in range(h):
        f = raw[p]; line = bytearray(raw[p+1:p+1+stride]); p += 1 + stride
        for i in range(stride):
            a = line[i-bpp] if i >= bpp else 0; b = prev[i]; c = prev[i-bpp] if i >= bpp else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c); pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pr) & 255
        out += line; prev = line
    return w, h, bpp, bytes(out)
def box_diff(a, b, x, y, size, scale):
    (w, h, bpp, pa), (_, _, _, pb) = a, b
    n = 0; total = 0
    for yy in range(int(y*scale), int((y+size)*scale)):
        for xx in range(int(x*scale), int((x+size)*scale)):
            i = (yy*w + xx) * bpp; total += 1
            if any(abs(pa[i+k] - pb[i+k]) > 24 for k in range(3)): n += 1
    return n, total
class SpaceChanged(Exception): pass
def target_visible():
    lwn = old("list_windows", {}); tw = next((w for w in lwn["windows"] if w["window_id"] == CLOCK[1]), None)
    return bool(tw and tw["on_current_space"] and tw["is_on_screen"])
def shot(name):
    # a capture is meaningful only if the target is on the current Space BEFORE and AFTER it
    if not target_visible(): raise SpaceChanged(f"before {name}")
    p = f"{SP}/{name}.png"; r = old("get_desktop_state", {"screenshot_out_file": p}); assert os.path.exists(p), r
    if not target_visible(): raise SpaceChanged(f"after {name}")
    return p
print("arranging: Clock at (100,150,600,500) as target, Calculator at (400,150) as a static occluder brought to front")
print("clock frame:", old("set_window_frame", {"pid": CLOCK[0], "window_id": CLOCK[1], "x": 100, "y": 150, "width": 600, "height": 500}))
print("occluder frame:", old("set_window_frame", {"pid": AM[0], "window_id": AM[1], "x": 400, "y": 150, "width": 300, "height": 520}))
old("bring_to_front", {"pid": AM[0], "window_id": AM[1]}); time.sleep(1.0)
lw = old("list_windows", {}); z = {w["window_id"]: (w["z_index"], w["is_on_screen"], w["bounds"]) for w in lw["windows"]}
print("z-order: clock", z.get(CLOCK[1]), "am", z.get(AM[1]))
assert z[AM[1]][0] > z[CLOCK[1]][0], "occluder must be stacked above Clock"
ob = z[AM[1]][2]; assert ob["x"] <= 550 <= ob["x"] + ob["width"] and ob["y"] <= 600 <= ob["y"] + ob["height"], f"covered point must lie inside the occluder: {ob}"
cb = z[CLOCK[1]][2]; assert cb["x"] <= 250 <= cb["x"] + cb["width"] and cb["y"] <= 600 <= cb["y"] + cb["height"], f"exposed point must lie inside the target: {cb}"
scale = old("get_screen_size", {}).get("scale_factor") or 2.0
print("scale", scale)
# Exposed / covered points from the LIVE z-order: a cell of Clock is exposed when no on-screen layer-0
# window stacked above Clock covers it (overlay windows excluded); covered when the occluder (and only
# windows we expect) covers it. Assuming a layout was the earlier mistake: Firefox sat above Clock.
def rects_above(target_id):
    tz = z[target_id][0]
    return [(w["window_id"], w["bounds"]) for w in lw["windows"]
            if w["is_on_screen"] and w["layer"] == 0 and w["z_index"] > tz and "cua" not in w["app_name"].lower()]
def inside(b, x, y): return b["x"] <= x <= b["x"] + b["width"] and b["y"] <= y <= b["y"] + b["height"]
cb = z[CLOCK[1]][2]; above = rects_above(CLOCK[1])
print("windows above Clock:", [(wid_, round(b["x"]), round(b["y"]), round(b["width"]), round(b["height"])) for wid_, b in above])
P1 = P2 = None
for gy in range(int(cb["y"]) + 40, int(cb["y"] + cb["height"]) - 40, 40):
    for gx in range(int(cb["x"]) + 40, int(cb["x"] + cb["width"]) - 40, 40):
        covering = [wid_ for wid_, b in above if any(inside(b, gx + dx, gy + dy) for dx in (-24, 24) for dy in (-24, 24))]
        if not covering and P1 is None: P1 = (gx, gy)
        if covering == [AM[1]] and P2 is None and all(inside(z[AM[1]][2], gx + dx, gy + dy) for dx in (-24, 24) for dy in (-24, 24)): P2 = (gx, gy)
print("exposed point", P1, "covered point", P2)
assert P1 and P2, "could not find both an exposed and an occluder-covered cell of Clock"
def measure():
    # park the cursor far away so its old glyph cannot pollute either box, then baseline without it
    new("set_agent_cursor_enabled", {"session": "occl", "enabled": True})
    new("move_cursor", {"session": "occl", "x": 30, "y": 1050}); time.sleep(1.5)
    new("set_agent_cursor_enabled", {"session": "occl", "enabled": False}); time.sleep(0.8)
    b0 = png_rgb(shot("occl_baseline"))
    new("set_agent_cursor_enabled", {"session": "occl", "enabled": True})
    # Only ACTION tools pin the overlay (OverlayCommand::PinAbove). A background x,y click is delivered
    # through the AX hit-test backend and deliberately leaves the cursor still, and the background pointer
    # route is refused for this window; a FOREGROUND click takes the pixel path: pin + glide, briefly front
    # Clock, click an empty area, then restore the prior frontmost app (Calculator) — which puts the
    # occluder back over the cursor, so the mask must refresh while the cursor sits idle.
    def click_at(px, py):
        lx, ly = (px - cb["x"]) * scale, (py - cb["y"]) * scale   # window-local screenshot pixels
        return new("click", {"session": "occl", "pid": CLOCK[0], "window_id": CLOCK[1], "x": lx, "y": ly, "delivery_mode": "foreground"})
    def restack(tag):
        # cua's foreground click restores the user's previous frontmost app over the target; re-establish
        # the stacking the oracle assumes (Clock, then Calculator above it) and verify it from the live z-order
        old("bring_to_front", {"pid": CLOCK[0], "window_id": CLOCK[1]}); time.sleep(0.8)
        old("bring_to_front", {"pid": AM[0], "window_id": AM[1]}); time.sleep(1.2)
        lwx = old("list_windows", {}); zx = {w["window_id"]: (w["z_index"], w["is_on_screen"], w["bounds"]) for w in lwx["windows"]}
        abv = [(w["window_id"], w["bounds"]) for w in lwx["windows"] if w["is_on_screen"] and w["layer"] == 0 and w["z_index"] > zx[CLOCK[1]][0] and "cua" not in w["app_name"].lower()]
        cov1 = [wid_ for wid_, b in abv if any(inside(b, P1[0]+dx, P1[1]+dy) for dx in (-24, 24) for dy in (-24, 24))]
        cov2 = [wid_ for wid_, b in abv if any(inside(b, P2[0]+dx, P2[1]+dy) for dx in (-24, 24) for dy in (-24, 24))]
        print(f"  restack {tag}: above Clock now {[w for w, _ in abv]}; P1 covered by {cov1}; P2 covered by {cov2}")
        if cov1 or cov2 != [AM[1]]: raise SpaceChanged(f"stacking changed at {tag}: P1 covered by {cov1}, P2 by {cov2}")
    r = click_at(*P1); print("click exposed", P1, r.get("effect"), r.get("route"), r.get("status"), r.get("refusal") or r.get("code")); time.sleep(1.5)
    restack("S1")
    st = new("get_agent_cursor_state", {"session": "occl"}); print("cursor now", st.get("position"))
    s1 = png_rgb(shot("occl_exposed"))
    r = click_at(*P2); print("click covered", P2, r.get("effect"), r.get("route"), r.get("status"), r.get("refusal") or r.get("code")); time.sleep(1.5)
    restack("S2")
    lw2 = old("list_windows", {}); z2 = {w["window_id"]: w["z_index"] for w in lw2["windows"]}
    print("z after restore: clock", z2.get(CLOCK[1]), "occluder", z2.get(AM[1]))
    st = new("get_agent_cursor_state", {"session": "occl"}); print("cursor now", st.get("position"))
    s2 = png_rgb(shot("occl_covered"))
    new("set_agent_cursor_enabled", {"session": "occl", "enabled": False})
    return s1, s2
for attempt in range(5):
    try:
        s1, s2 = measure(); break
    except SpaceChanged as e:
        print(f"attempt {attempt}: the target left the current Space ({e}); waiting for it to come back")
        for _ in range(60):
            if target_visible(): break
            time.sleep(2)
else:
    print("OCCLUSION_RESULT ABORT (target never stayed on the current Space through a full attempt)"); sys.exit(2)
# Oracle: compare the two POST-click screenshots (same activation/stacking state; only the cursor moved).
# S1 has the cursor at P1, S2 at P2. Exposed box (P1): S1 vs S2 must differ (glyph present then gone).
# Covered box (P2): S1 vs S2 must be identical (Calculator's pixels both times) iff the clip hides the cursor.
d_exposed = box_diff(s1, s2, P1[0]-24, P1[1]-24, 48, scale)
d_covered = box_diff(s1, s2, P2[0]-24, P2[1]-24, 48, scale)
print(f"exposed box P1: {d_exposed[0]}/{d_exposed[1]} pixels differ between S1 (cursor there) and S2 (cursor gone)")
print(f"covered box P2: {d_covered[0]}/{d_covered[1]} pixels differ between S1 (no cursor there) and S2 (cursor clipped under the occluder)")
ok = d_exposed[0] > 50 and d_covered[0] < 10
print("OCCLUSION_RESULT", "PASS" if ok else "FAIL")
