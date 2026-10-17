//! window2 observation surface facade — the Codex/Sky result BODY for
//! `get_window_state` / `list_windows` / `get_window` / `list_apps`, mapped from
//! cua-driver's JSON (specs 01 §4/§5 and 19; golden
//! `conformance/golden/tool-surface-window2.md`, which is the shipped
//! `sky_js/src/types/window2/*.d.ts`).
//!
//! Shapes are the golden's, key for key:
//! - `Window { app, id, title? }`
//! - `WindowState { window, screenshots: Screenshot[], accessibility: AccessibilityState | null }`
//! - `Screenshot { id, zIndex, url, originX?, originY?, width?, height? }`
//! - `AccessibilityState { tree, focused_element?, selected_text?, selected_elements?, document_text? }`
//! - `ListAppsApp { id, displayName?, isRunning?, lastUsedDate?, useCount?, windows }`
//!
//! Behaviour pinned from the shipped window2 client
//! (`sky_js/src/targets/windows/internal/computer_use_client_base.js`):
//! - both `include_screenshot` and `include_text` false is a `TypeError`
//!   ([`ERR_BOTH_FALSE`], verbatim);
//! - `include_screenshot` defaults true, `include_text` defaults false;
//! - a screenshot with no `id` gets [`SCREENSHOT_DEFAULT_ID`], no `zIndex` gets 0;
//! - optional fields are OMITTED when unavailable (the JS `k()` helper only sets
//!   defined values), never emitted as null/empty.
//!
//! The two revision counters ride at the top level the way the native Windows
//! engine returns them (`accessibilityRevision`, `accessibilitySnapshotCount`,
//! binary strings next to `screenshots`/`accessibility`); the JS client drops
//! them, so they are optional here and fed from the axdiff lineage.
//!
//! What cua CANNOT supply is reported explicitly in `_gaps` (our diagnostic key,
//! underscore-prefixed like cua's own `_note`) rather than left silently empty:
//! see the `GAP_*` constants. INFERRED mappings are labelled in the code.

pub mod cli;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Verbatim from the window2 client's `get_window_state` guard.
pub const ERR_BOTH_FALSE: &str = "get_window_state must request include_text, include_screenshot, or both";
/// Verbatim: the id the window2 client assigns when the engine omits one.
pub const SCREENSHOT_DEFAULT_ID: &str = "screenshot-0";

pub const GAP_FOCUSED_ELEMENT: &str =
    "accessibility.focused_element: cua-driver exposes no focused-element flag on its AX walk (spec 01 §4)";
pub const GAP_SELECTED_TEXT: &str =
    "accessibility.selected_text: cua-driver has no read path for AXSelectedText (spec 19, 'current-selection reader: absent')";
pub const GAP_DOCUMENT_TEXT: &str =
    "accessibility.document_text: cua-driver does not read the focused document's text (spec 01 §4)";
pub const GAP_TRANSIENT_SCREENSHOTS: &str =
    "screenshots: cua-driver captures one window screenshot per call; transient UI (menus/popovers) is not captured as additional z-indexed screenshots (spec 01 §4)";
