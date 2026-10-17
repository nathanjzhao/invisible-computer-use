"""Codex-equivalence conformance tests. Run: CONFORMANCE_TARGET=cua pytest conformance/
Layer 1 (format): the target's tool surface + output shapes + error codes + diff format
match the pinned golden/ formats extracted from real Codex.
These are xfail against raw cua-driver (documents exactly what our layer must add);
they must pass against OurLayerAdapter once built, and are validated by SkyGoldenAdapter.
"""
import os, json, pytest
import subprocess
from adapters import target_from_env

GOLDEN = os.path.join(os.path.dirname(__file__), "golden")
WINDOW2_TOOLS = ["list_windows","get_window","list_apps","launch_app","get_window_state",
                 "click","press_key","type_text","scroll","set_value","drag",
                 "perform_secondary_action","activate_window"]
ERROR_CODES = ["appNotAllowed","accessibilityError","permissionsNotGranted","permissionsPending",
               "ambiguousApp","blockedURL","screenLocked","userStoppedSession","userIntervened",
               "incompatibleClientVersion","runningApplicationNotFound"]
# spec 04 — the seven shipped packs keyed by bundle id (verbatim golden files)
APP_PACKS = {"com.tinyspeck.slackmacgap": "Slack.md", "com.spotify.client": "Spotify.md",
             "com.apple.ScreenContinuity": "iPhone Mirroring.md", "com.apple.iWork.Numbers": "Numbers.md",
             "com.apple.Music": "AppleMusic.md", "com.apple.clock": "Clock.md", "notion.id": "Notion.md"}
APP_INSTR_OPEN, APP_INSTR_CLOSE = "<app_specific_instructions>", "</app_specific_instructions>"
# Observed in the real service (golden/ax-diff-format.md, rules D1-D3 and N1)
AX_DIFF_MARKERS = ["The following is a diff from the previous accessibility tree for Window: ",
                   " with ~ and + representing changed and added elements, respectively. Removed elements are summarized by ID range.",
                   "There has been no change in the accessibility tree for Window: "]

@pytest.fixture(scope="session")
def drv(): return target_from_env()

def _tools(drv):
    # a target should expose tool discovery; cua: `list-tools`, our layer: MCP tools/list
    import subprocess
    if drv.name in ("cua-driver", "cua-driver-native"):
        argv = drv._argv("list-tools") if hasattr(drv, "_argv") else [drv.binary, "list-tools"]
        out = subprocess.run(argv, capture_output=True, text=True, env=drv.env).stdout
        return {l.split(":")[0].strip() for l in out.splitlines() if ":" in l}
    return set(getattr(drv, "tools", lambda: [])())

@pytest.mark.parametrize("tool", WINDOW2_TOOLS)
def test_tool_surface_present(drv, tool):
    tools = _tools(drv)
    if drv.name in ("cua-driver", "cua-driver-native") and tool in ("get_window","perform_secondary_action","activate_window"):
        pytest.xfail("cua names these differently / folds into invoke_menu+bring_to_front; our layer must expose the window2 name")
    assert tool in tools, f"{drv.name} missing Codex window2 tool {tool!r}"

def test_ax_diff_format_documented():
    g = open(os.path.join(GOLDEN, "ax-diff-format.md")).read()
    for m in AX_DIFF_MARKERS: assert m in g

def test_error_taxonomy_documented():
    g = open(os.path.join(GOLDEN, "error-taxonomy.md")).read()
    for c in ERROR_CODES: assert c in g

# Layer 1 behavioural checks (need a live target + fixture; skipped without CONFORMANCE_LIVE=1)
live = pytest.mark.skipif(os.environ.get("CONFORMANCE_LIVE") != "1", reason="needs live driver + fixture")

@live
def test_get_window_state_shape(drv):
    pid = int(os.environ["CONF_PID"]); wid = int(os.environ["CONF_WID"])
    st = drv.get_window_state(pid, wid, text=True)
    els = st.get("elements") or (st.get("accessibility") or {}).get("tree")
    assert els, "no accessibility tree/elements returned"

@live
def test_ax_diff_emitted_after_change(drv):
    """After a change + second get_window_state (diff on), output carries the ~/+/- diff header."""
    pid = int(os.environ["CONF_PID"]); wid = int(os.environ["CONF_WID"])
    drv.get_window_state(pid, wid, text=True)
    st2 = drv.get_window_state(pid, wid, text=True)
    tree = st2["diff"]["text"] if drv.name == "cua-driver-native" else _tree_text(st2)
    if drv.name == "cua-driver":
        pytest.xfail("cua returns full tree every turn; our layer must add the diff renderer (gap 02)")
    # The diff renderer is active iff the 2nd turn is a diff header (something
    # changed) OR the no-change short-circuit (nothing changed) — cua returns a
    # full tree in both cases, so either proves our layer added the renderer.
    assert any(m in tree for m in (AX_DIFF_MARKERS[0], AX_DIFF_MARKERS[2])), \
        f"our layer did not emit a diff-renderer output; got:\n{tree[:400]}"


