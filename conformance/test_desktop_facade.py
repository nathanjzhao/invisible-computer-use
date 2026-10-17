"""Conformance for the Linux full-desktop facade (specs/10-linux-mechanism.md, linux-SKILL.md).

Sky's Linux client has no window model: `get_screenshot`, `click`, `drag` (a path of two or more
points), `move`, each in desktop-screenshot pixels. Our layer serves them as `desktop_screenshot`,
`desktop_click`, `desktop_drag` and `desktop_move` over cua-driver's own desktop scope. `our` drives
the shipped `icu` binary over the scripted fixture driver (`fixtures/feature_driver.py` with the
desktop scope on, never a real desktop) and asserts what a model sees and exactly what the driver
was sent; `cua` and `native` mark each one xfail with the gap it documents (raw cua has no flat
desktop verbs, no multi-point path, no hold, no cursor in its screenshot). The same cases run for
real, against a display server, in app/tests/probe_linux_desktop_facade.py. Offline; needs
`impl/target/debug/icu`.
"""
import json
import pytest
from adapters import target_from_env
from test_parked_features import Session, drawn_pixels, png_pixels, screenshot_png

SCREEN = (800, 600)


@pytest.fixture(scope="module")
def drv():
    return target_from_env()


def only_our(drv, gap):
    if drv.name != "our-layer":
        pytest.xfail(f"{drv.name} has no {gap}; our layer adds it")


@pytest.fixture
def desktop(tmp_path):
    made = []
    def make(native=False, **env):
        home = tmp_path / f"d{len(made)}"; home.mkdir()
        s = Session(home, FEATURE_DRIVER_DESKTOP=1, **({"FEATURE_DRIVER_NATIVE": 1} if native else {}), **env); made.append(s); return s
    yield make
    for s in made: s.close()


def pt(x, y): return {"x": x, "y": y}


def test_the_four_desktop_verbs_are_tools_with_no_window_argument(drv):
    only_our(drv, "flat full-desktop verbs")
    tools = drv.tools()
    for name in ("desktop_screenshot", "desktop_click", "desktop_drag", "desktop_move"):
        assert name in tools, name


def test_a_click_is_a_desktop_scope_click_in_the_drivers_vocabulary(drv, desktop):
    only_our(drv, "flat full-desktop click")
    s = desktop()
    out, err = s.call("desktop_click", {"x": 120, "y": 80, "click_count": 2, "mouse_button": "r", "key": "Control_L"})
    assert not err, out
    sent = s.sent("click")[0]
    assert (sent["scope"], sent["x"], sent["y"], sent["count"], sent["button"], sent["modifier"]) == ("desktop", 120, 80, 2, "right", ["ctrl"])
    assert "pid" not in sent and "window_id" not in sent


def test_a_click_duration_needs_the_native_driver_and_a_stock_driver_is_refused_unsent(drv, desktop):
    only_our(drv, "click duration")
    native = desktop(native=True)
    out, err = native.call("desktop_click", {"x": 5, "y": 6, "duration": 300})
    assert not err, out
    assert native.sent("click")[0]["hold_ms"] == 300
    stock = desktop()
    out, err = stock.call("desktop_click", {"x": 5, "y": 6, "duration": 300})
    assert err and out["code"] == "desktopUnavailable" and out["needs"] == "click.hold_ms", out
    assert stock.sent("click") == [], "a stock driver would drop the field and click quickly; nothing was sent"


def test_a_drag_path_goes_to_a_native_driver_whole_and_a_stock_driver_gets_only_two_points(drv, desktop):
    only_our(drv, "multi-point drag path")
    path = [pt(10, 10), pt(200, 10), pt(200, 300), pt(20, 300)]
    native = desktop(native=True)
    out, err = native.call("desktop_drag", {"path": path, "key": "Shift_L"})
    assert not err, out
    sent = native.sent("drag")[0]
    assert sent["path"] == path and sent["modifier"] == ["shift"]
    assert (sent["from_x"], sent["from_y"], sent["to_x"], sent["to_y"]) == (10, 10, 20, 300)
    stock = desktop()
    out, err = stock.call("desktop_drag", {"path": path[:2]})
    assert not err, out
    two = stock.sent("drag")[0]
    assert (two["scope"], two["from_x"], two["to_y"]) == ("desktop", 10, 10) and "path" not in two
    out, err = stock.call("desktop_drag", {"path": path})
    assert err and out["code"] == "desktopUnavailable" and out["needs"] == "drag.path", out
    assert len(stock.sent("drag")) == 1, "the longer path was refused whole, not cut to its ends"


