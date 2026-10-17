"""Every captured Sky observation, replayed through the shipped `icu` binary.

`golden/sky-replay/*.json` holds, per captured scenario, the AX input each Sky row renders
from and a pointer to what the real SkyComputerUseService returned. Each turn goes through
`icu window2 map` on one lineage and one session, and `accessibility.tree` must equal Sky's
text byte for byte (rules in golden/ax-diff-format.md). A new capture is a new JSON file.
A turn with `known_gap` is one our driver's data cannot reproduce: our text must equal the
pinned file it names, and must still differ from Sky's.
"""
import glob, json, os, tempfile
import pytest
from adapters import target_from_env

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FIXTURES = sorted(glob.glob(os.path.join(ROOT, "conformance", "golden", "sky-replay", "*.json")))


def expected(capture, expect):
    if "text_file" in expect:
        return open(os.path.join(capture, expect["text_file"]), encoding="utf-8").read()
    results = json.load(open(os.path.join(capture, "results.json"), encoding="utf-8"))
    text = results[expect["results_key"]]["content"][0]["text"]
    return json.loads(text)[expect["json_field"]] if "json_field" in expect else text


def sessions():
    for f in FIXTURES:
        fx = json.load(open(f, encoding="utf-8"))
        for s in fx["sessions"]:
            yield pytest.param(os.path.join(ROOT, fx["capture"]), s, id=f"{os.path.basename(f)[:-5]}:{s['name']}")


@pytest.fixture(scope="module")
def drv():
    return target_from_env()


def test_replay_fixtures_exist():
    assert FIXTURES, "no golden/sky-replay fixtures"


@pytest.mark.parametrize("capture,session", list(sessions()))
def test_captured_sky_text_replays_byte_for_byte(drv, capture, session):
    if not hasattr(drv, "_w2"):
        pytest.xfail(f"{drv.name} renders no Sky accessibility text; our layer must add it")
    state = tempfile.mkdtemp(prefix="sky-replay-")
    for turn in session["turns"]:
        doc = tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8")
        json.dump(turn["doc"], doc); doc.close()
        argv = ["map", "--gws", doc.name, "--state", state, "--text", "--no-screenshot",
                "--app", session["bundle_id"], "--lineage", session["name"]]
        if turn.get("disable_diff"): argv.append("--disable-diff")
        st = drv._w2(*argv)
        os.unlink(doc.name)
        assert "_error" not in st, st
        want = expected(capture, turn["expect"])
        got = st["accessibility"]["tree"]
        if "known_gap" in turn:
            pinned = open(os.path.join(ROOT, turn["known_gap"]["ours"]), encoding="utf-8").read()
            assert got == pinned, f"{turn['step']} drifted from its pinned known gap"
            assert got != want, f"{turn['step']} now matches Sky; remove its known_gap"
            continue
        assert got == want, f"{turn['step']}:\n--- ours ---\n{got}\n--- sky ---\n{want}"
