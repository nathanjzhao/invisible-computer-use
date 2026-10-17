//! Readiness / capabilities: ONE total function with the SAME shape on the CLI
//! (`icu capabilities --json`), over MCP (`capabilities`) and in the library
//! ([`crate::Layer::readiness`]). Mirrors Codex's `PermissionCatalog` (spec 18)
//! and `ServerErrorCode` (spec 19): what the driver, the OS permissions and the
//! host allow right now, which features that unlocks, the typed errors this
//! layer can emit, and the active mode.
//!
//! Rules: driver probes have finite wall-clock deadlines; `unknown`
//! is first-class and is NEVER reported as available; it is ADVISORY (a call
//! can still fail a moment later, and the call-time typed error is
//! authoritative). Permission states Codex distinguishes but cua cannot observe
//! (`pending`, `restricted`) are typed here and unit-forced; on a live driver
//! they surface as `unknown` with the reason in `detail`.
use crate::{Layer, Taxonomy, ERRORS};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus { Granted, Denied, Pending, Restricted, NotApplicable, Unknown }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub status: PermissionStatus,
    pub how_to_grant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_deeplink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability { Available, Degraded, Unavailable, Unknown }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub available: Availability,
    /// Error codes (from [`ERRORS`]) or readiness facts that block or degrade it.
    pub blocked_by: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverReadiness {
    pub present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub minimum_version: String,
    pub support: Feature,
    /// `overall == ok`; None when the health report could not be read.
    pub healthy: Option<bool>,
    /// Carries the invisible-computer-use native patch (host readiness, native lineage).
    pub patched: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostReadiness {
    pub locked: Option<bool>,
    pub console: Option<bool>,
    /// The display is awake (false = asleep); None when not observable.
    pub display: Option<bool>,
    pub lid_closed: Option<bool>,
    /// Input can be delivered: locked == false and console != false. None when unknown.
    pub interactive: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeReadiness {
    pub active: String,
    pub profiles: BTreeMap<String, Availability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Readiness {
    pub platform: String,
    pub driver: DriverReadiness,
    pub permissions: BTreeMap<String, Permission>,
    pub host: HostReadiness,
    pub features: BTreeMap<String, Feature>,
    pub errors: Vec<Taxonomy>,
    pub mode: ModeReadiness,
    pub advisory: &'static str,
}

pub const ADVISORY: &str = "readiness is advisory: a call can fail a moment later; the call-time typed error is authoritative";
pub const MINIMUM_DRIVER_VERSION: &str = "0.24.0";

/// Minimum core tool set shared with pre-dispatch. Unknown discovery is distinct
/// from a successful empty list. This is the layer's global compatibility gate.
pub const REQUIRED_DRIVER_TOOLS: &[&str] = &["list_windows", "list_apps", "get_window_state", "click", "type_text"];

pub fn version_at_least(v: &str, min: &str) -> Option<bool> {
    Some(semver::Version::parse(v.trim().trim_start_matches('v')).ok()?
        >= semver::Version::parse(min).ok()?)
}

pub fn driver_support(version: Option<&str>, tools: Option<&[String]>) -> Feature {
    let version_ok = version.and_then(|v| version_at_least(v, MINIMUM_DRIVER_VERSION));
    let mut blocked = Vec::new();
    if version_ok == Some(false) { blocked.push(format!("driver version below {MINIMUM_DRIVER_VERSION}")); }
    if let Some(names) = tools {
        for required in REQUIRED_DRIVER_TOOLS {
            if !names.iter().any(|n| n == required) { blocked.push(format!("driver has no `{required}` tool")); }
        }
    }
    let available = if !blocked.is_empty() { Availability::Unavailable }
        else if version_ok.is_none() || tools.is_none() { Availability::Unknown }
        else { Availability::Available };
    if version_ok.is_none() { blocked.push("driver version unknown".into()); }
    if tools.is_none() { blocked.push("driver tools unknown".into()); }
    Feature { available, blocked_by: blocked, note: None }
}

fn check<'a>(hr: &'a Value, name: &str) -> Option<&'a Value> {
    hr.get("checks")?.as_array()?.iter().find(|c| c.get("name").and_then(Value::as_str) == Some(name))
}

fn status_of(entry: Option<&Value>) -> (PermissionStatus, Option<String>) {
    match entry.and_then(|c| c.get("status")).and_then(Value::as_str) {
        Some("pass") => (PermissionStatus::Granted, None),
        Some("fail") => (PermissionStatus::Denied, entry.and_then(|c| c.get("hint")).and_then(Value::as_str).map(str::to_string)),
        Some("skip") => (PermissionStatus::Unknown, entry.and_then(|c| c.get("message")).and_then(Value::as_str).map(str::to_string)),
        _ => (PermissionStatus::Unknown, Some("not reported by the driver".into())),
    }
}

fn perm(status: PermissionStatus, how: &str, link: Option<&str>, detail: Option<String>) -> Permission {
    Permission { status, how_to_grant: how.to_string(), settings_deeplink: link.map(str::to_string), detail }
}

/// Build the readiness from the driver's health report (`None` = the driver could not be
/// reached), the driver's tool names, and the layer configuration. Pure, so every edge row
/// is testable by feeding a report.
pub fn assess(hr: Option<&Value>, tools: Option<&[String]>, driver_error: Option<String>, active_mode: &str, memory_writable: bool) -> Readiness {
    let platform = hr.and_then(|h| h.get("platform")).and_then(Value::as_str).unwrap_or(if cfg!(target_os = "macos") { "darwin" } else if cfg!(target_os = "windows") { "win32" } else { "linux" }).to_string();
    let version = hr.and_then(|h| h.get("driver_version")).and_then(Value::as_str).map(str::to_string);
    let patched = hr.map(|h| check(h, "host_interactive").is_some());
    let support = driver_support(version.as_deref(), tools);
    let driver = DriverReadiness {
        present: hr.is_some(),
        version: version.clone(),
        minimum_version: MINIMUM_DRIVER_VERSION.into(),
        support: support.clone(),
        healthy: hr.and_then(|h| h.get("overall")).and_then(Value::as_str).map(|o| o == "ok"),
        patched,
        error: driver_error.clone(),
    };

    // ---- permissions, per OS (Codex: PermissionCatalog scopes; ours: what the driver needs)
    let mut permissions = BTreeMap::new();
    match platform.as_str() {
        "darwin" => {
            let (ax, ax_detail) = status_of(hr.and_then(|h| check(h, "tcc_accessibility")));
            permissions.insert("accessibility".into(), perm(ax, "System Settings > Privacy & Security > Accessibility: enable the driver (CuaDriver.app / the host app); `cua-driver permissions grant` walks through it", Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"), ax_detail));
            let (sr, sr_detail) = status_of(hr.and_then(|h| check(h, "tcc_screen_recording")));
            permissions.insert("screen_recording".into(), perm(sr, "System Settings > Privacy & Security > Screen & System Audio Recording: enable the driver; `cua-driver permissions grant`", Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"), sr_detail));
        }
        "win32" => {
            let (uia, d) = status_of(hr.and_then(|h| check(h, "ax_capability")));
            permissions.insert("ui_automation".into(), perm(uia, "run the driver inside the interactive desktop session (not Session 0 / a service / an SSH shell)", None, d));
            let (cap, d2) = status_of(hr.and_then(|h| check(h, "screen_capture_capability")));
            permissions.insert("screen_capture".into(), perm(cap, "a hardware-accelerated graphics adapter with current drivers (D3D11/DXGI)", None, d2));
        }
        _ => {
            let (atspi, d) = status_of(hr.and_then(|h| check(h, "ax_capability")));
            permissions.insert("at_spi".into(), perm(atspi, "start the accessibility bus (at-spi2-core: `/usr/libexec/at-spi-bus-launcher --launch-immediately`) and set GTK_A11Y/QT accessibility", None, d));
            let (cap, d2) = status_of(hr.and_then(|h| check(h, "screen_capture_capability")));
            permissions.insert("screen_capture".into(), perm(cap, "an X11 DISPLAY, or on Wayland a compositor with screencopy / the xdg-desktop-portal", None, d2));
            let (wl, d3) = status_of(hr.and_then(|h| check(h, "wayland_backend")));
            permissions.insert("wayland_backend".into(), perm(if wl == PermissionStatus::Unknown && hr.is_some() { PermissionStatus::NotApplicable } else { wl }, "only on Wayland: a supported compositor backend", None, d3));
        }
    }
    if hr.is_none() { for p in permissions.values_mut() { p.status = PermissionStatus::Unknown; p.detail = Some("driver unreachable".into()); } }

    // ---- host (slice 8)
    let hi = hr.and_then(|h| check(h, "host_interactive")).and_then(|c| c.get("data")).and_then(|d| d.get("host"));
    let b = |k: &str| hi.and_then(|h| h.get(k)).and_then(Value::as_bool);
    let host = HostReadiness {
        locked: b("screen_locked"),
        console: b("on_console"),
        display: b("display_asleep").map(|a| !a),
        lid_closed: b("lid_closed"),
        interactive: match (b("screen_locked"), b("on_console")) {
            (Some(true), _) | (_, Some(false)) => Some(false),
            (Some(false), Some(true)) => Some(true),
            (Some(false), None) => Some(true), // console not observable on this platform: locked is the deciding fact
            _ => None,
        },
    };

    // ---- features: unknown never becomes available. The AX permission is the one that gates
    // observation/input on each OS (Screen Recording / capture is a separate, screenshot-only scope).
    let ax_scope = match platform.as_str() { "darwin" => "accessibility", "win32" => "ui_automation", _ => "at_spi" };
    let ax_status = permissions.get(ax_scope).map(|p| p.status).unwrap_or(PermissionStatus::Unknown);
    let mut features = BTreeMap::new();
    let mut base_block: Vec<String> = Vec::new();
    if hr.is_none() { base_block.push("SkyComputerUseTransportError".into()); }
    if support.available == Availability::Unavailable { base_block.push("incompatibleClientVersion".into()); }
    if support.available == Availability::Unknown { base_block.push("driver support unknown".into()); }
    let ax_block = match ax_status { PermissionStatus::Granted => None, PermissionStatus::Denied | PermissionStatus::Restricted => Some("permissionsNotGranted"), PermissionStatus::Pending => Some("permissionsPending"), _ => Some("permissions unknown") };
    let observe = {
        let mut blocked = base_block.clone();
        if let Some(b) = ax_block { blocked.push(b.into()); }
        let available = if hr.is_none() || support.available == Availability::Unavailable || matches!(ax_status, PermissionStatus::Denied | PermissionStatus::Restricted | PermissionStatus::Pending) { Availability::Unavailable }
            else if support.available == Availability::Unknown || ax_block.is_some() { Availability::Unknown }
            else { Availability::Available };
        Feature { available, blocked_by: blocked, note: None }
    };
    let act = {
        let mut blocked = observe.blocked_by.clone();
        match host.interactive { Some(false) => blocked.push(if host.locked == Some(true) { "screenLocked".into() } else { "off console".into() }), None if hr.is_some() => blocked.push("host readiness unknown".into()), _ => {} }
        let available = if observe.available == Availability::Unavailable || blocked.iter().any(|b| b == "screenLocked" || b == "off console") { Availability::Unavailable }
            else if observe.available == Availability::Unknown || blocked.iter().any(|b| b.ends_with("unknown")) { Availability::Unknown } else { Availability::Available };
        Feature { available, blocked_by: blocked, note: (host.display == Some(false)).then(|| "the display is asleep; pixel-route input will wake it".into()) }
    };
    features.insert("observe".into(), observe.clone());
    features.insert("act".into(), act.clone());
    features.insert("packs".into(), Feature { available: Availability::Available, blocked_by: vec![], note: Some("embedded in the binary".into()) });
    features.insert("memory".into(), Feature { available: if memory_writable { Availability::Available } else { Availability::Unavailable }, blocked_by: if memory_writable { vec![] } else { vec!["memory root not writable".into()] }, note: None });
    let has = |t: &str| tools.is_some_and(|ts| ts.iter().any(|n| n == t));
    let driver_feature = |tool: &str, name: &str| {
        let available = if hr.is_none() { Availability::Unknown }
            else if support.available == Availability::Unavailable { Availability::Unavailable }
            else if tools.is_none() { Availability::Unknown }
            else if !has(tool) { Availability::Unavailable }
            else { support.available };
        Feature { available, blocked_by: if tools.is_some() && !has(tool) { vec![format!("driver has no {name}")] } else { base_block.clone() }, note: None }
    };
    features.insert("cursor".into(), driver_feature("set_agent_cursor_enabled", "agent cursor"));
    features.insert("pip".into(), driver_feature("start_recording", "recording/PiP hooks"));
    features.insert("recording".into(), driver_feature("start_recording", "recording"));
    for (name, note) in [("host_readiness", "screen lock / console / display / lid facts"), ("native_diff", "driver-side AX lineage (the layer's own lineage works regardless)")] {
        let available = match patched { Some(true) => Availability::Available, Some(false) => Availability::Unavailable, None => Availability::Unknown };
        features.insert(name.into(), Feature { available, blocked_by: if available == Availability::Unavailable { vec!["driver not patched".into()] } else { vec![] }, note: Some(note.into()) });
    }

    // ---- modes (slice 11 presets over the same surface)
    let mut profiles = BTreeMap::new();
    let attended = if act.available == Availability::Available && host.display != Some(false) { Availability::Available } else if act.available == Availability::Unknown { Availability::Unknown } else { Availability::Unavailable };
    profiles.insert("attended".into(), attended);
    profiles.insert("headless".into(), act.available);
    profiles.insert("replay".into(), if features["recording"].available == Availability::Available && act.available == Availability::Available { Availability::Available } else if act.available == Availability::Unknown { Availability::Unknown } else { Availability::Unavailable });

    Readiness { platform, driver, permissions, host, features, errors: ERRORS.to_vec(), mode: ModeReadiness { active: active_mode.into(), profiles }, advisory: ADVISORY }
}

impl Readiness {
    pub fn to_json(&self) -> Value { serde_json::to_value(self).unwrap_or(json!({})) }
    /// A one-screen text rendering for humans (`icu capabilities`).
    pub fn render(&self) -> String {
        let mut out = String::new();
        out += &format!("platform {}  driver {} {} healthy={:?} patched={:?}{}\n", self.platform, if self.driver.present { "present" } else { "ABSENT" }, self.driver.version.as_deref().unwrap_or("?"), self.driver.healthy, self.driver.patched, self.driver.error.as_deref().map(|e| format!("  ({e})")).unwrap_or_default());
        for (k, p) in &self.permissions { out += &format!("permission {k:<18} {:?}{}\n", p.status, p.detail.as_deref().map(|d| format!("  {d}")).unwrap_or_default()); }
        out += &format!("host locked={:?} console={:?} display_awake={:?} lid_closed={:?} interactive={:?}\n", self.host.locked, self.host.console, self.host.display, self.host.lid_closed, self.host.interactive);
        for (k, f) in &self.features { out += &format!("feature {k:<16} {:?}{}\n", f.available, if f.blocked_by.is_empty() { String::new() } else { format!("  blocked_by {}", f.blocked_by.join(", ")) }); }
        out += &format!("mode {}  profiles {}\n", self.mode.active, self.mode.profiles.iter().map(|(k, v)| format!("{k}={v:?}")).collect::<Vec<_>>().join(" "));
        out += &format!("{}\n", self.advisory);
        out
    }
}

impl Layer {
    /// Total: never fails; a driver that cannot be reached yields `driver.present = false`.
    pub fn readiness(&self) -> Readiness {
        let (hr, err) = match self.cua().call("health_report", json!({})) { Ok(v) => (Some(v), None), Err(e) => (None, Some(e.message)) };
        let tools = self.cua().tool_names().ok();
        let memory_writable = skysight::private::create_dir(&self.cfg.memory_root).is_ok();
        assess(hr.as_ref(), tools.as_deref(), err, &self.cfg.mode, memory_writable)
    }
}
