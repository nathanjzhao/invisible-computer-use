//! Tests for the full-desktop facade (`desktop.rs`): each drives the real `Layer` over a fake driver
//! that serves the desktop scope (`get_desktop_state`, `get_screen_size`, desktop-scope `click`,
//! `drag`, `move_cursor`) and records what it was asked. `native` is whether that driver advertises
//! the hold, path and modifier fields the patched native driver adds.
use super::*;
use crate::cua::{Cua, CuaError};
use crate::desktop::{parse_scroll, MAX_SCROLL_CLICKS};
use crate::tests::{layer_with_log, CallLog, FakeCua};
use std::sync::Arc;

const SCREEN: (u32, u32) = (800, 600);
const BG: [u8; 3] = [200, 210, 220];

struct Desktop { inner: FakeCua, native: bool, with_scope: bool, size_tool_fails: bool }

impl Cua for Desktop {
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        match tool {
            "get_screen_size" if self.size_tool_fails => {
                self.inner.calls.lock().unwrap().push((tool.into(), args));
                Err(CuaError { message: "$DISPLAY variable not set and no value was provided explicitly".into(), structured: None })
            }
            "get_screen_size" => { self.inner.calls.lock().unwrap().push((tool.into(), args)); Ok(json!({"width": SCREEN.0, "height": SCREEN.1, "scale_factor": 1.0})) }
            "get_desktop_state" => {
                self.inner.calls.lock().unwrap().push((tool.into(), args));
                Ok(json!({"platform": "linux", "display": "primary", "screenshot_png_b64": pointer::solid_png_b64(SCREEN.0, SCREEN.1, BG),
                          "screenshot_width": SCREEN.0, "screenshot_height": SCREEN.1, "screen_width": SCREEN.0, "screen_height": SCREEN.1, "scale_factor": 1.0}))
            }
            "click" | "drag" | "move_cursor" | "press_key" | "type_text" | "scroll" => { self.inner.calls.lock().unwrap().push((tool.into(), args.clone())); Ok(json!({"scope": "desktop", "path": "xtest_desktop", "effect": "unverifiable"})) }
            _ => self.inner.call(tool, args),
        }
    }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> {
        let mut names = self.inner.tool_names()?;
        if self.with_scope { names.extend(["get_desktop_state", "get_screen_size", "click", "drag", "move_cursor", "press_key", "type_text", "scroll"].map(String::from)); }
        names.sort(); names.dedup();
        Ok(names)
    }
    fn tool_schema(&self, tool: &str) -> Option<Value> {
        let mut props = json!({"x": {}});
        if self.with_scope && matches!(tool, "press_key" | "type_text" | "scroll") { props["scope"] = json!({}); }
        let native_fields: &[&str] = match tool { "click" => &["hold_ms"], "drag" => &["path", "hold"], "move_cursor" => &["modifier"], "press_key" => &["hold_ms"], "scroll" => &["modifier"], _ => &[] };
        if self.native { for f in native_fields { props[*f] = json!({}); } }
        Some(json!({"properties": props}))
    }
}

fn layer(native: bool, mutate: impl FnOnce(&mut Config)) -> (Layer, tempfile::TempDir, CallLog) {
    let (l, tmp, log) = layer_with_log(false, false);
    let mut cfg = l.cfg.clone();
    mutate(&mut cfg);
    let inner = FakeCua { calls: log.clone(), locked: false };
    (Layer::with_cua(cfg, Box::new(Desktop { inner, native, with_scope: true, size_tool_fails: false })), tmp, log)
}

fn driver_calls(log: &CallLog, tool: &str) -> Vec<Value> {
    log.lock().unwrap().iter().filter(|(t, _)| t == tool).map(|(_, a)| a.clone()).collect()
}

/// The driver `drag` calls that hold a button across calls (a drag handle step).
fn holds(log: &CallLog) -> Vec<Value> {
    driver_calls(log, "drag").into_iter().filter(|a| a.get("hold").is_some()).collect()
}

fn err(r: Result<Value, ToolError>) -> ToolError { r.expect_err("expected a refusal") }

fn decode(v: &Value) -> image::RgbaImage {
    use base64::Engine;
    let url = v["screenshots"][0]["url"].as_str().unwrap();
    let b64 = url.split_once("base64,").unwrap().1;
    image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).unwrap().to_rgba8()
}

fn changed(img: &image::RgbaImage) -> Vec<(u32, u32)> {
    (0..img.height()).flat_map(|y| (0..img.width()).map(move |x| (x, y))).filter(|&(x, y)| { let p = img.get_pixel(x, y); [p[0], p[1], p[2]] != BG }).collect()
}

// ---- click ------------------------------------------------------------------------------------

