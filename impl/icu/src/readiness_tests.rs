//! The edge matrix (impl/BUILD-PLAN.md, slice 10): one test per row, forced through the
//! pure `assess` function or the fake driver. Readiness is total, `unknown` is never
//! `available`, and the call-time error stays authoritative (TOCTOU).
use super::*;
use crate::readiness::{assess, Availability, PermissionStatus};
use crate::cua::{Cua, CuaError};
use std::sync::{Arc, Mutex};

fn hr(platform: &str, version: &str, checks: Vec<Value>) -> Value {
    json!({"schema_version": "1", "platform": platform, "driver_version": version, "overall": if checks.iter().any(|c| c["status"] == "fail") { "degraded" } else { "ok" }, "checks": checks})
}
fn chk(name: &str, status: &str) -> Value { json!({"name": name, "status": status, "message": "", "hint": if status == "fail" { "grant it" } else { "" }}) }
fn host(locked: Option<bool>, console: Option<bool>, asleep: Option<bool>) -> Value {
    json!({"name": "host_interactive", "status": if locked == Some(true) { "fail" } else { "pass" }, "message": "", "data": {"host": {"screen_locked": locked, "on_console": console, "display_asleep": asleep, "lid_closed": false, "active_displays": 1}}})
}
const TOOLS: [&str; 7] = ["list_windows", "list_apps", "get_window_state", "click", "type_text", "set_agent_cursor_enabled", "start_recording"];
fn tools() -> Vec<String> { TOOLS.iter().map(|s| s.to_string()).collect() }
fn mac(checks: Vec<Value>) -> Value { hr("darwin", "0.24.0", checks) }
fn ok_mac() -> Value { mac(vec![chk("tcc_accessibility", "pass"), chk("tcc_screen_recording", "pass"), host(Some(false), Some(true), Some(false))]) }

#[test]
fn permission_rows() {
    // granted
    let r = assess(Some(&ok_mac()), Some(&tools()), None, "attended", true);
    assert_eq!(r.permissions["accessibility"].status, PermissionStatus::Granted);
    assert_eq!(r.features["observe"].available, Availability::Available);
    assert_eq!(r.features["act"].available, Availability::Available);
    assert!(r.permissions["accessibility"].settings_deeplink.as_deref().unwrap().starts_with("x-apple.systempreferences:"));
    // denied -> observe and act unavailable, blocked by permissionsNotGranted, the driver's hint kept
    let r = assess(Some(&mac(vec![chk("tcc_accessibility", "fail"), chk("tcc_screen_recording", "pass"), host(Some(false), Some(true), Some(false))])), Some(&tools()), None, "attended", true);
    assert_eq!(r.permissions["accessibility"].status, PermissionStatus::Denied);
    assert_eq!(r.permissions["accessibility"].detail.as_deref(), Some("grant it"));
    assert_eq!(r.features["observe"].available, Availability::Unavailable);
    assert!(r.features["act"].blocked_by.contains(&"permissionsNotGranted".to_string()));
    // partial: screen recording denied alone keeps observation (text) available
    let r = assess(Some(&mac(vec![chk("tcc_accessibility", "pass"), chk("tcc_screen_recording", "fail"), host(Some(false), Some(true), Some(false))])), Some(&tools()), None, "attended", true);
    assert_eq!(r.permissions["screen_recording"].status, PermissionStatus::Denied);
    assert_eq!(r.features["observe"].available, Availability::Available);
    // unknown (the probe was skipped): never available
    let r = assess(Some(&mac(vec![chk("tcc_accessibility", "skip"), host(Some(false), Some(true), Some(false))])), Some(&tools()), None, "attended", true);
    assert_eq!(r.permissions["accessibility"].status, PermissionStatus::Unknown);
    assert_eq!(r.features["observe"].available, Availability::Unknown);
    assert_eq!(r.features["act"].available, Availability::Unknown);
    // declared but probe fails (no such check in the report): unknown, with the reason
    let r = assess(Some(&mac(vec![host(Some(false), Some(true), Some(false))])), Some(&tools()), None, "attended", true);
    assert_eq!(r.permissions["accessibility"].status, PermissionStatus::Unknown);
    assert_eq!(r.permissions["accessibility"].detail.as_deref(), Some("not reported by the driver"));
    // pending / restricted are typed states the driver cannot observe; forced here they block like denied
    for st in [PermissionStatus::Pending, PermissionStatus::Restricted] {
        let mut r = assess(Some(&ok_mac()), Some(&tools()), None, "attended", true);
        r.permissions.get_mut("accessibility").unwrap().status = st;
        let v = r.to_json();
        assert_eq!(v["permissions"]["accessibility"]["status"], json!(if st == PermissionStatus::Pending { "pending" } else { "restricted" }));
    }
    // revoked mid-session: the next assessment flips
    let before = assess(Some(&ok_mac()), Some(&tools()), None, "attended", true);
    let after = assess(Some(&mac(vec![chk("tcc_accessibility", "fail"), host(Some(false), Some(true), Some(false))])), Some(&tools()), None, "attended", true);
    assert_eq!((before.features["act"].available, after.features["act"].available), (Availability::Available, Availability::Unavailable));
}

