//! Unit tests over a fake driver that serves the real saved cua documents, so
//! the mapping (window2 -> cua), the gate and the MCP framing are checked on
//! real shapes without a live daemon.
use super::*;
use crate::cua::{Cua, CuaError};
use std::sync::{Arc, Mutex};

pub(crate) type CallLog = Arc<Mutex<Vec<(String, Value)>>>;

pub(crate) struct FakeCua {
    pub(crate) calls: CallLog,
    pub(crate) locked: bool,
}

pub(crate) fn fixture(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/fixtures").join(name);
    serde_json::from_slice(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

impl Cua for FakeCua {
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        self.calls.lock().unwrap().push((tool.to_string(), args.clone()));
        match tool {
            "list_apps" => Ok(json!({"apps": [{"pid": 26765, "bundle_id": "com.apple.clock", "name": "Clock"}, {"pid": 500, "bundle_id": "com.apple.Safari", "name": "Safari"}, {"pid": 0, "bundle_id": "notion.id", "name": "Notion", "running": false}]})),
            "list_windows" => Ok(json!({"windows": [
                {"window_id": 25427, "pid": 26765, "app_name": "Clock", "bundle_id": "com.apple.clock", "title": "Clock", "on_current_space": true, "is_on_screen": true, "bounds": {"x": 0, "y": 0, "width": 600, "height": 400}},
                {"window_id": 900, "pid": 500, "app_name": "Safari", "title": "Start Page", "on_current_space": true, "is_on_screen": true},
                {"window_id": 77, "pid": 4242, "app_name": "Helper", "title": "agent window", "on_current_space": true, "is_on_screen": true},
                {"window_id": 31337, "pid": 26765, "app_name": "Clock", "bundle_id": "com.apple.clock", "title": "Sign in", "on_current_space": true, "is_on_screen": true, "bounds": {"x": 0, "y": 0, "width": 600, "height": 400}}]})),
            "get_screen_size" => Ok(json!({"width": 1728, "height": 1117, "scale_factor": 2.0})),
            "get_window_state" => {
                let mut d = fixture("clock_gws.json"); d["pid"] = json!(args["pid"]); d["window_id"] = json!(args["window_id"]);
                if args["window_id"] == 31337 {
                    // a sign-in window: element 7 is a focused password field (macOS shape)
                    let el = &mut d["elements"][7];
                    el["role"] = json!("AXTextField"); el["subrole"] = json!("AXSecureTextField"); el["label"] = json!("Password"); el["focused"] = json!(true);
                }
                if args["include_screenshot"] == false {
                    for key in ["screenshot_png_b64", "screenshot_width", "screenshot_height", "screenshot_scale", "screenshots"] { d.as_object_mut().unwrap().remove(key); }
                } else {
                    // the driver's per-capture id (capture_log), distinct per call
                    d["screenshot_id"] = json!(format!("ftest-{}", self.calls.lock().unwrap().len()));
                }
                Ok(d)
            }
            "health_report" => Ok(json!({"schema_version": "1", "platform": "darwin", "driver_version": "0.24.0", "overall": "ok", "checks": [{"name": "tcc_accessibility", "status": "pass", "message": ""}, {"name": "tcc_screen_recording", "status": "pass", "message": ""}, {"name": "host_interactive", "status": if self.locked { "fail" } else { "pass" }, "message": "", "data": {"host": {"screen_locked": self.locked, "on_console": true}}}]})),
            // the macOS driver's refusal for an app that is not installed (launch_app.rs)
            "launch_app" if args["bundle_id"] == "dev.icu.golden.does-not-exist" => Err(CuaError { message: "No installed macOS app found for bundle_id 'dev.icu.golden.does-not-exist'.".into(), structured: Some(json!({"error": "APP_NOT_INSTALLED", "bundle_id": "dev.icu.golden.does-not-exist"})) }),
            "click" | "double_click" | "right_click" | "type_text" | "press_key" | "hotkey" | "scroll" | "set_value" | "drag" | "bring_to_front" | "launch_app" => {
                if self.locked { return Err(CuaError { message: format!("{tool} refused: the host session is not interactive (screen_locked)"), structured: Some(json!({"code": "host_not_interactive", "reasons": ["screen_locked"]})) }); }
                Ok(json!({"effect": "unverifiable", "route": "accessibility", "tool": tool, "delivery_mode": args.get("delivery_mode")}))
            }
            "verify_state" => Ok(json!({"verified": true, "expect": args.get("expect")})),
            "clipboard_read" => Ok(json!({"text": "PRIOR"})),
            "clipboard_write" => Ok(json!({"ok": true, "text": args.get("text")})),
            "set_agent_cursor_enabled" | "set_agent_cursor_theme" | "set_agent_cursor_motion" => Ok(json!({"ok": true})),
            "select_text" => Ok(json!({"selected": true, "text": args.get("text"), "location": 0, "length": 3})),
            "get_agent_cursor_state" => Ok(json!({"visible": true, "session": args.get("session")})),
            "start_session" => Ok(json!({"session": args.get("session").and_then(Value::as_str).unwrap_or("icu-test")})),
            "start_recording" | "stop_recording" | "get_recording_state" | "replay_trajectory" => Ok(json!({"recording": tool != "stop_recording", "tool": tool})),
            other => Err(CuaError { message: format!("unknown tool {other}"), structured: None }),
        }
    }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> { Ok(["list_windows", "list_apps", "get_window_state", "click", "type_text", "double_click", "right_click", "press_key", "hotkey", "scroll", "set_value", "drag", "bring_to_front", "launch_app", "health_report", "set_agent_cursor_enabled", "start_recording"].iter().map(|s| s.to_string()).collect()) }
}

/// The row ids of a rendered Sky tree (`\t\t12 button OK, ...`), after any pack.
pub(crate) fn tree_ids(tree: &str) -> Vec<u64> {
    let body = tree.split_once("</app_specific_instructions>\n").map_or(tree, |(_, b)| b);
    body.lines().filter_map(|l| l.trim_start_matches(['\t', '~', '+']).split(' ').next()?.parse().ok()).collect()
}

pub(crate) fn layer_with_log(locked: bool, require_approval: bool) -> (Layer, tempfile::TempDir, CallLog) {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = Config {
        driver: "fake".into(), socket: None, driver_daemon: true, state_dir: tmp.path().join("state"), memory_root: tmp.path().join("mem"),
        approvals_session: tmp.path().join("a/session.toml"), approvals_always: tmp.path().join("a/always.json"),
        denied_apps: vec!["com.apple.Safari".into()], forbidden_apps: vec![], require_app_approval: require_approval,
        allow_persistent_approval: true, risk_level: "medium".into(), session_id: "test".into(), mode: "attended".into(), default_deadline_ms: 15000, always_simulate_click: false, cursor: None, personal_packs_dir: tmp.path().join("packs"), personal_overrides_builtin: true, features: Features::default(),
    };
    let log: CallLog = Arc::new(Mutex::new(Vec::new()));
    (Layer::with_cua(cfg, Box::new(FakeCua { calls: log.clone(), locked })), tmp, log)
}

fn layer(locked: bool, require_approval: bool) -> (Layer, tempfile::TempDir) {
    let (l, t, _) = layer_with_log(locked, require_approval); (l, t)
}

/// The last call the fake driver saw for `tool`.
pub(crate) fn last(log: &CallLog, tool: &str) -> Value {
    log.lock().unwrap().iter().rev().find(|(t, _)| t == tool).map(|(_, a)| a.clone()).unwrap_or_else(|| panic!("no {tool} call"))
}

pub(crate) const CLOCK: fn() -> Value = || json!({"window": {"app": "com.apple.clock", "id": 25427}});

#[test]
fn tool_surface_is_the_window2_list_verbatim_plus_extras() {
    let names = tool_names();
    for w in WINDOW2_TOOLS { assert!(names.contains(&w.to_string()), "{w}"); }
    assert_eq!(&names[..13], &WINDOW2_TOOLS.iter().map(|s| s.to_string()).collect::<Vec<_>>()[..], "window2 names first, in the golden's order");
    let defs = tool_defs();
    let gws = defs.iter().find(|d| d["name"] == "get_window_state").unwrap();
    assert_eq!(gws["description"], "Capture selected state for an open window.");
    assert_eq!(gws["inputSchema"]["required"], json!(["window"]));
}

#[test]
fn pack_first_touch_is_per_session_not_per_state_dir() {
    // `icu mcp` mints a session per server start over one shared state dir.
    let (l, _t) = layer(false, false);
    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(false);
    let tree = |layer: &Layer| layer.dispatch("get_window_state", &a).unwrap()["accessibility"]["tree"].as_str().unwrap().to_string();
    let with_session = |session: &str| {
        let mut cfg = l.cfg.clone(); cfg.session_id = session.into();
        Layer::with_cua(cfg, Box::new(FakeCua { calls: Arc::new(Mutex::new(Vec::new())), locked: false }))
    };
    assert!(tree(&l).starts_with("<app_specific_instructions>\n"), "first touch in session test");
    assert!(!tree(&with_session("test")).contains("<app_specific_instructions>"), "a later one-shot call in the same session");
    assert!(tree(&with_session("mcp-2")).starts_with("<app_specific_instructions>\n"), "a new session gets its own first touch");
}

#[test]
fn get_window_state_returns_the_window2_shape_with_lineage_and_remembers_targets() {
    let (l, _t) = layer(false, false);
    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(false);
    let s1 = l.dispatch("get_window_state", &a).unwrap();
    assert_eq!(s1["window"]["app"], "com.apple.clock");
    assert_eq!(s1["window"]["id"], 25427);
    assert!(s1["screenshots"].as_array().unwrap().is_empty());
    let tree = s1["accessibility"]["tree"].as_str().unwrap();
    assert!(tree.starts_with("<app_specific_instructions>"), "first touch of a pack app carries the pack:\n{}", &tree[..80]);
    assert_eq!(s1["accessibilityRevision"], 1);
    let s2 = l.dispatch("get_window_state", &a).unwrap();
    assert!(s2["accessibility"]["tree"].as_str().unwrap().starts_with("There has been no change in the accessibility tree for "), "second turn is the no-change line");
    assert_eq!(s2["accessibilitySnapshotCount"], 2);
    // an element the model sees by stable id routes to the driver's token
    let stable = tree_ids(tree)[0];
    let mut c = CLOCK(); c["element_index"] = json!(stable);
    let r = l.dispatch("click", &c).unwrap();
    assert_eq!(r["tool"], "click");
    let mut bad = CLOCK(); bad["element_index"] = json!(999_999);
    let e = l.dispatch("click", &bad).unwrap_err();
    assert_eq!(e.code(), Some(CODE_STALE_ELEMENT), "our typed code stays in structured content");
    // Sky's string and number for an id the tree does not show (probe, sky-ipc.jsonl:54)
    assert_eq!(e.message, "Computer Use server error -10005: 999999 is an invalid element ID");
    assert_eq!(e.structured["number"], -10005);
    assert!(e.structured.get("reason").is_none(), "an id never shown is not called replaced");
    // an id an earlier tree showed and the latest does not: same Sky text, plus why
    let gone = tree_ids(tree)[1];
    { let mut t = l.targets.lock().unwrap(); let t = t.get_mut(&25427).unwrap(); t.rows.remove(&gone); t.elements.remove(&gone); }
    let mut old = CLOCK(); old["element_index"] = json!(gone);
    let e = l.dispatch("click", &old).unwrap_err();
    assert_eq!(e.message, format!("Computer Use server error -10005: {gone} is an invalid element ID"));
    assert_eq!(e.structured["reason"], "replaced");
    assert!(e.structured["hint"].as_str().unwrap().contains("re-created under a new id"));
    l.targets.lock().unwrap().clear();
    // a NEW process on the same state dir (one-shot `icu call`) can still act on the ids
    let fresh = Layer::with_cua(l.cfg.clone(), Box::new(FakeCua { calls: Arc::new(Mutex::new(Vec::new())), locked: false }));
    assert_eq!(fresh.dispatch("click", &c).unwrap()["tool"], "click");
    let (other, _t2) = layer(false, false);
    assert_eq!(other.dispatch("click", &c).unwrap_err().code(), Some(CODE_STALE_ELEMENT), "a different session has no targets");
    // both flags false is the verbatim window2 TypeError
    let mut both = CLOCK(); both["include_text"] = json!(false); both["include_screenshot"] = json!(false);
    let e = l.dispatch("get_window_state", &both).unwrap_err();
    assert_eq!(e.message, ERR_BOTH_FALSE);
}

#[test]
fn input_tools_map_onto_cua_and_need_a_window() {
    let (l, _t) = layer(false, false);
    l.dispatch("get_window_state", &CLOCK()).unwrap();
    assert_eq!(l.dispatch("click", &json!({"x": 1, "y": 2})).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT));
    let mut c = CLOCK(); c["x"] = json!(10); c["y"] = json!(20); c["mouse_button"] = json!("r");
    assert_eq!(l.dispatch("click", &c).unwrap()["tool"], "right_click");
    let mut d = CLOCK(); d["x"] = json!(10); d["y"] = json!(20); d["click_count"] = json!(2);
    assert_eq!(l.dispatch("click", &d).unwrap()["tool"], "double_click");
    let mut k = CLOCK(); k["key"] = json!("Control_L+a");
    assert_eq!(l.dispatch("press_key", &k).unwrap()["tool"], "hotkey");
    let mut k1 = CLOCK(); k1["key"] = json!("Return");
    assert_eq!(l.dispatch("press_key", &k1).unwrap()["tool"], "press_key");
    let mut s = CLOCK(); s["scrollX"] = json!(0); s["scrollY"] = json!(-120); s["x"] = json!(5); s["y"] = json!(5);
    assert_eq!(l.dispatch("scroll", &s).unwrap()["tool"], "scroll");
    let mut z = CLOCK(); z["scrollX"] = json!(0); z["scrollY"] = json!(0); z["x"] = json!(5); z["y"] = json!(5);
    assert_eq!(l.dispatch("scroll", &z).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT));
    let mut t = CLOCK(); t["text"] = json!("hi");
    assert_eq!(l.dispatch("type_text", &t).unwrap()["tool"], "type_text");
    let mut g = CLOCK(); g["from_x"] = json!(1); g["from_y"] = json!(1); g["to_x"] = json!(9);
    assert_eq!(l.dispatch("drag", &g).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT), "to_y missing");
    let unknown = json!({"window": {"app": "x", "id": 4242}, "text": "hi"});
    assert_eq!(l.dispatch("type_text", &unknown).unwrap_err().code(), Some(CODE_RUNNING_APPLICATION_NOT_FOUND));
}