def test_ourlayer_ax_diff_matches_golden_format(tmp_path):
    """Deterministic (no live app): feed axdiff two controlled snapshots and
    assert its output carries the golden headers/legend and ~/+/- markers, so
    the golden ax-diff contract gates the real implementation."""
    import subprocess, json
    cua_bin = os.environ.get("AXDIFF_CUA_BIN", os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
        "impl", "target", "debug", "axdiff-cua"))
    if not os.path.exists(cua_bin):
        pytest.fail(f"required axdiff-cua not built at {cua_bin} (cargo build --manifest-path impl/Cargo.toml)")
    t1 = {"elements": [
        {"element_index": 0, "role": "AXWindow", "label": "W", "depth": 0},
        {"element_index": 1, "parent_index": 0, "role": "AXTextField", "label": "f", "depth": 1},
        {"element_index": 2, "parent_index": 0, "role": "AXRow", "label": "old", "depth": 1}]}
    t2 = {"elements": [
        {"element_index": 0, "role": "AXWindow", "label": "W", "depth": 0},
        {"element_index": 1, "parent_index": 0, "role": "AXTextField", "label": "f", "value": "x", "depth": 1},
        {"element_index": 3, "parent_index": 0, "role": "AXButton", "label": "b", "depth": 1}]}
    p1 = tmp_path / "t1.json"; p2 = tmp_path / "t2.json"
    p1.write_text(json.dumps(t1)); p2.write_text(json.dumps(t2))
    out = subprocess.run([cua_bin, str(p1), str(p2)], capture_output=True, text=True).stdout
    assert out.startswith(AX_DIFF_MARKERS[0] + '"W"' + AX_DIFF_MARKERS[1] + "\n"), f"missing one-line header:\n{out}"
    # removed ids first, then marker-before-tabs rows (rules D2, D4)
    assert out.splitlines()[1:] == ["Removed element IDs: 2", "~\t1 text field f, Value: x", "+\t3 button b"], f"diff body:\n{out}"
    # cumulative variant
    outc = subprocess.run([cua_bin, str(p1), str(p2), "--cumulative"], capture_output=True, text=True).stdout
    assert "cumulative diff from the initial accessibility tree" in outc, f"missing cumulative header:\n{outc}"
    assert "\nRemoved element IDs: 2\n" in outc, f"missing removed-ID-range summary:\n{outc}"


# ---- spec 04: per-app instruction packs -------------------------------------

def _app_instructions(drv, bundle_id):
    fn = getattr(drv, "app_instructions", None)
    if drv.name in ("cua-driver", "cua-driver-native") or fn is None:
        pytest.xfail("cua has no per-app instruction layer / first-touch injection (gap 04); our layer must add it")
    return fn(bundle_id)

@pytest.mark.parametrize("bundle_id,pack", sorted(APP_PACKS.items()))
def test_app_instruction_pack_is_verbatim(drv, bundle_id, pack):
    """The pack our layer injects for each shipped bundle id is byte-identical to the golden file."""
    body = _app_instructions(drv, bundle_id)
    golden = open(os.path.join(GOLDEN, "app-instructions", pack), "rb").read()
    assert body is not None, f"{drv.name} has no pack for {bundle_id}"
    assert body == golden, f"pack for {bundle_id} is not the verbatim {pack}"

def test_app_instructions_absent_for_apps_without_a_pack(drv):
    for bid in ("com.apple.ActivityMonitor", "com.apple.finder", "", "COM.SPOTIFY.CLIENT"):
        assert _app_instructions(drv, bid) is None, f"unexpected pack for {bid!r}"

def _tree_text(st):
    """The accessibility text a target presents: window2 `accessibility.tree`, else cua's `tree_markdown`."""
    return (st.get("accessibility") or {}).get("tree") if st.get("accessibility") else (st.get("tree_markdown") or "")

def _first_touch_wrap(pack_bytes):
    # window_result.js: `<app_specific_instructions>\n${u}\n</app_specific_instructions>\n${s}`
    return APP_INSTR_OPEN + "\n" + pack_bytes.decode() + "\n" + APP_INSTR_CLOSE + "\n"