pub const GAP_USE_COUNT: &str =
    "useCount: cua-driver list_apps carries no usage count (spec 09 lists it as a Sky-only catalog field)";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// AppIdentifier: the bundle id when cua can resolve it, else the app name.
    pub app: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Screenshot {
    pub id: String,
    #[serde(rename = "zIndex")]
    pub z_index: i64,
    pub url: String,
    #[serde(rename = "originX", skip_serializing_if = "Option::is_none")]
    pub origin_x: Option<f64>,
    #[serde(rename = "originY", skip_serializing_if = "Option::is_none")]
    pub origin_y: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityState {
    pub tree: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_element: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_elements: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub window: Window,
    pub screenshots: Vec<Screenshot>,
    pub accessibility: Option<AccessibilityState>,
    #[serde(rename = "accessibilityRevision", skip_serializing_if = "Option::is_none")]
    pub accessibility_revision: Option<u64>,
    #[serde(rename = "accessibilitySnapshotCount", skip_serializing_if = "Option::is_none")]
    pub accessibility_snapshot_count: Option<u64>,
    /// Our explicit report of golden fields cua cannot populate (not part of the golden type).
    #[serde(rename = "_gaps")]
    pub gaps: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct App {
    pub id: String,
    #[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(rename = "isRunning", skip_serializing_if = "Option::is_none")]
    pub is_running: Option<bool>,
    #[serde(rename = "lastUsedDate", skip_serializing_if = "Option::is_none")]
    pub last_used_date: Option<String>,
    #[serde(rename = "useCount", skip_serializing_if = "Option::is_none")]
    pub use_count: Option<u64>,
    pub windows: Vec<Window>,
}

/// What the caller asked `get_window_state` for (window2 defaults: screenshot on, text off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateRequest {
    pub include_screenshot: bool,
    pub include_text: bool,
}

impl Default for StateRequest {
    fn default() -> Self { StateRequest { include_screenshot: true, include_text: false } }
}

/// The accessibility text our layer rendered for this turn (axdiff full/diff/no-change,
/// with the spec-04 pack prefix on first touch) plus the formatted lines of the
/// elements cua reports as `selected`, and the lineage counters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeInput {
    pub tree: String,
    pub selected_elements: Vec<String>,
    /// (accessibilityRevision, accessibilitySnapshotCount) from the axdiff lineage.
    pub revision: Option<(u64, u64)>,
    /// The focused element as a row of that tree (`1 text field (settable) Value: 123, ...`),
    /// when the layer pinned exactly one row. Otherwise the driver's own line is used.
    pub focused_element: Option<String>,
}

impl From<&axdiff::layer::Observation> for TreeInput {
    fn from(o: &axdiff::layer::Observation) -> TreeInput {
        TreeInput { tree: o.text.clone(), selected_elements: o.selected_elements.clone(), revision: Some(o.revision), focused_element: o.focused_element.clone() }
    }
}

/// JS `p()`: a string that is non-empty after trim (the value itself is kept untrimmed).
fn non_empty(s: Option<&str>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty()).map(str::to_string)
}

/// Validate decoded pixels before reporting a logical region. Legacy array entries
/// without per-surface scale/bounds retain unknown sizes rather than mixing units.
fn screenshot_size(sc: &Value, b64: &str) -> Result<(Option<f64>, Option<f64>), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64)
        .map_err(|e| format!("screenshot base64: {e}"))?;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384); limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| format!("screenshot PNG: {e}"))?;
    let (pw, ph) = (image.width() as f64, image.height() as f64);
    let number = |k: &str| sc.get(k).and_then(Value::as_f64);
    for (key, actual) in [("pixel_width", pw), ("pixel_height", ph)] {
        if sc.get(key).is_some() && number(key) != Some(actual) { return Err(format!("screenshot {key} disagrees with decoded PNG")); }
    }
    let (width, height) = if sc.get("coordinate_space").and_then(Value::as_str) == Some("logical") {
        (number("width"), number("height"))
    } else if let Some(scale) = number("scale").filter(|v| v.is_finite() && *v > 0.) {
        if number("width") != Some(pw) || number("height") != Some(ph) { return Err("screenshot legacy pixel dimensions disagree with PNG".into()); }
        (Some(pw / scale), Some(ph / scale))
    } else { (None, None) };
    if let (Some(w), Some(h)) = (width, height) {
        // Both dimensions are rounded from a common scale. A one-sided aspect
        // check magnifies the short axis's rounding error on portrait captures.
        let low_scale = ((pw - 0.5) / w).max((ph - 0.5) / h);
        let high_scale = ((pw + 0.5) / w).min((ph + 0.5) / h);
        if !w.is_finite() || !h.is_finite() || w <= 0. || h <= 0. || low_scale > high_scale + f64::EPSILON {
            return Err("screenshot logical dimensions disagree with decoded region aspect".into());
        }
        for (key, extent, pixels) in [("scale_x", w, pw), ("scale_y", h, ph)] {
            if let Some(scale) = number(key) {
                if !scale.is_finite() || scale <= 0. || (scale * extent - pixels).abs() > 1.0 {
                    return Err(format!("screenshot {key} disagrees with decoded PNG"));
                }
            }
        }
    } else if sc.get("coordinate_space").and_then(Value::as_str) == Some("logical") {
        return Err("screenshot logical dimensions missing".into());
    }
    Ok((width, height))
}

