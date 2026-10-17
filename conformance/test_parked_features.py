"""Conformance for the parked requirements that study/audit/gaps-parked-2026-10-04 built.

Each feature is a gap the matrix recorded against raw cua-driver, so the check is written once and
run by target: `our` drives the shipped `icu` binary over the scripted fixture driver
(`fixtures/feature_driver.py`, never a real desktop) and asserts what a model sees and what the
driver was actually sent; `cua` and `native` mark each one xfail with the gap it documents (raw
cua has no per-value descriptor, no cursor in screenshots, no settle window and so on; the labels
are coverage, not observed stock-driver failures). Offline; needs `impl/target/debug/icu`.
"""
import base64, json, os, struct, subprocess, time, zlib
from pathlib import Path
import pytest
from adapters import target_from_env

ROOT = Path(__file__).resolve().parents[1]
ICU = Path(os.environ.get("ICU_BIN", ROOT / "impl/target/debug/icu"))
FIXTURE_DRIVER = ROOT / "conformance/fixtures/feature_driver.py"
BG = (200, 210, 220)


@pytest.fixture(scope="module")
def drv():
    return target_from_env()


def only_our(drv, gap):
    """These checks run against our layer; the stock drivers are the documented gap."""
    if drv.name != "our-layer":
        pytest.xfail(f"{drv.name} has no {gap}; our layer adds it")


class Session:
    """One `icu mcp` session over the fixture driver, in its own state dir."""

    def __init__(self, tmp, elicitation=None, **env):
        assert ICU.is_file(), f"required binary absent: {ICU}; build the impl workspace"
        # `elicitation`: a list of results the client answers the server's elicitation requests with, in order
        self.answers, self.elicitations = list(elicitation or []), []
        self.log = tmp / "driver.log"
        e = {k: v for k, v in os.environ.items() if not k.startswith(("ICU_", "AUDIT_DRIVER_", "FEATURE_DRIVER_"))}
        e.update(ICU_DRIVER=str(FIXTURE_DRIVER), ICU_DRIVER_DAEMON="0", ICU_HOME=str(tmp), ICU_REQUIRE_APP_APPROVAL="0",
                 FEATURE_DRIVER_LOG=str(self.log))
        e.update({k: str(v) for k, v in env.items()})
        self.p = subprocess.Popen([str(ICU), "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=e)
        self.n = 0
        self.send({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {"elicitation": {}} if elicitation else {}, "clientInfo": {"name": "parked", "version": "0"}}})
        self.read()
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, m):
        self.p.stdin.write(json.dumps(m) + "\n"); self.p.stdin.flush()

    def read(self):
        line = self.p.stdout.readline()
        assert line, "icu mcp closed: " + self.p.stderr.read()[-500:]
        return json.loads(line)

    def call(self, name, args):
        """(structured content, is_error)"""
        self.n += 1
        self.send({"jsonrpc": "2.0", "id": self.n, "method": "tools/call", "params": {"name": name, "arguments": args}})
        while True:
            m = self.read()
            if m.get("method") == "elicitation/create":
                self.elicitations.append(m["params"])
                self.send({"jsonrpc": "2.0", "id": m["id"], "result": self.answers.pop(0) if self.answers else {"action": "cancel"}})
                continue
            if m.get("id") == self.n:
                r = m["result"]
                return r.get("structuredContent"), bool(r.get("isError"))

    def sent(self, tool):
        if not self.log.exists(): return []
        rows = [json.loads(l) for l in self.log.read_text().splitlines()]
        return [r["args"] for r in rows if r["tool"] == tool]

    def close(self):
        self.p.stdin.close(); self.p.wait(timeout=10)


@pytest.fixture
def session(tmp_path):
    made = []
    def make(**env):
        home = tmp_path / f"s{len(made)}"; home.mkdir()
        s = Session(home, **env); made.append(s); return s
    yield make
    for s in made: s.close()


WINDOW = {"app": "dev.icu.fixture", "id": 7}


