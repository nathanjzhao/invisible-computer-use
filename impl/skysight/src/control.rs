//! Computer-history control over a memory root shared by processes: the app that runs
//! the recorder and any CLI or MCP server that reads or changes it. Everything goes
//! through files under the root, read before every decision:
//!
//! - `settings.json` (the app's memory settings): `skysight` turns the recorder on,
//!   `paused` stops all memory generation, `recordingPaused` / `recordingPausedUntil`
//!   pause recording only, `observation` holds the rules, and `deleteGeneration` is
//!   bumped by every delete so a pass in flight writes nothing after it;
//! - `skysight/status.json`, the recorder host's heartbeat and current segment paths.
//!
//! The root is locked with `flock` on the folder itself (the app's store lock). The
//! functions named `*_locked` expect the caller to hold it; [`with_lock`] takes it.
//! This mirrors Codex's `computer_history_status / pause / resume / get_settings /
//! update_settings` and `skysightClearHistory`.

use crate::segments::{clear_summaries, ClearScope, Cleared, Segments};
use crate::stream::{ObservationRule, ObservationSettings};
use crate::workspace::Workspace;
use crate::{private, Ts};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SETTINGS_FILE: &str = "settings.json";
pub const GENERATION_KEY: &str = "deleteGeneration";
pub const STATUS_FILE: &str = "status.json";
/// A host that has not written its status for this long is not running.
pub const STALE_STATUS_SECS: i64 = 30;

/// Where things live under an app memory root.
#[derive(Clone, Debug)]
pub struct Layout { pub root: PathBuf }

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Layout { Layout { root: root.into() } }
    pub fn skysight(&self) -> PathBuf { self.root.join("skysight") }
    pub fn segments(&self) -> Segments { Segments::new(self.skysight().join("segments")) }
    pub fn status_path(&self) -> PathBuf { self.skysight().join(STATUS_FILE) }
    /// The phase-2 workspace (Codex's `~/.codex/memories`).
    pub fn workspace(&self) -> Workspace { Workspace::new(self.root.join("memories")) }
    pub fn settings_path(&self) -> PathBuf { self.root.join(SETTINGS_FILE) }
}

/// Hold the cross-process lock on `root` while `f` runs.
pub fn with_lock<R>(root: &Path, f: impl FnOnce() -> R) -> R {
    let _ = private::create_dir(root);
    let file = std::fs::File::open(root).ok().filter(|f| f.lock().is_ok());
    let r = f();
    drop(file);
    r
}

fn read_json(path: &Path) -> Value { std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({})) }

fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = path.parent() { private::create_dir(d)?; }
    let tmp = path.with_extension("json.tmp");
    private::write(&tmp, serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?)?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// The recorder-related settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecorderSettings {
    pub enabled: bool,
    /// All memory generation is paused (the app's Pause).
    pub paused: bool,
    pub recording_paused: bool,
    pub recording_paused_until: Option<Ts>,
    pub observation: ObservationSettings,
}

impl RecorderSettings {
    /// Recording is paused at `now` (a timed pause that ran out has ended).
    pub fn recording_paused_at(&self, now: Ts) -> bool {
        self.paused || (self.recording_paused && self.recording_paused_until.is_none_or(|u| u > now))
    }
    /// The recorder should run at `now`.
    pub fn should_record(&self, now: Ts) -> bool { self.enabled && !self.recording_paused_at(now) }
}

pub fn read_settings(layout: &Layout) -> RecorderSettings {
    let v = read_json(&layout.settings_path());
    RecorderSettings {
        enabled: v.get("skysight").and_then(Value::as_bool).unwrap_or(false),
        paused: v.get("paused").and_then(Value::as_bool).unwrap_or(false),
        recording_paused: v.get("recordingPaused").and_then(Value::as_bool).unwrap_or(false),
        recording_paused_until: v.get("recordingPausedUntil").and_then(Value::as_i64),
        observation: v.get("observation").cloned().and_then(|o| serde_json::from_value::<ObservationSettings>(o).ok())
            .filter(|o| o.validate().is_ok()).unwrap_or_default(),
    }
}