#[test]
fn host_and_environment_rows() {
    let with_host = |h: Value| assess(Some(&mac(vec![chk("tcc_accessibility", "pass"), chk("tcc_screen_recording", "pass"), h])), Some(&tools()), None, "attended", true);
    // locked
    let r = with_host(host(Some(true), Some(true), Some(false)));
    assert_eq!((r.host.locked, r.host.interactive), (Some(true), Some(false)));
    assert!(r.features["act"].blocked_by.contains(&"screenLocked".to_string()));
    assert_eq!(r.features["observe"].available, Availability::Available, "observation keeps working while locked");
    // no console (SSH / session 0 / fast user switch): act unavailable, not locked
    let r = with_host(host(Some(false), Some(false), Some(false)));
    assert_eq!(r.host.interactive, Some(false));
    assert!(r.features["act"].blocked_by.contains(&"off console".to_string()));
    // display asleep / lid: reported, act still available with a note; attended profile unavailable
    let r = with_host(host(Some(false), Some(true), Some(true)));
    assert_eq!(r.host.display, Some(false));
    assert_eq!(r.features["act"].available, Availability::Available);
    assert!(r.features["act"].note.as_deref().unwrap().contains("asleep"));
    assert_eq!(r.mode.profiles["attended"], Availability::Unavailable);
    assert_eq!(r.mode.profiles["headless"], Availability::Available);
    // console not observable (Linux without logind) but unlocked: interactive from the lock fact
    let r = with_host(host(Some(false), None, Some(false)));
    assert_eq!(r.host.interactive, Some(true));
    // host readiness unknown (unpatched driver): act unknown, never available
    let r = assess(Some(&mac(vec![chk("tcc_accessibility", "pass"), chk("tcc_screen_recording", "pass")])), Some(&tools()), None, "attended", true);
    assert_eq!((r.driver.patched, r.host.interactive, r.features["act"].available), (Some(false), None, Availability::Unknown));
    assert_eq!(r.features["host_readiness"].available, Availability::Unavailable);
    // driver missing / down: total, present=false, everything unknown or blocked by the transport code
    let r = assess(None, None, Some("failed to start `cua-driver mcp`".into()), "attended", true);
    assert!(!r.driver.present && r.driver.healthy.is_none());
    assert!(r.features["observe"].blocked_by.contains(&"SkyComputerUseTransportError".to_string()));
    assert_eq!(r.features["cursor"].available, Availability::Unknown);
    assert!(r.permissions.values().all(|p| p.status == PermissionStatus::Unknown));
    // wrong version
    let r = assess(Some(&hr("darwin", "0.9.0", vec![chk("tcc_accessibility", "pass"), host(Some(false), Some(true), Some(false))])), Some(&tools()), None, "attended", true);
    assert!(r.features["act"].blocked_by.contains(&"incompatibleClientVersion".to_string()));
    assert_eq!(r.features["act"].available, Availability::Unavailable);
    // secure input: not observable by the driver -> no field claims it; documented row
    assert!(r.to_json().get("secure_input").is_none());
    // the typed list is complete and every code has a remediation
    for code in ["appNotAllowed", "accessibilityError", "permissionsNotGranted", "permissionsPending", "ambiguousApp", "blockedURL", "screenLocked", "userStoppedSession", "userIntervened", "incompatibleClientVersion", "runningApplicationNotFound", "SkyComputerUseTransportError"] {
        let t = r.errors.iter().find(|t| t.code == code).unwrap_or_else(|| panic!("{code} missing from the typed list"));
        assert!(!t.remediation.is_empty());
    }
    assert_eq!(r.errors.iter().find(|t| t.code == "blockedURL").unwrap().emitted_by, "non_goal");
}