#[test]
fn a_click_reaches_the_driver_in_desktop_scope_with_sky_arguments_translated() {
    let (l, _t, log) = layer(false, |_| {});
    let r = l.dispatch("desktop_click", &json!({"x": 120.4, "y": 80.6, "click_count": 2, "mouse_button": "r", "key": "Control_L"})).unwrap();
    let sent = driver_calls(&log, "click");
    assert_eq!(sent.len(), 1);
    assert_eq!((sent[0]["scope"].as_str(), sent[0]["x"].as_i64(), sent[0]["y"].as_i64(), sent[0]["count"].as_i64(), sent[0]["button"].as_str()), (Some("desktop"), Some(120), Some(81), Some(2), Some("right")));
    assert_eq!(sent[0]["modifier"], json!(["ctrl"]), "the held chord is the driver's modifier list");
    assert!(sent[0].get("pid").is_none() && sent[0].get("window_id").is_none(), "no window argument on a desktop click");
    assert_eq!(r["_desktop_pointer"], json!({"x": 120.0, "y": 81.0}));
}

#[test]
fn a_click_defaults_to_one_left_click_and_sends_no_hold_or_chord() {
    let (l, _t, log) = layer(true, |_| {});
    l.dispatch("desktop_click", &json!({"x": 5, "y": 6})).unwrap();
    let sent = &driver_calls(&log, "click")[0];
    assert_eq!((sent["count"].as_i64(), sent["button"].as_str()), (Some(1), Some("left")));
    assert!(sent.get("hold_ms").is_none() && sent.get("modifier").is_none(), "{sent}");
}

#[test]
fn a_click_duration_is_the_native_hold_and_a_stock_driver_is_refused_not_silently_given_a_quick_click() {
    let (native, _t, nlog) = layer(true, |_| {});
    native.dispatch("desktop_click", &json!({"x": 5, "y": 6, "duration": 250})).unwrap();
    assert_eq!(driver_calls(&nlog, "click")[0]["hold_ms"], 250);
    let (stock, _t2, slog) = layer(false, |_| {});
    let e = err(stock.dispatch("desktop_click", &json!({"x": 5, "y": 6, "duration": 250})));
    assert_eq!(e.code(), Some(CODE_DESKTOP_UNAVAILABLE));
    assert_eq!(e.structured["needs"], "click.hold_ms");
    assert!(driver_calls(&slog, "click").is_empty(), "nothing was sent");
}

// ---- drag -------------------------------------------------------------------------------------

#[test]
fn a_two_point_drag_is_the_contracts_own_drag() {
    let (l, _t, log) = layer(false, |_| {});
    l.dispatch("desktop_drag", &json!({"path": [{"x": 10, "y": 20}, {"x": 300, "y": 400}]})).unwrap();
    let sent = &driver_calls(&log, "drag")[0];
    assert_eq!((sent["scope"].as_str(), sent["from_x"].as_i64(), sent["from_y"].as_i64(), sent["to_x"].as_i64(), sent["to_y"].as_i64()), (Some("desktop"), Some(10), Some(20), Some(300), Some(400)));
    assert!(sent.get("path").is_none(), "a stock driver is never handed a field it does not know: {sent}");
}

#[test]
fn a_multi_point_drag_sends_every_point_in_order_to_a_native_driver() {
    let (l, _t, log) = layer(true, |_| {});
    let path = json!([{"x": 10, "y": 10}, {"x": 200, "y": 10}, {"x": 200, "y": 300}, {"x": 20, "y": 300}]);
    let r = l.dispatch("desktop_drag", &json!({"path": path, "key": "Shift_L"})).unwrap();
    let sent = &driver_calls(&log, "drag")[0];
    assert_eq!(sent["path"], path, "every corner, in order");
    assert_eq!((sent["from_x"].as_i64(), sent["from_y"].as_i64(), sent["to_x"].as_i64(), sent["to_y"].as_i64()), (Some(10), Some(10), Some(20), Some(300)), "ends repeat the path's first and last point");
    assert_eq!(sent["modifier"], json!(["shift"]));
    assert_eq!(r["_desktop_pointer"], json!({"x": 20.0, "y": 300.0}), "the pointer rests at the last point");
}

#[test]
fn a_multi_point_drag_on_a_stock_driver_is_refused_whole_not_cut_to_its_ends() {
    let (l, _t, log) = layer(false, |_| {});
    let e = err(l.dispatch("desktop_drag", &json!({"path": [{"x": 1, "y": 1}, {"x": 50, "y": 50}, {"x": 90, "y": 10}]})));
    assert_eq!(e.code(), Some(CODE_DESKTOP_UNAVAILABLE));
    assert_eq!(e.structured["needs"], "drag.path");
    assert!(driver_calls(&log, "drag").is_empty());
    let e = err(l.dispatch("desktop_drag", &json!({"path": [{"x": 1, "y": 1}, {"x": 50, "y": 50}], "key": "Control_L"})));
    assert_eq!(e.code(), Some(CODE_DESKTOP_UNAVAILABLE), "a held key on a stock drag would be dropped too");
}

// ---- move -------------------------------------------------------------------------------------