@live
def test_app_instructions_injected_once_per_session(drv):
    """Live: a window of a pack app (CONF_PACK_PID/CONF_PACK_WID, e.g. Clock) gets the
    verbatim pack prefixed on the FIRST get_window_state of the session and not on the
    second; a non-pack window (CONF_PID/CONF_WID) never gets the tag."""
    if drv.name in ("cua-driver", "cua-driver-native"):
        pytest.xfail("cua returns the raw tree with no app-keyed prefix (gap 04)")
    if "CONF_PACK_PID" not in os.environ:
        pytest.skip("set CONF_PACK_PID/CONF_PACK_WID/CONF_PACK_BUNDLE_ID to a resolvable pack-app window")
    ppid = int(os.environ["CONF_PACK_PID"]); pwid = int(os.environ["CONF_PACK_WID"])
    bid = os.environ["CONF_PACK_BUNDLE_ID"]
    golden = open(os.path.join(GOLDEN, "app-instructions", APP_PACKS[bid]), "rb").read()
    t1 = _tree_text(drv.get_window_state(ppid, pwid, text=True))
    assert t1.startswith(_first_touch_wrap(golden)), f"first touch not prefixed with the verbatim pack:\n{t1[:300]}"
    assert t1.count(APP_INSTR_OPEN) == 1
    t2 = _tree_text(drv.get_window_state(ppid, pwid, text=True))
    assert APP_INSTR_OPEN not in t2 and APP_INSTR_CLOSE not in t2, f"second touch must be plain:\n{t2[:300]}"
    # a non-pack app never carries the tag — and CONF_PID must really be a non-pack app
    pid = int(os.environ["CONF_PID"]); wid = int(os.environ["CONF_WID"])
    apps = drv.call("list_apps", {})
    conf_app = next((a for a in apps if any(w["id"] == wid for w in a["windows"])), None)
    assert conf_app is not None, f"CONF_WID {wid} not found in list_apps"
    assert conf_app["id"] not in APP_PACKS, f"CONF_PID's app {conf_app['id']} has a pack; pick a non-pack window"
    for _ in range(2):
        t = _tree_text(drv.get_window_state(pid, wid, text=True))
        assert APP_INSTR_OPEN not in t


# ---- specs 01/19: window2 observation surface (golden/tool-surface-window2.md) ----

FIXTURES = os.path.join(os.path.dirname(__file__), "fixtures")
W2_GAP_FIELDS = ["accessibility.focused_element", "accessibility.selected_text", "accessibility.document_text", "screenshots"]

def _assert_window2_state_shape(st):
    """Key-for-key check against the golden `WindowState` type; optional fields must be
    absent (never null/empty) when unavailable; our diagnostics are underscore-prefixed."""
    assert set(st) >= {"window", "screenshots", "accessibility"}, f"missing top-level keys: {sorted(st)}"
    extra = set(st) - {"window", "screenshots", "accessibility", "accessibilityRevision", "accessibilitySnapshotCount"}
    assert all(k.startswith("_") for k in extra), f"non-golden keys must be underscore diagnostics: {extra}"
    w = st["window"]
    assert isinstance(w["app"], str) and w["app"] and isinstance(w["id"], int)
    assert set(w) <= {"app", "id", "title"} and ("title" not in w or (isinstance(w["title"], str) and w["title"]))
    assert isinstance(st["screenshots"], list)
    for sc in st["screenshots"]:
        assert set(sc) <= {"id", "zIndex", "url", "originX", "originY", "width", "height"}
        assert isinstance(sc["id"], str) and isinstance(sc["zIndex"], int) and sc["url"].startswith("data:image/")
        for k in ("originX", "originY", "width", "height"):
            if k in sc: assert isinstance(sc[k], (int, float))
    a = st["accessibility"]
    if a is not None:
        assert set(a) <= {"tree", "focused_element", "selected_text", "selected_elements", "document_text"}
        assert isinstance(a["tree"], str) and a["tree"]
        for k in ("focused_element", "selected_text", "document_text"):
            if k in a: assert isinstance(a[k], str) and a[k]
        if "selected_elements" in a:
            assert isinstance(a["selected_elements"], list) and a["selected_elements"] and all(isinstance(x, str) for x in a["selected_elements"])

def _w2(drv):
    if drv.name in ("cua-driver", "cua-driver-native"):
        pytest.xfail("cua returns its own get_window_state shape (elements/tree_markdown/screenshot_png_b64), not the window2 WindowState (gap 01)")
    if not hasattr(drv, "map_fixture"):
        pytest.skip("target has no fixture mapping")

def test_window2_state_shape_matches_golden_on_real_fixture(drv):
    """Deterministic: a real cua Clock document -> WindowState with the golden shape; the
    selected_elements lines are lines of the presented tree, with the layer's stable ids."""
    _w2(drv)
    st = drv.map_fixture(os.path.join(FIXTURES, "clock_gws.json"), app="com.apple.clock")
    assert "_error" not in st, st
    _assert_window2_state_shape(st)
    assert st["window"] == {"app": "com.apple.clock", "id": 25198, "title": "Clock"}
    sc = st["screenshots"][0]
    assert sc["id"] == "screenshot-0" and sc["zIndex"] == 0
    assert (sc["originX"], sc["originY"], sc["width"], sc["height"]) == (352.0, 98.0, 1024.0, 768.0)
    a = st["accessibility"]
    body = a["tree"].split("</app_specific_instructions>\n", 1)[-1]
    assert body.startswith('Window: "Clock", App: Clock.\n'), body[:80]
    tree_lines = [l.lstrip("\t") for l in body.splitlines()[1:]]
    assert all(line in tree_lines for line in a["selected_elements"]), a["selected_elements"]
    fixture = json.load(open(os.path.join(FIXTURES, "clock_gws.json")))
    n_selected = sum(1 for e in fixture["elements"] if e.get("selected"))
    assert len(a["selected_elements"]) == n_selected
    # every fixture element is a row (full tree on a fresh lineage), ids from 0, except the
    # closed menus under menu bar items, which Sky does not render (rule M2)
    import re
    by_index = {e["element_index"]: e for e in fixture["elements"]}
    def under_menu_bar_item(e):
        p = by_index.get(e.get("parent_index"))
        while p is not None:
            if p["role"] == "AXMenuBarItem": return True
            p = by_index.get(p.get("parent_index"))
        return False
    shown = [e for e in fixture["elements"] if not under_menu_bar_item(e)]
    assert sum(1 for l in tree_lines if re.match(r"^\d+ ", l)) == len(shown)
    assert tree_lines[0].startswith("0 ")
    assert st["accessibilityRevision"] == 1 and st["accessibilitySnapshotCount"] == 1