/// Build the golden `WindowState` from one cua `get_window_state` document.
///
/// `app_id` is the AppIdentifier for `window.app` (bundle id, else app name).
/// `tree` must be `Some` when `req.include_text` (the facade renders it through
/// the axdiff layer); it is ignored otherwise.
pub fn window_state_from_cua(gws: &Value, app_id: &str, req: StateRequest, tree: Option<&TreeInput>) -> Result<WindowState, String> {
    if !req.include_screenshot && !req.include_text {
        return Err(ERR_BOTH_FALSE.to_string());
    }
    if app_id.trim().is_empty() {
        // JS `w()` drops a window whose app is empty after trim
        return Err("window.app must be a non-empty string".to_string());
    }
    let window_id = gws.get("window_id").and_then(Value::as_u64)
        .ok_or_else(|| "cua-driver did not return window_id".to_string())?;
    let window = Window {
        app: app_id.to_string(),
        id: window_id,
        // INFERRED: cua reports "" for untitled windows; the golden says title is
        // present "when available", so an empty title is treated as unavailable.
        title: non_empty(gws.get("window_title").and_then(Value::as_str)),
    };

    let mut gaps = Vec::new();
    let mut screenshots = Vec::new();
    if req.include_screenshot {
        // Patched driver (spec 07): a z-ordered `screenshots` array covering the
        // window AND its related transient UI (open menus / popovers / dropdowns
        // / sheets), each surface carrying its own bounded pixels + origin. When
        // present, map every surface; the transient UI is captured, so no gap.
        if let Some(arr) = gws.get("screenshots").and_then(Value::as_array).filter(|a| !a.is_empty()) {
            for sc in arr {
                let b64 = sc.get("png_b64").and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "cua-driver screenshots entry did not carry png_b64".to_string())?;
                let mime = sc.get("mime_type").and_then(Value::as_str).unwrap_or("image/png");
                let g = |k: &str| sc.get(k).and_then(Value::as_f64);
                let (width, height) = screenshot_size(sc, b64)?;
                screenshots.push(Screenshot {
                    id: sc.get("id").and_then(Value::as_str).unwrap_or(SCREENSHOT_DEFAULT_ID).to_string(),
                    // zIndex: larger is visually above (spec 07).
                    z_index: sc.get("zIndex").and_then(Value::as_i64).unwrap_or(0),
                    url: format!("data:{mime};base64,{b64}"),
                    // originX/originY: screen origin of this bounded region (logical points).
                    origin_x: g("originX"),
                    origin_y: g("originY"),
                    width,
                    height,
                });
            }
        } else {
            // Unpatched driver: one occluded single-window surface, no transient
            // capture — keep the honest gap so the caller knows menus/popovers
            // are not in the frame.
            let b64 = gws.get("screenshot_png_b64").and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "cua-driver did not return a screenshot".to_string())?;
            let mime = gws.get("screenshot_mime_type").and_then(Value::as_str).unwrap_or("image/png");
            let bounds = gws.get("window_bounds");
            let f = |k: &str| bounds.and_then(|b| b.get(k)).and_then(Value::as_f64);
            screenshots.push(Screenshot {
                id: SCREENSHOT_DEFAULT_ID.to_string(),
                z_index: 0,
                url: format!("data:{mime};base64,{b64}"),
                // Screen origin / logical size of the captured region = the window's
                // desktop-space bounds (cua's `window_bounds` is in logical points).
                origin_x: f("x"),
                origin_y: f("y"),
                width: f("width"),
                height: f("height"),
            });
            gaps.push(GAP_TRANSIENT_SCREENSHOTS.to_string());
        }
    }

    let accessibility = if req.include_text {
        let t = tree.ok_or_else(|| "facade did not render an accessibility tree".to_string())?;
        // The patched driver reads the selection (spec 01/19): map it when present, else report
        // the gap as before, so an unpatched driver still gets an honest _gaps entry.
        let sv = |k: &str| gws.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let focused_element = t.focused_element.clone().or_else(|| sv("focused_element"));
        let selected_text = sv("selected_text");
        let document_text = sv("document_text");
        if focused_element.is_none() { gaps.push(GAP_FOCUSED_ELEMENT.to_string()); }
        if selected_text.is_none() { gaps.push(GAP_SELECTED_TEXT.to_string()); }
        if document_text.is_none() { gaps.push(GAP_DOCUMENT_TEXT.to_string()); }
        Some(AccessibilityState {
            tree: t.tree.clone(),
            focused_element,
            selected_text,
            selected_elements: if t.selected_elements.is_empty() { None } else { Some(t.selected_elements.clone()) },
            document_text,
        })
    } else {
        None
    };
    let revision = if req.include_text { tree.and_then(|t| t.revision) } else { None };

    Ok(WindowState {
        window,
        screenshots,
        accessibility,
        accessibility_revision: revision.map(|r| r.0),
        accessibility_snapshot_count: revision.map(|r| r.1),
        gaps,
    })
}