#[test]
fn a_move_is_a_desktop_scope_move_cursor_and_a_held_key_needs_the_native_driver() {
    let (l, _t, log) = layer(true, |_| {});
    l.dispatch("desktop_move", &json!({"x": 40, "y": 50})).unwrap();
    l.dispatch("desktop_move", &json!({"x": 41, "y": 51, "key": "Alt_L"})).unwrap();
    let sent = driver_calls(&log, "move_cursor");
    assert_eq!((sent[0]["scope"].as_str(), sent[0]["x"].as_i64(), sent[0]["y"].as_i64()), (Some("desktop"), Some(40), Some(50)));
    assert!(sent[0].get("modifier").is_none());
    assert_eq!(sent[1]["modifier"], json!(["alt"]));
    let (stock, _t2, slog) = layer(false, |_| {});
    stock.dispatch("desktop_move", &json!({"x": 1, "y": 2})).unwrap();
    let e = err(stock.dispatch("desktop_move", &json!({"x": 1, "y": 2, "key": "Alt_L"})));
    assert_eq!(e.code(), Some(CODE_DESKTOP_UNAVAILABLE));
    assert_eq!(driver_calls(&slog, "move_cursor").len(), 1, "only the plain move went out");
}

// ---- refusals before the driver acts ------------------------------------------------------------

#[test]
fn a_point_off_the_screen_is_refused_on_every_verb_and_nothing_is_sent() {
    let (l, _t, log) = layer(true, |_| {});
    for (tool, args) in [
        ("desktop_click", json!({"x": SCREEN.0, "y": 5})),
        ("desktop_click", json!({"x": 5, "y": SCREEN.1})),
        ("desktop_click", json!({"x": -1, "y": 5})),
        ("desktop_move", json!({"x": 9999, "y": 5})),
        ("desktop_drag", json!({"path": [{"x": 5, "y": 5}, {"x": 5, "y": 700}]})),
        ("desktop_drag", json!({"path": [{"x": 5, "y": 5}, {"x": 50, "y": 50}, {"x": 801, "y": 1}]})),
    ] {
        let e = err(l.dispatch(tool, &args));
        assert_eq!((e.code(), e.structured["reason"].as_str()), (Some(CODE_INVALID_ARGUMENT), Some("point_off_screen")), "{tool} {args}");
    }
    for t in ["click", "drag", "move_cursor"] { assert!(driver_calls(&log, t).is_empty(), "{t} must not have been called"); }
    // the last pixel of the screen is on it
    l.dispatch("desktop_click", &json!({"x": SCREEN.0 - 1, "y": SCREEN.1 - 1})).unwrap();
    assert_eq!(driver_calls(&log, "click").len(), 1);
}

#[test]
fn malformed_arguments_are_refused_before_the_driver_and_before_any_approval() {
    let (l, _t, log) = layer(true, |c| c.require_app_approval = true);
    for (tool, args) in [("desktop_click", json!({"x": "a", "y": 1})), ("desktop_click", json!({"x": 1, "y": 1, "click_count": 9})),
                         ("desktop_drag", json!({"path": [{"x": 1, "y": 1}]})), ("desktop_move", json!({"x": 1})), ("desktop_click", json!({"x": 1, "y": 1, "key": ""}))] {
        assert_eq!(err(l.dispatch(tool, &args)).code(), Some(CODE_INVALID_ARGUMENT), "{tool} {args}");
    }
    assert!(log.lock().unwrap().iter().all(|(t, _)| !matches!(t.as_str(), "click" | "drag" | "move_cursor")));
}

#[test]
fn a_driver_without_the_desktop_scope_is_refused_with_a_typed_error() {
    let (l, tmp, log) = layer_with_log(false, false);
    let cfg = l.cfg.clone();
    let _keep = tmp;
    let inner = FakeCua { calls: log.clone(), locked: false };
    let l = Layer::with_cua(cfg, Box::new(Desktop { inner, native: true, with_scope: false, size_tool_fails: false }));
    let e = err(l.dispatch("desktop_screenshot", &json!({})));
    assert_eq!(e.code(), Some(CODE_DESKTOP_UNAVAILABLE));
    assert_eq!(err(l.dispatch("desktop_click", &json!({"x": 1, "y": 1}))).code(), Some(CODE_DESKTOP_UNAVAILABLE));
}

// ---- the gate ---------------------------------------------------------------------------------

#[test]
fn the_whole_desktop_is_one_approval_and_an_unapproved_desktop_gets_no_input() {
    let (l, _t, log) = layer(true, |c| c.require_app_approval = true);
    let e = err(l.dispatch("desktop_click", &json!({"x": 5, "y": 5})));
    assert_eq!(e.code(), Some(CODE_APP_APPROVAL_REQUIRED));
    assert_eq!(e.structured["app"], desktop::DESKTOP_APP_ID);
    assert!(e.structured.get("elicitation").is_some(), "the elicitation is raised for the human");
    for tool in ["desktop_drag", "desktop_move"] {
        let args = if tool == "desktop_drag" { json!({"path": [{"x": 1, "y": 1}, {"x": 2, "y": 2}]}) } else { json!({"x": 1, "y": 1}) };
        assert_eq!(err(l.dispatch(tool, &args)).code(), Some(CODE_APP_APPROVAL_REQUIRED), "{tool}");
    }
    assert!(driver_calls(&log, "click").is_empty() && driver_calls(&log, "drag").is_empty() && driver_calls(&log, "move_cursor").is_empty());
    l.approve_app(&json!({"app": desktop::DESKTOP_APP_ID, "persist": "session"})).unwrap();
    l.dispatch("desktop_click", &json!({"x": 5, "y": 5})).unwrap();
    assert_eq!(driver_calls(&log, "click").len(), 1);
}