def test_window2_defaults_screenshot_only_and_both_false_rejected(drv):
    _w2(drv)
    st = drv.map_fixture(os.path.join(FIXTURES, "clock_gws.json"), include_text=False)
    _assert_window2_state_shape(st)
    assert st["accessibility"] is None and len(st["screenshots"]) == 1
    assert "accessibilityRevision" not in st
    err = drv.map_fixture(os.path.join(FIXTURES, "clock_gws.json"), include_text=False, include_screenshot=False)
    assert err.get("_exit") == 2 and "get_window_state must request include_text, include_screenshot, or both" in err["_error"]
    st = drv.map_fixture(os.path.join(FIXTURES, "clock_gws.json"), include_text=True, include_screenshot=False)
    assert st["screenshots"] == [] and st["accessibility"]["tree"]

def test_window2_gaps_are_reported_not_silently_empty(drv):
    _w2(drv)
    st = drv.map_fixture(os.path.join(FIXTURES, "clock_gws.json"))
    gaps = st["_gaps"]
    for f in W2_GAP_FIELDS:
        assert any(g.startswith(f + ":") for g in gaps), f"gap for {f} not reported: {gaps}"
    for k in ("focused_element", "selected_text", "document_text"):
        assert k not in st["accessibility"], f"{k} must be absent, not empty"

@live
def test_window2_live_state_and_window_rehydration(drv):
    if drv.name in ("cua-driver", "cua-driver-native"):
        pytest.xfail("cua has no window2 WindowState / Window / ListAppsApp shapes (gap 01)")
    ppid = int(os.environ["CONF_PACK_PID"]); pwid = int(os.environ["CONF_PACK_WID"]); bid = os.environ["CONF_PACK_BUNDLE_ID"]
    st = drv.get_window_state(ppid, pwid, screenshot=True, text=True, disable_diff=True)
    assert "_error" not in st, st
    _assert_window2_state_shape(st)
    assert st["window"]["app"] == bid and st["window"]["id"] == pwid
    import base64
    png = base64.b64decode(st["screenshots"][0]["url"].split(",", 1)[1])
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    a = st["accessibility"]
    tree_lines = a["tree"].splitlines()
    for line in a.get("selected_elements", []):
        assert line in tree_lines, line
    w = drv.call("get_window", {"id": pwid})
    assert {k: v for k, v in w.items() if not k.startswith("_")} == st["window"], "get_window must rehydrate the same Window"
    missing = drv.call("get_window", {"id": 1})
    assert missing.get("_exit") == 4
    windows = drv.call("list_windows", {})
    assert isinstance(windows, list) and any(x["id"] == pwid and x["app"] == bid for x in windows)
    apps = drv.call("list_apps", {})
    assert isinstance(apps, list)
    app = next(x for x in apps if x["id"] == bid)
    assert app["isRunning"] is True and any(x["id"] == pwid for x in app["windows"])
    assert "useCount" not in app


# ---- spec 14: Confirmations Policy + approval elicitation + allowlists -----------

CONFIRMATION_MODES = ["Hand-Off Required (User Must Do It)",
                      "Always Confirm at Action-Time (Even If Pre-Approved)",
                      "Pre-Approval Works (Otherwise Treat as \u201cAlways Confirm\u201d)",
                      "No Confirmation Needed (Always Allowed)"]

def _cf(drv):
    if drv.name in ("cua-driver", "cua-driver-native"):
        pytest.xfail("cua has a tool-level risk/consent engine but no Confirmations Policy, per-app elicitation, or bundle-id allowlist (gap 14)")
    if not hasattr(drv, "confirmations"):
        pytest.skip("target has no confirmations layer")

def _golden_taxonomy():
    """Oracle parsed from the golden text: {tag: mode_number}; ranges like 8.3–8.5 expanded."""
    import re
    out, mode = {}, None
    for line in open(os.path.join(GOLDEN, "confirmations-policy.md"), encoding="utf-8").read().splitlines():
        m = re.match(r"^#### (\d)\) ", line)
        if m: mode = int(m.group(1)); continue
        m = re.search(r"\*\*\[(.+?)\]\*\*", line)
        if m and mode is not None:
            for part in m.group(1).split(","):
                part = part.strip()
                if "\u2013" in part:
                    lo, hi = part.split("\u2013"); maj, lo_min = lo.split("."); hi_min = hi.split(".")[1]
                    for k in range(int(lo_min), int(hi_min) + 1): out[f"{maj}.{k}"] = mode
                else: out[part] = mode
    return out

