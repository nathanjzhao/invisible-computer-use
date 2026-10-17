//! Tests for the parked-requirements features (study/audit/gaps-parked-2026-10-04): per-value
//! truncation, the agent cursor in screenshots, `post_action_sleep_ms`, persistent predicates,
//! run metadata, the unified facade and the two-phase action. Each drives the real `Layer` over a
//! fake driver that serves the saved real Clock document, so the shapes are the driver's.
use super::*;
use crate::cua::{Cua, CuaError};
use crate::tests::{layer_with_log, tree_ids, CallLog, FakeCua, CLOCK};
use std::sync::{Arc, Mutex};

/// The shared fake with two edits to what `get_window_state` returns: one element whose value is
/// `long_value` characters long, and a screenshot of the size the document claims, so a pointer can
/// be drawn on it.
pub(crate) struct Tweaked {
    pub inner: FakeCua,
    pub long_value: usize,
    pub screenshot: (u32, u32),
}

/// The clock document's AXRadioButton rows carry values; element 7 is one.
const LONG_ELEMENT: usize = 7;

impl Cua for Tweaked {
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        let mut r = self.inner.call(tool, args.clone())?;
        if tool == "get_window_state" {
            if self.long_value > 0 { r["elements"][LONG_ELEMENT]["value"] = json!("v".repeat(self.long_value)); }
            if r.get("screenshot_png_b64").is_some() {
                r["screenshot_png_b64"] = json!(pointer::solid_png_b64(self.screenshot.0, self.screenshot.1, [200, 210, 220]));
            }
        }
        Ok(r)
    }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> { self.inner.tool_names() }
}

pub(crate) fn tweaked_layer(long_value: usize, mutate: impl FnOnce(&mut Config)) -> (Layer, tempfile::TempDir, CallLog) {
    let (l, tmp, log) = layer_with_log(false, false);
    let mut cfg = l.cfg.clone();
    mutate(&mut cfg);
    let inner = FakeCua { calls: log.clone(), locked: false };
    // the clock fixture claims a 1568x1176 capture of a 1024x768 window
    (Layer::with_cua(cfg, Box::new(Tweaked { inner, long_value, screenshot: (1568, 1176) })), tmp, log)
}

fn state(l: &Layer, extra: Value) -> Value {
    let mut a = CLOCK(); a["include_text"] = json!(true);
    for (k, v) in extra.as_object().unwrap() { a[k] = v.clone(); }
    l.dispatch("get_window_state", &a).unwrap()
}

fn decode_first_screenshot(s: &Value) -> image::RgbaImage {
    use base64::Engine;
    let url = s["screenshots"][0]["url"].as_str().unwrap();
    let b64 = url.split_once("base64,").unwrap().1;
    image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).unwrap().to_rgba8()
}

fn changed_pixels(img: &image::RgbaImage, bg: [u8; 3]) -> Vec<(u32, u32)> {
    (0..img.height()).flat_map(|y| (0..img.width()).map(move |x| (x, y))).filter(|&(x, y)| { let p = img.get_pixel(x, y); [p[0], p[1], p[2]] != bg }).collect()
}

// ---- per-value truncation -------------------------------------------------------------------

#[test]
fn a_long_value_is_clipped_in_the_tree_and_described_with_its_range_and_count() {
    let (l, _t, _) = tweaked_layer(10_000, |_| {});
    let s = state(&l, json!({"include_screenshot": false}));
    let tree = s["accessibility"]["tree"].as_str().unwrap();
    assert!(!tree.contains(&"v".repeat(4097)), "nothing past the 4096-character budget reaches the model");
    assert!(tree.contains("[value truncated: showing characters 0..4096 of 10000"), "the row says what it lost");
    let t = s["_value_truncations"].as_array().expect("descriptor list");
    assert_eq!(t.len(), 1);
    assert_eq!((t[0]["available_range"]["start"].as_u64(), t[0]["available_range"]["end"].as_u64(), t[0]["count"].as_u64()), (Some(0), Some(4096), Some(10_000)));
    // the descriptor's id is the id the tree shows, so the model can act on that element
    let id = t[0]["element_index"].as_u64().unwrap();
    assert!(tree_ids(tree).contains(&id), "stable id {id} is a row of the tree");
}

