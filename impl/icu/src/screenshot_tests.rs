//! Screenshot policy through the real Layer with identical AX and valid, changing PNGs.
//! All driver calls, including coordinate routing checks, stay in the synthetic driver.
use super::*;
use crate::cua::{Cua, CuaError};
use crate::tests::{layer_with_log, last, CallLog, FakeCua, CLOCK};
use std::sync::atomic::{AtomicUsize, Ordering};

struct ScreenshotCua {
    inner: FakeCua,
    elements: Vec<Value>,
    reads: AtomicUsize,
    missing_first_image: bool,
    changing_pixels: bool,
    pid: i64,
}

impl Cua for ScreenshotCua {
    fn tool_names(&self) -> Result<Vec<String>, CuaError> { self.inner.tool_names() }

    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        let mut doc = self.inner.call(tool, args.clone())?;
        if tool == "list_apps" { doc["apps"][0]["pid"] = json!(self.pid); }
        if tool == "list_windows" { doc["windows"][0]["pid"] = json!(self.pid); }
        if tool != "get_window_state" { return Ok(doc); }
        let n = self.reads.fetch_add(1, Ordering::SeqCst);
        doc["elements"] = json!(self.elements);
        // This synthetic tree is exhaustive. The saved native Clock fixture is not:
        // it reports elements_complete=false even when returned and total counts agree.
        doc["elements_complete"] = json!(true);
        doc["element_count"] = json!(self.elements.len());
        doc["returned_element_count"] = json!(self.elements.len());
        doc["total_element_count"] = json!(self.elements.len());
        doc["tree_markdown"] = json!("");
        doc["window_bounds"] = json!({"x": 0, "y": 0, "width": 8, "height": 6});
        if args["include_screenshot"] == true && !(n == 0 && self.missing_first_image) {
            // A real 16x12 PNG of an 8x6 logical window, changing red to blue with no AX change.
            let color = if n == 0 || !self.changing_pixels { [255, 0, 0] } else { [0, 0, 255] };
            let png = pointer::solid_png_b64(16, 12, color);
            doc["screenshot_png_b64"] = json!(png);
            doc["screenshot_width"] = json!(16);
            doc["screenshot_height"] = json!(12);
            doc["screenshot_frame_valid"] = json!(true);
            doc["screenshots"] = json!([{"id": format!("surface-{n}"), "png_b64": png,
                "coordinate_space": "logical", "width": 8, "height": 6,
                "pixel_width": 16, "pixel_height": 12, "originX": 0, "originY": 0}]);
        } else {
            for key in ["screenshot_png_b64", "screenshot_width", "screenshot_height", "screenshots"] {
                doc.as_object_mut().unwrap().remove(key);
            }
        }
        Ok(doc)
    }
}

/// A bounded AX projection hides the final canvas, while the real screenshot still changes.
struct ProjectedCua {
    inner: ScreenshotCua,
    metadata: Value,
}

impl Cua for ProjectedCua {
    fn tool_names(&self) -> Result<Vec<String>, CuaError> { self.inner.tool_names() }

    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        let mut doc = self.inner.call(tool, args)?;
        if tool == "get_window_state" {
            doc["elements"].as_array_mut().unwrap().truncate(2);
            doc["element_count"] = json!(2);
            doc["returned_element_count"] = json!(2);
            doc["total_element_count"] = json!(2);
            for (key, value) in self.metadata.as_object().unwrap() {
                if value.is_null() { doc.as_object_mut().unwrap().remove(key); }
                else { doc[key] = value.clone(); }
            }
        }
        Ok(doc)
    }
}

fn elements(surface: Option<(&str, &str)>) -> Vec<Value> {
    let mut elements = vec![
        json!({"element_index": 0, "depth": 0, "role": "AXWindow", "label": "Test", "actions": []}),
        json!({"element_index": 1, "parent_index": 0, "depth": 1, "role": "AXButton", "label": "OK", "actions": ["AXPress"],
            "element_token": "button", "frame": {"x": 1, "y": 1, "w": 2, "h": 2}}),
    ];
    if let Some((role, label)) = surface {
        elements.push(json!({"element_index": 2, "parent_index": 0, "depth": 1, "role": role, "label": label,
            "actions": if role == "AXButton" { vec!["AXPress"] } else { vec![] }}));
    }
    elements
}

fn with_config(cfg: Config, log: CallLog, surface: Option<(&str, &str)>, missing_first_image: bool) -> Layer {
    Layer::with_cua(cfg, Box::new(ScreenshotCua {
        inner: FakeCua { calls: log, locked: false }, elements: elements(surface),
        reads: AtomicUsize::new(0), missing_first_image,
        changing_pixels: surface.is_some(), pid: 26765,
    }))
}