def test_confirmations_policy_is_verbatim(drv):
    _cf(drv)
    _, out, _ = drv.confirmations("policy")
    assert out == open(os.path.join(GOLDEN, "confirmations-policy.md"), "rb").read()
    for i, title in enumerate(CONFIRMATION_MODES, 1):
        assert f"#### {i}) {title}\n".encode() in out

def test_confirmation_mode_taxonomy_matches_golden(drv):
    """Every category tag in the golden maps to the golden's mode, verbatim titles; tags
    outside the taxonomy are mode 4 (\"Any action outside this taxonomy\")."""
    _cf(drv)
    oracle = _golden_taxonomy()
    assert len(oracle) == 26
    for tag, n in oracle.items():
        _, out, _ = drv.confirmations("mode", tag)
        assert out.decode().strip() == f"{n}) {CONFIRMATION_MODES[n-1]}", tag
    for tag in ("99", "2", "x"):
        _, out, _ = drv.confirmations("mode", tag)
        assert out.decode().strip() == f"4) {CONFIRMATION_MODES[3]}"

def test_confirmation_mode_transitions(drv):
    """Mode -> decision under no / user-authored / third-party pre-approval, for every golden tag."""
    _cf(drv)
    expected = {1: ("HandOff", "HandOff", "HandOff"), 2: ("Confirm", "Confirm", "Confirm"),
                3: ("Confirm", "Proceed", "Confirm"), 4: ("Proceed", "Proceed", "Proceed")}
    for tag, n in _golden_taxonomy().items():
        got = tuple(drv.confirmations("decide", tag, *extra)[1].decode().strip()
                    for extra in ((), ("--pre-approved", "user"), ("--pre-approved", "third-party")))
        assert got == expected[n], f"tag {tag} (mode {n}): {got}"

def test_app_approval_store_roundtrip_and_scopes(drv, tmp_path):
    """Session vs always persistence on disk in the observed formats; a new session sees
    only the global list."""
    _cf(drv)
    sess = str(tmp_path / "sessions" / "s1.toml"); alw = str(tmp_path / "ComputerUseAppApprovals.json")
    st = lambda *a: drv.confirmations("store", "--session", sess, "--always", alw, *a)[1].decode().strip()
    assert st("allowed", "com.apple.Safari") == "false"
    assert st("approve", "com.apple.Safari", "session") == "ok"
    assert st("allowed", "com.apple.Safari") == "true"
    assert open(sess, "rb").read() == b'[apps]\nallowed = ["com.apple.Safari"]\n'
    assert not os.path.exists(alw)
    assert st("approve", "org.mozilla.firefox", "always") == "ok"
    assert open(alw, "rb").read() == b'{"approvedBundleIdentifiers":["org.mozilla.firefox"]}'
    assert json.loads(st("list")) == {"session": ["com.apple.Safari"], "always": ["org.mozilla.firefox"]}
    sess2 = str(tmp_path / "sessions" / "s2.toml")
    st2 = lambda *a: drv.confirmations("store", "--session", sess2, "--always", alw, *a)[1].decode().strip()
    assert st2("allowed", "com.apple.Safari") == "false" and st2("allowed", "org.mozilla.firefox") == "true"

def test_app_approval_elicitation_shape(drv):
    """The createElicitation payload from computer-use-policy.js, and result resolution."""
    _cf(drv)
    _, out, _ = drv.confirmations("elicit", "--bundle-id", "com.apple.Safari", "--display-name", "Safari", "--risk", "medium", "--persistent", "--tool", "click")
    e = json.loads(out)
    assert e["message"] == 'Allow Computer Use to use "Safari"?'
    m = e["meta"]
    assert m["codex_approval_kind"] == "mcp_tool_call" and m["connector_id"] == "computer-use" and m["connector_name"] == "Computer Use"
    assert m["persist"] == ["session", "always"] and m["riskLevel"] == "medium" and m["tool_name"] == "click"
    assert m["tool_params"] == {"app": "com.apple.Safari"}
    assert m["tool_params_display"] == [{"name": "app", "display_name": "App", "value": "Safari"}]
    _, out, _ = drv.confirmations("elicit", "--bundle-id", "b", "--display-name", "N", "--risk", "high")
    assert json.loads(out)["meta"]["persist"] == ["session"]
    for res, want in (('{"action":"accept","_meta":{"persist":"always"}}', "always"), ('{"action":"accept"}', "session")):
        assert drv.confirmations("resolve", "--display-name", "Safari", "--result", res)[1].decode().strip() == want
    for action in ("decline", "cancel"):
        code, _, err = drv.confirmations("resolve", "--display-name", "Safari", "--result", json.dumps({"action": action}), check=False)
        assert code == 6 and "Computer Use was not approved to use Safari" in err


# ---- spec 03: Skysight-style memory (corpus prompts are the contract) --------------

