"""The product entry point, driven the way a project will drive it: a real stdio MCP client
session against `icu mcp` (spawned as a subprocess), on a real app that is on the current
Space. Needs the patched daemon (NATIVE_CUA_SOCKET) and CONFORMANCE_LIVE=1; skipped otherwise.

Checks: initialize/tools/list framing, the window2 tool names first and verbatim, a real
`get_window_state` with the lineage across two turns, the spec-14 elicitation round trip
(server -> client `elicitation/create`, client accepts for the session, the tool proceeds), and
`host_state`. The input tool used is a one-notch `scroll` inside the window (harmless for any
app); a click on a container id shows the stale/non-actionable refusal."""
import json, os, subprocess, pytest

live = pytest.mark.skipif(os.environ.get("CONFORMANCE_LIVE") != "1", reason="needs the patched daemon + a window on the current Space")
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WINDOW2 = ["list_windows","get_window","list_apps","launch_app","get_window_state","click","press_key",
           "type_text","scroll","set_value","drag","perform_secondary_action","activate_window"]

class Client:
    def __init__(self, env):
        self.p = subprocess.Popen([os.environ.get("ICU_BIN", os.path.join(ROOT, "impl", "target", "debug", "icu")), "mcp"],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        self.n = 0; self.log = []
    def send(self, msg):
        self.p.stdin.write(json.dumps(msg) + "\n"); self.p.stdin.flush()
    def read(self):
        line = self.p.stdout.readline()
        assert line, "icu mcp closed: " + self.p.stderr.read()[-400:]
        m = json.loads(line); self.log.append(m); return m
    def request(self, method, params=None, on_elicit=None):
        self.n += 1; rid = self.n
        self.send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}})
        while True:
            m = self.read()
            if m.get("method") == "elicitation/create":
                assert on_elicit, "unexpected elicitation: " + json.dumps(m)[:300]
                self.send({"jsonrpc": "2.0", "id": m["id"], "result": on_elicit(m["params"])}); continue
            if m.get("id") == rid: return m
    def close(self):
        self.p.stdin.close(); self.p.wait(timeout=10)

@pytest.fixture
def client(tmp_path):
    env = dict(os.environ)
    env.update({"ICU_HOME": str(tmp_path), "ICU_DRIVER": os.environ.get("ICU_DRIVER", os.environ.get("NATIVE_CUA_BIN", "cua-driver"))})
    if os.environ.get("NATIVE_CUA_SOCKET"): env["ICU_SOCKET"] = os.environ["NATIVE_CUA_SOCKET"]
    c = Client(env); yield c; c.close()

def call(c, name, args, on_elicit=None):
    m = c.request("tools/call", {"name": name, "arguments": args}, on_elicit)
    r = m["result"]; return r.get("structuredContent"), r.get("isError", False)

@live
def test_mcp_session_on_a_real_app(client):
    init = client.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {"elicitation": {}}, "clientInfo": {"name": "conformance", "version": "0"}})
    assert init["result"]["serverInfo"]["name"] == "invisible-computer-use"
    client.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    tools = [t["name"] for t in client.request("tools/list")["result"]["tools"]]
    assert tools[:13] == WINDOW2, tools
    # a window on the current Space (the driver's placement facts), preferring a titled one
    drv = client.p.args and os.environ.get("ICU_DRIVER") or os.environ.get("NATIVE_CUA_BIN", "cua-driver")
    lw = json.loads(subprocess.run([drv, *(["--socket", os.environ["NATIVE_CUA_SOCKET"]] if os.environ.get("NATIVE_CUA_SOCKET") else []), "call", "list_windows", "{}"], capture_output=True, text=True).stdout)
    cands = [w for w in lw["windows"] if w.get("on_current_space") and w.get("is_on_screen") and w.get("title")]
    if not cands: pytest.skip("no titled window on the current Space")
    # some apps expose no AX window (terminals); take the first that resolves, honestly degraded ones are skipped
    for w in cands[:6]:
        win = {"app": w.get("bundle_id") or w["app_name"], "id": w["window_id"]}
        s1, err = call(client, "get_window_state", {"window": win, "include_text": True, "include_screenshot": False})
        if not err: break
        assert s1.get("code") == "accessibilityError", s1
    else:
        pytest.skip("no window on the current Space resolves through accessibility")
    tree = s1["accessibility"]["tree"]
    assert s1["window"]["id"] == w["window_id"] and s1["accessibilityRevision"] == 1
    s2, err = call(client, "get_window_state", {"window": win, "include_text": True, "include_screenshot": False})
    assert not err and s2["accessibilitySnapshotCount"] == 2
    assert s2["accessibility"]["tree"].startswith(("There has been no change in the accessibility tree for ", "The following is a diff from the previous accessibility tree"))
    # the gate on a harmless input: one wheel notch at a point inside the window
    answers = []
    def accept(params):
        answers.append(params); return {"action": "accept", "content": {"persist": "session"}}
    scroll = {"window": win, "scrollX": 0, "scrollY": 40, "x": 20, "y": 20}
    r, err = call(client, "scroll", scroll, on_elicit=accept)
    assert len(answers) == 1, "one elicitation for the first input on this app"
    assert answers[0]["message"].startswith("Allow Computer Use to use ") and answers[0]["_meta"]["codex_approval_kind"] == "mcp_tool_call"
    # past the gate the driver answers for itself (a delivery result, or its own refusal such as
    # `background_unavailable` on an Electron window, which surfaces with `driver_code` kept)
    assert (r or {}).get("code") not in ("appApprovalRequired", "appNotAllowed"), r
    if err: assert r.get("driver_code"), r
    r2, err2 = call(client, "scroll", scroll, on_elicit=accept)
    assert len(answers) == 1, "the session approval is remembered"
    assert (r2 or {}).get("code") not in ("appApprovalRequired", "appNotAllowed"), r2
    # ids the model sees route to driver elements; an id that was never in a state is refused with our code
    r3, err = call(client, "click", {"window": win, "element_index": 999_999})
    assert err and r3["code"] == "staleElementIndex", r3
    host, err = call(client, "host_state", {})
    assert not err and host["name"] == "host_interactive"