fn layer(surface: Option<(&str, &str)>, missing_first_image: bool) -> (Layer, tempfile::TempDir, CallLog) {
    let (base, tmp, log) = layer_with_log(false, false);
    let mut cfg = base.cfg.clone();
    cfg.features.post_action_sleep_ms = 0;
    (with_config(cfg, log.clone(), surface, missing_first_image), tmp, log)
}

fn state(l: &Layer, extra: Value) -> Result<Value, ToolError> {
    let mut args = CLOCK();
    args["auto_screenshot"] = json!(true);
    args["include_text"] = json!(true);
    for (k, v) in extra.as_object().unwrap() { args[k] = v.clone(); }
    l.dispatch("get_window_state", &args)
}

fn images(s: &Value) -> usize { s["screenshots"].as_array().unwrap().len() }

fn pixel(s: &Value) -> [u8; 3] {
    use base64::Engine;
    let url = s["screenshots"][0]["url"].as_str().expect("returned screenshot");
    let bytes = base64::engine::general_purpose::STANDARD.decode(url.split_once("base64,").unwrap().1).unwrap();
    let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
    assert_eq!((image.width(), image.height()), (16, 12));
    image.get_pixel(0, 0).0
}

#[test]
fn auto_keeps_changed_pixels_with_identical_ax_on_visual_surfaces() {
    for surface in [("AXImage", "Chart"), ("canvas", "Chart"), ("AXButton", ""), ("AXWebArea", "")] {
        let (l, _tmp, log) = layer(Some(surface), false);
        let first = state(&l, json!({})).unwrap();
        let second = state(&l, json!({})).unwrap();
        assert_eq!(first["accessibilityRevision"], second["accessibilityRevision"], "identical AX for {surface:?}");
        assert!(second["accessibility"]["tree"].as_str().unwrap().starts_with("There has been no change"));
        assert_eq!((images(&first), images(&second)), (1, 1), "{surface:?}: {}", second["_screenshot_reason"]);
        assert_eq!(pixel(&first), [255, 0, 0]);
        assert_eq!(pixel(&second), [0, 0, 255]);
        assert_eq!(second["_screenshot_needed"], true);
        assert_eq!(last(&log, "get_window_state")["include_screenshot"], true);
    }
}

#[test]
fn auto_keeps_hidden_canvas_pixels_when_ax_completeness_is_unproven() {
    for metadata in [
        json!({"elements_complete": false}),
        json!({"elements_complete": null}),
        json!({"total_element_count": 3}),
        json!({"returned_element_count": 3, "total_element_count": 3}),
        json!({"ax_diagnostics": {"truncated": true}}),
        json!({"truncated": true}),
        json!({"tree_markdown": "\n⚠️  AX tree truncated at 2 nodes (app has a very large accessibility tree)."}),
    ] {
        let (base, _tmp, log) = layer(None, false);
        let l = Layer::with_cua(base.cfg.clone(), Box::new(ProjectedCua {
            inner: ScreenshotCua {
                inner: FakeCua { calls: log, locked: false }, elements: elements(Some(("canvas", "Chart"))),
                reads: AtomicUsize::new(0), missing_first_image: false, changing_pixels: true, pid: 26765,
            }, metadata: metadata.clone(),
        }));
        let first = state(&l, json!({})).unwrap();
        assert!(!first["accessibility"]["tree"].as_str().unwrap().contains("Chart"), "the canvas is absent from AX");
        let second = state(&l, json!({})).unwrap();
        assert_eq!(first["accessibilityRevision"], second["accessibilityRevision"], "identical projected AX");
        assert_eq!(images(&second), 1, "metadata {metadata}: {}", second["_screenshot_reason"]);
        assert_eq!(pixel(&first), [255, 0, 0]);
        assert_eq!(pixel(&second), [0, 0, 255]);
        assert_eq!(images(&state(&l, json!({"include_screenshot": false})).unwrap()), 0, "explicit false remains authoritative");
    }
}