CORPUS = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "codex-corpus", "mac", "ComputerUse")
SKY_HEADINGS = ["## Memory summary", "### Relevant prior context", "### Important non-obvious context about the user", "## Recording summary", "## Citations"]
SKY_NAME_RE = r"^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}-[A-Za-z]{4}-(10min|6h)-[^/]+\.md$"

def _sk(drv):
    if drv.name in ("cua-driver", "cua-driver-native"):
        pytest.xfail("cua's computer-history is an encrypted action-metadata ledger with no summarizer, cadences, or memory files (gap 03)")
    if not hasattr(drv, "skysight"):
        pytest.skip("target has no skysight layer")

def test_skysight_prompts_are_verbatim(drv):
    _sk(drv)
    for arg, f in (("summarizer", "SkysightSummarizer.md"), ("memory", "SkysightMemoryInstructions.md")):
        assert drv.skysight("prompt", arg)[1] == open(os.path.join(CORPUS, f), "rb").read()

def _split(doc):
    assert doc.startswith("---\n")
    fm, body = doc[4:].split("\n---\n", 1)
    return fm, body

def test_skysight_summary_contract_and_injection_boundary(drv, tmp_path):
    """A real-shaped event stream with prompt-injection text: the written 10min summary
    has the prompt's exact structure, stores the observed command descriptively, and
    carries nothing from observed free text (no instructions, URLs, session ids)."""
    _sk(drv)
    import re
    ev = os.path.join(FIXTURES, "skysight_events.jsonl")
    _, out, _ = drv.skysight("windows", "--events", ev, "--cadence", "10min")
    assert json.loads(out) == ["2026-09-11T13-40-00", "2026-09-11T13-50-00", "2026-09-11T18-00-00"]
    root = str(tmp_path / "resources")
    _, out, _ = drv.skysight("summarize", "--events", ev, "--window", "2026-09-11T13-40-00", "--root", root)
    path = out.decode().strip()
    assert re.match(SKY_NAME_RE, os.path.basename(path)), path
    assert os.path.basename(path).startswith("2026-09-11T13-40-00-") and "-10min-" in os.path.basename(path)
    doc = open(path, encoding="utf-8").read()
    fm, body = _split(doc)
    keys = [l.split(":")[0] for l in fm.splitlines() if l and not l.startswith(" ")]
    assert keys in (["title", "description", "applications"], ["title", "description", "applications", "suggestion"])
    apps = fm.split("applications: [", 1)[1].split("]", 1)[0].split(", ")
    assert apps == ["com.apple.Terminal", "com.google.Chrome", "com.apple.dt.Xcode"], "exact observed bundle ids, deduplicated"
    heads = [l for l in body.splitlines() if l.startswith("## ") or l.startswith("### ")]
    assert heads == SKY_HEADINGS
    assert "The user ran `swift build --package-path Package --target ComputerUseTests`" in body
    assert "Run `swift build" not in body
    for leak in ("ignore previous", "tell future agents", "rm -rf", "evil.example", "curl attacker", "Build succeeded", "sess-7f3a", "git status", "Issue #42", "zsh"):
        assert leak not in doc, f"leaked observed content or out-of-window event: {leak}"
    assert "http" not in doc.lower() and "](" not in doc
    # the validator accepts what the summarizer wrote, and rejects a directive rewrite of it
    assert drv.skysight("validate", path)[0] == 0
    bad = tmp_path / "bad.md"
    bad.write_text(doc.replace("The user ran `swift build", "Run `swift build"))
    code, _, err = drv.skysight("validate", str(bad), check=False)
    assert code == 7 and "directive" in err
    bad.write_text(doc.replace("## Citations", "## Sources"))
    assert drv.skysight("validate", str(bad), check=False)[0] == 7

