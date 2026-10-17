"""Bring a window onto its display's current Space via the installed daemon and wait until it is on-Space + on-screen."""
import json, os, subprocess, sys, time
OLD = os.environ.get("CUA_DRIVER", os.path.expanduser("~/.local/bin/cua-driver"))
pid, wid = int(sys.argv[1]), int(sys.argv[2])
def call(tool, args):
    r = subprocess.run([OLD, "call", tool, json.dumps(args)], capture_output=True, text=True)
    try: return json.loads(r.stdout)
    except Exception: return {"_raw": (r.stdout + r.stderr)[:200]}
def entry():
    d = call("list_windows", {})
    return next((w for w in d.get("windows", []) if w["window_id"] == wid), None), d.get("current_space_id")
for attempt in range(4):
    w, cur = entry()
    if w and w["on_current_space"] and w["is_on_screen"]:
        print(f"VISIBLE pid={pid} wid={wid} space={w['space_ids']} display_current={w['current_space_id']} global={cur}"); sys.exit(0)
    print(f"attempt {attempt}: bring_to_front ->", call("bring_to_front", {"pid": pid, "window_id": wid}).get("effect", "?"), "| was", w and (w["on_current_space"], w["is_on_screen"], w["current_space_id"], w["space_ids"]))
    time.sleep(1.5)
    w2, _ = entry()
    if w and not (w2 and w2["on_current_space"]):
        # bring_to_front never steals the display's Space when another app is active; activating the
        # app through AppleScript makes macOS switch that display to the window's Space
        subprocess.run(["osascript", "-e", f'tell application "{w["app_name"]}" to activate'], capture_output=True)
        time.sleep(1.5)
w, cur = entry(); print("NOT_VISIBLE", w and (w["on_current_space"], w["is_on_screen"], w["current_space_id"], w["space_ids"])); sys.exit(1)
