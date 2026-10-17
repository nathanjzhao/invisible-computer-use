"""Drive the patched cua-driver on Linux (Xvfb + AT-SPI + GTK fixture) and verify the cross-turn
lineage: full_first -> no_change -> `~` after set_value -> disableDiff full -> cumulative; stable ids
persist across turns; the fixture's own event log proves the value change really happened.
Then the host gate: without any locker the lock state is an explicit null and never refuses; the X
saver blanking is display_asleep (reported, not refused); with xfce4-screensaver (a real locker with a
D-Bus presence) locked, health_report's host_interactive fails, input tools are refused with code
host_not_interactive, observation still answers; after deactivate input is delivered again. Then
i3lock, a locker with no D-Bus presence, is recognised by its X11 signature (fullscreen
override-redirect window on top that accepts input + keyboard grab) and refused the same way, after a
negative case shows a bare keyboard grab (what a menu holds) is not a lock."""
import json, os, subprocess, sys, time
DRV = os.environ.get("CUA_DRIVER", "/drive/cua-driver")
LOG = "/tmp/fx.log"
def sh(cmd): subprocess.run(cmd, shell=True, check=False)
def call(tool, args):
    r = subprocess.run([DRV, "call", tool, json.dumps(args)], capture_output=True, text=True)
    try: return json.loads(r.stdout)
    except Exception: return {"_raw": (r.stdout + r.stderr)[:400]}
def fail(msg):
    print("LINUX_DRIVE FAIL:", msg); sys.exit(1)
# --- environment (same recipe as linux-eval/harness/run_measure.sh)
sh("Xvfb :99 -screen 0 1440x900x24 >/tmp/xvfb.log 2>&1 &"); time.sleep(2)
env = subprocess.run("dbus-launch --sh-syntax", shell=True, capture_output=True, text=True).stdout
for line in env.splitlines():
    if line.startswith("DBUS_SESSION_BUS_ADDRESS="): os.environ["DBUS_SESSION_BUS_ADDRESS"] = line.split("=", 1)[1].rstrip(";").strip("'")
sh("openbox >/tmp/ob.log 2>&1 &"); time.sleep(1)
sh("picom --backend xrender >/tmp/picom.log 2>&1 &"); time.sleep(1)
sh("/usr/libexec/at-spi-bus-launcher --launch-immediately >/tmp/atspi.log 2>&1 &"); time.sleep(1)
if os.path.exists(LOG): os.remove(LOG)
sh(f"python3 /drive/fixture.py {LOG} 100 100 Fixture >/tmp/fx.out 2>&1 &")
for _ in range(50):
    if os.path.exists(LOG) and "ready" in open(LOG).read(): break
    time.sleep(0.2)
else: fail("fixture never became ready: " + open("/tmp/fx.out").read()[-400:])
fx_pid = [json.loads(l) for l in open(LOG) if "ready" in l][0]["pid"]
print("fixture pid", fx_pid)
sh(f"{DRV} serve --no-permissions-gate >/tmp/drv.log 2>&1 &"); time.sleep(4)
print("describe advertises:", [k for k in ("disable_diff", "disableDiff", "cumulative_diff") if k in subprocess.run([DRV, "describe", "get_window_state"], capture_output=True, text=True).stdout])
lw = call("list_windows", {})
wins = [w for w in lw.get("windows", []) if w.get("pid") == fx_pid or "Fixture" in (w.get("title") or "")]
if not wins: fail(f"fixture window not listed: {lw}")
w = wins[0]; pid, wid = w["pid"], w["window_id"]; print("window", pid, wid, w.get("title"))
def gws(extra=None):
    a = {"pid": pid, "window_id": wid, "include_screenshot": False, "include_text": True}
    a.update(extra or {}); return call("get_window_state", a)
def summ(tag, s):
    d = s.get("diff"); print(f"{tag}: elements={len(s.get('elements', []))} diff={d and (d['kind'], d['baseline'], d['accessibility_revision'], d['accessibility_snapshot_count'])}")
    if d: print("   text:", d["text"][:200].replace("\n", "|"))
    return d