#[test]
fn the_operator_can_deny_the_desktop_outright() {
    let (l, _t, log) = layer(true, |c| c.denied_apps.push(desktop::DESKTOP_APP_ID.into()));
    assert_eq!(err(l.dispatch("desktop_click", &json!({"x": 5, "y": 5}))).code(), Some(CODE_APP_NOT_ALLOWED));
    assert!(driver_calls(&log, "click").is_empty());
}

#[test]
fn a_stopped_session_refuses_desktop_input() {
    let (l, _t, log) = layer(true, |_| {});
    l.dispatch("stop_session", &json!({})).unwrap();
    assert_eq!(err(l.dispatch("desktop_click", &json!({"x": 5, "y": 5}))).code(), Some(CODE_USER_STOPPED_SESSION));
    assert!(driver_calls(&log, "click").is_empty());
}

// ---- screenshot, cursor, settle ------------------------------------------------------------------

#[test]
fn the_screenshot_is_the_whole_desktop_with_no_window_argument() {
    let (l, _t, log) = layer(false, |_| {});
    let s = l.dispatch("desktop_screenshot", &json!({})).unwrap();
    assert_eq!((s["target"].as_str(), s["screen"]["width"].as_u64(), s["screen"]["height"].as_u64()), (Some("desktop"), Some(800), Some(600)));
    let img = decode(&s);
    assert_eq!((img.width(), img.height()), SCREEN);
    assert_eq!(s["screenshots"][0]["width"], 800);
    assert!(s.get("_cursor_in_screenshot").is_none(), "no desktop action yet: no cursor to draw");
    assert!(changed(&img).is_empty());
    assert_eq!(driver_calls(&log, "get_desktop_state").len(), 1);
}

#[test]
fn the_agent_cursor_is_drawn_into_the_screenshot_at_the_last_desktop_pixel() {
    let (l, _t, _log) = layer(true, |c| c.cursor = Some(true));
    l.dispatch("desktop_click", &json!({"x": 300, "y": 200})).unwrap();
    let s = l.dispatch("desktop_screenshot", &json!({})).unwrap();
    assert_eq!(s["_cursor_in_screenshot"], json!({"x": 300.0, "y": 200.0, "size_px": 12}));
    let px = changed(&decode(&s));
    assert!(!px.is_empty());
    let (min_x, min_y) = (px.iter().map(|p| p.0).min().unwrap(), px.iter().map(|p| p.1).min().unwrap());
    assert!((299..=301).contains(&min_x) && (199..=201).contains(&min_y), "the tip is at the click pixel: {min_x},{min_y}");
    // a drag moves it to the path's last point
    l.dispatch("desktop_drag", &json!({"path": [{"x": 10, "y": 10}, {"x": 50, "y": 60}, {"x": 400, "y": 450}]})).unwrap();
    assert_eq!(l.dispatch("desktop_screenshot", &json!({})).unwrap()["_cursor_in_screenshot"]["x"], 400.0);
    // a per-call size and 0 to turn it off
    assert_eq!(l.dispatch("desktop_screenshot", &json!({"mouse_size_px": 30})).unwrap()["_cursor_in_screenshot"]["size_px"], 30);
    assert!(l.dispatch("desktop_screenshot", &json!({"mouse_size_px": 0})).unwrap().get("_cursor_in_screenshot").is_none());
}

#[test]
fn the_cursor_position_survives_to_the_next_process() {
    let (a, tmp, _log) = layer(true, |c| c.cursor = Some(true));
    a.dispatch("desktop_move", &json!({"x": 123, "y": 321})).unwrap();
    let cfg = a.cfg.clone();
    let _keep = tmp;
    let b = Layer::with_cua(cfg, Box::new(Desktop { inner: FakeCua { calls: Arc::new(Mutex::new(vec![])), locked: false }, native: true, with_scope: true, size_tool_fails: false }));
    assert_eq!(b.dispatch("desktop_screenshot", &json!({})).unwrap()["_cursor_in_screenshot"]["x"], 123.0, "one-shot `icu call` processes share the pointer through the state dir");
}

#[test]
fn desktop_actions_and_screenshots_share_the_settle_window() {
    let (l, _t, _log) = layer(true, |c| c.features.post_action_sleep_ms = 120);
    let first = l.dispatch("desktop_click", &json!({"x": 5, "y": 5})).unwrap();
    assert!(first.get("_waited_ms").is_none(), "nothing precedes the first action");
    let shot = l.dispatch("desktop_screenshot", &json!({})).unwrap();
    assert!(shot["_waited_ms"].as_u64().unwrap() >= 80, "the screenshot waited out the click's window: {shot}");
    let second = l.dispatch("desktop_click", &json!({"x": 6, "y": 6, "post_action_sleep_ms": 0})).unwrap();
    assert!(second.get("_waited_ms").is_none() || second["_waited_ms"].as_u64().unwrap() < 120);
    let third = l.dispatch("desktop_move", &json!({"x": 7, "y": 7})).unwrap();
    assert!(third.get("_waited_ms").is_none(), "post_action_sleep_ms 0 opened no window");
}