#[test]
fn feature_rows_and_other_platforms() {
    let win = hr("win32", "0.24.0", vec![chk("ax_capability", "fail"), chk("screen_capture_capability", "pass"), host(Some(false), Some(true), None)]);
    let r = assess(Some(&win), Some(&tools()), None, "headless", true);
    assert_eq!(r.permissions["ui_automation"].status, PermissionStatus::Denied);
    assert!(r.permissions["ui_automation"].how_to_grant.contains("Session 0"));
    assert_eq!(r.features["observe"].available, Availability::Unavailable);
    let linux = hr("linux", "0.24.0", vec![chk("ax_capability", "pass"), chk("screen_capture_capability", "pass"), chk("wayland_backend", "skip"), host(Some(false), None, Some(false))]);
    let r = assess(Some(&linux), Some(&["list_windows", "list_apps", "get_window_state", "click", "type_text"].map(String::from)), None, "headless", false);
    assert_eq!(r.permissions["wayland_backend"].status, PermissionStatus::NotApplicable);
    assert_eq!(r.features["observe"].available, Availability::Available);
    assert_eq!(r.features["cursor"].available, Availability::Unavailable, "needs-dep: the driver lacks the cursor tool");
    assert_eq!(r.features["memory"].available, Availability::Unavailable, "memory root not writable");
    assert_eq!(r.mode.profiles["replay"], Availability::Unavailable);
    // degraded: healthy false but observe available
    let deg = hr("darwin", "0.24.0", vec![chk("tcc_accessibility", "pass"), chk("bundle_identity", "fail"), host(Some(false), Some(true), Some(false))]);
    let r = assess(Some(&deg), Some(&tools()), None, "attended", true);
    assert_eq!((r.driver.healthy, r.features["observe"].available), (Some(false), Availability::Available));
    assert!(r.render().contains("feature observe"));
}

// ---- dispatch: the pre-dispatch refusals and the TOCTOU rule, through a configurable fake driver

struct ConfigurableCua { health: Mutex<Value>, tools: Vec<String>, apps: Value, log: Arc<Mutex<Vec<String>>> }
impl Cua for ConfigurableCua {
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        self.log.lock().unwrap().push(tool.to_string());
        match tool {
            "health_report" => Ok(self.health.lock().unwrap().clone()),
            "list_apps" => Ok(self.apps.clone()),
            "list_windows" => Ok(json!({"windows": [{"window_id": 1, "pid": 10, "app_name": "A", "on_current_space": true, "is_on_screen": true}]})),
            "get_window_state" => Ok(json!({"pid": args["pid"], "window_id": args["window_id"], "app_name": "A", "elements": [], "degraded": true, "degraded_reason": "ax_window_unresolved: off-Space", "snapshot_id": "s1"})),
            "bring_to_front" => Ok(json!({"code": "bring_to_front_exact_window_unverified"})),
            "type_text" => Err(CuaError { message: "type_text refused: the host session is not interactive (screen_locked)".into(), structured: Some(json!({"code": "host_not_interactive"})) }),
            "launch_app" => Ok(json!({"launched": true})),
            _ => Err(CuaError { message: "boom".into(), structured: None }),
        }
    }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> { Ok(self.tools.clone()) }
}

fn layer_with(health: Value, tools: Vec<String>) -> (Layer, tempfile::TempDir, Arc<Mutex<Vec<String>>>) {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = Config { driver: "fake".into(), socket: None, driver_daemon: true, state_dir: tmp.path().join("s"), memory_root: tmp.path().join("m"), approvals_session: tmp.path().join("a.toml"), approvals_always: tmp.path().join("b.json"),
        denied_apps: vec![], forbidden_apps: vec![], require_app_approval: false, allow_persistent_approval: true, risk_level: "medium".into(), session_id: "t".into(), mode: "attended".into(), default_deadline_ms: 15000, always_simulate_click: false, cursor: None, personal_packs_dir: tmp.path().join("packs"), personal_overrides_builtin: true, features: Features::default() };
    let log = Arc::new(Mutex::new(Vec::new()));
    let apps = json!({"apps": [{"pid": 10, "bundle_id": "com.a.one", "name": "A"}, {"pid": 0, "bundle_id": "com.twin.one", "name": "Twin"}, {"pid": 0, "bundle_id": "com.twin.two", "name": "Twin"}]});
    (Layer::with_cua(cfg, Box::new(ConfigurableCua { health: Mutex::new(health), tools, apps, log: log.clone() })), tmp, log)
}

