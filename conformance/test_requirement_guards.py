"""Guards for requirements whose evidence is shipped text or the MCP result shape rather than
desktop input: the patched driver's click guidance, the text content `icu mcp` writes for every
tool result, and the product README's tool table against the served tool list. Offline; the
driver behind `icu` is the isolated transport fixture, never a real driver or desktop."""
import json, os, re, subprocess
from pathlib import Path
import pytest
from adapters import target_from_env

ROOT = Path(__file__).resolve().parents[1]
ICU = Path(os.environ.get("ICU_BIN", ROOT / "impl/target/debug/icu"))
README = ROOT / "README.md"


def icu_mcp(tmp_path, requests):
    """One `icu mcp` session over the fixture driver; returns the responses by id."""
    assert ICU.is_file(), f"required binary absent: {ICU}; build impl workspace"
    env = {k: v for k, v in os.environ.items() if not k.startswith(("ICU_", "AUDIT_DRIVER_"))}
    env.update(ICU_DRIVER=str(ROOT / "conformance/fixtures/audit_driver.py"), ICU_DRIVER_DAEMON="0", ICU_HOME=str(tmp_path),
               ICU_REQUIRE_APP_APPROVAL="0", AUDIT_DRIVER_MODE="healthy", AUDIT_DRIVER_PID=str(tmp_path / "pid"))
    init = {"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "guard", "version": "0"}}}
    lines = [init, {"jsonrpc": "2.0", "method": "notifications/initialized"}]
    lines += [{"jsonrpc": "2.0", "id": i, "method": m, "params": p} for i, (m, p) in enumerate(requests, 1)]
    p = subprocess.run([str(ICU), "mcp"], input="".join(json.dumps(l) + "\n" for l in lines),
                       capture_output=True, text=True, env=env, timeout=20)
    out = {}
    for line in p.stdout.splitlines():
        m = json.loads(line)
        if "id" in m and "method" not in m: out[m["id"]] = m
    assert set(out) >= set(range(len(requests) + 1)), (p.stdout[-600:], p.stderr[-600:])
    return out


def test_native_click_guidance_prefers_element_targets():
    """spec 06 step 1: the shipped driver's click description tells the model to prefer
    element targets and to fall back to coordinates only for non-AX surfaces."""
    drv = target_from_env()
    if drv.name != "cua-driver-native":
        pytest.xfail("only the patched native driver binary is checked here")
    p = subprocess.run(drv._argv("describe", "click"), capture_output=True, text=True, env=drv.env, timeout=20)
    assert p.returncode == 0, p.stderr[-400:]
    text = " ".join(p.stdout.split())
    assert "Prefer `element_token` over pixel coordinates" in text, text[:400]
    assert "Reach for `x, y` only when the target is a canvas" in text, text[:400]


def test_mcp_tool_results_are_written_as_text_content(tmp_path):
    """The nodeRepl.write analogue: every tools/call result reaches the MCP client transcript
    as a text content item. Success text is the JSON of structuredContent; an error's text is
    its message and the result is flagged isError."""
    r = icu_mcp(tmp_path, [("tools/call", {"name": "confirmations_policy", "arguments": {"os": "macos"}}),
                           ("tools/call", {"name": "list_windows", "arguments": {}}),
                           ("tools/call", {"name": "get_window", "arguments": {"id": 1}})])
    for rid in (1, 2):
        res = r[rid]["result"]
        assert not res.get("isError"), res
        assert [c["type"] for c in res["content"]] == ["text"], res
        text, sc = json.loads(res["content"][0]["text"]), res["structuredContent"]
        # structuredContent must be an object (Claude Code rejects a bare array): a list result is
        # wrapped as {"result": [...]} there, while the text stays the bare JSON.
        assert isinstance(sc, dict), res
        assert text == (sc["result"] if (not isinstance(text, dict) and list(sc) == ["result"]) else sc), res
    err = r[3]["result"]
    assert err["isError"] is True and [c["type"] for c in err["content"]] == ["text"], err
    assert err["content"][0]["text"].strip(), err


def test_readme_tool_table_matches_the_served_tools(tmp_path):
    """BUILD-PLAN slice 14: the product README has install, API, tool table and limits, and the
    tool table names exactly the tools `icu mcp` serves."""
    text = README.read_text(encoding="utf-8")
    heads = re.findall(r"^## (.+)$", text, re.M)
    for h in ("Install", "Use it from an MCP client", "The tool surface", "Limits"):
        assert h in heads, (h, heads)
    assert "impl/install.sh" in text and (ROOT / "impl/install.sh").is_file()
    limits = text.split("## Limits", 1)[1].split("\n## ", 1)[0]
    assert limits.count("\n- ") >= 3, limits[:300]
    table = text.split("## The tool surface", 1)[1].split("\n## ", 1)[0]
    documented = set(re.findall(r"`([a-z_0-9]+)`", "\n".join(l.split("|")[1] for l in table.splitlines() if l.startswith("| `"))))
    served = {t["name"] for t in icu_mcp(tmp_path, [("tools/list", {})])[1]["result"]["tools"]}
    assert documented == served, {"served_not_documented": sorted(served - documented),
                                  "documented_not_served": sorted(documented - served)}