#[test]
fn the_tool_list_carries_the_eight_desktop_verbs_with_no_window_property() {
    let defs = tool_defs();
    for name in desktop::DESKTOP_TOOLS {
        let d = defs.iter().find(|d| d["name"] == name).unwrap_or_else(|| panic!("{name} is not a listed tool"));
        assert!(d["inputSchema"]["properties"].get("window").is_none(), "{name} takes no window");
    }
    assert_eq!(defs.iter().find(|d| d["name"] == "desktop_drag").unwrap()["inputSchema"]["required"], json!(["path"]));
}

#[test]
fn on_pure_wayland_the_screen_size_comes_from_the_capture_when_get_screen_size_cannot_answer() {
    // measured on sway: get_screen_size reads the X11 root window and fails with "$DISPLAY variable not set"
    let (l, tmp, log) = layer_with_log(false, false);
    let cfg = l.cfg.clone();
    let _keep = tmp;
    let l = Layer::with_cua(cfg, Box::new(Desktop { inner: FakeCua { calls: log.clone(), locked: false }, native: true, with_scope: true, size_tool_fails: true }));
    l.dispatch("desktop_click", &json!({"x": 10, "y": 20})).unwrap();
    assert_eq!(driver_calls(&log, "click").len(), 1, "the click went through");
    assert!(!driver_calls(&log, "get_desktop_state").is_empty(), "the size was read from the capture");
    // the bounds check still holds against the size that came from the capture
    let e = err(l.dispatch("desktop_click", &json!({"x": SCREEN.0, "y": 5})));
    assert_eq!(e.structured["reason"], "point_off_screen");
    assert_eq!(driver_calls(&log, "click").len(), 1);
}

// ---- press_key, type_text, scroll, drag_handle ------------------------------------------------------

#[test]
fn a_key_chord_is_the_last_name_pressed_with_the_names_before_it_held() {
    let (l, _t, log) = layer(true, |_| {});
    l.dispatch("desktop_press_key", &json!({"key": "Control_L + Shift_L+a"})).unwrap();
    l.dispatch("desktop_press_key", &json!({"key": "Return"})).unwrap();
    let sent = driver_calls(&log, "press_key");
    assert_eq!((sent[0]["scope"].as_str(), sent[0]["key"].as_str()), (Some("desktop"), Some("a")));
    assert_eq!(sent[0]["modifiers"], json!(["ctrl", "shift"]));
    assert_eq!(sent[1]["key"], "Return");
    assert!(sent[1].get("modifiers").is_none() && sent[1].get("hold_ms").is_none(), "{}", sent[1]);
    assert!(sent[0].get("pid").is_none() && sent[0].get("window_id").is_none(), "no window argument");
}

#[test]
fn a_key_duration_is_the_native_hold_and_a_stock_driver_is_refused_not_given_a_quick_tap() {
    let (native, _t, nlog) = layer(true, |_| {});
    native.dispatch("desktop_press_key", &json!({"key": "space", "duration": 700})).unwrap();
    assert_eq!(driver_calls(&nlog, "press_key")[0]["hold_ms"], 700);
    let (stock, _t2, slog) = layer(false, |_| {});
    stock.dispatch("desktop_press_key", &json!({"key": "space"})).unwrap();
    let e = err(stock.dispatch("desktop_press_key", &json!({"key": "space", "duration": 700})));
    assert_eq!((e.code(), e.structured["needs"].as_str()), (Some(CODE_DESKTOP_UNAVAILABLE), Some("press_key.hold_ms")));
    assert_eq!(driver_calls(&slog, "press_key").len(), 1, "only the plain press went out");
}

#[test]
fn a_malformed_key_press_or_text_is_refused_before_the_driver() {
    let (l, _t, log) = layer(true, |c| c.require_app_approval = true);
    for (tool, args) in [("desktop_press_key", json!({})), ("desktop_press_key", json!({"key": ""})), ("desktop_press_key", json!({"key": "a", "duration": 10001})),
                         ("desktop_press_key", json!({"key": 5})), ("desktop_type_text", json!({})), ("desktop_type_text", json!({"text": ""})), ("desktop_type_text", json!({"text": 5}))] {
        assert_eq!(err(l.dispatch(tool, &args)).code(), Some(CODE_INVALID_ARGUMENT), "{tool} {args}");
    }
    assert!(driver_calls(&log, "press_key").is_empty() && driver_calls(&log, "type_text").is_empty());
}

#[test]
fn typed_text_reaches_the_driver_whole_in_desktop_scope() {
    let (l, _t, log) = layer(false, |_| {});
    let r = l.dispatch("desktop_type_text", &json!({"text": "héllo\nworld"})).unwrap();
    let sent = driver_calls(&log, "type_text");
    assert_eq!((sent[0]["scope"].as_str(), sent[0]["text"].as_str()), (Some("desktop"), Some("héllo\nworld")));
    assert_eq!(r["_typed_chars"], 11);
    assert!(sent[0].get("pid").is_none() && sent[0].get("window_id").is_none() && sent[0].get("element_index").is_none());
}