pub fn generation(layout: &Layout) -> u64 { read_json(&layout.settings_path()).get(GENERATION_KEY).and_then(Value::as_u64).unwrap_or(0) }

/// Set keys of `settings.json`, keeping every other key. Call under the lock.
pub fn patch_settings_locked(layout: &Layout, patch: &serde_json::Map<String, Value>) -> Result<(), String> {
    let mut v = read_json(&layout.settings_path());
    if !v.is_object() { v = json!({}); }
    for (k, val) in patch { v[k.as_str()] = val.clone(); }
    // Inside [`with_signer`] the new fields and their signature go out in the same write (review
    // round 3, L2): there is no moment where the file holds one without the other.
    SIGNER.with(|m| if let Some(mac) = m.borrow().as_ref() { v[SIGNATURE_KEY] = json!(signature_of(&v, mac)); });
    write_json(&layout.settings_path(), &v)
}

thread_local! { static SIGNER: std::cell::RefCell<Option<Mac>> = const { std::cell::RefCell::new(None) }; }

/// Run `f` (settings writes under the lock) with every write signed by `mac`. The app wraps the
/// changes the user made in it; nothing else holds a signer.
pub fn with_signer<R>(mac: &Mac, f: impl FnOnce() -> R) -> R {
    SIGNER.with(|m| *m.borrow_mut() = Some(mac.clone()));
    struct Reset;
    impl Drop for Reset { fn drop(&mut self) { SIGNER.with(|m| *m.borrow_mut() = None); } }
    let _r = Reset;
    f()
}

/// `computer_history_update_settings`: `{enabled?, observation?, rule?}` where `rule` is
/// one [`ObservationRule`] to set (Codex's `skysightUpdateObservationPolicy`).
pub fn update_settings_locked(layout: &Layout, patch: &Value) -> Result<RecorderSettings, String> {
    let (s, out) = apply_patch(read_settings(layout), patch)?;
    patch_settings_locked(layout, &out)?;
    Ok(s)
}

/// The settings `patch` would produce from `s`, and the `settings.json` keys to write.
pub fn apply_patch(mut s: RecorderSettings, patch: &Value) -> Result<(RecorderSettings, serde_json::Map<String, Value>), String> {
    let fields = patch.as_object().ok_or("settings must be an object")?;
    let mut out = serde_json::Map::new();
    for (k, v) in fields {
        match k.as_str() {
            "enabled" => { s.enabled = v.as_bool().ok_or("enabled must be a boolean")?; out.insert("skysight".into(), json!(s.enabled)); }
            "observation" => {
                let o: ObservationSettings = serde_json::from_value(v.clone()).map_err(|e| format!("invalid observation settings: {e}"))?;
                o.validate()?;
                s.observation = o;
            }
            "rule" => {
                let r: ObservationRule = serde_json::from_value(v.clone()).map_err(|e| format!("invalid rule: {e}"))?;
                s.observation.set_rule(r)?;
            }
            "urlBlocklist" => {
                let l: Vec<String> = serde_json::from_value(v.clone()).map_err(|e| format!("invalid blocklist: {e}"))?;
                s.observation.url_blocklist = l.iter().map(|d| crate::stream::normalize_domain(d)).collect();
                s.observation.validate()?;
            }
            "observePrivateBrowsing" => { s.observation.observe_private_browsing = v.as_bool().ok_or("observePrivateBrowsing must be a boolean")?; }
            // One-field changes, applied to the settings as they are under the lock, so a panel
            // working from an older copy does not put back what changed since (fresh review, F8).
            "blockAdd" => {
                let d = crate::stream::normalize_domain(v.as_str().ok_or("blockAdd must be a domain")?);
                if !s.observation.url_blocklist.contains(&d) { s.observation.url_blocklist.push(d); }
                s.observation.validate()?;
            }
            "blockRemove" => {
                let d = crate::stream::normalize_domain(v.as_str().ok_or("blockRemove must be a domain")?);
                s.observation.url_blocklist.retain(|x| *x != d);
            }
            "ruleRemove" => {
                let r: ObservationRule = serde_json::from_value(v.clone()).map_err(|e| format!("invalid rule: {e}"))?;
                let dom = r.url_domain.as_deref().map(crate::stream::normalize_domain);
                s.observation.rules.retain(|x| !(x.scope == r.scope && x.bundle_id == r.bundle_id && x.url_domain == dom));
            }
            "defaultBehavior" => {
                s.observation.default_behavior = serde_json::from_value(v.clone()).map_err(|e| format!("invalid defaultBehavior: {e}"))?;
            }
            other => return Err(format!("unknown setting {other:?}")),
        }
    }
    out.insert("observation".into(), serde_json::to_value(&s.observation).map_err(|e| e.to_string())?);
    Ok((s, out))
}