def test_a_move_is_a_desktop_move_and_a_held_key_needs_the_native_driver(drv, desktop):
    only_our(drv, "flat full-desktop move")
    native = desktop(native=True)
    assert not native.call("desktop_move", {"x": 40, "y": 50, "key": "Alt_L"})[1]
    sent = native.sent("move_cursor")[0]
    assert (sent["scope"], sent["x"], sent["y"], sent["modifier"]) == ("desktop", 40, 50, ["alt"])
    stock = desktop()
    out, err = stock.call("desktop_move", {"x": 40, "y": 50, "key": "Alt_L"})
    assert err and out["code"] == "desktopUnavailable" and stock.sent("move_cursor") == []


@pytest.mark.parametrize("tool,args", [
    ("desktop_click", {"x": SCREEN[0], "y": 5}), ("desktop_click", {"x": 5, "y": SCREEN[1]}), ("desktop_click", {"x": -1, "y": 5}),
    ("desktop_move", {"x": 99999, "y": 5}), ("desktop_drag", {"path": [pt(5, 5), pt(50, 50), pt(801, 9)]}),
])
def test_a_point_off_the_screen_is_refused_not_clamped_to_an_edge(drv, desktop, tool, args):
    only_our(drv, "off-screen refusal (the driver clamps to the screen edge and acts there)")
    s = desktop(native=True)
    out, err = s.call(tool, args)
    assert err and out["reason"] == "point_off_screen", out
    assert s.sent("click") == [] and s.sent("drag") == [] and s.sent("move_cursor") == []


def test_malformed_arguments_are_refused_before_the_driver(drv, desktop):
    only_our(drv, "argument validation")
    s = desktop(native=True)
    for tool, args in [("desktop_click", {"x": "a", "y": 1}), ("desktop_click", {"x": 1, "y": 1, "click_count": 9}), ("desktop_drag", {"path": [pt(1, 1)]}),
                       ("desktop_move", {"x": 1}), ("desktop_click", {"x": 1, "y": 1, "key": ""}), ("desktop_click", {"x": 1, "y": 1, "mouse_button": "x"})]:
        out, err = s.call(tool, args)
        assert err and out["code"] == "invalidArgument", (tool, args, out)
    assert s.sent("click") == [] and s.sent("drag") == [] and s.sent("move_cursor") == []


def test_the_screenshot_is_the_whole_desktop_and_carries_the_agent_cursor_after_an_action(drv, desktop):
    only_our(drv, "flat full-desktop screenshot with the cursor drawn in")
    s = desktop(native=True)
    before, err = s.call("desktop_screenshot", {})
    assert not err and before["target"] == "desktop" and (before["screen"]["width"], before["screen"]["height"]) == SCREEN
    assert png_pixels(screenshot_png(before))[:2] == SCREEN
    assert drawn_pixels(before) == [] and "_cursor_in_screenshot" not in before, "no desktop action yet: no cursor"
    assert not s.call("desktop_click", {"x": 300, "y": 200})[1]
    after, err = s.call("desktop_screenshot", {"mouse_size_px": 20})
    assert not err and after["_cursor_in_screenshot"] == {"x": 300.0, "y": 200.0, "size_px": 20}
    px = drawn_pixels(after)
    assert px and min(p[0] for p in px) in range(299, 302) and min(p[1] for p in px) in range(199, 202), "the tip is at the click pixel"
    assert max(p[1] for p in px) - min(p[1] for p in px) <= 24
    assert drawn_pixels(s.call("desktop_screenshot", {"mouse_size_px": 0})[0]) == [], "0 turns it off"


def test_the_desktop_is_one_approval(drv, desktop):
    only_our(drv, "an approval for the whole desktop")
    s = desktop(native=True, ICU_REQUIRE_APP_APPROVAL=1)
    out, err = s.call("desktop_click", {"x": 5, "y": 5})
    assert err and out["code"] == "appApprovalRequired" and out["app"] == "icu.desktop", out
    assert s.sent("click") == []
    # a screenshot only observes, like get_window_state
    assert not s.call("desktop_screenshot", {})[1]