#[test]
fn a_keyboard_verb_needs_a_driver_whose_key_tool_has_a_desktop_scope() {
    let (l, tmp, log) = layer_with_log(false, false);
    let cfg = l.cfg.clone();
    let _keep = tmp;
    let l = Layer::with_cua(cfg, Box::new(Desktop { inner: FakeCua { calls: log.clone(), locked: false }, native: true, with_scope: false, size_tool_fails: false }));
    for (tool, args) in [("desktop_press_key", json!({"key": "a"})), ("desktop_type_text", json!({"text": "a"}))] {
        assert_eq!(err(l.dispatch(tool, &args)).code(), Some(CODE_DESKTOP_UNAVAILABLE), "{tool}");
    }
    assert!(driver_calls(&log, "press_key").is_empty() && driver_calls(&log, "type_text").is_empty());
}

#[test]
fn scroll_distance_is_whole_wheel_clicks_and_the_direction_spellings_are_skys() {
    assert_eq!(parse_scroll(&json!({"direction": "d"})).unwrap().clicks, 3, "the default is 120 pixels");
    for (pixels, clicks) in [(1.0, 1), (40.0, 1), (41.0, 2), (400.0, 10), (1e6, MAX_SCROLL_CLICKS)] {
        assert_eq!(parse_scroll(&json!({"direction": "up", "pixels": pixels})).unwrap().clicks, clicks, "{pixels}");
    }
    for (given, want) in [("u", "up"), ("DOWN", "down"), ("l", "left"), ("Right", "right")] {
        assert_eq!(parse_scroll(&json!({"direction": given})).unwrap().direction, want);
    }
    for bad in [json!({}), json!({"direction": "sideways"}), json!({"direction": "up", "pixels": 0}), json!({"direction": "up", "pixels": -3}),
                json!({"direction": "up", "pixels": "far"}), json!({"direction": "up", "x": 5}), json!({"direction": "up", "y": 5})] {
        assert_eq!(parse_scroll(&bad).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT), "{bad}");
    }
}

#[test]
fn a_scroll_goes_to_its_origin_else_where_the_pointer_is_else_the_middle_of_the_screen() {
    let (l, _t, log) = layer(true, |_| {});
    let first = l.dispatch("desktop_scroll", &json!({"direction": "down"})).unwrap();
    let sent = driver_calls(&log, "scroll");
    assert_eq!((sent[0]["scope"].as_str(), sent[0]["x"].as_i64(), sent[0]["y"].as_i64(), sent[0]["direction"].as_str(), sent[0]["amount"].as_i64()), (Some("desktop"), Some(400), Some(300), Some("down"), Some(3)));
    assert_eq!(first["_scroll_clicks"], 3);
    l.dispatch("desktop_move", &json!({"x": 100, "y": 150})).unwrap();
    l.dispatch("desktop_scroll", &json!({"direction": "u", "pixels": 80})).unwrap();
    l.dispatch("desktop_scroll", &json!({"direction": "right", "x": 600, "y": 500})).unwrap();
    let sent = driver_calls(&log, "scroll");
    assert_eq!((sent[1]["x"].as_i64(), sent[1]["y"].as_i64(), sent[1]["amount"].as_i64()), (Some(100), Some(150), Some(2)), "the pointer's last place");
    assert_eq!((sent[2]["x"].as_i64(), sent[2]["y"].as_i64(), sent[2]["direction"].as_str()), (Some(600), Some(500), Some("right")));
    assert!(sent[0].get("modifier").is_none());
}

#[test]
fn a_scroll_under_a_held_key_needs_the_native_driver_and_an_origin_off_the_screen_is_refused() {
    let (native, _t, nlog) = layer(true, |_| {});
    native.dispatch("desktop_scroll", &json!({"direction": "up", "key": "Control_L"})).unwrap();
    assert_eq!(driver_calls(&nlog, "scroll")[0]["modifier"], json!(["ctrl"]));
    let e = err(native.dispatch("desktop_scroll", &json!({"direction": "up", "x": SCREEN.0, "y": 1})));
    assert_eq!(e.structured["reason"], "point_off_screen");
    assert_eq!(driver_calls(&nlog, "scroll").len(), 1);
    let (stock, _t2, slog) = layer(false, |_| {});
    stock.dispatch("desktop_scroll", &json!({"direction": "down"})).unwrap();
    let e = err(stock.dispatch("desktop_scroll", &json!({"direction": "down", "key": "Shift_L"})));
    assert_eq!((e.code(), e.structured["needs"].as_str()), (Some(CODE_DESKTOP_UNAVAILABLE), Some("scroll.modifier")));
    assert_eq!(driver_calls(&slog, "scroll").len(), 1);
}