/// What a change from `before` to `after` would let the recorder keep that it did not before:
/// turning it on, recording private windows, recording every app by default, dropping or
/// relaxing a "never record" rule, adding an "always record" rule, unblocking a site, or
/// ending a pause. Empty means the change only tightens (or changes nothing). An agent may
/// tighten on its own; anything listed here needs the user's confirmation.
pub fn loosening(before: &RecorderSettings, after: &RecorderSettings, now: Ts) -> Vec<String> {
    use crate::stream::DefaultBehavior;
    let mut v = Vec::new();
    if !before.enabled && after.enabled { v.push("turn recording on".to_string()); }
    if before.recording_paused_at(now) && !after.recording_paused_at(now) { v.push("end the recording pause".to_string()); }
    // the new end is in the label (fresh review 3, item 4): an Allow answers that time, and a
    // later request for a sooner one is a new widening
    else if let Some(a) = after.recording_paused_until.filter(|a| before.recording_paused_at(now) && before.recording_paused_until.is_none_or(|b| *a < b)) {
        v.push(format!("end the recording pause sooner, at {}", crate::stream::iso(a)));
    }
    if before.paused && !after.paused { v.push("end the memory pause".to_string()); }
    let (b, a) = (&before.observation, &after.observation);
    if !b.observe_private_browsing && a.observe_private_browsing { v.push("record private browsing windows".to_string()); }
    if b.default_behavior == DefaultBehavior::DontObserve && a.default_behavior != DefaultBehavior::DontObserve {
        v.push("record every app and site by default".to_string());
    }
    // the scope is part of the label (and so of what the user allows): app or site (re-review 5)
    let name = |r: &ObservationRule| format!("{} {}", if r.scope == crate::stream::RuleScope::App { "app" } else { "site" },
        r.bundle_id.clone().or_else(|| r.url_domain.clone()).unwrap_or_default());
    for r in b.rules.iter().filter(|r| !r.observe) {
        if !a.rules.iter().any(|x| x == r) { v.push(format!("stop never-recording {}", name(r))); }
    }
    for r in a.rules.iter().filter(|r| r.observe) {
        if !b.rules.iter().any(|x| x == r) { v.push(format!("always record {}", name(r))); }
    }
    for d in &b.url_blocklist {
        if !a.url_blocklist.contains(d) { v.push(format!("unblock {d}")); }
    }
    v
}

/// `computer_history_pause`, for `duration` seconds or until resumed.
pub fn pause_locked(layout: &Layout, now: Ts, duration: Option<i64>) -> Result<(), String> {
    if duration.is_some_and(|d| d <= 0 || d > 30 * 86_400) { return Err("duration must be 1 second to 30 days".into()); }
    let mut m = serde_json::Map::new();
    m.insert("recordingPaused".into(), json!(true));
    m.insert("recordingPausedUntil".into(), json!(duration.map(|d| now + d)));
    patch_settings_locked(layout, &m)
}

pub fn resume_locked(layout: &Layout) -> Result<(), String> {
    let mut m = serde_json::Map::new();
    m.insert("recordingPaused".into(), json!(false));
    m.insert("recordingPausedUntil".into(), Value::Null);
    patch_settings_locked(layout, &m)
}