def png_pixels(png):
    """Decode an 8-bit non-interlaced RGB/RGBA PNG to (w, h, rows of (r, g, b) tuples). Stdlib only."""
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    pos, chunks, idat = 8, {}, b""
    while pos < len(png):
        n, t = struct.unpack(">I4s", png[pos:pos + 8]); body = png[pos + 8:pos + 8 + n]; pos += 12 + n
        if t == b"IDAT": idat += body
        else: chunks[t] = body
    w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", chunks[b"IHDR"])
    assert depth == 8 and interlace == 0 and ctype in (2, 6), (depth, ctype, interlace)
    bpp = 3 if ctype == 2 else 4
    raw, stride, rows, prev = zlib.decompress(idat), w * bpp, [], bytearray(w * bpp)
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c; pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append([tuple(line[x * bpp:x * bpp + 3]) for x in range(w)]); prev = line
    return w, h, rows


def screenshot_png(state):
    url = state["screenshots"][0]["url"]
    return base64.b64decode(url.split("base64,", 1)[1])


def drawn_pixels(state):
    w, h, rows = png_pixels(screenshot_png(state))
    return [(x, y) for y in range(h) for x in range(w) if rows[y][x] != BG]


# ---- per-value truncation descriptor (specs/00-COVERAGE.md:20) ----------------------------------

def test_a_long_value_is_clipped_and_described_with_its_range_and_count(drv, session):
    only_our(drv, "per-value truncation descriptor (cua caps element counts, not value lengths)")
    s = session(FEATURE_DRIVER_LONG_VALUE=20000)
    state, err = s.call("get_window_state", {"window": WINDOW, "include_text": True, "include_screenshot": False})
    assert not err, state
    tree = state["accessibility"]["tree"]
    assert "v" * 4097 not in tree, "values past the budget are not sent to the model"
    assert "[value truncated: showing characters 0..4096 of 20000" in tree
    (t,) = state["_value_truncations"]
    assert t["available_range"] == {"start": 0, "end": 4096} and t["count"] == 20000
    assert isinstance(t["element_index"], int)
    # the whole value is one argument away, and then there is no descriptor
    full, _ = s.call("get_window_state", {"window": WINDOW, "include_text": True, "include_screenshot": False, "max_value_chars": 0})
    assert "v" * 20000 in full["accessibility"]["tree"] and "_value_truncations" not in full


def test_values_inside_the_budget_carry_no_descriptor(drv, session):
    only_our(drv, "per-value truncation descriptor")
    s = session()
    state, _ = s.call("get_window_state", {"window": WINDOW, "include_text": True, "include_screenshot": False})
    assert "_value_truncations" not in state


# ---- agent cursor in screenshots + post_action_sleep_ms (specs/10-linux-mechanism.md:63) ----------

def test_the_agent_cursor_appears_in_the_next_screenshot_at_the_last_click(drv, session):
    only_our(drv, "agent cursor composited into screenshots (cua draws it in a separate overlay window)")
    s = session(ICU_POST_ACTION_SLEEP_MS=0)
    first, _ = s.call("get_window_state", {"window": WINDOW, "include_text": False})
    assert drawn_pixels(first) == [], "no pointer action yet: the capture is untouched"
    _, err = s.call("click", {"window": WINDOW, "x": 200, "y": 100})
    assert not err
    shot, _ = s.call("get_window_state", {"window": WINDOW, "include_text": False})
    px = drawn_pixels(shot)
    assert px, "the cursor is in the returned pixels"
    scale = 1568 / 1024
    tx, ty = 200 * scale, 100 * scale
    assert abs(min(p[0] for p in px) - tx) <= 2 and abs(min(p[1] for p in px) - ty) <= 2, (min(p[0] for p in px), min(p[1] for p in px), tx, ty)
    height = max(p[1] for p in px) - min(p[1] for p in px) + 1
    assert 9 <= height <= 14, f"mouse_size_px defaults to 12, drew {height}"
    assert shot["_cursor_in_screenshot"]["size_px"] == 12
    # the screenshot's dimensions did not change, so every pixel-to-point conversion still holds
    assert png_pixels(screenshot_png(shot))[:2] == (1568, 1176)
    big, _ = s.call("get_window_state", {"window": WINDOW, "include_text": False, "mouse_size_px": 40})
    bp = drawn_pixels(big)
    assert max(p[1] for p in bp) - min(p[1] for p in bp) + 1 >= 33
    off, _ = s.call("get_window_state", {"window": WINDOW, "include_text": False, "mouse_size_px": 0})
    assert drawn_pixels(off) == [], "a per-call mouse_size_px of 0 draws nothing"
    z = session(ICU_MOUSE_SIZE_PX=0, ICU_POST_ACTION_SLEEP_MS=0)
    z.call("get_window_state", {"window": WINDOW, "include_text": False})
    z.call("click", {"window": WINDOW, "x": 200, "y": 100})
    assert drawn_pixels(z.call("get_window_state", {"window": WINDOW, "include_text": False})[0]) == [], "ICU_MOUSE_SIZE_PX=0 disables it"


