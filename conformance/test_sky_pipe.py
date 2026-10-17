"""The length-prefixed Sky native pipe (specs/00-COVERAGE.md:151): `icu sky-pipe` serves the wire
protocol of Codex's `native-pipe.js` over the layer.

Raw cua-driver speaks newline JSON and `icu mcp` speaks MCP, so the stock targets (`cua`, `native`)
mark these xfail with the gap. For `our`: one test speaks the frames by hand (the version gate, the
8 MiB ceiling, a stranger's hang-up); the other runs the REAL `client.js` and `native-pipe.js` from
`reference/openai-sky/sky_js_src` under node, unmodified except for the tslib import path, against
the server, and asserts the shapes it gets back against the capture of the real service
(`study/audit/golden-trace-prep-2026-09-25/validation/probe/sky-ipc.jsonl`). Offline: the driver
behind `icu` is the scripted fixture, never a desktop.
"""
import json, os, re, shutil, socket, struct, subprocess, tempfile, time
from pathlib import Path
import pytest
from adapters import target_from_env

ROOT = Path(__file__).resolve().parents[1]
ICU = Path(os.environ.get("ICU_BIN", ROOT / "impl/target/debug/icu"))
FIXTURES = ROOT / "conformance/fixtures"
# Point at a locally installed targets/mac directory to check a newer client without
# replacing the archived baseline or executing its native service.
SKY_JS = Path(os.environ.get("CONFORMANCE_SKY_MAC_SOURCE", ROOT / "reference/openai-sky/sky_js_src/targets/mac"))
CAPTURE = ROOT / "study/audit/golden-trace-prep-2026-09-25/validation/probe/sky-ipc.jsonl"
API = "CodexComputerUseIPC-5"
APP = "dev.icu.fixture"


@pytest.fixture(scope="module")
def drv():
    return target_from_env()


def only_our(drv):
    if drv.name != "our-layer":
        pytest.xfail(f"{drv.name} has no length-prefixed Sky pipe (cua speaks newline JSON, icu mcp speaks MCP); `icu sky-pipe` adds it")


@pytest.fixture
def server(tmp_path):
    """`icu sky-pipe` over the fixture driver; yields the socket path."""
    assert ICU.is_file(), f"required binary absent: {ICU}"
    # a unix socket path must fit in SUN_LEN (104 bytes on macOS): pytest's tmp_path does not
    short = Path(tempfile.mkdtemp(prefix="icu-sky-", dir="/tmp"))
    sock = short / "pipe" / "computeruse.sock"
    env = {k: v for k, v in os.environ.items() if not k.startswith(("ICU_", "FEATURE_DRIVER_", "SKY_CUA"))}
    env.update(ICU_DRIVER=str(FIXTURES / "feature_driver.py"), ICU_DRIVER_DAEMON="0", ICU_HOME=str(tmp_path / "home"),
               ICU_REQUIRE_APP_APPROVAL="0", ICU_POST_ACTION_SLEEP_MS="0", FEATURE_DRIVER_LOG=str(tmp_path / "driver.log"))
    p = subprocess.Popen([str(ICU), "sky-pipe", "--socket", str(sock)], env=env, stderr=subprocess.PIPE, text=True)
    deadline = time.time() + 10
    while not sock.exists():
        assert p.poll() is None, p.stderr.read()
        assert time.time() < deadline, "sky-pipe did not start"
        time.sleep(0.05)
    yield sock
    p.terminate(); p.wait(timeout=10)
    shutil.rmtree(short, ignore_errors=True)


def frame(msg):
    b = json.dumps(msg).encode()
    return struct.pack("<I", len(b)) + b


def read_frame(s):
    head = b""
    while len(head) < 4:
        chunk = s.recv(4 - len(head))
        if not chunk: return None
        head += chunk
    (n,) = struct.unpack("<I", head)
    body = b""
    while len(body) < n:
        chunk = s.recv(n - len(body))
        if not chunk: return None
        body += chunk
    return json.loads(body)


def connect(path):
    s = socket.socket(socket.AF_UNIX); s.settimeout(10); s.connect(str(path)); return s