#[test]
fn max_value_chars_is_a_per_call_and_a_configured_budget() {
    let (l, _t, _) = tweaked_layer(10_000, |_| {});
    let all = state(&l, json!({"include_screenshot": false, "max_value_chars": 0}));
    assert!(all["accessibility"]["tree"].as_str().unwrap().contains(&"v".repeat(10_000)), "0 returns the whole value");
    assert!(all.get("_value_truncations").is_none(), "nothing clipped, no descriptor");
    let small = state(&l, json!({"include_screenshot": false, "max_value_chars": 100}));
    assert_eq!(small["_value_truncations"][0]["available_range"]["end"], 100);
    let (l2, _t2, _) = tweaked_layer(10_000, |c| c.features.max_value_chars = 50);
    assert_eq!(state(&l2, json!({"include_screenshot": false}))["_value_truncations"][0]["available_range"]["end"], 50, "the configured default");
    let (l3, _t3, _) = tweaked_layer(10_000, |c| c.features.max_value_chars = 0);
    assert!(state(&l3, json!({"include_screenshot": false})).get("_value_truncations").is_none(), "configured off");
}

#[test]
fn values_inside_the_budget_are_untouched() {
    let (l, _t, _) = tweaked_layer(0, |_| {});
    assert!(state(&l, json!({"include_screenshot": false})).get("_value_truncations").is_none());
}

// ---- the agent cursor in screenshots ----------------------------------------------------------

#[test]
fn after_a_click_the_next_screenshot_shows_the_agent_cursor_where_it_landed() {
    let (l, _t, _) = tweaked_layer(0, |_| {});
    let bg = [200, 210, 220];
    // before any pointer action there is no cursor to draw
    let first = state(&l, json!({"include_text": false}));
    assert!(changed_pixels(&decode_first_screenshot(&first), bg).is_empty(), "no pointer yet, the capture is untouched");
    assert!(first.get("_cursor_in_screenshot").is_none());
    // click at window point (200, 100): the clock fixture returns 1568 px for 1024 pt, scale 1.53125
    let mut c = CLOCK(); c["x"] = json!(200.0); c["y"] = json!(100.0);
    l.dispatch("click", &c).unwrap();
    let s = state(&l, json!({"include_text": false}));
    let (tx, ty) = (200.0 * 1568.0 / 1024.0, 100.0 * 1568.0 / 1024.0);
    assert_eq!(s["_cursor_in_screenshot"]["size_px"], 12, "Sky's default size");
    let px = changed_pixels(&decode_first_screenshot(&s), bg);
    assert!(!px.is_empty(), "the cursor is in the pixels");
    let (min_x, min_y) = (px.iter().map(|p| p.0).min().unwrap() as f64, px.iter().map(|p| p.1).min().unwrap() as f64);
    assert!((min_x - tx).abs() <= 2.0 && (min_y - ty).abs() <= 2.0, "tip at ({tx:.0},{ty:.0}), drawn from ({min_x},{min_y})");
    assert!(px.iter().all(|p| (p.0 as f64) < tx + 14.0 && (p.1 as f64) < ty + 16.0), "about 12 px, not a crosshair across the window");
}

#[test]
fn mouse_size_px_zero_and_a_hidden_cursor_draw_nothing_and_a_per_call_size_wins() {
    let bg = [200, 210, 220];
    let click = |l: &Layer| { state(l, json!({"include_text": false})); let mut c = CLOCK(); c["x"] = json!(200.0); c["y"] = json!(100.0); l.dispatch("click", &c).unwrap(); };
    let (l, _t, _) = tweaked_layer(0, |c| c.features.mouse_size_px = 0);
    click(&l);
    assert!(changed_pixels(&decode_first_screenshot(&state(&l, json!({"include_text": false}))), bg).is_empty(), "configured 0");
    // a per-call size overrides the configured one
    let big = decode_first_screenshot(&state(&l, json!({"include_text": false, "mouse_size_px": 40})));
    let tall = changed_pixels(&big, bg);
    assert!(tall.iter().map(|p| p.1).max().unwrap() - tall.iter().map(|p| p.1).min().unwrap() >= 30, "40 px pointer");
    // headless: the agent cursor is off, so it is not drawn into the capture either
    let (h, _t2, _) = tweaked_layer(0, |c| c.mode = "headless".into());
    click(&h);
    assert!(changed_pixels(&decode_first_screenshot(&state(&h, json!({"include_text": false}))), bg).is_empty(), "cursor off in headless mode");
}