#[test]
fn approval_gate_elicits_then_allows_and_policy_denies_verbatim() {
    let (l, _t) = layer(false, true);
    let mut t = CLOCK(); t["text"] = json!("hi");
    let e = l.dispatch("type_text", &t).unwrap_err();
    assert_eq!(e.code(), Some(CODE_APP_APPROVAL_REQUIRED));
    assert_eq!(e.structured["elicitation"]["message"], "Allow Computer Use to use \"Clock\"?");
    assert_eq!(e.structured["elicitation"]["meta"]["tool_name"], "type_text");
    assert_eq!(e.structured["elicitation"]["meta"]["persist"], json!(["session", "always"]));
    // decline is the verbatim "not approved" error and records nothing
    let d = l.dispatch("approve_app", &json!({"app": "com.apple.clock", "display_name": "Clock", "action": "decline"})).unwrap_err();
    assert_eq!(d.message, "Computer Use was not approved to use Clock");
    assert_eq!(l.dispatch("type_text", &t).unwrap_err().code(), Some(CODE_APP_APPROVAL_REQUIRED));
    l.dispatch("approve_app", &json!({"app": "com.apple.clock", "action": "accept", "persist": "session"})).unwrap();
    assert_eq!(l.dispatch("type_text", &t).unwrap()["tool"], "type_text");
    assert_eq!(std::fs::read_to_string(&l.cfg.approvals_session).unwrap(), "[apps]\nallowed = [\"com.apple.clock\"]\n");
    // org policy: denied app, verbatim refusal, before dispatch
    let safari = json!({"window": {"app": "com.apple.Safari", "id": 900}, "text": "hi"});
    let e = l.dispatch("type_text", &safari).unwrap_err();
    assert_eq!(e.code(), Some(CODE_APP_NOT_ALLOWED));
    assert_eq!(e.message, "Computer Use is blocked from using the app 'com.apple.Safari' by your organization's policy.");
    assert_eq!(l.dispatch("launch_app", &json!({"app": "com.apple.Safari"})).unwrap_err().code(), Some(CODE_APP_NOT_ALLOWED));
}

#[test]
fn driver_errors_with_structured_content_are_not_transport_errors() {
    use crate::cua::CuaError;
    // structured present but no `code` (an anyhow bail with exit_code) -> driverError
    let e = ToolError::from_cua(CuaError { message: "AX action failed: not advertised".into(), structured: Some(json!({"exit_code": 1})) });
    assert_eq!(e.code(), Some(CODE_DRIVER_ERROR));
    // no structured content at all -> transport error
    let e2 = ToolError::from_cua(CuaError { message: "cua-driver mcp exited".into(), structured: None });
    assert_eq!(e2.code(), Some(CODE_TRANSPORT_ERROR));
    // a known driver code still maps through
    let e3 = ToolError::from_cua(CuaError { message: "locked".into(), structured: Some(json!({"code": "host_not_interactive"})) });
    assert_eq!(e3.code(), Some(CODE_SCREEN_LOCKED));
}

#[test]
fn locked_host_surfaces_as_screen_locked_and_observation_still_answers() {
    let (l, _t) = layer(true, false);
    let mut t = CLOCK(); t["text"] = json!("hi");
    let e = l.dispatch("type_text", &t).unwrap_err();
    assert_eq!((e.code(), e.structured["driver_code"].as_str()), (Some(CODE_SCREEN_LOCKED), Some("host_not_interactive")));
    assert_eq!(l.dispatch("host_state", &json!({})).unwrap()["status"], "fail");
    let mut a = CLOCK(); a["include_text"] = json!(true);
    assert!(l.dispatch("get_window_state", &a).is_ok());
}

#[test]
fn mcp_server_frames_list_call_and_elicitation() {
    use crate::mcp::{Io, Server};
    let (l, _t) = layer(false, true);
    let mut srv = Server::new(&l);
    // a client WITH elicitation: the server asks, the client accepts for the session, the tool proceeds
    let input = format!("{}\n{}\n{}\n{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {"elicitation": {}}, "clientInfo": {"name": "t", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "type_text", "arguments": {"window": {"app": "com.apple.clock", "id": 25427}, "text": "hi"}}}),
    );
    // the elicitation answer must follow the server's request (id 1 from the server's counter)
    let input = input + &format!("{}\n", json!({"jsonrpc": "2.0", "id": 1, "result": {"action": "accept", "content": {"persist": "session"}}}));
    let mut reader = std::io::Cursor::new(input.into_bytes());
    let mut out: Vec<u8> = Vec::new();
    { let mut io = Io { reader: &mut reader, writer: &mut out }; srv.serve(&mut io).unwrap(); }
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "invisible-computer-use");
    assert_eq!(lines[1]["result"]["tools"].as_array().unwrap().len(), tool_defs().len());
    let elicit = &lines[2];
    assert_eq!(elicit["method"], "elicitation/create");
    assert_eq!(elicit["params"]["message"], "Allow Computer Use to use \"Clock\"?");
    assert_eq!(elicit["params"]["_meta"]["codex_approval_kind"], "mcp_tool_call");
    assert_eq!(lines[3]["id"], 3);
    assert_eq!(lines[3]["result"]["structuredContent"]["tool"], "type_text");
    assert!(l.approval_store().is_allowed("com.apple.clock"));

    // a client WITHOUT elicitation gets the structured error instead
    let (l2, _t2) = layer(false, true);
    let mut srv2 = Server::new(&l2);
    let input = format!("{}\n{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "type_text", "arguments": {"window": {"app": "com.apple.clock", "id": 25427}, "text": "hi"}}}));
    let mut reader = std::io::Cursor::new(input.into_bytes());
    let mut out: Vec<u8> = Vec::new();
    { let mut io = Io { reader: &mut reader, writer: &mut out }; srv2.serve(&mut io).unwrap(); }
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[1]["result"]["isError"], true);
    assert_eq!(lines[1]["result"]["structuredContent"]["code"], CODE_APP_APPROVAL_REQUIRED);
    assert!(lines[1]["result"]["structuredContent"]["elicitation"]["meta"].is_object());
}