s1 = gws(); d1 = summ("turn1", s1)
if not d1: fail("no diff field: " + json.dumps(s1)[:500])
if d1["kind"] != "full_first" or d1["accessibility_snapshot_count"] != 1: fail("turn1 not full_first")
ids1 = {e["element_index"]: e["stable_id"] for e in s1["elements"]}
if not ids1 or len(set(ids1.values())) != len(ids1): fail("stable ids missing or not unique")
s2 = gws(); d2 = summ("turn2", s2)
if d2["kind"] != "no_change" or not d2["text"].startswith("There has been no change in the accessibility tree for "): fail("turn2 not no_change")
ids2 = {e["element_index"]: e["stable_id"] for e in s2["elements"]}
if set(ids2.values()) != set(ids1.values()): fail("stable id set changed on an unchanged tree")
entry = next((e for e in s2["elements"] if "text" in e["role"].lower() or "entry" in e["role"].lower()), None)
if not entry: fail("no text entry in elements: " + str([e["role"] for e in s2["elements"]]))
r = call("set_value", {"pid": pid, "window_id": wid, "element_token": entry["element_token"], "value": "lineage ok"}); print("set_value:", {k: r.get(k) for k in ("effect", "route", "status", "code")}); time.sleep(0.8)
if "textChanged" not in open(LOG).read(): fail("fixture did not record the value change")
s3 = gws(); d3 = summ("turn3 (after set_value)", s3)
if d3["kind"] != "diff" or not d3["text"].startswith("The following is a diff from the previous accessibility tree") or "~ " not in d3["text"]: fail("turn3 is not a `~` diff")
ids3 = {e["element_index"]: e["stable_id"] for e in s3["elements"]}
if ids3.get(entry["element_index"]) != ids2[entry["element_index"]]: fail("the changed entry did not keep its stable id")
if d3["accessibility_revision"] != d2["accessibility_revision"] + 1: fail("revision did not advance on change")
s4 = gws({"disableDiff": True}); d4 = summ("turn4 disableDiff", s4)
if d4["kind"] != "full" or not d4["text"].startswith("[1] ") or "diff from" in d4["text"]: fail("disableDiff did not render the full tree")
s5 = gws({"cumulative_diff": True}); d5 = summ("turn5 cumulative", s5)
if d5["baseline"] != "initial" or d5["accessibility_snapshot_count"] != 5: fail("cumulative baseline/counters wrong")
if d5["kind"] == "diff" and not d5["text"].startswith("The following is a cumulative diff from the initial accessibility tree"): fail("cumulative header missing")

# --- host gate: a real locker (xfce4-screensaver, D-Bus org.xfce.ScreenSaver) and the X blank / DPMS facts
def host_check(tag):
    hr = call("health_report", {"include": ["host_interactive"]})
    chk = next((c for c in hr.get("checks", []) if c.get("name") == "host_interactive"), None)
    print(f"{tag}: host_interactive={chk and (chk['status'], chk.get('data', {}).get('host'), chk.get('data', {}).get('error_detail'))}")
    if not chk: fail("health_report has no host_interactive check: " + json.dumps(hr)[:400])
    return chk