/// Where a window is relative to what the user can see (Spaces / displays).
///
/// macOS accessibility only enumerates windows on their display's CURRENT
/// Space, so an off-Space window walks to an empty, `ax_window_unresolved`
/// tree. cua reports the facts per window in `list_windows`: the per-window
/// `current_space_id` is the active Space of the window's display (with
/// "Displays have separate Spaces" that can differ per display), and
/// `on_current_space` + `is_on_screen` say whether the walk can see it now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub window_id: u64,
    /// Space ids the window lives on (None when cua could not tell).
    pub space_ids: Option<Vec<u64>>,
    /// The active Space of the window's display.
    pub display_current_space: Option<u64>,
    /// The global active Space cua reports for the listing.
    pub global_current_space: Option<u64>,
    pub on_current_space: Option<bool>,
    pub is_on_screen: bool,
}

impl Placement {
    /// Can the accessibility walk see this window right now?
    pub fn observable(&self) -> bool { self.on_current_space == Some(true) && self.is_on_screen }
    /// What to do when it is not observable (the window2 escape hatch is `activate_window`).
    pub fn hint(&self) -> Option<String> {
        if self.observable() { return None; }
        let spaces = self.space_ids.as_ref().map(|v| v.iter().map(u64::to_string).collect::<Vec<_>>().join(",")).unwrap_or_else(|| "?".into());
        let shown = self.display_current_space.map(|s| s.to_string()).unwrap_or_else(|| "?".into());
        Some(format!(
            "window {} is on Space {spaces} but its display currently shows Space {shown}{}; accessibility cannot enumerate off-Space windows — call activate_window (brings it to the front and switches its display to its Space) or switch Space, then get_window_state again",
            self.window_id,
            if self.is_on_screen { "" } else { " (not on screen)" }
        ))
    }
}