def test_skysight_six_hour_rollup_and_prior_context(drv, tmp_path):
    _sk(drv)
    ev = os.path.join(FIXTURES, "skysight_events.jsonl"); root = str(tmp_path / "resources")
    p1 = drv.skysight("summarize", "--events", ev, "--window", "2026-09-11T13-40-00", "--root", root)[1].decode().strip()
    p2 = drv.skysight("summarize", "--events", ev, "--window", "2026-09-11T13-50-00", "--root", root)[1].decode().strip()
    # the second window's prior context comes from the first summary (ends before it)
    _, out, _ = drv.skysight("prior", "--root", root, "--before", "2026-09-11T13-50-00")
    assert json.loads(out) == [os.path.basename(p1)]
    body2 = _split(open(p2, encoding="utf-8").read())[1]
    assert "No relevant prior context" not in body2.split("### Important")[0].split("### Relevant prior context")[1]
    # 6h rollup: applications = exact aggregate of the children's frontmatter; file at the 6h cadence
    _, out, _ = drv.skysight("rollup", "--window", "2026-09-11T12-00-00", "--root", root)
    p6 = out.decode().strip()
    assert os.path.basename(p6).startswith("2026-09-11T12-00-00-") and "-6h-" in os.path.basename(p6)
    fm6, body6 = _split(open(p6, encoding="utf-8").read())
    apps6 = fm6.split("applications: [", 1)[1].split("]", 1)[0].split(", ")
    assert apps6 == ["com.apple.Terminal", "com.google.Chrome", "com.apple.dt.Xcode", "com.apple.Music"]
    assert "com.tinyspeck.slackmacgap" not in fm6, "the 18:05 event is outside this 6h window"
    lines6 = body6.splitlines()
    assert [l for l in lines6 if l.startswith("## ")] == [SKY_HEADINGS[0], SKY_HEADINGS[3], SKY_HEADINGS[4]]
    subs = [l for l in lines6 if l.startswith("### ")]
    assert subs[:2] == SKY_HEADINGS[1:3], "the two required subsections come first, under Memory summary"
    rec_start = lines6.index(SKY_HEADINGS[3]); cit_start = lines6.index(SKY_HEADINGS[4])
    extra = [(i, l) for i, l in enumerate(lines6) if l.startswith("### ") and l not in SKY_HEADINGS]
    assert extra and all(rec_start < i < cit_start for i, _ in extra), "child-window subsections live only under Recording summary"
    assert drv.skysight("validate", p6)[0] == 0
    # a window with no children cannot be rolled up
    assert drv.skysight("rollup", "--window", "2026-09-12T00-00-00", "--root", root, check=False)[0] != 0
    # phase2 tag
    assert drv.skysight("phase2-tag", "The user works in Xcode.")[1].decode().strip() == "The user works in Xcode. [skysight memory]"
    # grep is over bodies only (frontmatter is presentation metadata)
    _, out, _ = drv.skysight("grep", "--root", root, "swift build")
    assert os.path.basename(p1) in json.loads(out)
    _, out, _ = drv.skysight("grep", "--root", root, "You worked in")
    assert json.loads(out) == [], "frontmatter (description) is presentation metadata, not evidence"


# ---- slice 7: native diff integration (impl/native/cua-driver.patch) ---------------

def _native(drv):
    if drv.name != "cua-driver-native":
        pytest.xfail("only the patched native driver carries the structured `diff` field and `elements[].stable_id` (spec 02 steps 2-6)")

def test_native_get_window_state_advertises_disable_diff(drv):
    _native(drv)
    import subprocess
    out = subprocess.run(drv._argv("describe", "get_window_state"), capture_output=True, text=True, env=drv.env).stdout
    assert '"disable_diff"' in out and '"disableDiff"' in out and '"cumulative_diff"' in out
    assert "diff from the previous accessibility tree" in out

@live
def test_native_diff_lineage_stable_ids_and_disable_diff(drv):
    """Live on the pack-app window: turn 1 is a full tree (revision 1, snapshot 1), turn 2 a
    diff or the no-change line with the verbatim Codex strings, stable ids survive across
    turns, disableDiff yields a full tree while the lineage still advances."""
    _native(drv)
    pid = int(os.environ["CONF_PACK_PID"]); wid = int(os.environ["CONF_PACK_WID"])
    # the daemon may already hold a lineage for this window (earlier calls this session):
    # assert the CONTRACT, not fresh-lineage counts — kinds, verbatim strings, ids that
    # persist, counters that advance by exactly one per call
    s1 = drv.get_window_state(pid, wid, text=True)
    d1 = s1["diff"]
    assert d1["baseline"] == "previous" and d1["kind"] in ("full_first", "diff", "no_change", "full_fallback"), d1["kind"]
    if d1["kind"] == "full_first":
        assert d1["accessibility_revision"] == 1 and d1["accessibility_snapshot_count"] == 1
        assert d1["text"].startswith("[1] "), d1["text"][:80]
    ids1 = {e["element_index"]: e["stable_id"] for e in s1["elements"]}
    assert ids1 and all(isinstance(v, int) for v in ids1.values())
    assert len(set(ids1.values())) == len(ids1), "stable ids are unique within a snapshot"
    s2 = drv.get_window_state(pid, wid, text=True)
    d2 = s2["diff"]
    assert d2["kind"] in ("diff", "no_change", "full_fallback"), d2["kind"]
    if d2["kind"] == "diff":
        assert d2["text"].startswith(AX_DIFF_MARKERS[0]) and AX_DIFF_MARKERS[1] in d2["text"]
        assert d2["accessibility_revision"] == d1["accessibility_revision"] + 1
    elif d2["kind"] == "no_change":
        assert d2["text"].startswith(AX_DIFF_MARKERS[2])
        assert d2["accessibility_revision"] == d1["accessibility_revision"]
    assert d2["accessibility_snapshot_count"] == d1["accessibility_snapshot_count"] + 1
    ids2 = {e["element_index"]: e["stable_id"] for e in s2["elements"]}
    # stable identity: with no change, the id SET is identical; otherwise the overlap is large
    if d2["kind"] == "no_change":
        assert set(ids2.values()) == set(ids1.values()), "unchanged tree must keep exactly the same stable ids"
    else:
        assert len(set(ids2.values()) & set(ids1.values())) >= 0.8 * min(len(ids1), len(ids2))
    s3 = drv.get_window_state(pid, wid, text=True, disable_diff=True)
    d3 = s3["diff"]
    assert d3["kind"] == "full" and d3["text"].startswith("[1] ") and AX_DIFF_MARKERS[0] not in d3["text"]
    assert d3["accessibility_snapshot_count"] == d2["accessibility_snapshot_count"] + 1
    ids3 = {e["element_index"]: e["stable_id"] for e in s3["elements"]}
    assert set(ids3.values()) & set(ids2.values()), "disable_diff must not restart the lineage"
    # the token contract is untouched: element_token still names the CURRENT snapshot
    assert all(e["element_token"].endswith(f':{e["element_index"]}') for e in s3["elements"])
    # the golden's second variant: cumulative diff from the lineage's INITIAL tree
    s4 = drv.call("get_window_state", {"pid": pid, "window_id": wid, "include_screenshot": False, "include_text": True, "cumulative_diff": True})
    d4 = s4["diff"]
    assert d4["baseline"] == "initial" and d4["accessibility_snapshot_count"] == d3["accessibility_snapshot_count"] + 1
    assert d4["kind"] in ("diff", "no_change", "full_fallback")
    if d4["kind"] == "diff":
        assert d4["text"].startswith("The following is a cumulative diff from the initial accessibility tree"), d4["text"][:120]
        assert "Removed elements are summarized by ID range." in d4["text"]
        assert "- [" not in d4["text"]