#[test]
fn memory_service_capture_and_consolidate() {
    let (l, _t, _log) = layer_with_log(false, false);
    // capture: the fake's list_windows has Clock frontmost (on current space, on screen) with a bundle id
    let ev = l.events_path();
    assert_eq!(l.capture_step(&ev).unwrap(), true);
    assert_eq!(l.capture_step(&ev).unwrap(), false, "unchanged frontmost is not re-recorded");
    let line = std::fs::read_to_string(&ev).unwrap();
    assert!(line.contains("\"app_focus\"") && line.contains("com.apple.clock"), "{line}");
    assert!(!line.contains("Clock\",\"title") && !line.to_lowercase().contains("start page"), "no titles captured");
    // summarize that captured stream, then consolidate
    let first_ts = serde_json::from_str::<serde_json::Value>(line.lines().next().unwrap()).unwrap()["ts"].as_str().unwrap().to_string();
    l.dispatch("memory_summarize", &json!({"window": first_ts, "events_file": ev.to_str().unwrap()})).unwrap();
    let c = l.dispatch("memory_consolidate", &json!({})).unwrap();
    assert!(std::path::Path::new(c["memory_md"].as_str().unwrap()).exists());
    assert!(std::path::Path::new(c["user_profile"].as_str().unwrap()).exists());
    assert!(c["task_arc"].as_array().unwrap().iter().all(|l| l.as_str().unwrap().ends_with("[skysight memory]")));
}

#[test]
fn press_key_with_the_windows_key_is_hard_denied() {
    let (l, _t, log) = layer_on_platform(Some("win32"));
    // the Windows/Meta key is non-negotiably refused (spec 14 hard-deny), before any gate
    for k in ["Win+R", "Meta", "Super_L", "Cmd+space", "Windows+D", "ctrl+win+left"] {
        let e = l.dispatch("press_key", &json!({"key": k, "window": {"pid": 26765, "id": 1}, "os": "macos", "platform": "darwin", "approved": true})).unwrap_err();
        assert_eq!(e.code(), Some(crate::CODE_APP_NOT_ALLOWED), "not hard-denied: {k}");
        assert_eq!(e.structured["hard_denied"], json!(true), "{k}");
    }
    assert!(log.lock().unwrap().iter().all(|(tool, _)| tool == "health_report"), "denial must precede input and app/session setup");
    // an ordinary chord is never hard-denied (it may still fail downstream, but not with appNotAllowed)
    let ordinary = l.dispatch("press_key", &json!({"key": "Return", "window": {"pid": 26765, "id": 1}}));
    assert!(ordinary.as_ref().err().and_then(|e| e.code()) != Some(crate::CODE_APP_NOT_ALLOWED), "ordinary key wrongly hard-denied: {ordinary:?}");
}

struct PlatformCua { inner: Box<dyn Cua>, platform: Option<&'static str> }
impl Cua for PlatformCua {
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        let mut result = self.inner.call(tool, args)?;
        if tool == "health_report" {
            if self.platform == Some("unreachable") { return Err(CuaError { message: "fixture offline".into(), structured: None }); }
            match self.platform {
                Some(platform) => result["platform"] = json!(platform),
                None => { result.as_object_mut().unwrap().remove("platform"); }
            }
        }
        Ok(result)
    }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> { self.inner.tool_names() }
}

fn layer_on_platform(platform: Option<&'static str>) -> (Layer, tempfile::TempDir, CallLog) {
    let (l, tmp, log) = layer_with_log(false, false);
    (Layer::with_cua(l.cfg, Box::new(PlatformCua { inner: l.cua, platform })), tmp, log)
}

#[test]
fn windows_key_denial_does_not_guess_an_unknown_driver_platform() {
    for platform in [Some("win32"), Some("windows"), Some("unknown"), Some("unreachable"), None] {
        let (l, _t, log) = layer_on_platform(platform);
        for (tool, args) in [("press_key", json!({"key":"Cmd+c", "os":"macos"})), ("hotkey", json!({"keys":["Meta", "c"], "platform":"darwin", "approved":true}))] {
            let e = l.dispatch(tool, &args).unwrap_err();
            assert_eq!(e.structured["hard_denied"], true, "{platform:?}: {e:?}");
            assert!(e.structured["remediation"].as_str().unwrap().contains("cannot override"));
        }
        assert!(log.lock().unwrap().iter().all(|(tool, _)| tool == "health_report"));
    }
}

#[test]
fn windows_control_shortcuts_and_linux_meta_policy_are_preserved() {
    let (l, _t, log) = layer_on_platform(Some("win32"));
    let mut args = CLOCK(); args["key"] = json!("Control_L+c");
    l.dispatch("press_key", &args).unwrap();
    assert_eq!(last(&log, "hotkey")["keys"], json!(["ctrl", "c"]));
    let (linux, _t, _) = layer_on_platform(Some("linux"));
    assert!(linux.hard_deny("press_key", &json!({"key":"Super_L+c"})).is_ok());
}

#[test]
fn alt_chords_use_a_modifier_accepted_by_every_native_hotkey_parser() {
    for platform in ["win32", "darwin", "linux"] {
        let (l, _t, log) = layer_on_platform(Some(platform));
        for key in ["Alt_L+Tab", "Alt_R+Tab", "alt+Tab", "option+Tab"] {
            let mut args = CLOCK(); args["key"] = json!(key);
            l.dispatch("press_key", &args).unwrap();
            assert_eq!(last(&log, "hotkey")["keys"], json!(["alt", "tab"]),
                "{platform}/{key}: Windows hotkey parser does not classify option as a modifier");
        }
    }
}

#[test]
fn direct_input_cannot_bypass_windows_key_denial() {
    let (l, _t, log) = layer_on_platform(Some("win32"));
    let mut args = CLOCK(); args["key"] = json!("Win+r");
    let result = l.input("press_key", &args);
    assert!(result.as_ref().err().is_some_and(|e| e.structured["hard_denied"] == true),
        "direct input bypassed Windows policy: {result:?}; calls={:?}", log.lock().unwrap());
    assert!(log.lock().unwrap().iter().all(|(tool, _)| tool == "health_report"));
}

#[test]
fn direct_input_cannot_bypass_session_stop() {
    let (l, _t, log) = layer_with_log(false, false);
    l.dispatch("stop_session", &json!({})).unwrap();
    let mut args = CLOCK(); args["key"] = json!("Return");
    let result = l.input("press_key", &args);
    assert_eq!(result.as_ref().err().and_then(|e| e.code()), Some(CODE_USER_STOPPED_SESSION),
        "direct input bypassed Stop: {result:?}; calls={:?}", log.lock().unwrap());
    assert!(log.lock().unwrap().is_empty());
    l.dispatch("resume_session", &json!({})).unwrap();
    l.input("press_key", &args).unwrap();
    assert_eq!(last(&log, "press_key")["key"], "return");
}

#[test]
fn macos_command_shortcuts_reach_the_driver() {
    let (l, _t, log) = layer_with_log(false, false);
    for key in ["Cmd+c", "Command+c", "Meta_L+c", "Super_L+c"] {
        let mut args = CLOCK();
        args["key"] = json!(key);
        let result = l.dispatch("press_key", &args);
        assert!(result.is_ok(), "macOS shortcut {key} was refused: {result:?}");
        assert_eq!(last(&log, "hotkey")["keys"], json!(["cmd", "c"]));
    }
    assert_eq!(log.lock().unwrap().iter().filter(|(tool, _)| tool == "hotkey").count(), 4);
}

#[test]
fn capture_records_the_system_active_app_not_the_topmost_window() {
    // A helper window from another process sits above the active app's windows.
    struct Front;
    impl Cua for Front {
        fn call(&self, tool: &str, _args: Value) -> Result<Value, CuaError> {
            match tool {
                "list_apps" => Ok(json!({"apps": [
                    {"pid": 7, "bundle_id": "com.apple.CursorUIViewService", "name": "CursorUIViewService", "active": false},
                    {"pid": 8, "bundle_id": "com.apple.calculator", "name": "Calculator", "active": true}]})),
                "list_windows" => Ok(json!({"windows": [
                    {"window_id": 1, "pid": 7, "app_name": "CursorUIViewService", "title": "", "on_current_space": true, "is_on_screen": true},
                    {"window_id": 2, "pid": 8, "app_name": "Calculator", "title": "Calculator", "on_current_space": true, "is_on_screen": true}]})),
                _ => Err(CuaError { message: format!("unexpected {tool}"), structured: None }),
            }
        }
        fn tool_names(&self) -> Result<Vec<String>, CuaError> { Ok(vec![]) }
    }
    let (template, tmp, _log) = layer_with_log(false, false);
    let l = Layer::with_cua(template.cfg.clone(), Box::new(Front));
    let stream = tmp.path().join("front.jsonl");
    assert!(l.capture_step(&stream).unwrap());
    let text = std::fs::read_to_string(&stream).unwrap();
    assert!(text.contains("com.apple.calculator") && !text.contains("CursorUIViewService"), "{text}");
    assert!(!text.contains("title"), "{text}");
}

#[test]
fn capture_expires_event_records_past_the_retention_window() {
    let (l, tmp, _log) = layer_with_log(false, false);
    let stream = tmp.path().join("stream.jsonl");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let old = skysight::format_ts(now - 8 * 86_400);
    let recent = skysight::format_ts(now - 86_400);
    std::fs::write(&stream, format!(
        "{{\"ts\":\"{old}\",\"kind\":\"app_focus\",\"bundle_id\":\"com.expired.app\"}}\n{{\"ts\":\"{recent}\",\"kind\":\"app_focus\",\"bundle_id\":\"com.recent.app\"}}\n")).unwrap();
    l.capture_step(&stream).unwrap();
    let text = std::fs::read_to_string(&stream).unwrap();
    assert!(!text.contains("com.expired.app"), "an 8-day-old record outlived the 7-day retention:\n{text}");
    assert!(text.contains("com.recent.app") && text.contains("com.apple.clock"), "{text}");
}

#[test]
fn capture_loop_drives_the_driver_and_dedups_unchanged_focus() {
    let (l, tmp, _log) = layer_with_log(false, false);
    let stream = tmp.path().join("stream.jsonl");
    let stop = std::sync::atomic::AtomicBool::new(false);
    // the fake driver's frontmost app is constant, so the loop records exactly one focus event
    let n = l.capture_loop(&stream, 4, std::time::Duration::ZERO, &stop).unwrap();
    assert_eq!(n, 1);
    let text = std::fs::read_to_string(&stream).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("\"app_focus\"") && text.contains("com.apple.clock"));
    assert!(!text.to_lowercase().contains("title") && !text.to_lowercase().contains("start page"), "no titles or text");
}