/// What the host writes to `status.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HostStatus {
    /// `running`, `paused` or `stopped`.
    pub state: String,
    pub event_stream_root_path: Option<String>,
    pub current_segment_events_path: Option<String>,
    pub current_segment_metadata_path: Option<String>,
    pub suppressed_events_path: Option<String>,
    pub started_at_ms: Option<i64>,
    pub ended_at_ms: Option<i64>,
    pub heartbeat_ms: i64,
    pub accessibility: Option<bool>,
    pub input_monitoring: Option<bool>,
    pub screen_locked: Option<bool>,
    pub last_error: Option<String>,
    pub platform: String,
}

pub fn write_status(layout: &Layout, s: &HostStatus) -> Result<(), String> {
    write_json(&layout.status_path(), &serde_json::to_value(s).map_err(|e| e.to_string())?)
}

/// `computer_history_status`, in Codex's shape plus the settings and what keeps the
/// recorder from running.
pub fn status(layout: &Layout, now_ms: i64) -> Value {
    let s = read_settings(layout);
    let host: Option<HostStatus> = std::fs::read(layout.status_path()).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let live = host.as_ref().is_some_and(|h| now_ms - h.heartbeat_ms <= STALE_STATUS_SECS * 1000);
    let h = host.clone().unwrap_or_default();
    let state = if !s.enabled { "stopped" } else if s.recording_paused_at(now_ms / 1000) { "paused" } else if live && h.state == "running" { "running" } else { "stopped" };
    let running = state == "running";
    json!({
        "state": state,
        "eventStreamRootPath": layout.segments().root,
        "currentSegmentEventsPath": if running { h.current_segment_events_path.clone() } else { None },
        "currentSegmentMetadataPath": if running { h.current_segment_metadata_path.clone() } else { None },
        "suppressedEventsPath": if running { h.suppressed_events_path.clone() } else { None },
        "startedAtMs": h.started_at_ms,
        "endedAtMs": if running { None } else { h.ended_at_ms },
        "memoriesRootPath": layout.workspace().root,
        "enabled": s.enabled,
        "recordingPausedUntil": s.recording_paused_until.filter(|_| s.recording_paused),
        "hostAlive": live,
        "permissions": {"accessibility": h.accessibility, "inputMonitoring": h.input_monitoring},
        "screenLocked": h.screen_locked,
        "lastError": h.last_error,
    })
}

/// `computer_history_get_settings`.
pub fn get_settings(layout: &Layout) -> Value {
    let s = read_settings(layout);
    json!({"enabled": s.enabled, "paused": s.recording_paused_at(crate::now_secs()), "recordingPausedUntil": s.recording_paused_until,
        "observation": s.observation})
}

/// Clear history in a scope: the raw records in its span (only `bundle`'s, when given),
/// then the 10min/6h summaries derived from that span. The delete generation is bumped
/// first, so a summary or consolidation in flight writes nothing, and the recorder host
/// drops what it had buffered. Call under the lock.
pub fn clear_history_locked(layout: &Layout, scope: ClearScope, interval: Option<(i64, i64)>, bundle: Option<&str>, now: Ts) -> Result<Cleared, String> {
    let (from, to) = scope.span(now, interval)?;
    let mut m = serde_json::Map::new();
    m.insert(GENERATION_KEY.into(), json!(generation(layout) + 1));
    patch_settings_locked(layout, &m)?;
    let mut c = layout.segments().clear(from, to, bundle, now)?;
    let ws = layout.workspace();
    let sc = crate::segments::clear_summaries_with(&ws.store(), from, to, bundle, now - crate::segments::SUMMARY_BACKFILL_SECS)?;
    c.summaries = sc.removed;
    c.not_rebuilt = sc.not_rebuilt;
    c.stripped = sc.stripped;
    // the stripped ones too: their baseline copy still restates what was cleared
    let rel: Vec<String> = c.summaries.iter().chain(&c.stripped).map(|n| format!("extensions/skysight/resources/{n}")).collect();
    ws.forget_in_baseline(&rel);
    // An older layout kept summaries beside the events.
    c.summaries.extend(clear_summaries(&crate::Store::new(layout.skysight().join("resources")), from, to, bundle)?);
    // The focus-only fallback's events (platforms without the event-stream recorder).
    c.records += crate::capture::clear_events(&layout.skysight().join("events.jsonl"), from, to, bundle)?;
    if scope == ClearScope::All && bundle.is_none() { ws.wipe_outputs(); }
    Ok(c)
}