def test_post_action_sleep_ms_spaces_actions_and_screenshots(drv, session):
    only_our(drv, "post_action_sleep_ms settle window")
    s = session(ICU_POST_ACTION_SLEEP_MS=300)
    s.call("get_window_state", {"window": WINDOW, "include_text": False})
    click = {"window": WINDOW, "x": 10, "y": 10}
    a, _ = s.call("click", click)
    assert "_waited_ms" not in a, "nothing before the first action"
    t = time.monotonic()
    b, _ = s.call("click", click)
    assert time.monotonic() - t >= 0.25 and b["_waited_ms"] >= 250, b
    t = time.monotonic()
    shot, _ = s.call("get_window_state", {"window": WINDOW, "include_text": False})
    assert time.monotonic() - t >= 0.25 and shot["_waited_ms"] >= 250, "a screenshot also waits out the last action"
    # the default is Sky's 100 ms, and 0 turns it off
    d = session()
    d.call("get_window_state", {"window": WINDOW, "include_text": False})
    d.call("click", click)
    assert d.call("click", click)[0]["_waited_ms"] >= 80
    z = session(ICU_POST_ACTION_SLEEP_MS=0)
    z.call("get_window_state", {"window": WINDOW, "include_text": False})
    z.call("click", click)
    assert "_waited_ms" not in z.call("click", click)[0]


# ---- two-phase prepare / approve / commit (specs/15-native-app-tools.md:36) -----------------------

def _actionable(s):
    """An element id of the latest window state that prepare_action accepts."""
    state, _ = s.call("get_window_state", {"window": WINDOW, "include_text": True})
    # the first read renders the tree; a second one is the "no change" line, so keep the ids
    ids = [int(l.strip("\t~+ ").split(" ")[0]) for l in state["accessibility"]["tree"].splitlines() if l.strip("\t~+ ")[:1].isdigit()]
    s.ids = ids or getattr(s, "ids", [])
    for i in s.ids:
        plan, err = s.call("prepare_action", {"tool": "click", "arguments": {"window": WINDOW, "element_index": i}})
        if not err: return i, plan
    raise AssertionError("no actionable element in the fixture state")


def test_prepare_approve_commit_runs_an_irreversible_action_once_and_only_after_the_user_says_so(drv, session):
    only_our(drv, "two-phase prepare/approve/commit (cua dispatches every action in one call)")
    s = session(elicitation=[{"action": "accept", "content": {"persist": "once"}}, {"action": "cancel"}, {"action": "accept", "content": {"persist": "once", "text": "edited by the user"}}])
    el, plan = _actionable(s)
    assert plan["tool"] == "click" and plan["app"]["id"] == "dev.icu.fixture" and plan["target"]["element_index"] == el
    assert s.sent("click") == [], "prepare performs nothing"
    # the server asks the human at commit; the answer is accept
    out, err = s.call("commit_action", {"prepared_id": plan["prepared_id"]})
    assert not err, out
    assert len(s.elicitations) == 1 and s.elicitations[0]["message"].startswith("Let Computer Use click")
    assert out["outcome"] in ("committed", "outcome_uncertain")
    assert len(s.sent("click")) == 1
    again, err = s.call("commit_action", {"prepared_id": plan["prepared_id"]})
    assert err and again["code"] == "planUnavailable" and again["reason"] == "already_committed"
    assert len(s.sent("click")) == 1, "at most once"
    # a cancelled approval is final, in the model's words
    _, plan2 = _actionable(s)
    out, err = s.call("commit_action", {"prepared_id": plan2["prepared_id"]})
    assert err and out["code"] == "approvalCanceled" and "Do not ask the user to approve it again or retry" in out["message"]
    assert len(s.sent("click")) == 1
    retry, err = s.call("commit_action", {"prepared_id": plan2["prepared_id"]})
    assert err and retry["code"] == "approvalCanceled", "retrying does not reopen it"
    # the user edits the text in the dialog
    typed, _ = s.call("prepare_action", {"tool": "type_text", "arguments": {"window": WINDOW, "text": "original"}})
    assert typed["editable"] == "text" and typed["text"] == "original"
    out, err = s.call("commit_action", {"prepared_id": typed["prepared_id"]})
    assert not err and out["edited"] is True
    assert s.sent("type_text")[-1]["text"] == "edited by the user"
    # a model cannot approve for itself
    refused, err = s.call("approve_prepared", {"prepared_id": plan2["prepared_id"]})
    assert err and "not callable over MCP" in refused["message"]