#[test]
fn memory_serve_tick_summarizes_the_captured_stream_on_cadence() {
    let (l, _t, _log) = layer_with_log(false, false);
    let mut sched = skysight::schedule::Scheduler::new();
    let at = |s: &str| skysight::parse_ts(s).unwrap();
    // nothing captured yet: a tick is a no-op, not an error
    assert_eq!(l.memory_tick(&mut sched, at("2026-09-11T14:00:00Z")).unwrap()["written"], json!([]));
    std::fs::create_dir_all(&l.cfg.memory_root).unwrap();
    std::fs::write(l.events_path(), concat!(
        r#"{"ts":"2026-09-11T13:41:00Z","kind":"app_focus","bundle_id":"com.apple.Terminal"}"#, "\n",
        r#"{"ts":"2026-09-11T13:52:00Z","kind":"app_focus","bundle_id":"com.apple.Music"}"#, "\n")).unwrap();
    let t = l.memory_tick(&mut sched, at("2026-09-11T13:55:00Z")).unwrap();
    let written: Vec<String> = serde_json::from_value(t["written"].clone()).unwrap();
    assert_eq!(written.len(), 1, "{t}");
    assert!(written[0].starts_with("2026-09-11T13-40-00-") && written[0].contains("-10min-"), "{t}");
    assert!(!t["consolidated"].is_null(), "{t}");
    let t = l.memory_tick(&mut sched, at("2026-09-11T18:00:00Z")).unwrap();
    let written: Vec<String> = serde_json::from_value(t["written"].clone()).unwrap();
    assert_eq!(written.iter().map(|w| w.contains("-6h-")).collect::<Vec<_>>(), [false, true], "{t}");
    let memory = std::fs::read_to_string(l.cfg.memory_root.join("MEMORY.md")).unwrap();
    assert_eq!(memory.matches(skysight::PHASE2_TAG).count(), 3, "{memory}");
}

#[test]
fn memory_consolidate_preserves_the_host_memory_index() {
    let (l, _t, _log) = layer_with_log(false, false);
    // a pre-existing host MEMORY.md (the icu memory index) must survive consolidation
    std::fs::create_dir_all(&l.cfg.memory_root).unwrap();
    let host_line = "- [Build loop state](build-loop-state.md) host index line";
    std::fs::write(l.cfg.memory_root.join("MEMORY.md"), format!("# MEMORY\n\n{host_line}\n")).unwrap();
    let ev = l.events_path();
    l.capture_step(&ev).unwrap();
    let first_ts = serde_json::from_str::<serde_json::Value>(std::fs::read_to_string(&ev).unwrap().lines().next().unwrap()).unwrap()["ts"].as_str().unwrap().to_string();
    l.dispatch("memory_summarize", &json!({"window": first_ts, "events_file": ev.to_str().unwrap()})).unwrap();
    l.dispatch("memory_consolidate", &json!({})).unwrap();
    let mem = std::fs::read_to_string(l.cfg.memory_root.join("MEMORY.md")).unwrap();
    assert!(mem.contains(host_line), "consolidate clobbered the host memory index:\n{mem}");
    assert!(mem.contains("## Skysight activity"), "the skysight section was merged in");
}

#[test]
fn memory_and_policy_tools_answer() {
    let (l, _t) = layer(false, false);
    assert_eq!(l.dispatch("memory_list", &json!({})).unwrap(), json!([]));
    let ev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/fixtures/skysight_events.jsonl");
    let first_ts = std::fs::read_to_string(&ev).unwrap().lines().next().map(|l| serde_json::from_str::<Value>(l).unwrap()["ts"].as_str().unwrap().to_string()).unwrap();
    let w = l.dispatch("memory_summarize", &json!({"window": first_ts, "events_file": ev})).unwrap();
    assert!(w["file"].as_str().unwrap().contains("-10min-"));
    assert_eq!(l.dispatch("memory_list", &json!({})).unwrap().as_array().unwrap().len(), 1);
    let p = l.dispatch("confirmations_policy", &json!({})).unwrap();
    assert!(p["policy"].as_str().unwrap().contains("Hand-Off Required"));
    let d = l.dispatch("confirmation_decision", &json!({"tag": "2.4"})).unwrap();
    assert_eq!(d["decision"], "HandOff");
    let d = l.dispatch("confirmation_decision", &json!({"tag": "14", "pre_approval": "user"})).unwrap();
    assert_eq!(d["decision"], "Proceed");
}

#[test]
fn paste_saves_writes_pastes_and_restores_the_clipboard() {
    let (l, _t, log) = layer_with_log(false, false);
    let mut p = CLOCK(); p["text"] = json!("hello");
    l.dispatch("paste", &p).unwrap();
    let writes: Vec<Value> = log.lock().unwrap().iter().filter(|(t, _)| t == "clipboard_write").map(|(_, a)| a.clone()).collect();
    assert_eq!(writes.len(), 2, "write our text, then restore");
    assert_eq!(writes[0]["text"], "hello");
    assert_eq!(writes[1]["text"], "PRIOR", "prior clipboard restored");
    // the paste chord went through hotkey (cmd on darwin, the fake reports darwin)
    assert_eq!(last(&log, "hotkey")["keys"], json!(["cmd", "v"]));
}

#[test]
fn simulate_click_uses_the_element_pixel_center() {
    let (mut cfg, _t, log) = { let (l, t, log) = layer_with_log(false, false); (l.cfg.clone(), t, log) };
    cfg.always_simulate_click = true;
    let l = Layer::with_cua(cfg, Box::new(FakeCua { calls: log.clone(), locked: false }));
    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(true);
    let s1 = l.dispatch("get_window_state", &a).unwrap();
    let tree = s1["accessibility"]["tree"].as_str().unwrap().to_string();
    // an id that has a frame center: the click goes by pixel (x,y), not by element_token
    let ids: Vec<u64> = tree_ids(&tree);
    let mut clicked_by_pixel = false;
    for id in &ids {
        let mut c = CLOCK(); c["element_index"] = json!(id);
        if l.dispatch("click", &c).is_ok() {
            let last = last(&log, "click");
            if last.get("x").is_some() && last.get("element_token").is_none() { clicked_by_pixel = true; break; }
        }
    }
    assert!(clicked_by_pixel, "simulate_click sent pixel x,y for an element with a frame");
}

#[test]
fn get_window_state_carries_the_screenshot_classifier_verdict() {
    let (l, _t) = layer(false, false);
    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(false);
    let s = l.dispatch("get_window_state", &a).unwrap();
    assert!(s["_screenshot_needed"].is_boolean(), "advisory verdict is always present");
    assert!(s["_screenshot_reason"].as_str().unwrap().len() > 0);
    // explicit include_screenshot=false is honored regardless of the verdict
    assert!(s["screenshots"].as_array().unwrap().is_empty());
    // auto_screenshot with no explicit flag lets the classifier decide; the verdict and the
    // presence of screenshots agree
    let mut b = CLOCK(); b["include_text"] = json!(true); b["auto_screenshot"] = json!(true);
    let s2 = l.dispatch("get_window_state", &b).unwrap();
    let needed = s2["_screenshot_needed"].as_bool().unwrap();
    assert_eq!(!s2["screenshots"].as_array().unwrap().is_empty(), needed, "auto honors the classifier: {needed}");
}

#[test]
fn mode_delivery_verify_settle_and_knobs() {
    use crate::exec::Mode;
    // headless forces background and refuses an explicit foreground
    let (mut cfg, _t, log) = { let (l, t, log) = layer_with_log(false, false); (l.cfg.clone(), t, log) };
    cfg.mode = "headless".into();
    let l = Layer::with_cua(cfg.clone(), Box::new(FakeCua { calls: log.clone(), locked: false }));
    l.dispatch("get_window_state", &CLOCK()).unwrap();
    let mut c = CLOCK(); c["x"] = json!(10); c["y"] = json!(10);
    let r = l.dispatch("click", &c).unwrap();
    assert_eq!(last(&log, "click")["delivery_mode"], "background");
    assert!(r["_elapsed_ms"].is_number());
    let mut fg = CLOCK(); fg["x"] = json!(1); fg["y"] = json!(1); fg["delivery_mode"] = json!("foreground");
    assert_eq!(l.dispatch("click", &fg).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT), "headless refuses foreground");
    // attended defaults foreground; verify predicates run after and attach
    let la = Layer::with_cua({ let mut c = cfg.clone(); c.mode = "attended".into(); c }, Box::new(FakeCua { calls: log.clone(), locked: false }));
    let mut t = CLOCK(); t["text"] = json!("hi"); t["verify"] = json!([{"kind": "value", "element_index": 1, "equals": "hi"}]);
    let r = la.dispatch("type_text", &t).unwrap();
    assert_eq!(last(&log, "type_text")["delivery_mode"], "foreground");
    assert_eq!(r["_verify"]["verified"], true);
    assert_eq!(Mode::parse(&cfg.mode).delivery(), "background");
    // cursor + recording knobs pass through to the driver session
    assert_eq!(la.dispatch("cursor", &json!({"enabled": true})).unwrap()["visible"], true);
    assert_eq!(last(&log, "set_agent_cursor_enabled")["enabled"], true);
    assert!(log.lock().unwrap().iter().any(|(t, _)| t == "start_session"), "a driver session is started before the cursor call");
    assert_ne!(last(&log, "set_agent_cursor_enabled")["session"], "default", "cursor uses the started session, not a bare label");
    assert_eq!(la.dispatch("record_start", &json!({"output_dir": "/tmp/rec"})).unwrap()["recording"], true);
    assert_eq!(la.dispatch("verify_state", &json!({"window": CLOCK()["window"], "expect": [{"kind": "exists"}]})).unwrap()["verified"], true);
}

#[test]
fn attended_input_detaches_the_cursor_and_sets_the_fast_preset() {
    // FIX 1 (spec 08 detachComputerUseCursor): in attended mode the first input tool must
    // enable the cursor DETACHED (so the action does not block on the ~1.4s glide) and apply the
    // fast motion preset. Headless / cursor-off never enables it, so it cannot pay the glide.
    let (cfg, _t, log) = { let (l, t, log) = layer_with_log(false, false); (l.cfg.clone(), t, log) };
    let la = Layer::with_cua({ let mut c = cfg.clone(); c.mode = "attended".into(); c }, Box::new(FakeCua { calls: log.clone(), locked: false }));
    la.dispatch("get_window_state", &CLOCK()).unwrap();
    let mut c = CLOCK(); c["x"] = json!(10); c["y"] = json!(10);
    la.dispatch("click", &c).unwrap();
    let enable = last(&log, "set_agent_cursor_enabled");
    assert_eq!(enable["enabled"], true, "attended enables the cursor");
    assert_eq!(enable["detached"], true, "attended detaches the cursor from the action path");
    let motion = last(&log, "set_agent_cursor_motion");
    assert_eq!(motion["glide_duration_ms"], 200, "fast fixed glide, not the ~1.4s speed-based glide");
    assert_eq!(motion["idle_hide_ms"], 1500, "cursor fades quickly after the action");

    // headless: the cursor is disabled, so no glide is ever paid and detached is moot
    let lh = Layer::with_cua({ let mut c = cfg.clone(); c.mode = "headless".into(); c }, Box::new(FakeCua { calls: log.clone(), locked: false }));
    let mut c2 = CLOCK(); c2["x"] = json!(5); c2["y"] = json!(5);
    lh.dispatch("click", &c2).unwrap();
    assert_eq!(last(&log, "set_agent_cursor_enabled")["enabled"], false, "headless keeps the cursor off");
}

#[test]
fn select_text_maps_element_prefix_and_suffix_to_the_driver() {
    // FIX 5 (spec 06): the window2 select_text maps element_index -> the driver's
    // element addressing and passes text/prefix/suffix through, gated like any input.
    let (l, _t, log) = layer_with_log(false, false);
    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(false);
    let s1 = l.dispatch("get_window_state", &a).unwrap();
    let tree = s1["accessibility"]["tree"].as_str().unwrap();
    let stable: u64 = tree_ids(tree)[0];
    let mut c = CLOCK(); c["element_index"] = json!(stable); c["text"] = json!("cat"); c["prefix"] = json!("dog "); c["suffix"] = json!(" bird");
    let r = l.dispatch("select_text", &c).unwrap();
    assert_eq!(r["selected"], true);
    let sent = last(&log, "select_text");
    assert_eq!(sent["text"], "cat");
    assert_eq!(sent["prefix"], "dog ");
    assert_eq!(sent["suffix"], " bird");
    // it addressed a real element (token or index+snapshot), not a bare stable id
    assert!(sent.get("element_token").is_some() || (sent.get("element_index").is_some() && sent.get("snapshot_id").is_some()), "select_text addressed the driver element: {sent}");
    // no delivery_mode on this AX-only mutation (like set_value)
    assert!(sent.get("delivery_mode").is_none(), "select_text is AX-only, no delivery_mode");
    // missing text is an invalid argument
    let mut bad = CLOCK(); bad["element_index"] = json!(stable);
    assert_eq!(l.dispatch("select_text", &bad).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT));
    // element_index is optional: omitting it targets the focused element (no element addressing sent)
    let mut foc = CLOCK(); foc["text"] = json!("cat");
    l.dispatch("select_text", &foc).unwrap();
    let sent2 = last(&log, "select_text");
    assert_eq!(sent2["text"], "cat");
    assert!(sent2.get("element_token").is_none() && sent2.get("element_index").is_none(), "focused fallback sends no element addressing: {sent2}");
}