// ---------------------------------------------------------------------------------------------
// Who may widen recording (review round 2, N2, and the Codex comparison, delta (a)).
//
// `settings.json`'s recorder fields are the user's: only the app writes them, and it signs them
// with a key agents cannot read (the macOS Keychain, through the app; see the app's
// `settings_key`). The recorder obeys them only while the signature holds; a hand edit, by an
// agent with a shell or anyone else, stops recording until the user looks at it in the app.
//
// An agent (over MCP, or `icu call` from a shell) writes a request instead, to
// `skysight/agent-request.json`. The recorder obeys the meet of the two: whatever is stricter,
// field by field, so a request can only tighten what is recorded and takes effect at once. The
// app adopts a request that only tightens, with a notice in the panel; one that widens waits
// there for the user's Allow or Deny. Nothing an agent can write turns recording on, ends a
// pause the user set, removes a rule or a blocked site, or records private windows.
// ---------------------------------------------------------------------------------------------

/// The app's signer: a MAC over the recorder fields, as hex. The recorder obeys
/// `settings.json` only when its stored MAC matches.
pub type Mac = std::sync::Arc<dyn Fn(&[u8]) -> String + Send + Sync>;

pub const SIGNATURE_KEY: &str = "recorderSignature";
pub const AGENT_REQUEST_FILE: &str = "agent-request.json";
pub const AGENT_CHANGES_FILE: &str = "agent-changes.jsonl";

impl Layout {
    pub fn agent_request_path(&self) -> PathBuf { self.skysight().join(AGENT_REQUEST_FILE) }
    pub fn agent_changes_path(&self) -> PathBuf { self.skysight().join(AGENT_CHANGES_FILE) }
    pub fn pending_notes(&self) -> PathBuf { self.skysight().join("pending-notes") }
}

/// The `settings.json` keys the signature covers (review round 3, N2): everything that decides
/// what is recorded, how long it is kept, and where its text is sent.
///
/// - `skysight`, `paused`, `recordingPaused`, `recordingPausedUntil`, `observation`: what the
///   recorder keeps.
/// - `chatMemories`: whether runs are mined for memories at all.
/// - `retentionDays`: how long raw segments and run journals are kept (an agent could set years).
/// - `provider`, `extractModel`, `consolidationModel`: which model, local or cloud, is sent the
///   typed text the summaries and passes are built from.
///
/// Not signed: `idleMinutes` (when a quiet session is summarized: it moves a pass in time, not
/// what is kept or where it goes), `deleteGeneration` (bumped by every delete; raising it only
/// drops writes in flight) and the signature itself.
pub const SIGNED_KEYS: [&str; 10] = ["skysight", "paused", "recordingPaused", "recordingPausedUntil", "observation",
    "chatMemories", "retentionDays", "provider", "extractModel", "consolidationModel"];

/// The bytes the signature covers: the signed keys as they are in the file (missing ones as
/// null), canonical (sorted keys, no spaces).
fn signed_payload(v: &Value) -> Vec<u8> {
    let m: serde_json::Map<String, Value> = SIGNED_KEYS.iter().map(|k| (k.to_string(), v.get(*k).cloned().unwrap_or(Value::Null))).collect();
    serde_json::to_vec(&Value::Object(m)).unwrap_or_default()
}

/// The signature `v` would carry.
pub fn signature_of(v: &Value, mac: &Mac) -> String { mac(&signed_payload(v)) }