def test_a_prepared_action_for_a_vanished_element_is_refused_not_redirected(drv, session):
    only_our(drv, "two-phase prepare/approve/commit")
    s = session(elicitation=[{"action": "accept", "content": {"persist": "once"}}])
    _, plan = _actionable(s)
    # the window is re-observed and the element no longer exists under that label: the fixture's
    # tree is fixed, so expire the plan the way time does instead
    import glob
    (path,) = glob.glob(str(Path(s.log).parent / "state" / "prepared" / "*.json")) or glob.glob(str(Path(s.log).parent / ".icu" / "state" / "prepared" / "*.json"))
    p = json.loads(Path(path).read_text()); p["plan"]["expires_unix"] = 1; Path(path).write_text(json.dumps(p))
    out, err = s.call("commit_action", {"prepared_id": plan["prepared_id"]})
    assert err and out["code"] == "planUnavailable" and out["reason"] == "expired"
    assert s.sent("click") == []
    bad, err = s.call("commit_action", {"prepared_id": "../../etc/passwd"})
    assert err and bad["code"] == "planUnavailable"


# ---- unified getState / getApp facade (specs/14-approval-confirmations.md:46) ---------------------

def test_get_state_is_one_call_inventory_and_get_app_selects_by_name_id_or_path(drv, session):
    only_our(drv, "the unified cua.getState()/getApp() facade (cua-driver exposes the separate per-window tools)")
    s = session()
    st, err = s.call("get_state", {})
    assert not err, st
    assert {a["id"] for a in st["apps"]} == {"dev.icu.fixture", "dev.icu.other"}
    assert st["frontmost"] == {"app": "dev.icu.fixture", "id": 7, "title": "Doc"}
    assert st["surfaces"] == {"native_apps": True, "browsers": False, "tabs": False} and "tabs" not in st
    assert s.sent("get_window_state") == [], "an inventory observes nothing"
    for wanted in ("Fixture", "dev.icu.fixture", "/Applications/Fixture.app"):
        r, err = s.call("get_app", {"app": wanted})
        assert not err, (wanted, r)
        assert r["app"]["id"] == "dev.icu.fixture" and r["window"]["id"] == 7 and r["launched"] is False
        assert r["state"]["accessibility"]["tree"].startswith("<app_specific_instructions>") or "Window:" in r["state"]["accessibility"]["tree"]
        assert len(r["state"]["screenshots"]) == 1
    bad, err = s.call("get_app", {"app": "No Such App"})
    assert err and bad["code"] == "invalidApp" and "Invalid app: No Such App" in bad["message"]
    assert s.sent("launch_app") == [{"name": "No Such App"}], "an unlisted app is launched once before it is called invalid"


# ---- model, prompt and run metadata on recorded events (specs/00-COVERAGE.md:169) ----------------