#[test]
fn auto_omits_labeled_static_ui_only_after_returning_visual_grounding() {
    let (l, _tmp, log) = layer(None, false);
    let first = state(&l, json!({})).unwrap();
    assert_eq!(images(&first), 1, "actual FULL(first) establishes grounding");
    for _ in 0..2 {
        let next = state(&l, json!({})).unwrap();
        assert_eq!(images(&next), 0);
        assert!(next["accessibility"]["tree"].as_str().unwrap().contains("no change"));
        assert_eq!(next["_screenshot_needed"], false);
        // Auto saves returned image payloads, not native capture work.
        assert_eq!(last(&log, "get_window_state")["include_screenshot"], true);
    }
    let old_id = &first["screenshots"][0]["id"];
    assert_eq!(l.check_screenshot_id(25427, &json!({"screenshotId": old_id})).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    assert_eq!(l.px(25427, 1.0).unwrap_err().code(), Some(CODE_STALE_ELEMENT), "an omitted capture supplies no returned-image coordinates");
}

#[test]
fn auto_establishes_grounding_after_text_only_state_and_across_sessions() {
    let (l, _tmp, log) = layer(None, false);
    assert_eq!(images(&state(&l, json!({"include_screenshot": false})).unwrap()), 0);
    let first = state(&l, json!({})).unwrap();
    assert_eq!(images(&first), 1, "unchanged AX is not evidence of a prior image");
    assert_eq!(images(&state(&l, json!({})).unwrap()), 0);
    let same_session = with_config(l.cfg.clone(), log.clone(), None, false);
    assert_eq!(images(&state(&same_session, json!({})).unwrap()), 0, "one-shot calls preserve actual grounding even after omission");
    let mut cfg = l.cfg.clone();
    cfg.session_id = "new-session".into();
    let new_session = with_config(cfg, log, None, false);
    assert_eq!(images(&state(&new_session, json!({})).unwrap()), 1, "a new session has not received the prior session's image");
}

#[test]
fn failed_image_delivery_does_not_establish_visual_grounding() {
    let (l, _tmp, _) = layer(None, true);
    assert!(state(&l, json!({})).is_err(), "the first capture has no image");
    assert_eq!(images(&state(&l, json!({})).unwrap()), 1, "AX lineage alone must not suppress the retry");
}

#[test]
fn reused_window_id_in_another_process_needs_its_own_visual_grounding() {
    let (l, _tmp, log) = layer(None, false);
    assert_eq!(images(&state(&l, json!({})).unwrap()), 1);
    let replacement = Layer::with_cua(l.cfg.clone(), Box::new(ScreenshotCua {
        inner: FakeCua { calls: log, locked: false }, elements: elements(None),
        reads: AtomicUsize::new(0), missing_first_image: false, changing_pixels: false, pid: 60000,
    }));
    assert_eq!(images(&state(&replacement, json!({"include_screenshot": false})).unwrap()), 0);
    assert_eq!(images(&state(&replacement, json!({})).unwrap()), 1, "same session and window id, different process");
}

#[test]
fn visual_grounding_is_per_window() {
    let (l, _tmp, _) = layer(None, false);
    assert_eq!(images(&state(&l, json!({})).unwrap()), 1);
    let second_window = json!({"window": {"app": "com.apple.clock", "id": 31337}, "include_screenshot": false});
    assert_eq!(images(&state(&l, second_window).unwrap()), 0);
    assert_eq!(images(&state(&l, json!({"window": {"app": "com.apple.clock", "id": 31337}})).unwrap()), 1);
}

#[test]
fn auto_with_text_disabled_always_returns_an_image() {
    let (l, _tmp, _) = layer(None, false);
    for extra in [json!({"include_text": false}), json!({"include_text": false})] {
        let s = state(&l, extra).unwrap();
        assert_eq!(images(&s), 1);
        assert_eq!(s["_screenshot_needed"], true);
    }
    let mut args = CLOCK();
    args["auto_screenshot"] = json!(true);
    assert_eq!(images(&l.dispatch("get_window_state", &args).unwrap()), 1, "include_text defaults false");
}

#[test]
fn explicit_screenshot_requests_win_over_auto_and_both_false_is_invalid() {
    let (l, _tmp, log) = layer(None, false);
    for _ in 0..2 { assert_eq!(images(&state(&l, json!({"include_screenshot": true})).unwrap()), 1); }
    let (visual, _visual_tmp, _) = layer(Some(("AXImage", "Chart")), false);
    for _ in 0..2 { assert_eq!(images(&state(&visual, json!({"include_screenshot": false})).unwrap()), 0); }
    assert_eq!(images(&state(&l, json!({"include_screenshot": false})).unwrap()), 0);
    assert_eq!(last(&log, "get_window_state")["include_screenshot"], false);
    let before = log.lock().unwrap().len();
    let e = state(&l, json!({"include_screenshot": false, "include_text": false})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_INVALID_ARGUMENT));
    assert_eq!(log.lock().unwrap().len(), before, "invalid output selection must not capture");
}

#[test]
fn auto_keeps_screenshot_binding_and_returned_pixel_scale() {
    let (l, _tmp, log) = layer(Some(("AXImage", "Chart")), false);
    let first = state(&l, json!({})).unwrap();
    let second = state(&l, json!({})).unwrap();
    let capture = last(&log, "get_window_state");
    assert_eq!(capture["include_screenshot"], true);
    let bound = l.targets.lock().unwrap()[&25427].driver_screenshot_id.clone().unwrap();
    let mut args = CLOCK();
    args["x"] = json!(2.0); args["y"] = json!(1.0);
    args["screenshotId"] = first["screenshots"][0]["id"].clone();
    assert_eq!(l.dispatch("click", &args).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    args["screenshotId"] = second["screenshots"][0]["id"].clone();
    l.dispatch("click", &args).unwrap();
    let click = last(&log, "click");
    assert_eq!(click["screenshot_id"], bound);
    assert_eq!((click["x"].as_f64(), click["y"].as_f64()), (Some(4.0), Some(2.0)));
}