#[test]
fn an_element_click_puts_the_cursor_on_the_elements_center_and_it_survives_one_shot_processes() {
    let (l, _t, _) = tweaked_layer(0, |_| {});
    let s = state(&l, json!({}));
    let id = tree_ids(s["accessibility"]["tree"].as_str().unwrap())[1..].iter().copied().find(|id| {
        let mut c = CLOCK(); c["element_index"] = json!(id); l.dispatch("click", &c).is_ok()
    }).expect("an actionable element");
    // a new Layer over the same state dir (a later `icu call`) still knows where the cursor is
    let mut cfg = l.cfg.clone(); cfg.session_id = "later".into();
    let later = Layer::with_cua(cfg, Box::new(Tweaked { inner: FakeCua { calls: Arc::new(Mutex::new(vec![])), locked: false }, long_value: 0, screenshot: (1568, 1176) }));
    let shot = state(&later, json!({"include_text": true}));
    assert!(shot["_cursor_in_screenshot"].is_object(), "element {id}: {shot:?}");
}

// ---- post_action_sleep_ms ---------------------------------------------------------------------

#[test]
fn the_next_action_waits_out_post_action_sleep_ms_and_a_zero_config_does_not() {
    let (l, _t, _) = tweaked_layer(0, |c| c.features.post_action_sleep_ms = 250);
    let mut c = CLOCK(); c["x"] = json!(10.0); c["y"] = json!(10.0);
    state(&l, json!({"include_screenshot": true, "include_text": false}));
    let first = l.dispatch("click", &c).unwrap();
    assert!(first.get("_waited_ms").is_none(), "the first action has nothing to wait for");
    let t = std::time::Instant::now();
    let second = l.dispatch("click", &c).unwrap();
    assert!(t.elapsed() >= std::time::Duration::from_millis(200), "slept {:?}", t.elapsed());
    assert!(second["_waited_ms"].as_u64().unwrap() >= 200, "{second:?}");
    // a screenshot after an action waits too (Sky: before the next action, screenshot or audio op)
    let t = std::time::Instant::now();
    let shot = state(&l, json!({"include_text": false}));
    assert!(t.elapsed() >= std::time::Duration::from_millis(200) && shot["_waited_ms"].as_u64().unwrap() >= 200, "{:?}", t.elapsed());
    // a text-only observation is not a screenshot and does not wait
    l.dispatch("click", &c).unwrap();
    let t = std::time::Instant::now();
    state(&l, json!({"include_screenshot": false}));
    assert!(t.elapsed() < std::time::Duration::from_millis(150));

    let (z, _t2, _) = tweaked_layer(0, |c| c.features.post_action_sleep_ms = 0);
    state(&z, json!({"include_screenshot": true, "include_text": false}));
    z.dispatch("click", &c).unwrap();
    let t = std::time::Instant::now();
    let second = z.dispatch("click", &c).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_millis(150) && second.get("_waited_ms").is_none());
}

#[test]
fn post_action_sleep_ms_can_be_set_for_one_action() {
    let (l, _t, _) = tweaked_layer(0, |c| c.features.post_action_sleep_ms = 0);
    let mut c = CLOCK(); c["x"] = json!(10.0); c["y"] = json!(10.0);
    state(&l, json!({"include_screenshot": true, "include_text": false}));
    let mut slow = c.clone(); slow["post_action_sleep_ms"] = json!(300);
    l.dispatch("click", &slow).unwrap();
    let t = std::time::Instant::now();
    l.dispatch("click", &c).unwrap();
    assert!(t.elapsed() >= std::time::Duration::from_millis(250), "{:?}", t.elapsed());
}