def test_record_start_tags_the_recording_with_model_prompt_and_run(drv, session, tmp_path):
    only_our(drv, "run metadata on recorded events (cua action.json has tool/args/result, not model/prompt/eval)")
    s = session()
    out, err = s.call("record_start", {"output_dir": str(tmp_path / "rec"), "run": {"model": "claude-sonnet-5", "prompt": "abc", "run_id": "r-1", "eval": {"suite": "osworld"}}})
    assert not err, out
    (sent,) = s.sent("start_recording")
    m = sent["run_metadata"]
    assert (m["model"], m["run_id"], m["eval"]) == ("claude-sonnet-5", "r-1", {"suite": "osworld"})
    assert m["prompt"] == "abc" and m["prompt_sha256"] == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    assert m["icu"]["mode"] == "attended"
    # no run, no metadata: the recording is exactly what it was before
    s2 = session()
    s2.call("record_start", {"output_dir": str(tmp_path / "rec2")})
    assert "run_metadata" not in s2.sent("start_recording")[0]
    # a host that spawns one icu per run sets it once
    s3 = session(ICU_RUN_METADATA=json.dumps({"model": "from-env", "run_id": "r-env"}))
    s3.call("record_start", {"output_dir": str(tmp_path / "rec3"), "run": {"model": "from-call"}})
    m3 = s3.sent("start_recording")[0]["run_metadata"]
    assert (m3["model"], m3["run_id"]) == ("from-call", "r-env")
    bad, is_err = s3.call("record_start", {"output_dir": str(tmp_path / "rec4"), "run": "gpt"})
    assert is_err and bad["code"] == "invalidArgument" and len(s3.sent("start_recording")) == 1


# ---- "close enough" cursor targeting (specs/00-COVERAGE.md:91) --------------------------------

def test_close_enough_thresholds_reach_the_drivers_cursor_motion(drv, session):
    only_our(drv, "close-enough cursor targeting (the stock glide always runs to the end)")
    s = session()
    s.call("cursor", {"enabled": True})
    assert "close_enough_distance_px" not in s.sent("set_agent_cursor_motion")[-1], "off unless asked for"
    s.call("cursor", {"close_enough_distance_px": 24, "close_enough_progress": 0.9})
    m = s.sent("set_agent_cursor_motion")[-1]
    assert (m["close_enough_distance_px"], m["close_enough_progress"]) == (24, 0.9)
    c = session(ICU_CLOSE_ENOUGH_PX=30)
    c.call("get_window_state", {"window": WINDOW, "include_text": False})
    c.call("click", {"window": WINDOW, "x": 5, "y": 5})
    assert c.sent("set_agent_cursor_motion")[-1]["close_enough_distance_px"] == 30, "the configured default rides every session"


# ---- consult memory before broad discovery (specs/03-memory-skysight-vs-history.md:49) -----------

def test_the_server_tells_the_model_to_consult_memory_before_broad_discovery(drv, session):
    only_our(drv, "the Skysight-style memory tools and their consult-first policy")
    s = session()
    s.send({"jsonrpc": "2.0", "id": 900, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "p", "version": "0"}}})
    while True:
        m = s.read()
        if m.get("id") == 900: break
    text = m["result"]["instructions"]
    assert "Before you write, name, format, send or schedule anything for the user, check memory" in text and "memory_list" in text and "memory_grep" in text
    assert "even when it never mentions earlier work" in text and "If the prompt already gives the exact value" in text
    assert "or memory is empty or has nothing relevant, continue with normal discovery" in text
    s.send({"jsonrpc": "2.0", "id": 901, "method": "tools/list"})
    while True:
        m = s.read()
        if m.get("id") == 901: break
    tools = {t["name"]: t["description"] for t in m["result"]["tools"]}
    for name in ("memory_list", "memory_prior", "memory_grep"):
        assert "before broad desktop discovery" in tools[name], name