h0 = host_check("host before any locker")
host0 = h0["data"]["host"]
if host0.get("screen_locked") is not False: fail("on X11 the locker signature is observable: no locker must be false, and on_console (logind) an explicit null")
if "on_console" not in host0 or host0["on_console"] is not None: fail("without logind, on_console must be an explicit null")
if h0["status"] != "pass": fail("unlocked host must pass")
if "display_asleep" not in host0 or host0["display_asleep"] is not False: fail("X11 with the saver off must report display_asleep=false")
xset_q = subprocess.run("xset q", shell=True, capture_output=True, text=True).stdout
print("DPMS on this X server:", "absent" if "does not have the DPMS Extension" in xset_q else "present")
sh("xset s activate"); time.sleep(1)
h_blank = host_check("host with the X saver blanking")
if h_blank["data"]["host"].get("display_asleep") is not True: fail("X saver blank must report display_asleep=true")
if h_blank["status"] == "fail": fail("a dark display alone must not fail the check")
c_blank = call("click", {"pid": pid, "window_id": wid, "x": 30, "y": 30})
if "effect" not in c_blank: fail("a blank (not locked) screen must not refuse input: " + json.dumps(c_blank)[:300])
sh("xset s reset"); time.sleep(1)
sh("xfce4-screensaver >/tmp/xss.log 2>&1 &"); time.sleep(4)
print("locker:", subprocess.run("xfce4-screensaver-command --query", shell=True, capture_output=True, text=True).stdout.strip()[:120], "| dbus names:", [n for n in subprocess.run("dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.ListNames", shell=True, capture_output=True, text=True).stdout.split('"') if "ScreenSaver" in n])
h_locker = host_check("host with the locker running, unlocked")
if h_locker["data"]["host"].get("screen_locked") is not False or h_locker["status"] != "pass": fail("locker present and inactive must report screen_locked=false: " + open("/tmp/xss.log").read()[-300:])
sh("xfce4-screensaver-command --lock"); time.sleep(3)
h1 = host_check("host locked")
if h1["status"] != "fail" or h1["data"]["host"].get("screen_locked") is not True or not h1.get("hint"): fail("locked host must fail with screen_locked=true and a hint")
c = call("click", {"pid": pid, "window_id": wid, "x": 30, "y": 30})
print("click while locked:", json.dumps(c)[:300])
if c.get("code") != "host_not_interactive" or c.get("reasons") != ["screen_locked"]: fail("click was not refused with host_not_interactive/screen_locked")
t = call("type_text", {"pid": pid, "window_id": wid, "text": "must not reach the password field"})
if t.get("code") != "host_not_interactive": fail("type_text was not refused while locked: " + json.dumps(t)[:300])
s6 = gws(); d6 = summ("observe while locked", s6)
if not d6: fail("observation must still answer while locked")
sh("xfce4-screensaver-command --deactivate"); time.sleep(2)
h2 = host_check("host after deactivate")
if h2["data"]["host"].get("screen_locked") is not False or h2["status"] != "pass": fail("host did not return to unlocked after deactivate")
c2 = call("click", {"pid": pid, "window_id": wid, "x": 30, "y": 30})
print("click after unlock:", {k: c2.get(k) for k in ("effect", "route", "code")})
if c2.get("code") == "host_not_interactive" or "effect" not in c2: fail("click after unlock was not delivered: " + json.dumps(c2)[:300])
# --- a locker with no D-Bus presence: i3lock (fullscreen override-redirect window + keyboard grab)
sh("pkill -f xfce4-screensaver; sleep 1")
h3 = host_check("host with no locker again")
if h3["data"]["host"].get("screen_locked") is not False: fail("with X11 reachable and no locker, the signature must say unlocked (false), not null")
# negative case first: a keyboard grab with no fullscreen input window (what a menu popup does) must not read as a lock
GRAB = r"""
import ctypes, time
x = ctypes.CDLL("libX11.so.6"); x.XOpenDisplay.restype = ctypes.c_void_p; x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]; x.XDefaultRootWindow.restype = ctypes.c_ulong
x.XGrabKeyboard.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]; x.XFlush.argtypes = [ctypes.c_void_p]
d = x.XOpenDisplay(None); r = x.XGrabKeyboard(d, x.XDefaultRootWindow(d), 0, 1, 1, 0); x.XFlush(d); print("grab status", r, flush=True); time.sleep(30)
"""
sh(f"python3 -c '{GRAB}' >/tmp/grab.log 2>&1 &"); time.sleep(2)
print("grab helper:", open("/tmp/grab.log").read().strip())
h_grab = host_check("host with a bare keyboard grab (menu-like)")
if h_grab["data"]["host"].get("screen_locked") is not False: fail("a bare keyboard grab must not read as locked")
c_grab = call("click", {"pid": pid, "window_id": wid, "x": 30, "y": 30})
if "effect" not in c_grab: fail("click under a bare grab was refused: " + json.dumps(c_grab)[:300])
sh("pkill -f 'grab status' ; pkill -f XGrabKeyboard; sleep 1")
sh("i3lock -n -c 000000 >/tmp/i3lock.log 2>&1 &"); time.sleep(3)
h4 = host_check("host under i3lock")
if h4["status"] != "fail" or h4["data"]["host"].get("screen_locked") is not True: fail("i3lock must be recognised by its X11 signature: " + open("/tmp/i3lock.log").read()[-200:])
c4 = call("type_text", {"pid": pid, "window_id": wid, "text": "must not reach i3lock"})
print("type_text under i3lock:", json.dumps(c4)[:200])
if c4.get("code") != "host_not_interactive": fail("type_text was not refused under i3lock")
sh("pkill i3lock; sleep 2")
h5 = host_check("host after i3lock exits")
if h5["data"]["host"].get("screen_locked") is not False or h5["status"] != "pass": fail("host did not return to unlocked after i3lock exited")
c5 = call("click", {"pid": pid, "window_id": wid, "x": 30, "y": 30})
if "effect" not in c5: fail("click after i3lock exit was not delivered: " + json.dumps(c5)[:300])
print("LINUX_DRIVE PASS")
