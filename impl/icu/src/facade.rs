//! The unified `getState` / `getApp` facade (specs/14-approval-confirmations.md:46).
//!
//! Codex's newer `@oai/cua` package replaced the split `sky` (computer) and browser surfaces with
//! one persistent session that has two entry points: `cua.getState()`, "a snapshot of all enabled
//! surfaces" (apps, browsers, tabs) to take an inventory, and `cua.getApp(name)`, which selects an
//! app by name, bundle id or path and returns "its initial UI state". The window2 tools remain the
//! per-window verbs; this is the thin wrapper the spec calls for, not new mechanism:
//!
//! - `get_state` returns the inventory in one call: every app with its open windows (the
//!   `list_apps` shape), the frontmost window, and which surfaces this driver serves.
//! - `get_app` takes an app by name, bundle id or `.app`/`.exe` path, launches it when it has no
//!   window (the guidance's `launch_app` then re-list), picks its window and returns the app, the
//!   window and that window's state (screenshot + accessibility text), so a task starts from one
//!   call instead of `list_apps`, `launch_app`, `list_windows`, `get_window_state`.
//!
//! Browser tabs are not part of this driver's surface (cua's CDP tools are separate tools), so
//! `get_state` says so in `surfaces` and `_gaps` instead of returning an empty list that reads as
//! "no tabs are open". The MCP names `get_state` and `get_app` are INFERRED (Codex's are the
//! JavaScript methods).
use super::*;

pub const GAP_BROWSERS: &str = "browsers and tabs: this driver has no browser/tab inventory in the window2 surface, so `browsers` and `tabs` are not listed (not empty); cua's browser tools are separate";

/// The `.app` / `.exe` stem of a path-like identifier, else the text unchanged.
fn stem(app: &str) -> String {
    let base = app.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(app);
    base.strip_suffix(".app").or_else(|| base.strip_suffix(".exe")).unwrap_or(base).to_string()
}

pub(crate) fn matches_app(app: &Value, wanted: &str) -> bool {
    let s = |k: &str| app.get(k).and_then(Value::as_str);
    let stem = stem(wanted);
    s("id") == Some(wanted) || s("displayName").is_some_and(|n| n.eq_ignore_ascii_case(wanted) || n.eq_ignore_ascii_case(&stem))
}

impl Layer {
    /// `cua.getState()`: the inventory of every enabled surface in one call.
    pub fn get_state(&self, _args: &Value) -> Result<Value, ToolError> {
        let apps = self.list_apps()?;
        let windows = self.call("list_windows", json!({}))?;
        let front = windows.get("windows").and_then(Value::as_array).and_then(|ws| ws.iter().find(|w| {
            w.get("is_on_screen").and_then(Value::as_bool) == Some(true) && w.get("on_current_space").and_then(Value::as_bool) != Some(false)
        }));
        let frontmost = front.and_then(|w| {
            let id = w.get("window_id").and_then(Value::as_u64)?;
            apps.as_array()?.iter().find_map(|a| a.get("windows")?.as_array()?.iter().find(|x| x.get("id").and_then(Value::as_u64) == Some(id)).cloned())
        });
        Ok(json!({
            "apps": apps,
            "frontmost": frontmost,
            "surfaces": {"native_apps": true, "browsers": false, "tabs": false},
            "_gaps": [GAP_BROWSERS],
        }))
    }