def test_the_first_result_for_an_app_memory_knows_carries_a_memory_prior_in_the_object_a_client_reads(drv, session, tmp_path):
    """The prior is only useful where the model reads it. Claude Code shows the model `structuredContent` and drops the
    text blocks, so it has to be there (study/audit/gaps-memory-4-2026-10-05). Real `icu mcp`, scripted driver, a real folder."""
    only_our(drv, "the memory prior on the first result for an app")
    mem = tmp_path / "memory"; mem.mkdir(); mem.chmod(0o700)
    body = ("---\ntitle: Renamed the planning document\ndescription: Naming of a document\napplications: [dev.icu.fixture]\n---\n\n## Memory summary\n\n"
            "The user renamed a document to ACME-2026-plan.\n\n### Relevant prior context\n\nNone noted.\n\n### Important non-obvious context about the user\n\n"
            "Documents are named with the ACME- prefix.\n\n## Recording summary\n\nA few minutes.\n\n## Citations\n\n- events.jsonl\n")
    f = mem / "2026-10-03T12-15-00-abcd-10min-renamed-the-planning-document.md"; f.write_text(body); f.chmod(0o600)
    s = session(ICU_MEMORY_ROOT=mem)
    first, err = s.call("list_windows", {})
    assert not err and isinstance(first.get("result"), list), first
    prior = first.get("memory_prior", "")
    assert "Memory has 1 earlier entry about dev.icu.fixture" in prior and "Renamed the planning document" in prior and "memory_grep" in prior, first
    again, _ = s.call("list_windows", {})
    assert "memory_prior" not in again, "once per app"
    hits, _ = s.call("memory_grep", {"needle": "dev.icu.fixture"})
    assert "ACME-2026-plan" in hits["result"][0]["text"], "the grep the prior names returns the entry"
    other = session(ICU_MEMORY_ROOT=tmp_path / "empty")
    assert "memory_prior" not in other.call("list_windows", {})[0], "no folder, no prior"


# ---- out-of-window persistent-state predicates (specs/12-verification-recording.md:61) -----------

def test_file_and_process_predicates_check_state_outside_any_window(drv, session, tmp_path):
    only_our(drv, "file/process predicates (cua verify_state checks one authorized window)")
    target = tmp_path / "exported.txt"
    s = session(ICU_PERSISTENT_PREDICATES=1)
    ask = lambda *preds, **kw: s.call("verify_state", {"expect": list(preds), "timeout_ms": 0, **kw})[0]
    f = {"file": {"path": str(target)}}
    r = ask(f)
    assert (r["status"], r["predicates"][0]["status"]) == ("unsatisfied", "unsatisfied"), r
    assert ask({"file": {"path": str(target), "exists": False}})["status"] == "satisfied", "absence is provable"
    target.write_text("hello world")
    r = ask({"file": {"path": str(target), "contains": "world", "size_bytes": 11}})
    assert r["status"] == "satisfied" and r["stable"] is True, r
    assert "hello" not in json.dumps(r), "contents are never returned"
    import hashlib
    assert ask({"file": {"path": str(target), "sha256": hashlib.sha256(b"hello world").hexdigest()}})["status"] == "satisfied"
    assert ask({"file": {"path": str(target), "sha256": "0" * 64}})["status"] == "unsatisfied"
    assert ask({"process": {"pid": 2_000_000_000, "exists": False}})["status"] == "satisfied"
    # no window was named and the driver was never asked: these escape the one-window rule on purpose
    assert s.sent("verify_state") == []
    # mixed with a window predicate: the driver only sees its own, positions are the caller's
    r = ask({"window": {"exists": True}}, f, **{"window": WINDOW})
    assert [p["index"] for p in r["predicates"]] == [0, 1], r
    assert s.sent("verify_state")[-1]["expect"] == [{"window": {"exists": True}}]
    assert r["status"] == "satisfied"


def test_persistent_predicates_are_opt_in_and_confined_to_the_roots(drv, session, tmp_path):
    only_our(drv, "file/process predicates")
    target = tmp_path / "x.txt"; target.write_text("x")
    off = session()
    r, _ = off.call("verify_state", {"expect": [{"file": {"path": str(target)}}], "timeout_ms": 0})
    assert r["status"] == "unknown" and r["predicates"][0]["unknown_reason"] == "capability_disabled", r
    scoped = session(ICU_PERSISTENT_PREDICATES=1, ICU_PREDICATE_ROOTS=str(tmp_path))
    assert scoped.call("verify_state", {"expect": [{"file": {"path": str(target)}}], "timeout_ms": 0})[0]["status"] == "satisfied"
    out = scoped.call("verify_state", {"expect": [{"file": {"path": "/etc/hosts"}}], "timeout_ms": 0})[0]
    assert out["predicates"][0]["unknown_reason"] == "invalid_predicate", "outside the roots is not probed"
    esc = scoped.call("verify_state", {"expect": [{"file": {"path": f"{tmp_path}/../../etc/hosts"}}], "timeout_ms": 0})[0]
    assert esc["predicates"][0]["unknown_reason"] == "invalid_predicate", "`..` does not escape"