def test_the_skill_contract_is_a_resource_and_the_instructions_point_at_it(drv, desktop):
    only_our(drv, "the linux-SKILL contract for the desktop verbs")
    s = desktop()
    s.send({"jsonrpc": "2.0", "id": 900, "method": "resources/list", "params": {}})
    listed = s.read()["result"]["resources"]
    assert [r["uri"] for r in listed] == ["icu://skills/linux-desktop"]
    s.send({"jsonrpc": "2.0", "id": 901, "method": "resources/read", "params": {"uri": "icu://skills/linux-desktop"}})
    text = s.read()["result"]["contents"][0]["text"]
    for needle in ("desktop_screenshot", "desktop_click", "desktop_drag", "desktop_move", "point_off_screen", "desktopUnavailable", "path"):
        assert needle in text, needle
    s.send({"jsonrpc": "2.0", "id": 902, "method": "resources/read", "params": {"uri": "icu://nope"}})
    assert "error" in s.read()
    s.send({"jsonrpc": "2.0", "id": 903, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}})
    assert "icu://skills/linux-desktop" in s.read()["result"]["instructions"]


def test_the_keyboard_scroll_and_handle_verbs_are_tools(drv):
    only_our(drv, "flat full-desktop keyboard, scroll and drag handle")
    tools = drv.tools()
    for name in ("desktop_press_key", "desktop_scroll", "desktop_type_text", "desktop_drag_handle"):
        assert name in tools, name


def test_a_key_chord_and_typed_text_go_to_the_desktop_scope_key_tools(drv, desktop):
    only_our(drv, "flat full-desktop press_key and type_text")
    s = desktop(native=True)
    out, err = s.call("desktop_press_key", {"key": "Control_L+Shift_L+a", "duration": 400})
    assert not err, out
    sent = s.sent("press_key")[0]
    assert (sent["scope"], sent["key"], sent["modifiers"], sent["hold_ms"]) == ("desktop", "a", ["ctrl", "shift"], 400)
    out, err = s.call("desktop_type_text", {"text": "hello"})
    assert not err, out
    assert (s.sent("type_text")[0]["scope"], s.sent("type_text")[0]["text"]) == ("desktop", "hello")
    stock = desktop()
    assert not stock.call("desktop_press_key", {"key": "Return"})[1]
    out, err = stock.call("desktop_press_key", {"key": "Return", "duration": 400})
    assert err and out["code"] == "desktopUnavailable" and out["needs"] == "press_key.hold_ms", out
    assert len(stock.sent("press_key")) == 1


def test_a_scroll_is_wheel_clicks_from_an_origin_and_a_held_key_needs_the_native_driver(drv, desktop):
    only_our(drv, "flat full-desktop scroll")
    s = desktop(native=True)
    out, err = s.call("desktop_scroll", {"direction": "d", "pixels": 200, "x": 100, "y": 150, "key": "Control_L"})
    assert not err, out
    sent = s.sent("scroll")[0]
    assert (sent["scope"], sent["x"], sent["y"], sent["direction"], sent["amount"], sent["modifier"]) == ("desktop", 100, 150, "down", 5, ["ctrl"])
    assert out["_scroll_clicks"] == 5
    out, err = s.call("desktop_scroll", {"direction": "up"})
    assert not err and (s.sent("scroll")[1]["x"], s.sent("scroll")[1]["y"]) == (100, 150), "no origin: where the pointer was left"
    stock = desktop()
    out, err = stock.call("desktop_scroll", {"direction": "up", "key": "Shift_L"})
    assert err and out["code"] == "desktopUnavailable" and out["needs"] == "scroll.modifier", out
    assert stock.sent("scroll") == []


def test_a_drag_handle_holds_the_button_across_calls_and_ending_it_is_always_allowed(drv, desktop):
    only_our(drv, "flat full-desktop drag handle")
    s = desktop(native=True, ICU_REQUIRE_APP_APPROVAL=1)
    out, err = s.call("desktop_drag_handle", {"action": "start", "x": 5, "y": 5})
    assert err and out["code"] == "appApprovalRequired", out
    assert s.sent("drag") == []
    out, err = s.call("desktop_drag_handle", {"action": "end"})
    assert err and out["reason"] == "drag_not_started", out
    s2 = desktop(native=True)
    assert not s2.call("desktop_drag_handle", {"action": "start", "x": 5, "y": 5})[1]
    out, err = s2.call("desktop_drag_handle", {"action": "start", "x": 6, "y": 6})
    assert err and out["reason"] == "drag_already_started", out
    assert not s2.call("desktop_drag_handle", {"action": "move_to", "x": 60, "y": 70})[1]
    assert not s2.call("desktop_drag_handle", {"action": "end"})[1]
    seen = [(a["hold"], a["to_x"], a["to_y"]) for a in s2.sent("drag")]
    assert seen == [("start", 5, 5), ("move_to", 60, 70), ("end", 60, 70)]
    stock = desktop()
    out, err = stock.call("desktop_drag_handle", {"action": "start", "x": 5, "y": 5})
    assert err and out["code"] == "desktopUnavailable" and out["needs"] == "drag.hold", out
