"""Driver adapters for the Codex-equivalence conformance suite.

A conformance test is written once against the abstract Driver interface, then
run against multiple targets:
  - CuaAdapter        : the real cua-driver (our build base / current baseline)
  - OurLayerAdapter   : our Codex-like layer (the thing we are building; stub until it exists)
  - SkyGoldenAdapter  : replays a captured real-Codex trace (the ground-truth oracle)

"Exact same as Codex" = OurLayerAdapter's observable behaviour matches SkyGoldenAdapter's
on the same task, and both satisfy the pinned golden formats in golden/.
"""
from __future__ import annotations
import json, os, subprocess, glob
from dataclasses import dataclass

class Driver:
    name = "abstract"
    def call(self, tool: str, args: dict) -> dict:
        raise NotImplementedError
    # convenience
    def list_windows(self): return self.call("list_windows", {})
    def get_window_state(self, pid, window_id, screenshot=False, text=True, disable_diff=None):
        a = {"pid": pid, "window_id": window_id, "include_screenshot": screenshot, "include_text": text}
        if disable_diff is not None: a["disableDiff"] = disable_diff
        return self.call("get_window_state", a)

class CuaAdapter(Driver):
    name = "cua-driver"
    def __init__(self, binary=None, env=None):
        self.binary = binary or os.path.expanduser("~/.local/bin/cua-driver")
        self.env = {**os.environ, **(env or {})}
    def call(self, tool, args):
        p = subprocess.run([self.binary, "call", tool], input=json.dumps(args),
                           capture_output=True, text=True, env=self.env)
        try: return json.loads(p.stdout)
        except Exception: return {"_raw": p.stdout[:400], "_err": p.stderr[:200]}

class OurLayerAdapter(Driver):
    """Our layer over cua-driver, reached ONLY through the product entry point: the `icu`
    binary (`icu call <tool> <json>` is the same code path as `icu mcp`). Presents the
    Codex window2 tool surface; get_window_state carries the axdiff lineage + packs; input
    tools pass the spec-14 approval gate (disabled here: the tests exercise the mechanics
    through `icu confirmations`). The crates' own dev verbs are reached as `icu <crate> ...`
    so every conformance check runs the shipped binary."""
    name = "our-layer"
    def __init__(self, endpoint=None):
        self.cua = CuaAdapter()
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        self.icu_bin = os.environ.get("ICU_BIN", os.path.join(root, "impl", "target", "debug", "icu"))
        # session = state dir: a FRESH dir per adapter (pytest session) unless the
        # caller pins one, so first-touch/lineage state never leaks across runs
        import tempfile
        self.state_dir = os.environ.get("AXDIFF_STATE_DIR") or tempfile.mkdtemp(prefix="axdiff-conf-state-")
        self.home = tempfile.mkdtemp(prefix="icu-conf-home-")
        self.env = dict(os.environ)
        self.env.update({"ICU_STATE_DIR": self.state_dir, "ICU_HOME": self.home, "ICU_REQUIRE_APP_APPROVAL": "0",
                         "ICU_DRIVER": os.environ.get("ICU_DRIVER", self.cua.binary)})
        if os.environ.get("ICU_SOCKET"): self.env["ICU_SOCKET"] = os.environ["ICU_SOCKET"]
    def _run(self, *argv, check=True):
        import subprocess
        r = subprocess.run([self.icu_bin, *argv], capture_output=True, env=self.env)
        if check and r.returncode != 0: raise RuntimeError(r.stderr.decode()[:300])
        return r.returncode, r.stdout, r.stderr.decode()
    def app_instructions(self, bundle_id: str):
        """Spec 04: the per-app pack our layer prefixes on first touch, as raw
        bytes (verbatim contract), or None when the app has no pack."""
        code, out, err = self._run("app-instructions", "--bundle-id", bundle_id, check=False)
        if code == 3: return None
        if code != 0: raise RuntimeError(err[:300])
        return out
    def tools(self):
        # real discovery from the shipped binary (same table `icu mcp` serves in tools/list)
        return [l.strip() for l in self._run("tools")[1].decode().splitlines() if l.strip()]
    def confirmations(self, *argv, check=True):
        """Spec 14 mechanics via `icu confirmations ...`. Returns (exit, stdout_bytes, stderr)."""
        return self._run("confirmations", *argv, check=check)
    def skysight(self, *argv, check=True):
        """Spec 03 store via `icu skysight ...`. Returns (exit, stdout_bytes, stderr)."""
        return self._run("skysight", *argv, check=check)
    def _w2(self, *argv):
        code, out, err = self._run("window2", *argv, check=False)
        if code != 0: return {"_error": err.strip(), "_exit": code}
        o = json.loads(out)
        if isinstance(o, dict): o["_layer_log"] = err
        return o
    def _window(self, args):
        return {"app": args.get("app") or "", "id": args.get("window_id") if args.get("window_id") is not None else args.get("id")}
    def call(self, tool, args):
        """The product surface: `icu call <tool> <json>` with the window2 argument shapes
        (a `window` object, not pid+window_id)."""
        a = dict(args)
        if tool == "get_window_state":
            a = {k: v for k, v in args.items() if k not in ("pid", "window_id", "app", "id")}
            a.update({"window": self._window(args), "include_text": args.get("include_text", False), "include_screenshot": args.get("include_screenshot", True)})
        elif tool in ("get_window", "activate_window"):
            a = {"window": {"app": args.get("app") or "", "id": args.get("id")}} if tool == "activate_window" else {"id": args.get("id")}
        elif tool == "placement":
            a = {"id": args.get("id")}
        code, out, err = self._run("call", tool, json.dumps(a), check=False)
        try: o = json.loads(out)
        except Exception: return {"_error": err.strip(), "_exit": code, "_raw": out.decode()[:400]}
        if code != 0 and isinstance(o, dict): o.setdefault("_error", err.strip()); o["_exit"] = code
        if isinstance(o, dict): o["_layer_log"] = err
        return o
    def placement_fixture(self, lw_path, window_id):
        """Deterministic: placement diagnostics from a saved list_windows document."""
        return self._w2("placement", "--id", str(window_id), "--lw", lw_path)
    def map_fixture(self, gws_path, include_text=True, include_screenshot=True, app=None):
        """Deterministic: build a WindowState from a saved cua document (fresh lineage)."""
        import tempfile
        argv = ["map", "--gws", gws_path, "--state", tempfile.mkdtemp(prefix="w2map-")]
        if include_text: argv.append("--text")
        if not include_screenshot: argv.append("--no-screenshot")
        if app: argv += ["--app", app]
        return self._w2(*argv)