#[test]
fn a_drag_handle_presses_moves_and_releases_through_the_native_hold() {
    let (l, _t, log) = layer(true, |c| c.cursor = Some(true));
    l.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 10, "y": 20})).unwrap();
    let mid = l.dispatch("desktop_drag_handle", &json!({"action": "move_to", "x": 200.4, "y": 210})).unwrap();
    assert_eq!(mid["_desktop_pointer"], json!({"x": 200.0, "y": 210.0}));
    assert_eq!(l.dispatch("desktop_screenshot", &json!({})).unwrap()["_cursor_in_screenshot"]["x"], 200.0, "the screenshot between steps shows where the held button is");
    l.dispatch("desktop_drag_handle", &json!({"action": "move_to", "x": 300, "y": 310})).unwrap();
    l.dispatch("desktop_drag_handle", &json!({"action": "end"})).unwrap();
    let sent = holds(&log);
    let seen: Vec<(String, Option<i64>, Option<i64>)> = sent.iter().map(|s| (s["hold"].as_str().unwrap().to_string(), s["to_x"].as_i64(), s["to_y"].as_i64())).collect();
    assert_eq!(seen, [("start".into(), Some(10), Some(20)), ("move_to".into(), Some(200), Some(210)), ("move_to".into(), Some(300), Some(310)), ("end".into(), Some(300), Some(310))]);
    assert!(driver_calls(&log, "click").is_empty() && driver_calls(&log, "drag").len() == sent.len(), "a handle is not a one-shot drag");
    // after end a new handle may start
    l.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 1, "y": 1})).unwrap();
}

#[test]
fn a_drag_handle_used_out_of_order_is_refused_and_nothing_is_sent() {
    let (l, _t, log) = layer(true, |_| {});
    for action in ["move_to", "end"] {
        let e = err(l.dispatch("desktop_drag_handle", &json!({"action": action, "x": 5, "y": 5})));
        assert_eq!((e.code(), e.structured["reason"].as_str()), (Some(CODE_INVALID_ARGUMENT), Some("drag_not_started")), "{action}");
    }
    assert!(holds(&log).is_empty());
    l.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 5, "y": 5})).unwrap();
    let e = err(l.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 6, "y": 6})));
    assert_eq!(e.structured["reason"], "drag_already_started");
    assert_eq!(holds(&log).len(), 1, "the second start never reached the driver");
    for bad in [json!({}), json!({"action": "drop"}), json!({"action": "start"}), json!({"action": "start", "x": 1}), json!({"action": "move_to", "x": "a", "y": 1})] {
        assert_eq!(err(l.dispatch("desktop_drag_handle", &bad)).code(), Some(CODE_INVALID_ARGUMENT), "{bad}");
    }
}

#[test]
fn a_drag_handle_point_off_the_screen_is_refused_before_the_button_goes_down_or_moves() {
    let (l, _t, log) = layer(true, |_| {});
    let e = err(l.dispatch("desktop_drag_handle", &json!({"action": "start", "x": SCREEN.0, "y": 5})));
    assert_eq!(e.structured["reason"], "point_off_screen");
    assert!(holds(&log).is_empty());
    l.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 5, "y": 5})).unwrap();
    let e = err(l.dispatch("desktop_drag_handle", &json!({"action": "move_to", "x": 5, "y": -1})));
    assert_eq!(e.structured["reason"], "point_off_screen");
    assert_eq!(holds(&log).len(), 1);
}

#[test]
fn a_stock_driver_has_no_drag_handle_and_is_refused_before_anything_is_pressed() {
    let (l, _t, log) = layer(false, |_| {});
    let e = err(l.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 5, "y": 5})));
    assert_eq!((e.code(), e.structured["needs"].as_str()), (Some(CODE_DESKTOP_UNAVAILABLE), Some("drag.hold")));
    assert!(holds(&log).is_empty());
    assert!(!l.handle_path().exists(), "a refused start leaves no handle behind");
}

/// The env the layer reads to decide the driver is on native Wayland; only a layer marked one-shot looks at it, so no other test is affected.
static WAYLAND_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn a_drag_handle_the_driver_would_lose_with_the_call_is_refused_and_nothing_is_pressed() {
    // the rule: one call, its own driver, native Wayland (the held button is that process's virtual pointer)
    assert!(desktop::handle_start_is_lost(true, false, true));
    for (one_shot, shared, wayland) in [(false, false, true), (true, true, true), (true, false, false)] {
        assert!(!desktop::handle_start_is_lost(one_shot, shared, wayland), "{one_shot} {shared} {wayland}");
    }
    let _env = WAYLAND_ENV.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("WAYLAND_DISPLAY", "wayland-test");
    std::env::set_var("CUA_DRIVER_RS_ENABLE_WAYLAND", "1");
    let (l, _t, log) = layer(true, |_| {});
    l.set_one_shot();
    let e = err(l.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 5, "y": 5})));
    assert_eq!((e.code(), e.structured["needs"].as_str()), (Some(CODE_DESKTOP_UNAVAILABLE), Some("driver_session")));
    assert!(holds(&log).is_empty() && !l.handle_path().exists(), "nothing pressed, no handle recorded");
    // the same call over a shared driver, or from a session (`icu mcp`, not marked one-shot), goes through
    let (shared, _t2, log2) = layer(true, |c| c.socket = Some("/tmp/a-running-driver.sock".into()));
    shared.set_one_shot();
    shared.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 5, "y": 5})).expect("a shared driver keeps the button");
    assert_eq!(holds(&log2).len(), 1);
    let (session, _t3, log3) = layer(true, |_| {});
    session.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 5, "y": 5})).expect("a session keeps the button");
    assert_eq!(holds(&log3).len(), 1);
    // without the Wayland opt-in the driver is on X11, where the press lives in the server
    std::env::remove_var("CUA_DRIVER_RS_ENABLE_WAYLAND");
    let (x11, _t4, log4) = layer(true, |_| {});
    x11.set_one_shot();
    x11.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 5, "y": 5})).expect("X11 keeps the press in the server");
    assert_eq!(holds(&log4).len(), 1);
    std::env::remove_var("WAYLAND_DISPLAY");
}