    /// `cua.getApp(app)`: select an app by name, bundle id or path and return its initial UI state.
    pub fn get_app(&self, args: &Value) -> Result<Value, ToolError> {
        let wanted = args.get("app").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::new(CODE_INVALID_ARGUMENT, "`app` (a name, bundle id or path) is required"))?;
        let find = |apps: &Value| -> Vec<Value> { apps.as_array().map(|a| a.iter().filter(|x| matches_app(x, wanted)).cloned().collect()).unwrap_or_default() };
        let mut launched = false;
        let mut found = find(&self.list_apps()?);
        // the real driver lists every installed app, with `isRunning: false` for the ones that are not open
        let not_running = |apps: &[Value]| apps.iter().all(|a| a.get("isRunning").and_then(Value::as_bool) == Some(false));
        if found.is_empty() || not_running(&found) {
            // not running or not listed: launch it (the policy and Invalid app refusal live there), then look again
            self.launch_app(&json!({"app": wanted}))?;
            launched = true;
            found = find(&self.list_apps()?);
        }
        let app = match found.len() {
            0 => return Err(ToolError::new(CODE_INVALID_APP, format!("Invalid app: {wanted}"))),
            1 => found.remove(0),
            _ => {
                let ids: Vec<&str> = found.iter().filter_map(|a| a.get("id").and_then(Value::as_str)).collect();
                let mut e = ToolError::new(CODE_AMBIGUOUS_APP, format!("{wanted:?} matches {} apps: {}", found.len(), ids.join(", ")));
                e.structured["candidates"] = json!(ids);
                return Err(e);
            }
        };
        let id = app.get("id").and_then(Value::as_str).unwrap_or(wanted).to_string();
        self.check_app_policy(&id)?;
        let mut windows: Vec<Value> = app.get("windows").and_then(Value::as_array).cloned().unwrap_or_default();
        if windows.is_empty() && launched {
            // a fresh launch may not have drawn its window yet: one more look
            std::thread::sleep(std::time::Duration::from_millis(500));
            let again = find(&self.list_apps()?);
            windows = again.first().and_then(|a| a.get("windows")).and_then(Value::as_array).cloned().unwrap_or_default();
        }
        let Some(window) = pick_window(&windows) else {
            return Ok(json!({"app": app, "window": null, "state": null, "launched": launched,
                             "hint": "the app has no open window; call launch_app or activate_window, then get_app again"}));
        };
        let mut req = json!({"window": window, "include_screenshot": true, "include_text": true});
        if let Some(v) = args.get("max_value_chars") { req["max_value_chars"] = v.clone(); }
        let state = self.get_window_state(&req)?;
        let mut out = json!({"app": app, "window": window, "state": state, "launched": launched});
        if windows.len() > 1 {
            // the first listed window is the frontmost one (the driver lists in z-order); say so
            out["selected"] = json!(if windows.first() == Some(&window) { "frontmost" } else { "first titled window" });
            out["windows"] = json!(windows);
        }
        Ok(out)
    }
}

/// The window `get_app` observes: the first one with a title. A real Mac lists untitled helper surfaces
/// (a freshly launched Calculator lists four of them ahead of its window) that have no accessibility
/// tree; when none is titled, the first listed (frontmost) window.
pub(crate) fn pick_window(windows: &[Value]) -> Option<Value> {
    let titled = |w: &&Value| w.get("title").and_then(Value::as_str).is_some_and(|t| !t.trim().is_empty());
    windows.iter().find(titled).or_else(|| windows.first()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_observed_window_is_the_first_titled_one() {
        let ghost = json!({"app": "com.apple.calculator", "id": 43});
        let real = json!({"app": "com.apple.calculator", "id": 39, "title": "Calculator"});
        assert_eq!(pick_window(&[ghost.clone(), ghost.clone(), real.clone()]), Some(real));
        assert_eq!(pick_window(&[ghost.clone()]), Some(ghost));
        assert_eq!(pick_window(&[]), None);
    }

    #[test]
    fn a_path_matches_by_its_app_stem() {
        assert_eq!(stem("/System/Applications/Clock.app/"), "Clock");
        assert_eq!(stem("C:\\Windows\\notepad.exe"), "notepad");
        assert_eq!(stem("Clock"), "Clock");
        let clock = json!({"id": "com.apple.clock", "displayName": "Clock", "windows": []});
        for wanted in ["com.apple.clock", "Clock", "clock", "/System/Applications/Clock.app", "/System/Applications/Clock.app/"] {
            assert!(matches_app(&clock, wanted), "{wanted}");
        }
        assert!(!matches_app(&clock, "Clocks"));
    }
}