# ---- Spaces / displays robustness (window2 activate_window + placement diagnostics) -----------

# ---- slice 8: host readiness (lock screen / console / display sleep / lid) ----------
def test_native_health_report_carries_host_interactive(drv):
    """health_report gains `host_interactive` (cua-driver-core::host_state) whose data says what the
    platform can observe; an interactive test host passes with screen_locked=false, and the tool
    description documents the refusal code input tools get while locked."""
    _native(drv)
    hr = drv.call("health_report", {"include": ["host_interactive"]})
    chk = next((c for c in hr.get("checks", []) if c.get("name") == "host_interactive"), None)
    assert chk, hr
    host = chk["data"]["host"]
    if chk["status"] == "fail" and host.get("screen_locked") is True:
        pytest.skip("this host is locked right now, so the check fails honestly; run again unlocked")
    assert chk["status"] == "pass" and host["screen_locked"] is False and host["on_console"] is True, chk
    assert "display_count" in chk["data"] and chk["data"]["display_count"] >= 1
    for key in ("screen_locked", "on_console", "display_asleep", "lid_closed", "active_displays"):
        assert key in host, f"{key} must be present (an explicit null when unobservable): {host}"
    desc = subprocess.run(drv._argv("describe", "health_report"), capture_output=True, text=True).stdout
    assert "host_interactive" in desc and "host_not_interactive" in desc

def test_window2_off_space_placement_is_diagnosed_from_real_listing(drv):
    """A real list_windows capture from this Mac (one display): Clock lives on Space 1 while the
    display currently shows Space 15 (off-Space, not on screen); Warp is on the current Space.
    The facade must report the per-window facts and point at activate_window."""
    _w2(drv)
    lw_path = os.path.join(FIXTURES, "list_windows_offspace.json")
    lw = json.load(open(lw_path))
    clock = next(w for w in lw["windows"] if w["app_name"] == "Clock")
    warp = next(w for w in lw["windows"] if w["app_name"] == "Warp")
    r = drv.placement_fixture(lw_path, clock["window_id"])
    assert "_error" not in r, r
    p = r["_placement"]
    assert p["on_current_space"] is False and p["is_on_screen"] is False
    assert p["space_ids"] == clock["space_ids"] and p["display_current_space"] == clock["current_space_id"]
    assert p["display_current_space"] not in p["space_ids"], "the display shows a different Space than the window's"
    assert "activate_window" in r["_hint"] and f"Space {clock['space_ids'][0]}" in r["_hint"] and f"shows Space {clock['current_space_id']}" in r["_hint"]
    r2 = drv.placement_fixture(lw_path, warp["window_id"])
    assert r2["_placement"]["on_current_space"] is True and r2["_hint"] is None
    missing = drv.placement_fixture(lw_path, 1)
    assert missing.get("_exit") == 4

@live
def test_window2_activate_window_makes_an_off_space_window_observable(drv):
    """Live: activate_window on the pack window brings it onto its display's current Space and
    a following get_window_state resolves; placement reports observable."""
    if drv.name in ("cua-driver", "cua-driver-native"):
        pytest.xfail("cua has no window2 activate_window / placement diagnostics (gap 01)")
    wid = int(os.environ["CONF_PACK_WID"]); pid = int(os.environ["CONF_PACK_PID"])
    r = drv.call("activate_window", {"id": wid})
    assert "_error" not in r, r
    assert r["id"] == wid and r["_placement"]["on_current_space"] is True and r["_placement"]["is_on_screen"] is True
    assert r["_hint"] is None
    st = drv.get_window_state(pid, wid, screenshot=False, text=True)
    assert "_error" not in st, st
    assert st["accessibility"]["tree"]
    p = drv.call("placement", {"id": wid})
    assert p["_placement"]["on_current_space"] is True