#[test]
fn a_drag_handle_survives_to_the_next_process_and_ending_it_is_never_refused() {
    let (a, tmp, _log) = layer(true, |c| c.require_app_approval = true);
    a.approve_app(&json!({"app": desktop::DESKTOP_APP_ID, "persist": "session"})).unwrap();
    a.dispatch("desktop_drag_handle", &json!({"action": "start", "x": 5, "y": 5})).unwrap();
    // the next `icu call` process: same state dir, the session now stopped
    let cfg = a.cfg.clone();
    let _keep = tmp;
    let log2: CallLog = Arc::new(std::sync::Mutex::new(vec![]));
    let b = Layer::with_cua(cfg, Box::new(Desktop { inner: FakeCua { calls: log2.clone(), locked: false }, native: true, with_scope: true, size_tool_fails: false }));
    b.dispatch("stop_session", &json!({})).unwrap();
    assert_eq!(err(b.dispatch("desktop_drag_handle", &json!({"action": "move_to", "x": 9, "y": 9}))).code(), Some(CODE_USER_STOPPED_SESSION), "moving is refused on a stopped session");
    b.dispatch("desktop_drag_handle", &json!({"action": "end"})).expect("letting go is always allowed");
    assert_eq!(holds(&log2).len(), 1);
    assert_eq!(holds(&log2)[0]["hold"], "end");
    assert!(!b.handle_path().exists());
}

#[test]
fn the_new_verbs_are_gated_like_the_rest_and_a_stopped_session_refuses_them() {
    let (l, _t, log) = layer(true, |c| c.require_app_approval = true);
    for (tool, args) in [("desktop_press_key", json!({"key": "a"})), ("desktop_type_text", json!({"text": "a"})), ("desktop_scroll", json!({"direction": "up"})), ("desktop_drag_handle", json!({"action": "start", "x": 1, "y": 1}))] {
        assert_eq!(err(l.dispatch(tool, &args)).code(), Some(CODE_APP_APPROVAL_REQUIRED), "{tool}");
    }
    for t in ["press_key", "type_text", "scroll", "drag"] { assert!(driver_calls(&log, t).is_empty(), "{t}"); }
    l.approve_app(&json!({"app": desktop::DESKTOP_APP_ID, "persist": "session"})).unwrap();
    l.dispatch("desktop_type_text", &json!({"text": "ok"})).unwrap();
    l.dispatch("stop_session", &json!({})).unwrap();
    for (tool, args) in [("desktop_press_key", json!({"key": "a"})), ("desktop_type_text", json!({"text": "a"})), ("desktop_scroll", json!({"direction": "up"}))] {
        assert_eq!(err(l.dispatch(tool, &args)).code(), Some(CODE_USER_STOPPED_SESSION), "{tool}");
    }
    assert_eq!(driver_calls(&log, "type_text").len(), 1);
}

struct Reporting { inner: Desktop, platform: &'static str }
impl Cua for Reporting {
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        let mut r = self.inner.call(tool, args)?;
        if tool == "health_report" { r["platform"] = json!(self.platform); }
        Ok(r)
    }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> { self.inner.tool_names() }
    fn tool_schema(&self, tool: &str) -> Option<Value> { self.inner.tool_schema(tool) }
}

#[test]
fn the_windows_key_is_denied_on_a_windows_driver_and_ordinary_on_a_linux_one() {
    let desktop = |platform: &'static str| {
        let (l, tmp, log) = layer_with_log(false, false);
        let cfg = l.cfg.clone();
        let inner = Desktop { inner: FakeCua { calls: log.clone(), locked: false }, native: true, with_scope: true, size_tool_fails: false };
        (Layer::with_cua(cfg, Box::new(Reporting { inner, platform })), tmp, log)
    };
    let (win, _t, wlog) = desktop("windows");
    let e = err(win.dispatch("desktop_press_key", &json!({"key": "Super_L+r"})));
    assert_eq!((e.code(), e.structured["hard_denied"].clone()), (Some(CODE_APP_NOT_ALLOWED), json!(true)));
    assert!(driver_calls(&wlog, "press_key").is_empty(), "denied before anything was sent");
    let (linux, _t2, llog) = desktop("linux");
    linux.dispatch("desktop_press_key", &json!({"key": "Super_L+d"})).expect("Sky's own example chord is ordinary on Linux");
    assert_eq!(driver_calls(&llog, "press_key")[0]["modifiers"], json!(["super"]));
}
