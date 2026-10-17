#!/usr/bin/env python3
"""A scripted stand-in for cua-driver over MCP stdio, for the parked-feature conformance tests.
It never touches a desktop. It serves the saved real Clock `get_window_state` document (with a
long value and a full-size PNG when asked), accepts every input tool, and appends each tools/call
it receives to FEATURE_DRIVER_LOG as one JSON line, so a test can assert what the layer actually
sent. FEATURE_DRIVER_LONG_VALUE=N makes element 7's value N characters long;
FEATURE_DRIVER_FAIL=tool1,tool2 makes those tools return a driver refusal.
FEATURE_DRIVER_DESKTOP=1 adds the desktop scope (get_desktop_state, get_screen_size, move_cursor, and the
desktop-scope click and drag) on an 800x600 screen; FEATURE_DRIVER_NATIVE=1 also advertises, in tools/list
inputSchema, the hold_ms, path and modifier fields the patched native driver adds, as a stock driver does not."""
import base64, json, os, struct, sys, zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
LOG = os.environ.get("FEATURE_DRIVER_LOG")
LONG = int(os.environ.get("FEATURE_DRIVER_LONG_VALUE", "0") or 0)
FAIL = set(filter(None, os.environ.get("FEATURE_DRIVER_FAIL", "").split(",")))
DESKTOP = os.environ.get("FEATURE_DRIVER_DESKTOP") == "1"
NATIVE = os.environ.get("FEATURE_DRIVER_NATIVE") == "1"
SCREEN = (800, 600)
BG = (200, 210, 220)

TOOLS = ["list_windows", "list_apps", "get_window_state", "click", "double_click", "right_click", "type_text",
         "press_key", "hotkey", "scroll", "set_value", "drag", "select_text", "launch_app", "bring_to_front",
         "health_report", "start_session", "set_agent_cursor_enabled", "set_agent_cursor_motion",
         "set_agent_cursor_theme", "get_agent_cursor_state", "verify_state", "start_recording",
         "stop_recording", "get_recording_state", "replay_trajectory", "clipboard_read", "clipboard_write"]
if DESKTOP: TOOLS += ["get_desktop_state", "get_screen_size", "move_cursor"]
NATIVE_FIELD = {"click": "hold_ms", "drag": "path", "move_cursor": "modifier", "press_key": "hold_ms", "scroll": "modifier"}
SCOPED = ("press_key", "type_text", "scroll")  # the desktop scope the keyboard and scroll verbs take
APPS = [{"pid": 100, "bundle_id": "dev.icu.fixture", "name": "Fixture"}, {"pid": 101, "bundle_id": "dev.icu.other", "name": "Other"}]
WINDOWS = [
    {"window_id": 7, "pid": 100, "app_name": "Fixture", "bundle_id": "dev.icu.fixture", "title": "Doc", "on_current_space": True, "is_on_screen": True, "bounds": {"x": 0, "y": 0, "width": 1024, "height": 768}},
    {"window_id": 8, "pid": 101, "app_name": "Other", "bundle_id": "dev.icu.other", "title": "Elsewhere", "on_current_space": True, "is_on_screen": True, "bounds": {"x": 0, "y": 0, "width": 640, "height": 480}},
]


def png(w, h, rgb):
    row = b"\x00" + bytes(rgb) * w
    raw = row * h
    def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def window_state(args):
    d = json.loads((HERE / "clock_gws.json").read_text())
    d["pid"], d["window_id"] = args["pid"], args["window_id"]
    if LONG: d["elements"][7]["value"] = "v" * LONG
    if args.get("include_screenshot") is False:
        for k in ("screenshot_png_b64", "screenshot_width", "screenshot_height", "screenshot_scale", "screenshots"): d.pop(k, None)
    else:
        d["screenshot_png_b64"] = base64.b64encode(png(1568, 1176, BG)).decode()
        d["screenshot_id"] = "fx-%d" % os.getpid()
    return d


def main():
    for line in sys.stdin:
        req = json.loads(line)
        if "id" not in req: continue
        m = req["method"]
        if m == "initialize":
            result = {"protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {"name": "feature-driver", "version": "1"}}
        elif m == "tools/list":
            def props(n):
                p = {}
                if NATIVE and n in NATIVE_FIELD and (DESKTOP or n not in SCOPED): p.update({"x": {}, NATIVE_FIELD[n]: {}})
                if DESKTOP and n in SCOPED: p.update({"x": {}, "scope": {}})
                if DESKTOP and NATIVE and n == "drag": p["hold"] = {}
                return p
            result = {"tools": [{"name": n, **({"inputSchema": {"type": "object", "properties": props(n)}} if props(n) else {})} for n in TOOLS]}
        else:
            name, args = req["params"]["name"], req["params"].get("arguments") or {}
            if LOG and name != "health_report":
                with open(LOG, "a") as f: f.write(json.dumps({"tool": name, "args": args}) + "\n")
            if name in FAIL:
                result = {"isError": True, "content": [{"type": "text", "text": f"{name} refused"}], "structuredContent": {"code": "fixture_refused"}}
            else:
                payload = {"effect": "unverifiable", "route": "accessibility", "tool": name}
                if name == "list_apps": payload = {"apps": APPS}
                elif name == "list_windows": payload = {"windows": WINDOWS}
                elif name == "get_window_state": payload = window_state(args)
                elif name == "health_report":
                    payload = {"overall": "ok", "platform": "darwin", "driver_version": "0.24.0", "checks": [
                        {"name": "tcc_accessibility", "status": "pass"}, {"name": "tcc_screen_recording", "status": "pass"},
                        {"name": "host_interactive", "status": "pass", "data": {"host": {"screen_locked": False, "on_console": True, "display_asleep": False}}}]}
                elif name == "start_session": payload = {"session": args.get("session", "fx")}
                elif name == "verify_state": payload = {"status": "satisfied", "stable": True, "elapsed_ms": 1, "samples": 1,
                                                        "predicates": [{"index": i, "status": "satisfied", "unknown_reason": None, "observed_json": None} for i, _ in enumerate(args.get("expect", []))]}
                elif name in ("start_recording", "get_recording_state", "stop_recording"): payload = {"recording": name != "stop_recording", "args": args}
                elif name == "clipboard_read": payload = {"text": ""}
                elif name == "get_screen_size": payload = {"width": SCREEN[0], "height": SCREEN[1], "scale_factor": 1.0}
                elif name == "get_desktop_state":
                    payload = {"platform": "linux", "display": "primary", "screenshot_png_b64": base64.b64encode(png(SCREEN[0], SCREEN[1], BG)).decode(),
                               "screenshot_width": SCREEN[0], "screenshot_height": SCREEN[1], "screen_width": SCREEN[0], "screen_height": SCREEN[1], "scale_factor": 1.0}
                result = {"structuredContent": payload}
        print(json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": result}), flush=True)


main()