def test_the_pipe_speaks_length_prefixed_json_rpc_with_a_version_gate(drv, server):
    only_our(drv)
    s = connect(server)
    s.sendall(frame({"id": 1, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": API}}))
    # the real service's reply to this exact ping, from the capture
    real = [json.loads(json.loads(l)["raw"]) for l in CAPTURE.read_text().splitlines() if json.loads(l)["dir"] == "recv"][0]
    assert read_frame(s) == {**real, "id": 1}
    s.sendall(frame({"id": 2, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": "CodexComputerUseIPC-3"}}))
    old = read_frame(s)
    assert old["error"]["code"] == -10013 and old["id"] == 2, old  # ServerErrorCode.incompatibleClientVersion
    # two frames in one write both get answered, in order
    s.sendall(frame({"id": 3, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": API}}) + frame({"id": 4, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": API}}))
    assert [read_frame(s)["id"], read_frame(s)["id"]] == [3, 4]
    s.close()
    # a frame over the 8 MiB ceiling ends the connection; so does a message that is not JSON
    big = connect(server)
    big.sendall(struct.pack("<I", 8388609))
    assert read_frame(big) is None
    junk = connect(server)
    junk.sendall(struct.pack("<I", 3) + b"abc")
    assert read_frame(junk) is None
    ok = connect(server)
    ok.sendall(frame({"id": 9, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": API}}))
    assert read_frame(ok)["result"]["serverApiVersion"] == API, "the server survived both"
    assert oct(os.stat(server).st_mode & 0o777) == "0o600" and oct(os.stat(server.parent).st_mode & 0o777) == "0o700"


@pytest.mark.skipif(shutil.which("node") is None, reason="needs node to run Codex's own client code")
def test_codexs_own_client_drives_the_layer_through_the_pipe(drv, server, tmp_path):
    only_our(drv)
    work = tmp_path / "js"; work.mkdir()
    (work / "package.json").write_text('{"type": "module"}')
    for name in ("client.js", "native-pipe.js", "errors.js"):
        src = (SKY_JS / name).read_text()
        (work / name).write_text(re.sub(r'from"[^"]*tslib\.es6\.js"', 'from"./tslib-shim.js"', src))
    (work / "tslib-shim.js").write_text(TSLIB)
    shutil.copy(FIXTURES / "sky_client_harness.mjs", work / "harness.mjs")
    p = subprocess.run(["node", "harness.mjs", str(server), APP], cwd=work, capture_output=True, text=True, timeout=60)
    assert p.returncode == 0, p.stdout[-800:] + p.stderr[-1500:]
    out = json.loads(p.stdout.splitlines()[-1])

    real = {}
    ids = {}
    for l in CAPTURE.read_text().splitlines():
        d = json.loads(l); raw = json.loads(d["raw"])
        if d["dir"] == "send" and "requestType" in raw.get("params", {}): ids[raw["id"]] = raw["params"]["requestType"]
        elif d["dir"] == "recv" and raw.get("id") in ids: real.setdefault(ids[raw["id"]] + (":error" if "error" in raw else ""), raw)
    shape = lambda v: sorted(v.keys())

    assert any(a["bundleIdentifier"] == APP for a in out["apps"])
    assert shape(out["policy"]) == shape(real["ComputerUseIPCAppPolicyRequest"]["result"]) and out["policy"]["decision"] == "allowed"
    s1 = out["state1"]
    assert {"app", "skyshot"} <= set(s1) and {"app", "skyshot"} <= set(real["ComputerUseIPCAppGetSkyshotRequest"]["result"])
    assert s1["app"]["bundleIdentifier"] == APP and s1["skyshot"]["text"].startswith("Window: ")
    assert s1["skyshot"]["screenshot"]["mimeType"] == "image/png" and s1["skyshot"]["screenshot"]["url"].startswith("file://")
    assert os.path.getsize(s1["skyshot"]["screenshot"]["url"][len("file://"):].replace("%20", " ")) > 0
    assert out["ids"], "the tree has rows"
    assert out["click"] and shape(out["click"]["result"]) == shape(real["ComputerUseIPCAppPerformActionRequest"]["result"]), "the real answer to an action is {app}"
    assert out["state2"]["skyshot"]["text"].startswith("There has been no change in the accessibility tree for Window") or out["state2"]["skyshot"]["text"].startswith("The following is a diff")
    assert out["state3"]["skyshot"]["text"].startswith("Window: "), "disableDiff returns the full tree"
    for k in ("typed", "key", "scroll", "drag"):
        assert "ok" in out[k], (k, out[k])
    log = [json.loads(l) for l in (tmp_path / "driver.log").read_text().splitlines()]
    tools = [r["tool"] for r in log]
    assert "type_text" in tools and "scroll" in tools and "drag" in tools and ("press_key" in tools or "hotkey" in tools)
    assert [r for r in log if r["tool"] == "type_text"][0]["args"]["text"] == "hello from the real client"
    assert "ok" in out["pasteText"] and "ok" in out["selectedText"]
    for key in ("pasteHtml", "pasteMd", "selectionBefore", "selectionAfter"):
        assert out[key]["error"]["code"] == -10005, (key, out[key])
        assert "supports only" in out[key]["error"]["message"]
    writes = [r["args"]["text"] for r in log if r["tool"] == "clipboard_write"]
    assert writes == ["plain client paste", ""], "only the plain paste and its restore may write the clipboard"
    selections = [r["args"] for r in log if r["tool"] == "select_text"]
    assert len(selections) == 1 and selections[0]["text"] == "cat"
    assert (selections[0]["prefix"], selections[0]["suffix"]) == ("dog ", " bird")
    inv = out["invalidElement"]["error"]
    assert (inv["name"], inv["code"], inv["message"]) == ("SkyComputerUseError", -10005, "Computer Use server error -10005: 99999 is an invalid element ID")
    bad = out["invalidApp"]["error"]
    assert (bad["code"], bad["message"]) == (-10010, "Invalid app: dev.icu.does-not-exist")
    assert (bad["code"], bad["message"].split(":")[0]) == (real["ComputerUseIPCAppPolicyRequest:error"]["error"]["code"], "Invalid app")
    assert out["audio"]["error"]["code"] == -10003, "audio is a non-goal"
    assert out["oldVersion"]["error"]["errorName"] == "incompatibleClientVersion", out["oldVersion"]


def test_expired_request_is_refused_over_the_wire_without_input(drv, server, tmp_path):
    only_our(drv)
    with connect(server) as s:
        s.sendall(frame({"id": 1, "jsonrpc": "2.0", "method": "request", "params": {
            "clientApiVersion": API, "deadlineUnixMilliseconds": 1,
            "requestType": "ComputerUseIPCAppPerformActionRequest",
            "request": {"app": APP, "action": {"type": {"_0": "expired"}}},
        }}))
        reply = read_frame(s)
        assert reply["error"]["code"] == -10005 and "deadline elapsed" in reply["error"]["message"]
    log_path = tmp_path / "driver.log"
    rows = [json.loads(l) for l in log_path.read_text().splitlines()] if log_path.exists() else []
    assert not any(r["tool"] == "type_text" for r in rows)


TSLIB = r'''
export function __awaiter(thisArg, _arguments, P, generator) {
  function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
  return new (P || (P = Promise))(function (resolve, reject) {
    function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
    function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
    function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
    step((generator = generator.apply(thisArg, _arguments || [])).next());
  });
}
export function __classPrivateFieldGet(receiver, state, kind, f) {
  if (kind === "a" && !f) throw new TypeError("Private accessor was defined without a getter");
  if (typeof state === "function" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError("Cannot read private member from an object whose class did not declare it");
  return kind === "m" ? f : kind === "a" ? f.call(receiver) : f ? f.value : state.get(receiver);
}
export function __classPrivateFieldSet(receiver, state, value, kind, f) {
  if (kind === "m") throw new TypeError("Private method is not writable");
  if (kind === "a" && !f) throw new TypeError("Private accessor was defined without a setter");
  if (typeof state === "function" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError("Cannot write private member to an object whose class did not declare it");
  return (kind === "a" ? f.call(receiver, value) : f ? f.value = value : state.set(receiver, value)), value;
}
'''