#[test]
fn type_text_addresses_the_indexed_element_not_the_current_focus() {
    // B4 (review3/mac): an element_index on type_text was dropped, so the driver typed into
    // the app's key window. It must reach the driver as element addressing.
    let (l, _t, log) = layer_with_log(false, false);
    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(false);
    let s1 = l.dispatch("get_window_state", &a).unwrap();
    let stable: u64 = tree_ids(s1["accessibility"]["tree"].as_str().unwrap())[0];
    let mut c = CLOCK(); c["element_index"] = json!(stable); c["text"] = json!("ZQX");
    l.dispatch("type_text", &c).unwrap();
    let sent = last(&log, "type_text");
    assert_eq!(sent["text"], "ZQX");
    assert!(sent.get("element_token").is_some() || (sent.get("element_index").is_some() && sent.get("snapshot_id").is_some()), "type_text addressed the driver element: {sent}");
    // an id the latest tree does not show is refused before the driver is called
    let mut stale = CLOCK(); stale["element_index"] = json!(99999); stale["text"] = json!("x");
    let calls = log.lock().unwrap().len();
    assert_eq!(l.dispatch("type_text", &stale).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    assert!(log.lock().unwrap()[calls..].iter().all(|(t, _)| t != "type_text"), "nothing typed for a stale id");
    // without element_index it still types into the current focus
    let mut foc = CLOCK(); foc["text"] = json!("a");
    l.dispatch("type_text", &foc).unwrap();
    let sent2 = last(&log, "type_text");
    assert!(sent2.get("element_token").is_none() && sent2.get("element_index").is_none(), "{sent2}");
}

#[test]
fn cua_arguments_are_exact_tokens_scaled_pixels_and_chords() {
    let (l, _t, log) = layer_with_log(false, false);
    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(false);
    let s1 = l.dispatch("get_window_state", &a).unwrap();
    let tree = s1["accessibility"]["tree"].as_str().unwrap().to_string();
    // an actionable element: its stable id routes to THIS snapshot's token
    let doc = fixture("clock_gws.json");
    let first_actionable = doc["elements"].as_array().unwrap().iter().find(|e| e.get("element_index").is_some()).unwrap();
    let token = first_actionable["element_token"].as_str().unwrap();
    let source_ix = first_actionable["element_index"].as_i64().unwrap();
    // find the stable id for that source index through the render: the layer's map is private, so
    // act on each id in the tree until the fake receives the token
    let ids: Vec<u64> = tree_ids(&tree);
    let mut routed = false;
    for id in &ids {
        let mut c = CLOCK(); c["element_index"] = json!(id);
        if l.dispatch("click", &c).is_ok() && last(&log, "click")["element_token"] == token { routed = true; break; }
    }
    assert!(routed, "no stable id routed to the token of source index {source_ix}");
    // A text-only observation has no image geometry. Capture before pixel input.
    l.dispatch("get_window_state", &CLOCK()).unwrap();
    // The 2x backing image is resized to 1568 / 1024 pixels per logical point.
    let mut c = CLOCK(); c["x"] = json!(100); c["y"] = json!(50);
    l.dispatch("click", &c).unwrap();
    assert_eq!((last(&log, "click")["x"].as_f64(), last(&log, "click")["y"].as_f64()), (Some(153.125), Some(76.5625)));
    let mut g = CLOCK(); g["from_x"] = json!(1); g["from_y"] = json!(2); g["to_x"] = json!(3); g["to_y"] = json!(4);
    l.dispatch("drag", &g).unwrap();
    assert_eq!(last(&log, "drag")["to_y"], json!(6.125));
    // chords and keys
    let mut k = CLOCK(); k["key"] = json!("Control_L+Shift_L+period");
    l.dispatch("press_key", &k).unwrap();
    assert_eq!(last(&log, "hotkey")["keys"], json!(["ctrl", "shift", "."]));
    // scroll: both axes, vertical first, notches from 40 px, at the scaled point
    let mut s = CLOCK(); s["scrollX"] = json!(-80); s["scrollY"] = json!(120); s["x"] = json!(10); s["y"] = json!(10);
    l.dispatch("scroll", &s).unwrap();
    let scrolls: Vec<Value> = log.lock().unwrap().iter().filter(|(t, _)| t == "scroll").map(|(_, a)| a.clone()).collect();
    assert_eq!(scrolls.len(), 2);
    assert_eq!((scrolls[0]["direction"].as_str(), scrolls[0]["amount"].as_u64(), scrolls[0]["x"].as_f64()), (Some("down"), Some(3), Some(15.3125)));
    assert_eq!((scrolls[1]["direction"].as_str(), scrolls[1]["amount"].as_u64()), (Some("left"), Some(2)));
    // set_value routes the element and carries the value; perform_secondary_action maps known labels only
    let mut v = CLOCK(); v["element_index"] = json!(ids[1]); v["value"] = json!("9");
    match l.dispatch("set_value", &v) { Ok(_) => assert_eq!(last(&log, "set_value")["value"], "9"), Err(e) => assert_eq!(e.code(), Some(CODE_STALE_ELEMENT)) }
    // perform_secondary_action sends a listed label as the AX action it names (the patched
    // driver would also take Raise / Scroll *, but not every label Sky shows)
    let mut p = CLOCK(); p["element_index"] = json!(ids[1]); p["action"] = json!("Scroll Up");
    match l.dispatch("perform_secondary_action", &p) {
        Ok(_) => assert_eq!(last(&log, "click")["action"], "AXScrollUpByPage"),
        Err(e) => assert_eq!(e.code(), Some(CODE_STALE_ELEMENT)),
    }
    // still requires an element target
    let mut pn = CLOCK(); pn["action"] = json!("Raise");
    assert_eq!(l.dispatch("perform_secondary_action", &pn).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT));
    // screenshotId must be cached for the window
    let mut sid = CLOCK(); sid["x"] = json!(1); sid["y"] = json!(1); sid["screenshotId"] = json!("nope");
    assert_eq!(l.dispatch("click", &sid).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    let mut a2 = CLOCK(); a2["include_screenshot"] = json!(true);
    let s2 = l.dispatch("get_window_state", &a2).unwrap();
    let real_sid = s2["screenshots"][0]["id"].as_str().unwrap().to_string();
    sid["screenshotId"] = json!(real_sid);
    assert!(l.dispatch("click", &sid).is_ok());
    // a screenshot-only observation still refreshes the targets (the snapshot moved on)
    assert!(l.cfg.state_dir.join("25427.targets.json").exists());
}

/// mac-r4 note: `screenshotId` was checked against icu's cache and dropped, so every
/// pixel action reached the driver unbound and its `ambiguous_capture` advice ("pass
/// the screenshot_id") could not be followed through icu. A valid `screenshotId` now
/// goes to the driver as that capture's own `screenshot_id` on every pixel request.
#[test]
fn a_screenshot_id_binds_pixel_actions_to_the_drivers_capture() {
    let (l, _t, log) = layer_with_log(false, false);
    let mut a = CLOCK(); a["include_screenshot"] = json!(true); a["include_text"] = json!(true);
    let state = l.dispatch("get_window_state", &a).unwrap();
    let element = tree_ids(state["accessibility"]["tree"].as_str().unwrap())[1];
    let sid = state["screenshots"][0]["id"].as_str().unwrap().to_string();
    let driver_id = log.lock().unwrap().iter().rposition(|(t, _)| t == "get_window_state").map(|n| format!("ftest-{}", n + 1)).unwrap();
    let with = |mut args: Value| { args["screenshotId"] = json!(sid); args };
    l.dispatch("click", &with(json!({"window": CLOCK()["window"], "x": 10, "y": 20}))).unwrap();
    assert_eq!(last(&log, "click")["screenshot_id"], json!(driver_id));
    l.dispatch("click", &with(json!({"window": CLOCK()["window"], "x": 10, "y": 20, "click_count": 2}))).unwrap();
    assert_eq!(last(&log, "double_click")["screenshot_id"], json!(driver_id));
    l.dispatch("click", &with(json!({"window": CLOCK()["window"], "x": 10, "y": 20, "mouse_button": "right"}))).unwrap();
    assert_eq!(last(&log, "right_click")["screenshot_id"], json!(driver_id));
    l.dispatch("scroll", &with(json!({"window": CLOCK()["window"], "x": 10, "y": 20, "scrollY": 80, "scrollX": 40}))).unwrap();
    assert!(log.lock().unwrap().iter().filter(|(t, _)| t == "scroll").all(|(_, a)| a["screenshot_id"] == json!(driver_id)));
    l.dispatch("drag", &with(json!({"window": CLOCK()["window"], "from_x": 1, "from_y": 2, "to_x": 3, "to_y": 4}))).unwrap();
    assert_eq!(last(&log, "drag")["screenshot_id"], json!(driver_id));
    // without screenshotId nothing is bound; an element click never carries one
    l.dispatch("click", &json!({"window": CLOCK()["window"], "x": 10, "y": 20})).unwrap();
    assert!(last(&log, "click").get("screenshot_id").is_none());
    l.dispatch("click", &with(json!({"window": CLOCK()["window"], "element_index": element}))).unwrap();
    let args = last(&log, "click");
    assert!(args.get("element_index").is_some() || args.get("element_token").is_some(), "{args}");
    assert!(args.get("screenshot_id").is_none(), "{args}");
    // a newer capture rebinds the same surface id to its own driver id
    l.dispatch("get_window_state", &a).unwrap();
    let newer = log.lock().unwrap().iter().rposition(|(t, _)| t == "get_window_state").map(|n| format!("ftest-{}", n + 1)).unwrap();
    assert_ne!(newer, driver_id);
    l.dispatch("click", &with(json!({"window": CLOCK()["window"], "x": 10, "y": 20}))).unwrap();
    assert_eq!(last(&log, "click")["screenshot_id"], json!(newer));
}

#[test]
fn app_facts_are_cached_so_the_gate_does_not_relist_apps_per_action() {
    let (l, _t, log) = layer_with_log(false, false);
    l.dispatch("get_window_state", &CLOCK()).unwrap();
    let win = json!({"app": "com.apple.clock", "id": 25427});
    // first action resolves the app (list_apps); subsequent actions on the same window reuse it
    l.dispatch("click", &json!({"window": win, "x": 1, "y": 1})).unwrap();
    let after_first = log.lock().unwrap().iter().filter(|(t, _)| t == "list_apps").count();
    l.dispatch("click", &json!({"window": win, "x": 2, "y": 2})).unwrap();
    l.dispatch("type_text", &json!({"window": win, "text": "hi"})).unwrap();
    let after_more = log.lock().unwrap().iter().filter(|(t, _)| t == "list_apps").count();
    assert_eq!(after_first, after_more, "the app's identity is cached; no extra list_apps per action");
}

#[test]
fn gate_covers_named_launches_identity_less_apps_and_activate_window() {
    let (l, _t, log) = layer_with_log(false, true);
    // a denied app launched by display name is still denied (resolved through list_apps)
    let e = l.dispatch("launch_app", &json!({"app": "safari"})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_APP_NOT_ALLOWED));
    assert!(log.lock().unwrap().iter().all(|(t, _)| t != "launch_app"), "never reached the driver");
    l.dispatch("launch_app", &json!({"app": "Clock"})).unwrap();
    assert_eq!(last(&log, "launch_app")["bundle_id"], "com.apple.clock", "a name resolves to its bundle id");
    // an app the driver does not list is still gated, keyed by its window's app name
    let e = l.dispatch("type_text", &json!({"window": {"app": "Helper", "id": 77}, "text": "x"})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_APP_APPROVAL_REQUIRED));
    assert_eq!(e.structured["app"], "Helper");
    // activate_window respects the org policy
    let e = l.dispatch("activate_window", &json!({"window": {"app": "com.apple.Safari", "id": 900}})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_APP_NOT_ALLOWED));
    assert!(log.lock().unwrap().iter().all(|(t, _)| t != "bring_to_front"));
    // approve_app cannot approve a policy-denied app either
    assert_eq!(l.dispatch("approve_app", &json!({"app": "com.apple.Safari", "action": "accept"})).unwrap_err().code(), Some(CODE_APP_NOT_ALLOWED));
}