/// Whether `v` (a `settings.json`) carries the app's signature.
pub fn value_signed(v: &Value, mac: &Mac) -> bool {
    let Some(sig) = v.get(SIGNATURE_KEY).and_then(Value::as_str) else { return false };
    let want = signature_of(v, mac);
    sig.len() == want.len() && sig.bytes().zip(want.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// Whether `settings.json` carries the app's signature.
pub fn signature_valid(layout: &Layout, mac: &Mac) -> bool { value_signed(&read_json(&layout.settings_path()), mac) }

/// Sign the settings as they are now, in one write. Call under the lock.
pub fn sign_locked(layout: &Layout, mac: &Mac) -> Result<(), String> {
    with_signer(mac, || patch_settings_locked(layout, &serde_json::Map::new()))
}

/// Write `s` as the recorder fields (the app only). Call under the lock, then [`sign_locked`].
pub fn write_recorder_locked(layout: &Layout, s: &RecorderSettings) -> Result<(), String> {
    let mut m = serde_json::Map::new();
    m.insert("skysight".into(), json!(s.enabled));
    m.insert("paused".into(), json!(s.paused));
    m.insert("recordingPaused".into(), json!(s.recording_paused));
    m.insert("recordingPausedUntil".into(), json!(s.recording_paused_until));
    m.insert("observation".into(), serde_json::to_value(&s.observation).map_err(|e| e.to_string())?);
    patch_settings_locked(layout, &m)
}

/// What an agent asked to change, field by field (review round 3, N3). A request holds only the
/// fields the agent changed, so it is replayed on the user's settings as they are when it is read:
/// a field the user changed since is not turned back, and a pending request does not hold any
/// field the agent never touched.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentDelta {
    pub enabled: Option<bool>,
    /// `(recordingPaused, recordingPausedUntil)`.
    pub pause: Option<(bool, Option<Ts>)>,
    pub default_behavior: Option<crate::stream::DefaultBehavior>,
    pub observe_private_browsing: Option<bool>,
    pub block_add: Vec<String>,
    pub block_remove: Vec<String>,
    /// Rules to set (replacing one for the same app or site).
    pub rules_set: Vec<ObservationRule>,
    /// Rules to drop, by scope and target.
    pub rules_remove: Vec<ObservationRule>,
}

fn same_target(x: &ObservationRule, y: &ObservationRule) -> bool { x.scope == y.scope && x.bundle_id == y.bundle_id && x.url_domain == y.url_domain }

impl AgentDelta {
    /// What turns `from` into `to`.
    pub fn between(from: &RecorderSettings, to: &RecorderSettings) -> AgentDelta {
        let (a, b) = (&from.observation, &to.observation);
        AgentDelta {
            enabled: (from.enabled != to.enabled).then_some(to.enabled),
            pause: ((from.recording_paused, from.recording_paused_until) != (to.recording_paused, to.recording_paused_until)).then_some((to.recording_paused, to.recording_paused_until)),
            default_behavior: (a.default_behavior != b.default_behavior).then_some(b.default_behavior),
            observe_private_browsing: (a.observe_private_browsing != b.observe_private_browsing).then_some(b.observe_private_browsing),
            block_add: b.url_blocklist.iter().filter(|d| !a.url_blocklist.contains(d)).cloned().collect(),
            block_remove: a.url_blocklist.iter().filter(|d| !b.url_blocklist.contains(d)).cloned().collect(),
            rules_set: b.rules.iter().filter(|r| !a.rules.contains(r)).cloned().collect(),
            rules_remove: a.rules.iter().filter(|r| !b.rules.iter().any(|x| same_target(x, r))).cloned().collect(),
        }
    }
    pub fn is_empty(&self) -> bool { *self == AgentDelta::default() }
    /// `s` with this request applied.
    pub fn apply(&self, s: &RecorderSettings) -> RecorderSettings {
        let mut s = s.clone();
        if let Some(e) = self.enabled { s.enabled = e; }
        if let Some((p, u)) = self.pause { s.recording_paused = p; s.recording_paused_until = u; }
        let o = &mut s.observation;
        if let Some(d) = self.default_behavior { o.default_behavior = d; }
        if let Some(p) = self.observe_private_browsing { o.observe_private_browsing = p; }
        o.url_blocklist.retain(|d| !self.block_remove.contains(d));
        for d in &self.block_add { if !o.url_blocklist.contains(d) { o.url_blocklist.push(d.clone()); } }
        o.rules.retain(|r| !self.rules_remove.iter().any(|x| same_target(x, r)));
        for r in &self.rules_set { o.rules.retain(|x| !same_target(x, r)); o.rules.push(r.clone()); }
        s
    }
}

fn read_delta(layout: &Layout) -> Option<AgentDelta> {
    let t = crate::private::read_bounded(&layout.agent_request_path(), 256 * 1024)?;
    serde_json::from_str::<AgentDelta>(&t).ok().filter(|d| !d.is_empty())
}

/// An agent's standing request, replayed on the user's settings as they are now.
pub fn read_agent_request(layout: &Layout) -> Option<RecorderSettings> {
    let d = read_delta(layout)?;
    let s = d.apply(&read_settings(layout));
    s.observation.validate().ok().map(|()| s)
}

/// Field by field, the stricter of `a` and `b`: recording only when both record, paused when
/// either is (until the later end), private windows only when both allow them, "record nothing
/// by default" when either says so, every blocked site and every "never record" rule of either,
/// and an "always record" rule only when both have it.
pub fn meet(a: &RecorderSettings, b: &RecorderSettings, now: Ts) -> RecorderSettings {
    use crate::stream::DefaultBehavior;
    let (pa, pb) = (a.recording_paused && a.recording_paused_until.is_none_or(|u| u > now), b.recording_paused && b.recording_paused_until.is_none_or(|u| u > now));
    let until = match (pa, pb) {
        (true, true) => match (a.recording_paused_until, b.recording_paused_until) { (Some(x), Some(y)) => Some(x.max(y)), _ => None },
        (true, false) => a.recording_paused_until,
        (false, true) => b.recording_paused_until,
        (false, false) => None,
    };
    let (oa, ob) = (&a.observation, &b.observation);
    let mut o = ObservationSettings {
        default_behavior: if oa.default_behavior == DefaultBehavior::DontObserve || ob.default_behavior == DefaultBehavior::DontObserve { DefaultBehavior::DontObserve } else { oa.default_behavior.clone() },
        observe_private_browsing: oa.observe_private_browsing && ob.observe_private_browsing,
        ..oa.clone()
    };
    o.url_blocklist = oa.url_blocklist.iter().chain(&ob.url_blocklist).cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    let same = |x: &ObservationRule, y: &ObservationRule| x.scope == y.scope && x.bundle_id == y.bundle_id && x.url_domain == y.url_domain;
    let mut rules: Vec<ObservationRule> = Vec::new();
    for r in oa.rules.iter().chain(&ob.rules) {
        if rules.iter().any(|x| same(x, r)) { continue; }
        let never = oa.rules.iter().chain(&ob.rules).any(|x| same(x, r) && !x.observe);
        let both = oa.rules.iter().any(|x| same(x, r) && x.observe) && ob.rules.iter().any(|x| same(x, r) && x.observe);
        if never { rules.push(ObservationRule { observe: false, ..r.clone() }); } else if both { rules.push(r.clone()); }
    }
    o.rules = rules;
    RecorderSettings { enabled: a.enabled && b.enabled, paused: a.paused || b.paused, recording_paused: pa || pb, recording_paused_until: until, observation: o }
}

/// What the recorder obeys: the signed recorder fields (all off when the signature does not
/// hold, or with no signer) met with an agent's request.
pub fn effective_settings(layout: &Layout, mac: Option<&Mac>, now: Ts) -> RecorderSettings {
    let mut base = read_settings(layout);
    if !mac.is_some_and(|m| signature_valid(layout, m)) { base.enabled = false; }
    match read_agent_request(layout) { Some(r) => meet(&base, &r, now), None => base }
}

/// The settings as an agent sees them (no signer at hand): the recorder fields met with its own
/// request. Used for status and for whether memory goes into instructions, never to record.
pub fn requested_settings(layout: &Layout, now: Ts) -> RecorderSettings {
    let base = read_settings(layout);
    match read_agent_request(layout) { Some(r) => meet(&base, &r, now), None => base }
}

/// An agent's change: applied to its standing request (or the recorder fields when it has none)
/// and saved as the request. Returns the request and what in it widens recording, which waits
/// for the user. Call under the lock.
pub fn agent_request_locked(layout: &Layout, change: impl FnOnce(RecorderSettings) -> Result<RecorderSettings, String>, now: Ts) -> Result<(RecorderSettings, Vec<String>), String> {
    let main = read_settings(layout);
    let standing = read_agent_request(layout);
    let base = standing.clone().unwrap_or_else(|| main.clone());
    let next = change(base)?;
    next.observation.validate()?;
    // One widening request at a time (fresh review, F2): while the user is looking at one, an
    // agent may tighten further but not add to what it asks for.
    if let Some(st) = &standing {
        let (asked, now_asks) = (loosening(&main, st, now), loosening(&main, &next, now));
        if !asked.is_empty() && now_asks.iter().any(|w| !asked.contains(w)) {
            return Err(format!("a request to record more is waiting for the user ({}); ask again after they answer", asked.join("; ")));
        }
    }
    let delta = AgentDelta::between(&main, &next);
    if delta.is_empty() { let _ = std::fs::remove_file(layout.agent_request_path()); return Ok((next, vec![])); }
    if let Some(d) = layout.agent_request_path().parent() { private::create_dir(d)?; }
    private::write(&layout.agent_request_path(), serde_json::to_vec_pretty(&delta).map_err(|e| e.to_string())?)?;
    Ok((next.clone(), loosening(&main, &next, now)))
}

/// The user's answer to an agent's request, or the app adopting one that only tightens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolve { AdoptIfTightening, Allow, Deny }

/// Settle the agent's request: adopt it (when it only tightens, or the user allowed it), or keep
/// only its tightening (the user denied the rest). The recorder fields are signed again and the
/// change is logged for the panel. Returns what changed, `None` when the request stays pending
/// (or there is none). Call under the lock.
pub fn resolve_agent_request_locked(layout: &Layout, mac: &Mac, how: Resolve, now: Ts) -> Result<Option<Value>, String> {
    let Some(req) = read_agent_request(layout) else { return Ok(None) };
    let main = read_settings(layout);
    let widening = loosening(&main, &req, now);
    let tightening = loosening(&req, &main, now);
    let next = match how {
        Resolve::AdoptIfTightening if !widening.is_empty() => return Ok(None),
        Resolve::AdoptIfTightening | Resolve::Allow => req.clone(),
        Resolve::Deny => meet(&main, &req, now),
    };
    if signature_valid(layout, mac) || how != Resolve::AdoptIfTightening {
        with_signer(mac, || write_recorder_locked(layout, &next))?;
    } else {
        // An unsigned file is not signed here: the user has to look at it first.
        return Ok(None);
    }
    let _ = std::fs::remove_file(layout.agent_request_path());
    let entry = json!({"at": now, "how": match how { Resolve::Deny => "denied", Resolve::Allow => "allowed", _ => "adopted" },
        "tightened": tightening, "widened": if how == Resolve::Deny { vec![] } else { widening.clone() }, "refused": if how == Resolve::Deny { widening } else { vec![] }});
    let mut log = crate::private::read_bounded(&layout.agent_changes_path(), 256 * 1024).unwrap_or_default();
    log.push_str(&format!("{entry}\n"));
    let keep: Vec<&str> = log.lines().rev().take(50).collect::<Vec<_>>().into_iter().rev().collect();
    private::write(&layout.agent_changes_path(), keep.join("\n") + "\n")?;
    Ok(Some(entry))
}

/// The panel's last `n` agent changes, newest first.
pub fn agent_changes(layout: &Layout, n: usize) -> Vec<Value> {
    crate::private::read_bounded(&layout.agent_changes_path(), 256 * 1024).unwrap_or_default().lines().rev().take(n)
        .filter_map(|l| serde_json::from_str(l).ok()).collect()
}
