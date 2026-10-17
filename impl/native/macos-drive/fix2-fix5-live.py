#!/usr/bin/env python3
"""Live verification for the two screen-unlock-gated review fixes, run when the
Mac is unlocked with an active display:

  FIX 2 (spec 07): get_window_state returns a z-ordered screenshots[] that
    includes an open transient surface (a real right-click context menu). Saves
    the decoded menu PNG as evidence.
  FIX 5 (spec 06): select_text sets AXSelectedTextRange on the focused text
    element, and the selection reader (read_selection) reads the selected run
    back.

It drives a throwaway TextEdit document opened from a temp file (a reliable
editable AXTextArea, unlike a bare `open -a TextEdit` which shows an Open panel).

Usage: python3 impl/native/macos-drive/fix2-fix5-live.py
Exit 0 = PASS, 2 = BLOCKED (screen locked / no display), 1 = FAIL.
"""
import subprocess, json, sys, os, base64, time

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
RUST = os.path.join(ROOT, "vendor-cua/libs/cua-driver/rust")
DRV = os.path.join(RUST, "target/release/cua-driver")
ICU = os.path.join(ROOT, "impl/target/release/icu")
SOCK = "/tmp/icu-live.sock"
OUT = "/tmp/icu-live-evidence"
DOC = "/tmp/icu-fix5.txt"
DOC_TEXT = "the quick brown fox jumps over the lazy dog"
os.makedirs(OUT, exist_ok=True)


def sh(*a, **k):
    return subprocess.run(a, capture_output=True, text=True, **k)


def decode_json(stdout):
    # Start at the EARLIEST of '{' or '[': `icu list_windows` returns a top-level
    # JSON array, so scanning for '{' first would parse only its first element
    # object instead of the array.
    i = stdout.find("{")
    j = stdout.find("[")
    starts = [x for x in (i, j) if x >= 0]
    if not starts:
        return None
    try:
        return json.JSONDecoder().raw_decode(stdout[min(starts):])[0]
    except Exception:
        return None


def fresh_daemon():
    # kill any daemon on our socket, clear the stale socket file, start fresh so
    # the embedded Screen-Recording responsibility chain is clean.
    # Reuse an already-healthy daemon: restarting it and immediately polling
    # races the AX walk (the fresh daemon is not yet ready to resolve a window),
    # which shows up as a spurious "never AX-resolved". Only (re)start when the
    # socket has no daemon or an unhealthy one.
    if daemon_healthy():
        return
    ps = sh("bash", "-lc", f"ps ax | grep 'cua-driver serve --socket {SOCK}' | grep -v grep | awk '{{print $1}}'")
    for pid in ps.stdout.split():
        sh("kill", pid)
    try:
        os.remove(SOCK)
    except FileNotFoundError:
        pass
    # the installed CuaDriver.app must be running to host the responsibility chain
    if not sh("bash", "-lc", "pgrep -f 'CuaDriver.app/Contents/MacOS/cua-driver'").stdout.strip():
        subprocess.Popen(["open", "-a", "CuaDriver"])
        time.sleep(2)
    env = dict(os.environ, CUA_DRIVER_EMBEDDED="1")
    subprocess.Popen([DRV, "serve", "--socket", SOCK, "--no-permissions-gate"],
                     env=env, stdout=open("/tmp/icu-live-driver.log", "w"), stderr=subprocess.STDOUT)
    # wait until it actually answers list_windows, not a fixed sleep
    for _ in range(24):
        time.sleep(0.5)
        if daemon_healthy():
            return


def daemon_healthy():
    r = sh(DRV, "call", "list_windows", "{}", "--socket", SOCK)
    return r.returncode == 0 and "[" in r.stdout


def icu(*args):
    env = dict(os.environ, ICU_DRIVER=DRV, ICU_SOCKET=SOCK,
               ICU_REQUIRE_APP_APPROVAL="0", ICU_CURSOR="off")
    r = subprocess.run([ICU, *args], capture_output=True, text=True, env=env)
    return decode_json(r.stdout), r


def icu_call(tool, payload):
    d, r = icu("call", tool, json.dumps(payload))
    return d


def host():
    d, _ = icu("call", "host_state")
    h = (d or {}).get("data", {}).get("host", d or {})
    return h


def find_doc_window():
    d = icu_call("list_windows", {})
    wins = d if isinstance(d, list) else (d or {}).get("windows", [])
    te = [w for w in wins if w.get("app") == "com.apple.TextEdit" and (w.get("title") or "").endswith("icu-fix5.txt")]
    return te[0]["id"] if te else None