#[test]
fn mcp_refuses_approve_app_negotiates_version_and_keeps_interleaved_answers() {
    use crate::mcp::{Io, Server};
    let (l, _t) = layer(false, true);
    let mut srv = Server::new(&l);
    let input = format!("{}\n{}\n{}\n{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {"elicitation": {}}, "clientInfo": {"name": "t", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "approve_app", "arguments": {"app": "com.apple.clock"}}}),
        // call A elicits (server id 1); before answering, the client sends call B on another app (server id 2);
        // the answers come back B first, then A: A's answer must not be lost
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "type_text", "arguments": {"window": {"app": "com.apple.clock", "id": 25427}, "text": "a"}}}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "type_text", "arguments": {"window": {"app": "Helper", "id": 77}, "text": "b"}}}),
    ) + &format!("{}\n{}\n",
        // A's answer arrives while the server is waiting for B's: it must be kept for A
        json!({"jsonrpc": "2.0", "id": 1, "result": {"action": "accept", "content": {"persist": "session"}}}),
        json!({"jsonrpc": "2.0", "id": 2, "result": {"action": "accept", "content": {"persist": "session"}}}));
    let mut reader = std::io::Cursor::new(input.into_bytes());
    let mut out: Vec<u8> = Vec::new();
    { let mut io = Io { reader: &mut reader, writer: &mut out }; srv.serve(&mut io).unwrap(); }
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(lines[1]["result"]["isError"], true, "approve_app is not an MCP tool");
    assert!(lines[1]["result"]["content"][0]["text"].as_str().unwrap().contains("not callable over MCP"));
    let by_id = |id: u64| lines.iter().find(|m| m.get("id").and_then(Value::as_u64) == Some(id) && m.get("method").is_none()).cloned();
    assert_eq!(by_id(3).unwrap()["result"]["structuredContent"]["tool"], "type_text", "call A completed with its own answer");
    assert_eq!(by_id(4).unwrap()["result"]["structuredContent"]["tool"], "type_text", "call B completed");
    assert!(l.approval_store().is_allowed("com.apple.clock") && l.approval_store().is_allowed("Helper"));
}