class SkyGoldenAdapter(Driver):
    """Replays a captured real-Codex trace directory (the ground-truth oracle).
    Each file is <tool>__<hash>.json = {"args":..., "result":...} recorded from live Codex."""
    name = "sky-golden"
    def __init__(self, trace_dir):
        self.trace = {}
        for f in glob.glob(os.path.join(trace_dir, "*.json")):
            d = json.load(open(f)); self.trace[(d["tool"], json.dumps(d["args"], sort_keys=True))] = d["result"]
    def call(self, tool, args):
        return self.trace.get((tool, json.dumps(args, sort_keys=True)), {"_missing": True})

class NativeCuaAdapter(CuaAdapter):
    """The vendored cua-driver rebuilt with impl/native/cua-driver.patch (slices 6+7):
    the SAME tool surface as cua, plus the structured `diff` field / `elements[].stable_id`
    from the per-window revision store and the overlay occlusion clip. Binary:
    $NATIVE_CUA_BIN or vendor-cua/libs/cua-driver/rust/target/release/cua-driver."""
    name = "cua-driver-native"
    def __init__(self):
        _default = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                                "vendor-cua", "libs", "cua-driver", "rust", "target", "release", "cua-driver")
        super().__init__(binary=os.environ.get("NATIVE_CUA_BIN", _default))
        # `call` is a proxy to a daemon; point it at a daemon RUNNING THE PATCHED BINARY
        # (`cua-driver serve --socket $NATIVE_CUA_SOCKET --no-permissions-gate`), otherwise the
        # installed app daemon (old code) would answer.
        self.socket = os.environ.get("NATIVE_CUA_SOCKET")
    def _argv(self, *rest):
        return [self.binary, *(["--socket", self.socket] if self.socket else []), *rest]
    def call(self, tool, args):
        p = subprocess.run(self._argv("call", tool), input=json.dumps(args),
                           capture_output=True, text=True, env=self.env)
        try: return json.loads(p.stdout)
        except Exception: return {"_raw": p.stdout[:400], "_err": p.stderr[:200]}

def target_from_env() -> Driver:
    t = os.environ.get("CONFORMANCE_TARGET", "cua")
    if t == "cua": return CuaAdapter()
    if t == "native": return NativeCuaAdapter()
    if t == "our": return OurLayerAdapter()
    if t.startswith("golden:"): return SkyGoldenAdapter(t.split(":", 1)[1])
    raise SystemExit(f"unknown CONFORMANCE_TARGET={t}")