def frontmost_bundle():
    # the user's real front app, so we can restore it on exit
    r = sh("bash", "-lc",
           "osascript -e 'tell application \"System Events\" to get bundle identifier of first process whose frontmost is true' 2>/dev/null")
    return r.stdout.strip() or None


def restore_front(bundle):
    if bundle:
        sh("open", "-b", bundle)


def main():
    # This harness drives real apps and briefly opens a context menu; it must run
    # ONLY in a bounded session the user is present for, never unattended on an
    # active desktop. The keyboard-grab fix lives in the driver; this proves the
    # FIX 2 / FIX 5 evidence.
    if os.environ.get("ICU_BOUNDED_TEST") != "1":
        print("REFUSED: set ICU_BOUNDED_TEST=1 to run this in an arranged bounded session.")
        return 2

    original_front = frontmost_bundle()
    fresh_daemon()
    h = host()
    if h.get("screen_locked") or (h.get("active_displays") or 0) == 0:
        print(f"BLOCKED: screen locked / no active display: {h}")
        return 2

    try:
        return _drive()
    finally:
        # Balanced cleanup on success, failure, and interruption: close the
        # throwaway doc and hand the user's real front app back. The driver
        # already restores the WindowServer key connection per click; this
        # restores the visible/app-level front the harness disturbed.
        sh("killall", "TextEdit")
        restore_front(original_front)


def _drive():
    # reliable editable doc from a temp file
    sh("killall", "TextEdit")
    time.sleep(1)
    open(DOC, "w").write(DOC_TEXT)
    subprocess.Popen(["open", "-a", "TextEdit", DOC])

    wid = None
    for _ in range(20):
        time.sleep(0.7)
        wid = find_doc_window()
        if wid:
            # wait until it AX-resolves (document_text present)
            st = icu_call("get_window_state", {"window": {"app": "com.apple.TextEdit", "id": wid},
                                               "include_text": True, "include_screenshot": False})
            acc = (st or {}).get("accessibility") or {}
            if acc.get("document_text"):
                break
    else:
        print("FAIL: TextEdit doc window never AX-resolved")
        return 1
    print(f"doc window id={wid}, document_text read live = {acc.get('document_text')!r}")

    passed = {"fix5_select": False, "fix2_menu": False}

    # ---- FIX 5: select "quick" via the focused-element fallback, read it back ----
    r = icu_call("select_text", {"window": {"app": "com.apple.TextEdit", "id": wid}, "text": "quick"})
    time.sleep(0.4)
    st2 = icu_call("get_window_state", {"window": {"app": "com.apple.TextEdit", "id": wid},
                                        "include_text": True, "include_screenshot": False})
    sel = ((st2 or {}).get("accessibility") or {}).get("selected_text")
    print(f"FIX5 select_text -> {(r or {}).get('selected')} location={(r or {}).get('location')} length={(r or {}).get('length')}; reader selected_text={sel!r}")
    passed["fix5_select"] = bool(r and r.get("selected")) and sel == "quick"

    # ---- FIX 2: open a right-click context menu, capture the transient surface ----
    icu_call("click", {"window": {"app": "com.apple.TextEdit", "id": wid}, "x": 150, "y": 120, "mouse_button": "right"})
    time.sleep(0.8)
    shot = icu_call("get_window_state", {"window": {"app": "com.apple.TextEdit", "id": wid},
                                         "include_screenshot": True, "include_text": False})
    ss = (shot or {}).get("screenshots", [])
    print(f"FIX2 screenshots count = {len(ss)}")
    saved = []
    for k, s in enumerate(ss):
        u = s.get("url", "")
        print(f"  [{k}] id={s.get('id')} z={s.get('zIndex')} origin=({s.get('originX')},{s.get('originY')}) wh=({s.get('width')}x{s.get('height')})")
        if ";base64," in u:
            b = base64.b64decode(u.split(";base64,", 1)[1])
            fn = os.path.join(OUT, f"surface-{k}-z{s.get('zIndex')}.png")
            open(fn, "wb").write(b)
            saved.append((k, s.get("zIndex"), fn, len(b)))
    # a transient surface = a captured screenshot above the base window (zIndex>0), non-trivial size
    transient = [t for t in saved if (t[1] or 0) > 0 and t[3] > 2000]
    passed["fix2_menu"] = len(transient) > 0
    if transient:
        print("FIX2 transient menu surface(s):", [(t[0], t[2], f"{t[3]}B") for t in transient])

    # dismiss the menu, close the doc
    icu_call("press_key", {"window": {"app": "com.apple.TextEdit", "id": wid}, "key": "escape"})
    sh("killall", "TextEdit")

    print("RESULT:", passed)
    return 0 if all(passed.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