#[test]
fn default_driver_prefers_our_installed_patched_build() {
    // ICU_DRIVER unset -> resolve to ~/.local/bin/cua-driver when it's installed
    // (our patched build, via stage-driver.sh / install.sh), else bare `cua-driver`.
    let tmp = std::env::temp_dir().join(format!("icu-cfg-{}", std::process::id()));
    let bin = tmp.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();

    // Not installed: fall back to a plain PATH lookup.
    assert_eq!(default_driver(&tmp), "cua-driver", "no installed driver -> bare name");

    // Installed: use the explicit path so it doesn't hinge on PATH ordering.
    let installed = bin.join("cua-driver");
    std::fs::write(&installed, b"x").unwrap();
    assert_eq!(
        default_driver(&tmp),
        installed.to_string_lossy(),
        "installed driver -> explicit ~/.local/bin/cua-driver path"
    );

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn logical_points_follow_the_returned_screenshot_size() {
    let (l, _t, log) = layer_with_log(false, false);
    let mut state = CLOCK();
    state["include_text"] = json!(true);
    state["include_screenshot"] = json!(true);
    l.dispatch("get_window_state", &state).unwrap();
    // Captured Clock metadata: 1024 logical points, 2048 backing pixels,
    // returned at 1568 pixels. Native click reverses that resize, then Retina.
    let mut click = CLOCK(); click["x"] = json!(100); click["y"] = json!(50);
    l.dispatch("click", &click).unwrap();
    let sent = last(&log, "click");
    assert_eq!(sent["x"], json!(153.125), "logical points were multiplied by backing scale instead of returned-image scale");
    assert_eq!(sent["y"], json!(76.5625));
    let native_x = sent["x"].as_f64().unwrap() * (2048.0 / 1568.0) / 2.0;
    assert!((native_x - 100.0).abs() < 1e-9);

    click["click_count"] = json!(2);
    l.dispatch("click", &click).unwrap();
    assert_eq!(last(&log, "double_click")["x"], json!(153.125));
    click["mouse_button"] = json!("r");
    l.dispatch("click", &click).unwrap();
    assert_eq!(last(&log, "right_click")["y"], json!(76.5625));

    let mut drag = CLOCK();
    for (k, v) in [("from_x", 100), ("from_y", 50), ("to_x", 200), ("to_y", 100)] { drag[k] = json!(v); }
    l.dispatch("drag", &drag).unwrap();
    let sent = last(&log, "drag");
    assert_eq!((sent["from_x"].as_f64(), sent["from_y"].as_f64(), sent["to_x"].as_f64(), sent["to_y"].as_f64()),
        (Some(153.125), Some(76.5625), Some(306.25), Some(153.125)));

    let mut scroll = CLOCK(); scroll["x"] = json!(100); scroll["y"] = json!(50); scroll["scrollY"] = json!(40);
    l.dispatch("scroll", &scroll).unwrap();
    assert_eq!(last(&log, "scroll")["y"], json!(76.5625));

    // The AXWindow center is (512,384) relative to the fixture's (352,98) origin.
    let stable = l.targets.lock().unwrap()[&25427].elements.iter().find(|(_, (ix, _))| *ix == 0).map(|(id, _)| *id).unwrap();
    let mut simulated = CLOCK(); simulated["element_index"] = json!(stable); simulated["simulate_click"] = json!(true);
    l.dispatch("click", &simulated).unwrap();
    assert_eq!((last(&log, "click")["x"].as_f64(), last(&log, "click")["y"].as_f64()), (Some(784.0), Some(588.0)));
    assert!(last(&log, "click").get("element_token").is_none());

    // Separate CLI invocations load the corrected units from disk.
    let fresh = Layer::with_cua(l.cfg.clone(), Box::new(FakeCua { calls: log.clone(), locked: false }));
    fresh.dispatch("drag", &drag).unwrap();
    assert_eq!(last(&log, "drag")["to_x"], json!(306.25));
    fresh.dispatch("click", &simulated).unwrap();
    assert_eq!(last(&log, "click")["x"], json!(784.0));
}

#[test]
fn pixel_input_refuses_missing_text_only_and_legacy_geometry() {
    let (l, _t, log) = layer_with_log(false, false);
    let mut click = CLOCK(); click["x"] = json!(100); click["y"] = json!(50);
    let mut scroll = click.clone(); scroll["scrollY"] = json!(40);
    let mut drag = CLOCK();
    for key in ["from_x", "from_y", "to_x", "to_y"] { drag[key] = json!(100); }
    for (tool, args) in [("click", &click), ("scroll", &scroll), ("drag", &drag)] {
        assert_eq!(l.dispatch(tool, args).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    }
    l.dispatch("get_window_state", &CLOCK()).unwrap();
    let mut legacy: Value = serde_json::from_slice(&std::fs::read(l.targets_path(25427)).unwrap()).unwrap();
    let stable = l.targets.lock().unwrap()[&25427].elements.iter().find(|(_, (ix, _))| *ix == 0).map(|(id, _)| *id).unwrap();
    legacy.as_object_mut().unwrap().remove("pixel_scale");
    legacy.as_object_mut().unwrap().remove("centers_px");
    legacy["scale"] = json!(2.0);
    legacy["centers"] = json!({stable.to_string(): [1024.0, 768.0]});
    std::fs::write(l.targets_path(25427), serde_json::to_vec(&legacy).unwrap()).unwrap();
    let fresh = Layer::with_cua(l.cfg.clone(), Box::new(FakeCua { calls: log.clone(), locked: false }));
    assert_eq!(fresh.dispatch("click", &click).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    let mut simulated = CLOCK(); simulated["element_index"] = json!(stable); simulated["simulate_click"] = json!(true);
    assert_eq!(fresh.dispatch("click", &simulated).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    assert!(!log.lock().unwrap().iter().any(|(tool, _)| ["click", "scroll", "drag", "get_screen_size"].contains(&tool.as_str())));
    simulated["simulate_click"] = json!(false);
    fresh.dispatch("click", &simulated).unwrap();
    assert!(last(&log, "click").get("element_token").is_some(), "legacy AX tokens retain their units");

    let mut text_only = CLOCK(); text_only["include_text"] = json!(true); text_only["include_screenshot"] = json!(false);
    l.dispatch("get_window_state", &text_only).unwrap();
    assert_eq!(l.dispatch("click", &click).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    simulated["simulate_click"] = json!(true);
    assert_eq!(l.dispatch("click", &simulated).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    let fresh = Layer::with_cua(l.cfg.clone(), Box::new(FakeCua { calls: log.clone(), locked: false }));
    assert_eq!(fresh.dispatch("click", &click).unwrap_err().code(), Some(CODE_STALE_ELEMENT));
    fresh.dispatch("get_window_state", &CLOCK()).unwrap();
    fresh.dispatch("click", &click).unwrap();
    assert_eq!(last(&log, "click")["x"], json!(153.125));
}

#[test]
fn returned_geometry_requires_finite_coherent_image_dimensions() {
    for (width, height, expected) in [(1024, 768, 1.0), (2048, 1536, 2.0), (1568, 1176, 1.53125), (333, 249, 333.0 / 1024.0)] {
        let mut doc = fixture("clock_gws.json");
        doc["screenshot_width"] = json!(width); doc["screenshot_height"] = json!(height);
        doc["screenshot_scale"] = json!(99); // backing/primary-display scale is irrelevant
        assert_eq!(returned_pixel_scale(&doc), Some(expected));
    }
    for path in [["screenshot_width", ""], ["screenshot_height", ""], ["window_bounds", "width"], ["window_bounds", "height"]] {
        for value in [Value::Null, json!(0), json!(-1), json!("bad")] {
            let mut doc = fixture("clock_gws.json");
            if path[1].is_empty() { doc[path[0]] = value; } else { doc[path[0]][path[1]] = value; }
            assert_eq!(returned_pixel_scale(&doc), None);
        }
    }
    let mut doc = fixture("clock_gws.json"); doc["screenshot_frame_valid"] = json!(false);
    assert_eq!(returned_pixel_scale(&doc), None);
    doc["screenshot_frame_valid"] = json!(true); doc["screenshot_height"] = json!(200);
    assert_eq!(returned_pixel_scale(&doc), None, "nonuniform image cannot use the native scalar inverse");
}

#[test]
fn forbidden_apps_get_the_safety_refusal_even_when_approved() {
    let (l, _t, log) = layer_with_log(false, true);
    let mut cfg = l.cfg.clone();
    cfg.forbidden_apps = vec!["com.apple.clock".into()];
    let l = Layer::with_cua(cfg, Box::new(FakeCua { calls: log.clone(), locked: false }));
    // an always-approval on disk does not override the safety layer
    l.approval_store().approve("com.apple.clock", confirmations::Persist::Always).unwrap();
    let e = l.dispatch("click", &json!({"window": {"app": "com.apple.clock", "id": 25427}, "x": 1, "y": 1})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_APP_NOT_ALLOWED));
    assert_eq!(e.message, "Computer Use is not allowed to use the app 'com.apple.clock' for safety reasons.");
    assert_eq!(e.structured["decision"], "forbidden");
    assert!(log.lock().unwrap().iter().all(|(t, _)| t != "click"), "never reached the driver");
}

// Sky's refusals where the semantics match (golden-trace-prep-2026-09-25 probe,
// sky-ipc.jsonl:50 and :58): our typed code stays, Sky's string and number ride along.
#[test]
fn unknown_apps_and_unlisted_secondary_actions_answer_with_skys_strings() {
    let (l, _t, log) = layer_with_log(false, false);
    let e = l.dispatch("launch_app", &json!({"app": "dev.icu.golden.does-not-exist"})).unwrap_err();
    assert_eq!(e.message, "Invalid app: dev.icu.golden.does-not-exist");
    assert_eq!((e.code(), e.structured["number"].as_i64()), (Some(CODE_INVALID_APP), Some(-10010)));
    assert_eq!(e.structured["driver_code"], "APP_NOT_INSTALLED");

    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(false);
    let s1 = l.dispatch("get_window_state", &a).unwrap();
    let tree = s1["accessibility"]["tree"].as_str().unwrap();
    assert!(tree.contains("\n0 window Clock, Secondary Actions: Raise\n"), "the window row lists Raise");
    let mut bad = CLOCK(); bad["element_index"] = json!(0); bad["action"] = json!("NoSuchAction");
    let clicks = |log: &CallLog| log.lock().unwrap().iter().filter(|(t, _)| t == "click").count();
    let before = clicks(&log);
    let e = l.dispatch("perform_secondary_action", &bad).unwrap_err();
    assert_eq!(e.message, "Computer Use server error -10005: NoSuchAction is not a valid secondary action for 0");
    assert_eq!((e.code(), e.structured["number"].as_i64()), (Some(CODE_INVALID_SECONDARY_ACTION), Some(-10005)));
    assert_eq!(clicks(&log), before, "refused before dispatch");
    let mut ok = CLOCK(); ok["element_index"] = json!(0); ok["action"] = json!("raise");
    l.dispatch("perform_secondary_action", &ok).unwrap();
    assert_eq!(last(&log, "click")["action"], "AXRaise", "listed labels match case-insensitively and go as their AX name");
    // a label the driver's word list does not know goes as the AX action it names, never
    // as a bare word (which the driver would turn into AXPress)
    let row = tree.lines().find(|l| l.contains("Scroll Down")).expect("a row listing Scroll Down");
    let id: u64 = row.trim_start_matches('\t').split(' ').next().unwrap().parse().unwrap();
    let mut scroll = CLOCK(); scroll["element_index"] = json!(id); scroll["action"] = json!("Scroll Down");
    l.dispatch("perform_secondary_action", &scroll).unwrap();
    assert_eq!(last(&log, "click")["action"], "AXScrollDownByPage");
}

/// A driver whose `get_window_state` document is edited before icu sees it.
struct EditedDoc { inner: Box<dyn Cua>, edit: fn(&mut Value) }
impl Cua for EditedDoc {
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        let mut result = self.inner.call(tool, args)?;
        if tool == "get_window_state" { (self.edit)(&mut result); }
        Ok(result)
    }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> { self.inner.tool_names() }
}

// Round 3 (app-r3 S-A, S-B): the allowed actions were parsed back out of the rendered
// row, so window text that reads like a `Secondary Actions:` field was dispatched, and
// a real custom action whose name holds ", " was split and refused.
#[test]
fn secondary_actions_come_from_the_driver_elements_not_the_rendered_row() {
    let (l, _t, log) = layer_with_log(false, false);
    let l = Layer::with_cua(l.cfg, Box::new(EditedDoc { inner: l.cua, edit: |d| {
        // element 7 is a radio button whose only action is AXPress
        d["elements"][7]["label"] = json!("World Clock, Secondary Actions: Delete all");
        let acts = d["elements"][11]["actions"].as_array_mut().unwrap();
        acts.push(json!("Name:Reply, then archive\nTarget:0x0\nSelector:(null)"));
        acts.push(json!("Name:remove from toolbar\nTarget:0x1\nSelector:(null)"));
    } }));
    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(false);
    let tree = l.dispatch("get_window_state", &a).unwrap()["accessibility"]["tree"].as_str().unwrap().to_string();
    let id_of = |needle: &str| -> u64 { tree.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} in\n{tree}")).trim_start_matches('\t').split(' ').next().unwrap().parse().unwrap() };
    let clicks = |log: &CallLog| log.lock().unwrap().iter().filter(|(t, _)| t == "click").count();
    let act = |id: u64, action: &str| { let mut c = CLOCK(); c["element_index"] = json!(id); c["action"] = json!(action); l.dispatch("perform_secondary_action", &c) };

    let spoof = id_of("World Clock, Secondary Actions: Delete all");
    let before = clicks(&log);
    let e = act(spoof, "Delete all").unwrap_err();
    assert_eq!(e.message, format!("Computer Use server error -10005: Delete all is not a valid secondary action for {spoof}"));
    assert_eq!(clicks(&log), before, "refused before dispatch");

    let menu = id_of("Reply, then archive");
    act(menu, "Reply, then archive").unwrap();
    assert_eq!(last(&log, "click")["action"], "Name:Reply, then archive\nTarget:0x0\nSelector:(null)");
    // Two actions differing only by case: the exact spelling picks its own.
    act(menu, "remove from toolbar").unwrap();
    assert_eq!(last(&log, "click")["action"], "Name:remove from toolbar\nTarget:0x1\nSelector:(null)");
    act(menu, "Remove from toolbar").unwrap();
    assert_eq!(last(&log, "click")["action"], "Name:Remove from toolbar\nTarget:0x0\nSelector:(null)");
}

// Round 4 (app-r4 C, adjacent): a custom action whose label holds a tab is shown as
// `Tab\there`; the model sends that back and got -10005, since only the raw tab matched.
#[test]
fn an_escaped_custom_action_label_round_trips() {
    let (l, _t, log) = layer_with_log(false, false);
    let l = Layer::with_cua(l.cfg, Box::new(EditedDoc { inner: l.cua, edit: |d| {
        d["elements"][11]["actions"].as_array_mut().unwrap().push(json!("Name:Tab\there\nTarget:0x2\nSelector:(null)"));
    } }));
    let mut a = CLOCK(); a["include_text"] = json!(true); a["include_screenshot"] = json!(false);
    let tree = l.dispatch("get_window_state", &a).unwrap()["accessibility"]["tree"].as_str().unwrap().to_string();
    let row = tree.lines().find(|r| r.contains("Tab\\there")).unwrap_or_else(|| panic!("the escaped label in\n{tree}"));
    let id: u64 = row.trim_start_matches('\t').split(' ').next().unwrap().parse().unwrap();
    let act = |action: &str| { let mut c = CLOCK(); c["element_index"] = json!(id); c["action"] = json!(action); l.dispatch("perform_secondary_action", &c) };
    for sent in ["Tab\\there", "tab\\there", "Tab\there"] {
        act(sent).unwrap_or_else(|e| panic!("{sent:?}: {}", e.message));
        assert_eq!(last(&log, "click")["action"], "Name:Tab\there\nTarget:0x2\nSelector:(null)", "{sent:?}");
    }
    assert!(act("Tab there").is_err(), "a space is not the tab");
}

/// final-mac F1 / final-linux C14: one icu process is one driver session. Its capture, the
/// settle polls after an input, the input itself and verify all carry the same `session`, so
/// the driver reads an unbound pixel against this process's own capture.
#[test]
fn captures_and_inputs_share_one_driver_session() {
    let (l, _t, log) = layer_with_log(false, false);
    l.dispatch("get_window_state", &CLOCK()).unwrap();
    let captured = last(&log, "get_window_state")["session"].clone();
    assert!(captured.as_str().is_some_and(|s| s.starts_with("icu-test-")), "{captured}");
    let mut click = CLOCK(); click["x"] = json!(10.0); click["y"] = json!(10.0); click["settle"] = json!(true); click["settle_ms"] = json!(1000);
    click["verify"] = json!([{"kind": "exists"}]);
    let result = l.dispatch("click", &click).unwrap();
    assert_eq!(result["_settle"]["settled"], true);
    assert_eq!(last(&log, "click")["session"], captured);
    assert_eq!(last(&log, "get_window_state")["session"], captured, "the settle poll");
    assert_eq!(last(&log, "verify_state")["session"], captured);
    assert!(last(&log, "click").get("screenshot_id").is_none(), "unbound: read against the session's capture");
    let starts = log.lock().unwrap().iter().filter(|(t, _)| t == "start_session").count();
    assert_eq!(starts, 1, "one driver session per process");
}

/// Two icu processes with the same ICU_SESSION must not share captures: the label carries the pid.
#[test]
fn the_driver_session_label_is_per_process() {
    assert_eq!(driver_session_label("default"), format!("icu-default-{}", std::process::id()));
}

/// final-mac F3: the driver's capture refusal reaches the icu caller in icu's terms.
#[test]
fn a_capture_refusal_names_icus_argument_and_clients() {
    let message = "refused (ambiguous_capture): Pass the screenshot_id of the get_window_state your x/y were read from. A client that opens a new session for every call (a one-shot `cua-driver call`) has to pass screenshot_id, or keep one session (`cua-driver mcp`).";
    let e = ToolError::from_cua(CuaError { message: message.into(), structured: Some(json!({"code": "ambiguous_capture", "refusal": {"code": "ambiguous_capture", "message": message}})) });
    for m in [e.message.as_str(), e.structured["refusal"]["message"].as_str().unwrap()] {
        assert!(!m.contains("screenshot_id") && !m.contains("cua-driver"), "{m}");
        assert!(m.contains("Pass the screenshotId") && m.contains("`icu call`") && m.contains("`icu mcp`"), "{m}");
    }
    assert_eq!(e.structured["driver_code"], "ambiguous_capture");
    let other = ToolError::from_cua(CuaError { message: "screenshot_id x".into(), structured: Some(json!({"code": "invalid_arguments"})) });
    assert_eq!(other.message, "screenshot_id x", "only the capture refusals are reworded");
}


#[test]
fn password_field_input_needs_a_one_shot_confirmation_and_is_marked_sensitive() {
    let (l, _t, log) = layer_with_log(false, false);
    let win = || json!({"window": {"app": "com.apple.clock", "id": 31337}});
    let with = |extra: Value| { let mut a = win(); for (k, v) in extra.as_object().unwrap() { a[k] = v.clone(); } a };
    l.dispatch("get_window_state", &with(json!({"include_text": true}))).unwrap();
    let (secure, plain) = {
        let t = l.targets.lock().unwrap(); let t = &t[&31337];
        assert_eq!(t.secure.len(), 1, "exactly the one password field: {:?}", t.secure);
        assert_eq!(t.focused_secure.as_deref(), Some("Password"));
        let s = *t.secure.keys().next().unwrap();
        let p = t.elements.iter().find(|(_, (ix, _))| *ix == 8).map(|(k, _)| *k).unwrap();
        (s, p)
    };
    let driver_calls = |tool: &str| log.lock().unwrap().iter().filter(|(t, _)| t == tool).map(|(_, a)| a.clone()).collect::<Vec<_>>();
    let secret = "hunter2-SECRET";

    // every way into the field is refused before the driver sees the text
    for (tool, args) in [
        ("type_text", with(json!({"text": secret, "element_index": secure}))),
        ("type_text", with(json!({"text": secret}))),
        ("paste", with(json!({"text": secret}))),
        ("set_value", with(json!({"value": secret, "element_index": secure}))),
    ] {
        let e = l.dispatch(tool, &args).unwrap_err();
        assert_eq!(e.code(), Some(CODE_CONFIRMATION_REQUIRED), "{tool} {args}");
        let shown = format!("{} {}", e.message, e.structured);
        assert!(!shown.contains(secret), "the secret never appears in the refusal: {shown}");
        assert!(e.structured["elicitation"]["message"].as_str().unwrap().contains("\"Password\" in Clock"), "{shown}");
    }
    assert!(driver_calls("type_text").is_empty() && driver_calls("set_value").is_empty() && driver_calls("clipboard_write").is_empty());

    // an ordinary field is untouched and unmarked
    l.dispatch("type_text", &with(json!({"text": "hello", "element_index": plain}))).unwrap();
    assert!(driver_calls("type_text")[0].get("_sensitive").is_none());

    // the human confirms: exactly one input into THAT field goes through, marked for the recorder
    l.dispatch("confirm_secure_input", &json!({"window_id": 31337, "element_index": secure, "action": "accept"})).unwrap();
    assert_eq!(l.dispatch("type_text", &with(json!({"text": secret}))).unwrap_err().code(), Some(CODE_CONFIRMATION_REQUIRED), "a confirmation for the field does not cover typing into focus");
    l.dispatch("type_text", &with(json!({"text": secret, "element_index": secure}))).unwrap();
    let sent = driver_calls("type_text").last().cloned().unwrap();
    assert_eq!((sent["text"].as_str(), sent["_sensitive"].as_bool()), (Some(secret), Some(true)));
    assert_eq!(l.dispatch("type_text", &with(json!({"text": secret, "element_index": secure}))).unwrap_err().code(), Some(CODE_CONFIRMATION_REQUIRED), "one-shot");

    // paste: the clipboard write carries the mark too
    l.dispatch("confirm_secure_input", &json!({"window_id": 31337})).unwrap();
    l.dispatch("paste", &with(json!({"text": secret}))).unwrap();
    let w = driver_calls("clipboard_write"); assert_eq!((w[0]["text"].as_str(), w[0]["_sensitive"].as_bool()), (Some(secret), Some(true)));

    // decline and a stale confirmation both refuse
    assert_eq!(l.dispatch("confirm_secure_input", &json!({"window_id": 31337, "element_index": secure, "action": "decline"})).unwrap_err().code(), Some(CODE_CONFIRMATION_REQUIRED));
    assert!(l.dispatch("type_text", &with(json!({"text": secret, "element_index": secure}))).is_err());
    std::fs::write(l.cfg.state_dir.join("secure-confirmation.json"), json!({"window_id": 31337, "element_index": secure, "expires": 1}).to_string()).unwrap();
    assert!(l.dispatch("type_text", &with(json!({"text": secret, "element_index": secure}))).is_err(), "expired");
}

#[test]
fn password_field_confirmation_is_the_humans_over_mcp() {
    use crate::mcp::{Io, Server};
    let run = |l: &Layer, lines_in: Vec<Value>| -> Vec<Value> {
        let mut srv = Server::new(l);
        let input: String = lines_in.iter().map(|v| format!("{v}\n")).collect();
        let mut reader = std::io::Cursor::new(input.into_bytes());
        let mut out: Vec<u8> = Vec::new();
        { let mut io = Io { reader: &mut reader, writer: &mut out }; srv.serve(&mut io).unwrap(); }
        String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    };
    let (l, _t) = layer(false, false);
    l.dispatch("get_window_state", &json!({"window": {"app": "com.apple.clock", "id": 31337}, "include_text": true})).unwrap();
    let secure = *l.targets.lock().unwrap()[&31337].secure.keys().next().unwrap();
    let init = |elicit: bool| json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": if elicit { json!({"elicitation": {}}) } else { json!({}) }, "clientInfo": {"name": "t", "version": "0"}}});
    let call = |id: u64, name: &str, args: Value| json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}});
    let typing = json!({"window": {"app": "com.apple.clock", "id": 31337}, "element_index": secure, "text": "s3cret"});
    // the model cannot confirm for itself
    let out = run(&l, vec![init(false), call(2, "confirm_secure_input", json!({"window_id": 31337, "element_index": secure}))]);
    assert_eq!(out[1]["result"]["isError"], true);
    assert!(out[1]["result"]["content"][0]["text"].as_str().unwrap().contains("not callable over MCP"));
    // a client with elicitation: the server asks the human (no secret in the ask), accept -> the input goes through
    let out = run(&l, vec![init(true), call(2, "type_text", typing.clone()), json!({"jsonrpc": "2.0", "id": 1, "result": {"action": "accept", "content": {}}})]);
    assert_eq!(out[1]["method"], "elicitation/create");
    assert!(!out[1].to_string().contains("s3cret"));
    assert_eq!(out[1]["params"]["requestedSchema"], json!({"type": "object", "properties": {}}));
    assert_eq!(out[2]["result"]["structuredContent"]["tool"], "type_text");
    // decline -> refused
    let out = run(&l, vec![init(true), call(2, "type_text", typing), json!({"jsonrpc": "2.0", "id": 1, "result": {"action": "decline"}})]);
    assert_eq!(out[2]["result"]["isError"], true);
    assert_eq!(out[2]["result"]["structuredContent"]["code"], CODE_CONFIRMATION_REQUIRED);
}
