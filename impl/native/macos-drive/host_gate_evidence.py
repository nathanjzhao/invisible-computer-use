"""macOS evidence for the host gate (cua-driver-core::host_state) on the patched daemon.

Phase 1 (always): health_report host_interactive passes on the interactive session, the lid and
display facts match the OS (ioreg / CGDisplay), and a click on the target is delivered.
Phase 2 (HOST_LOCK=1): lock the screen (Cmd+Ctrl+Q via System Events), then show health_report
fails with screen_locked=true, an input tool is refused with code host_not_interactive, and
get_window_state still answers. The script cannot unlock the Mac; the person at the machine does.
Env: NATIVE_CUA_BIN, NATIVE_CUA_SOCKET (default /tmp/icu-native.sock), TARGET_PID, TARGET_WID;
HOST_EXPECT_LOCKED=1 when the Mac is already locked (runs the locked checks only, no keystroke).
"""
import json, os, subprocess, sys, time
BIN = os.environ.get("NATIVE_CUA_BIN", "vendor-cua/libs/cua-driver/rust/target/release/cua-driver")
SOCK = os.environ.get("NATIVE_CUA_SOCKET", "/tmp/icu-native.sock")
PID, WID = int(os.environ["TARGET_PID"]), int(os.environ["TARGET_WID"])
def call(tool, args):
    p = subprocess.run([BIN, "--socket", SOCK, "call", tool], input=json.dumps(args), capture_output=True, text=True)
    try: return json.loads(p.stdout)
    except Exception: return {"_raw": (p.stdout + p.stderr)[:400]}
def host(tag):
    hr = call("health_report", {"include": ["host_interactive"]})
    c = next((c for c in hr.get("checks", []) if c.get("name") == "host_interactive"), None)
    print(f"{tag}: {c and (c['status'], c.get('data'), c.get('hint'))}")
    if c: c["host"] = c.get("data", {}).get("host", {})
    return c
def fail(m): print("HOST_GATE_RESULT FAIL:", m); sys.exit(1)
ALREADY = os.environ.get("HOST_EXPECT_LOCKED") == "1"   # the Mac is locked right now: run the locked checks only
h0 = host("host before")
if ALREADY:
    if not h0 or h0["host"].get("screen_locked") is not True: fail("HOST_EXPECT_LOCKED=1 but the host is not locked")
elif not h0 or h0["status"] != "pass" or h0["host"].get("screen_locked") is not False: fail("interactive host must pass with screen_locked=false")
if ALREADY: h1 = h0
lid_os = "Yes" in subprocess.run("ioreg -r -k AppleClamshellState -d 4 | grep AppleClamshellState", shell=True, capture_output=True, text=True).stdout
if h0["host"].get("lid_closed") != lid_os: fail(f"lid_closed {h0['data'].get('lid_closed')} != ioreg {lid_os}")
if not ALREADY:
    c0 = call("click", {"pid": PID, "window_id": WID, "x": 40, "y": 40, "background": True})
    print("click before:", {k: c0.get(k) for k in ("effect", "route", "code")})
    if "effect" not in c0: fail("click was not delivered on the interactive host: " + json.dumps(c0)[:300])
    if os.environ.get("HOST_LOCK") != "1":
        print("HOST_GATE_RESULT PASS (phase 1 only; set HOST_LOCK=1 to lock the screen and drive phase 2)"); sys.exit(0)
    subprocess.run(["osascript", "-e", 'tell application "System Events" to keystroke "q" using {command down, control down}'])
    time.sleep(4)
    h1 = host("host locked")
if not h1 or h1["status"] != "fail" or h1["host"].get("screen_locked") is not True: fail("locked host must fail with screen_locked=true")
c1 = call("click", {"pid": PID, "window_id": WID, "x": 40, "y": 40, "background": True})
print("click locked:", json.dumps(c1)[:300])
if c1.get("code") != "host_not_interactive" or c1.get("reasons") != ["screen_locked"]: fail("click was not refused while locked")
t1 = call("type_text", {"pid": PID, "window_id": WID, "text": "must not reach the password field"})
if t1.get("code") != "host_not_interactive": fail("type_text was not refused while locked: " + json.dumps(t1)[:300])
s1 = call("get_window_state", {"pid": PID, "window_id": WID, "include_screenshot": False, "include_text": True})
print("observe locked: elements=", len(s1.get("elements", [])), "diff=", (s1.get("diff") or {}).get("kind"), "code=", s1.get("code"))
print("HOST_GATE_RESULT PASS (screen is locked now; unlock it at the machine)")