/// Placement of `window_id` from a cua `list_windows` document (None if not listed).
pub fn placement_for(list_windows: &Value, window_id: u64) -> Option<Placement> {
    let global = list_windows.get("current_space_id").and_then(Value::as_u64);
    let w = list_windows.get("windows")?.as_array()?.iter().find(|w| w.get("window_id").and_then(Value::as_u64) == Some(window_id))?;
    Some(Placement {
        window_id,
        space_ids: w.get("space_ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()),
        display_current_space: w.get("current_space_id").and_then(Value::as_u64),
        global_current_space: global,
        on_current_space: w.get("on_current_space").and_then(Value::as_bool),
        is_on_screen: w.get("is_on_screen").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// One cua `list_windows` entry -> golden `Window`. `app_id_for_pid` supplies the
/// AppIdentifier (bundle id) for the owning pid; falls back to cua's `app_name`.
/// Returns None for entries with no usable window id (the JS `w()` parser drops those).
pub fn window_from_cua(entry: &Value, app_id_for_pid: &dyn Fn(i64) -> Option<String>) -> Option<Window> {
    let id = entry.get("window_id").and_then(Value::as_u64)?;
    let pid = entry.get("pid").and_then(Value::as_i64);
    let app = pid.and_then(app_id_for_pid)
        .or_else(|| non_empty(entry.get("app_name").and_then(Value::as_str)))?;
    Some(Window { app, id, title: non_empty(entry.get("title").and_then(Value::as_str)) })
}

/// cua `list_windows` document -> golden `Window[]`, with the owning pid kept for grouping.
pub fn windows_from_cua(list_windows: &Value, app_id_for_pid: &dyn Fn(i64) -> Option<String>) -> Vec<(i64, Window)> {
    list_windows.get("windows").and_then(Value::as_array).map(|ws| {
        ws.iter().filter_map(|w| {
            let pid = w.get("pid").and_then(Value::as_i64)?;
            window_from_cua(w, app_id_for_pid).map(|win| (pid, win))
        }).collect()
    }).unwrap_or_default()
}

/// `get_window`: rehydrate an open window by id from the current listing.
pub fn get_window(windows: &[(i64, Window)], id: u64) -> Option<Window> {
    windows.iter().find(|(_, w)| w.id == id).map(|(_, w)| w.clone())
}

/// cua `list_apps` document (+ the current window listing, since cua's own
/// per-app `windows` array is empty) -> golden `ListAppsApp[]`.
pub fn apps_from_cua(list_apps: &Value, windows: &[(i64, Window)]) -> Vec<App> {
    list_apps.get("apps").and_then(Value::as_array).map(|apps| {
        apps.iter().filter_map(|a| {
            let id = non_empty(a.get("bundle_id").and_then(Value::as_str))
                .or_else(|| non_empty(a.get("name").and_then(Value::as_str)))?;
            let pid = a.get("pid").and_then(Value::as_i64);
            let wins = pid.map(|p| windows.iter().filter(|(wp, _)| *wp == p).map(|(_, w)| w.clone()).collect())
                .unwrap_or_default();
            Some(App {
                id,
                display_name: non_empty(a.get("name").and_then(Value::as_str)),
                is_running: a.get("running").and_then(Value::as_bool),
                last_used_date: non_empty(a.get("last_used").and_then(Value::as_str)),
                use_count: None, // GAP_USE_COUNT
                windows: wins,
            })
        }).collect()
    }).unwrap_or_default()
}

/// The `app_id_for_pid` resolver built from a cua `list_apps` document.
pub fn app_id_resolver(list_apps: &Value) -> impl Fn(i64) -> Option<String> + '_ {
    move |pid| {
        list_apps.get("apps")?.as_array()?.iter()
            .find(|a| a.get("pid").and_then(Value::as_i64) == Some(pid))
            .and_then(|a| non_empty(a.get("bundle_id").and_then(Value::as_str)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn gws() -> Value {
        json!({
            "window_id": 25198, "window_title": "Clock", "pid": 55372, "app_name": "Clock",
            "window_bounds": {"x": 352.0, "y": 98.0, "width": 1024.0, "height": 768.0},
            "screenshot_png_b64": "AAAA", "screenshot_mime_type": "image/png",
            "screenshot_width": 1568, "screenshot_height": 1176, "screenshot_scale": 2.0,
            "elements": [], "tree_markdown": "- [0] AXWindow \"Clock\""
        })
    }
    fn tree() -> TreeInput {
        TreeInput { focused_element: None, tree: "[1] AXWindow \"Clock\"\n  [2] AXRow \"x\"".into(),
                    selected_elements: vec!["  [2] AXRow \"x\"".into()], revision: Some((3, 3)) }
    }

    #[test]
    fn both_false_is_the_verbatim_type_error() {
        let r = window_state_from_cua(&gws(), "com.apple.clock", StateRequest { include_screenshot: false, include_text: false }, None);
        assert_eq!(r.unwrap_err(), ERR_BOTH_FALSE);
    }

    #[test]
    fn default_request_is_screenshot_only_with_null_accessibility() {
        let s = window_state_from_cua(&gws(), "com.apple.clock", StateRequest::default(), None).unwrap();
        assert_eq!(s.accessibility, None);
        assert_eq!(s.screenshots.len(), 1);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["accessibility"], Value::Null, "accessibility is null when text not requested");
        assert!(v.get("accessibilityRevision").is_none());
        assert_eq!(s.gaps, vec![GAP_TRANSIENT_SCREENSHOTS.to_string()]);
    }

    #[test]
    fn screenshot_maps_to_data_url_with_default_id_and_window_bounds() {
        let s = window_state_from_cua(&gws(), "com.apple.clock", StateRequest::default(), None).unwrap();
        let sc = &s.screenshots[0];
        assert_eq!(sc.id, "screenshot-0");
        assert_eq!(sc.z_index, 0);
        assert_eq!(sc.url, "data:image/png;base64,AAAA");
        assert_eq!((sc.origin_x, sc.origin_y, sc.width, sc.height), (Some(352.0), Some(98.0), Some(1024.0), Some(768.0)));
        let v = serde_json::to_value(sc).unwrap();
        assert_eq!(v["zIndex"], 0);
        assert!(v.get("originX").is_some() && v.get("z_index").is_none(), "camelCase keys as in the golden");
    }

    fn png_fixture(w: u32, h: u32) -> String {
        use base64::Engine;
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(w, h).write_to(&mut out, image::ImageFormat::Png).unwrap();
        base64::engine::general_purpose::STANDARD.encode(out.into_inner())
    }

    #[test]
    fn patched_screenshots_array_maps_all_z_ordered_surfaces_and_drops_the_transient_gap() {
        // spec 07: the patched driver returns a z-ordered `screenshots` array —
        // target (zIndex 0) plus transient UI above it, each with inline pixels.
        let mut g = gws();
        g["screenshots"] = json!([
            {"id": "screenshot-0", "zIndex": 0, "originX": 352.0, "originY": 98.0, "width": 1024, "height": 768, "coordinate_space": "logical", "mime_type": "image/png", "png_b64": png_fixture(1024, 768)},
            {"id": "screenshot-1", "zIndex": 1, "originX": 400.0, "originY": 150.0, "width": 220, "height": 320, "coordinate_space": "logical", "mime_type": "image/png", "png_b64": png_fixture(220, 320)}
        ]);
        let s = window_state_from_cua(&g, "com.apple.clock", StateRequest::default(), None).unwrap();
        assert_eq!(s.screenshots.len(), 2, "target + one transient surface");
        assert_eq!(s.screenshots[0].id, "screenshot-0");
        assert_eq!(s.screenshots[0].z_index, 0);
        let menu = &s.screenshots[1];
        assert_eq!(menu.id, "screenshot-1");
        assert_eq!(menu.z_index, 1);
        assert_eq!(menu.url, format!("data:image/png;base64,{}", png_fixture(220, 320)));
        assert_eq!((menu.origin_x, menu.origin_y, menu.width, menu.height), (Some(400.0), Some(150.0), Some(220.0), Some(320.0)));
        // the transient UI IS captured now, so the honest gap is gone
        assert!(!s.gaps.iter().any(|x| x == GAP_TRANSIENT_SCREENSHOTS), "no transient gap when the array is present");
    }

    #[test]
    fn screenshot_requested_but_missing_is_an_error_not_an_empty_array() {
        let mut g = gws(); g.as_object_mut().unwrap().remove("screenshot_png_b64");
        assert!(window_state_from_cua(&g, "a", StateRequest::default(), None).is_err());
        let mut g = gws(); g["screenshot_png_b64"] = json!("");
        assert!(window_state_from_cua(&g, "a", StateRequest::default(), None).is_err());
        // but not requested -> no screenshot, no error
        let s = window_state_from_cua(&g, "a", StateRequest { include_screenshot: false, include_text: true }, Some(&tree())).unwrap();
        assert!(s.screenshots.is_empty());
        assert!(!s.gaps.iter().any(|g| g == GAP_TRANSIENT_SCREENSHOTS));
    }

    #[test]
    fn text_request_carries_tree_selected_elements_revision_and_explicit_gaps() {
        let s = window_state_from_cua(&gws(), "com.apple.clock", StateRequest { include_screenshot: true, include_text: true }, Some(&tree())).unwrap();
        let a = s.accessibility.as_ref().unwrap();
        assert_eq!(a.tree, tree().tree);
        assert_eq!(a.selected_elements.as_deref(), Some(&["  [2] AXRow \"x\"".to_string()][..]));
        assert_eq!((a.focused_element.as_deref(), a.selected_text.as_deref(), a.document_text.as_deref()), (None, None, None));
        assert_eq!((s.accessibility_revision, s.accessibility_snapshot_count), (Some(3), Some(3)));
        for g in [GAP_FOCUSED_ELEMENT, GAP_SELECTED_TEXT, GAP_DOCUMENT_TEXT, GAP_TRANSIENT_SCREENSHOTS] {
            assert!(s.gaps.iter().any(|x| x == g), "missing gap report {g}");
        }
        // unavailable optionals are omitted, not null
        let v = serde_json::to_value(&s).unwrap();
        let acc = v["accessibility"].as_object().unwrap();
        assert_eq!(acc.keys().cloned().collect::<Vec<_>>(), vec!["selected_elements", "tree"]);
        // text requested without a rendered tree is an error, never an empty tree
        assert!(window_state_from_cua(&gws(), "a", StateRequest { include_screenshot: false, include_text: true }, None).is_err());
    }

    #[test]
    fn no_selected_elements_omits_the_field() {
        let mut t = tree(); t.selected_elements.clear();
        let s = window_state_from_cua(&gws(), "a", StateRequest { include_screenshot: false, include_text: true }, Some(&t)).unwrap();
        assert_eq!(s.accessibility.unwrap().selected_elements, None);
    }

    #[test]
    fn empty_or_whitespace_app_id_is_rejected() {
        for a in ["", "  "] {
            assert!(window_state_from_cua(&gws(), a, StateRequest::default(), None).is_err());
        }
        let lw = json!({"windows": [{"pid": 1, "window_id": 10, "app_name": "  ", "title": "t"}]});
        assert!(windows_from_cua(&lw, &|_| None).is_empty(), "whitespace app_name is not an app id");
        let la = json!({"apps": [{"pid": 1, "bundle_id": " ", "name": "\t", "running": true, "windows": []}]});
        assert!(apps_from_cua(&la, &[]).is_empty());
    }

    #[test]
    fn window_title_empty_is_unavailable() {
        let mut g = gws(); g["window_title"] = json!("");
        let s = window_state_from_cua(&g, "a", StateRequest::default(), None).unwrap();
        assert_eq!(s.window.title, None);
        let s = window_state_from_cua(&gws(), "a", StateRequest::default(), None).unwrap();
        assert_eq!(s.window, Window { app: "a".into(), id: 25198, title: Some("Clock".into()) });
    }

    #[test]
    fn placement_reads_per_window_space_facts_and_hints_activate_window() {
        // real shape captured on this Mac: the window lives on Space 1 while its display currently shows Space 15
        let lw = json!({"current_space_id": 1, "windows": [
            {"window_id": 25427, "pid": 1, "app_name": "Clock", "title": "Clock", "current_space_id": 15, "on_current_space": false, "is_on_screen": false, "space_ids": [1]},
            {"window_id": 85, "pid": 2, "app_name": "Warp", "title": "w", "current_space_id": 15, "on_current_space": true, "is_on_screen": true, "space_ids": [15]},
            {"window_id": 7, "pid": 3, "app_name": "X", "title": "", "current_space_id": null, "on_current_space": null, "is_on_screen": false, "space_ids": null}
        ]});
        let p = placement_for(&lw, 25427).unwrap();
        assert_eq!((p.on_current_space, p.is_on_screen, p.display_current_space, p.global_current_space, p.space_ids.clone()), (Some(false), false, Some(15), Some(1), Some(vec![1])));
        assert!(!p.observable());
        let h = p.hint().unwrap();
        assert!(h.contains("Space 1") && h.contains("shows Space 15") && h.contains("activate_window") && h.contains("not on screen"), "{h}");
        let w = placement_for(&lw, 85).unwrap();
        assert!(w.observable() && w.hint().is_none());
        let u = placement_for(&lw, 7).unwrap();
        assert!(!u.observable());
        assert!(u.hint().unwrap().contains("Space ?"));
        assert!(placement_for(&lw, 999).is_none());
    }

    #[test]
    fn windows_and_apps_mapping() {
        let lw = json!({"windows": [
            {"pid": 1, "window_id": 10, "app_name": "Clock", "title": "Clock"},
            {"pid": 1, "window_id": 11, "app_name": "Clock", "title": ""},
            {"pid": 2, "window_id": 20, "app_name": "Unknown", "title": "U"},
            {"pid": 3, "app_name": "NoId", "title": "dropped"}
        ]});
        let la = json!({"apps": [
            {"pid": 1, "bundle_id": "com.apple.clock", "name": "Clock", "running": true, "last_used": "2026-09-10T00:00:00Z", "windows": []},
            {"pid": null, "bundle_id": "com.x.notrunning", "name": "X", "running": false, "windows": []},
            {"pid": 4, "bundle_id": "", "name": "NameOnly", "running": true, "windows": []},
            {"pid": 5, "bundle_id": "", "name": "", "running": true, "windows": []}
        ]});
        let resolve = app_id_resolver(&la);
        let ws = windows_from_cua(&lw, &resolve);
        assert_eq!(ws.len(), 3, "entry without window_id is dropped");
        assert_eq!(ws[0].1, Window { app: "com.apple.clock".into(), id: 10, title: Some("Clock".into()) });
        assert_eq!(ws[1].1.title, None);
        assert_eq!(ws[2].1.app, "Unknown", "unresolvable pid falls back to app_name");
        assert_eq!(get_window(&ws, 11).unwrap().id, 11);
        assert_eq!(get_window(&ws, 999), None);
        let apps = apps_from_cua(&la, &ws);
        assert_eq!(apps.len(), 3, "an app with neither bundle id nor name is dropped");
        assert_eq!(apps[0].id, "com.apple.clock");
        assert_eq!(apps[0].windows.iter().map(|w| w.id).collect::<Vec<_>>(), vec![10, 11]);
        assert_eq!((apps[0].is_running, apps[0].display_name.as_deref(), apps[0].use_count), (Some(true), Some("Clock"), None));
        assert_eq!(apps[0].last_used_date.as_deref(), Some("2026-09-10T00:00:00Z"));
        assert!(apps[1].windows.is_empty() && apps[1].is_running == Some(false));
        assert_eq!(apps[2].id, "NameOnly");
        let v = serde_json::to_value(&apps[0]).unwrap();
        assert!(v.get("displayName").is_some() && v.get("isRunning").is_some() && v.get("lastUsedDate").is_some() && v.get("useCount").is_none());
    }
}