#[test]
fn pre_dispatch_emits_the_taxonomy_before_the_driver() {
    let win = json!({"window": {"app": "com.a.one", "id": 1}});
    // permissions denied -> permissionsNotGranted before any driver call for observation/input
    let (l, _t, log) = layer_with(mac(vec![chk("tcc_accessibility", "fail"), host(Some(false), Some(true), Some(false))]), tools());
    let e = l.dispatch("type_text", &json!({"window": win["window"], "text": "x"})).unwrap_err();
    assert_eq!((e.code(), e.structured["number"].as_i64()), (Some(CODE_PERMISSIONS_NOT_GRANTED), Some(-10009)));
    assert!(!log.lock().unwrap().iter().any(|t| t == "type_text"), "never reached the driver");
    assert!(e.structured["remediation"].as_str().unwrap().contains("how_to_grant"));
    // wrong driver version -> incompatibleClientVersion for everything but readiness itself
    let (l, _t, _) = layer_with(hr("darwin", "0.1.0", vec![chk("tcc_accessibility", "pass"), host(Some(false), Some(true), Some(false))]), tools());
    assert_eq!(l.dispatch("list_windows", &json!({})).unwrap_err().code(), Some(CODE_INCOMPATIBLE_CLIENT_VERSION));
    assert!(l.dispatch("capabilities", &json!({})).is_ok());
    // a driver missing a required tool
    let (l, _t, _) = layer_with(ok_mac(), vec!["list_windows".into()]);
    assert_eq!(l.dispatch("list_apps", &json!({})).unwrap_err().code(), Some(CODE_INCOMPATIBLE_CLIENT_VERSION));
    // stop / resume
    let (l, _t, _) = layer_with(ok_mac(), tools());
    l.dispatch("stop_session", &json!({})).unwrap();
    assert_eq!(l.dispatch("list_windows", &json!({})).unwrap_err().code(), Some(CODE_USER_STOPPED_SESSION));
    l.dispatch("resume_session", &json!({})).unwrap();
    assert!(l.dispatch("list_windows", &json!({})).is_ok());
    // ambiguousApp
    let e = l.dispatch("launch_app", &json!({"app": "Twin"})).unwrap_err();
    assert_eq!((e.code(), e.structured["number"].as_i64()), (Some(CODE_AMBIGUOUS_APP), Some(-10018)));
    assert_eq!(e.structured["candidates"], json!(["com.twin.one", "com.twin.two"]));
    // off-Space: accessibilityError with the driver's reason
    let e = l.dispatch("get_window_state", &json!({"window": win["window"], "include_text": true})).unwrap_err();
    assert_eq!((e.code(), e.structured["number"].as_i64()), (Some(CODE_ACCESSIBILITY_ERROR), Some(-10008)));
    // userIntervened: the person kept their app in front
    let e = l.dispatch("activate_window", &json!({"window": win["window"]})).unwrap_err();
    assert_eq!((e.code(), e.structured["driver_code"].as_str()), (Some(CODE_USER_INTERVENED), Some("bring_to_front_exact_window_unverified")));
    // transport: a driver failure with no structured content
    let e = l.dispatch("press_key", &json!({"window": win["window"], "key": "a"})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_TRANSPORT_ERROR));
}

#[test]
fn toctou_readiness_is_advisory_and_the_call_time_error_wins() {
    // readiness says everything is fine (cached), the host locks a moment later: the driver's
    // refusal comes through as screenLocked at call time; readiness is not consulted again
    let (l, _t, log) = layer_with(ok_mac(), tools());
    assert_eq!(l.readiness_cached().features["act"].available, Availability::Available);
    let e = l.dispatch("type_text", &json!({"window": {"app": "com.a.one", "id": 1}, "text": "x"})).unwrap_err();
    assert_eq!((e.code(), e.structured["number"].as_i64(), e.structured["driver_code"].as_str()), (Some(CODE_SCREEN_LOCKED), Some(-10020), Some("host_not_interactive")));
    let health_reads = log.lock().unwrap().iter().filter(|t| *t == "health_report").count();
    assert!(health_reads <= 2, "readiness reused within its TTL, not re-probed per call: {health_reads}");
}

#[test]
fn a_portal_only_wayland_capture_is_unknown_never_granted() {
    // the driver reports a portal-only session's capture as skip (not verified); the layer must
    // not turn that into a granted screen_capture permission
    let linux = hr("linux", "0.24.0", vec![chk("ax_capability", "pass"), chk("screen_capture_capability", "skip"), chk("wayland_backend", "skip"), host(Some(false), None, Some(false))]);
    let r = assess(Some(&linux), Some(&tools()), None, "attended", true);
    assert_ne!(r.permissions["screen_capture"].status, PermissionStatus::Granted);
    assert_eq!(r.permissions["screen_capture"].status, PermissionStatus::Unknown);
}
