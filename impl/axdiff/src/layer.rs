//! The stateful observation layer over cua-driver, as a library (the
//! `axdiff-layer` binary and the `window2` facade both drive it).
//!
//! One call = one turn: given cua's `get_window_state` document for a
//! `(pid, window)`, step the on-disk revision lineage for that window (first
//! observation -> full tree; later ones -> diff with stable ids), and prefix the
//! per-app instruction pack on the app's first successful observation in the
//! session (spec 04; session = the state dir).
//!
//! Persisted per state dir:
//! - `<pid>_<window>.prev.json` / `.initial.json` — the lineage (`RevState`);
//! - `app_instructions_delivered.json` — the session's delivered pack keys.
//!
//! ASSUMPTIONS (documented in impl/BUILD-PLAN.md): a degraded/empty snapshot
//! (cua's own `degraded` contract) steps nothing and delivers nothing — no text
//! is being presented; the first non-degraded observation is the first touch.
//! `accessibilityRevision` / `accessibilitySnapshotCount` are INFERRED
//! semantics for the Windows-engine counters: the snapshot count increments on
//! every non-degraded observation, the revision only when the tree changed.

use crate::sky::{self, SkyOptions};
use crate::{DiffOptions, DiffOutput, Element, Revision};
use app_instructions::{pack_for_bundle_id, supplement_for_bundle_id, DeliveryState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Persisted lineages from before the Sky rendering (ids from 1, legacy row content)
/// carry format 0 and start a fresh lineage instead of diffing against old rows.
const LINEAGE_FORMAT: u32 = 2;

#[derive(Serialize, Deserialize)]
struct RevState {
    #[serde(default)]
    format: u32,
    root: Vec<Element>,
    next_id: u64,
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    snapshot_count: u64,
}

#[derive(Clone, Debug)]
pub struct LayerConfig {
    pub driver: String,
    pub state_dir: PathBuf,
    pub cumulative: bool,
    pub reset: bool,
    pub opts: DiffOptions,
    /// Personal per-app instruction packs (`<dir>/<bundle_id>.md`), spec 04 /
    /// `computerUsePersonalInstructionsOverridesBuiltIn`. When a file exists for the app,
    /// `personal_overrides_builtin` decides whether it REPLACES the shipped pack or is
    /// PREPENDED before it. None disables the feature.
    pub personal_packs_dir: Option<PathBuf>,
    pub personal_overrides_builtin: bool,
    /// The client session whose first touch a pack belongs to (Sky: one delivered
    /// set per client). None keeps a single set per state dir, for the one-shot
    /// axdiff_layer CLI.
    pub delivery_scope: Option<String>,
}

impl LayerConfig {
    /// Where this session's delivered pack keys persist across one-shot calls.
    pub fn delivered_path(&self) -> PathBuf {
        match &self.delivery_scope {
            None => self.state_dir.join("app_instructions_delivered.json"),
            Some(scope) => {
                let safe: String = scope.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
                self.state_dir.join("app_instructions_delivered").join(format!("{safe}.json"))
            }
        }
    }
}

impl Default for LayerConfig {
    fn default() -> Self {
        LayerConfig {
            driver: std::env::var("AXDIFF_DRIVER").unwrap_or_else(|_| "cua-driver".into()),
            state_dir: std::env::var("AXDIFF_STATE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir().join("axdiff-state")),
            cumulative: false,
            reset: false,
            opts: DiffOptions::default(),
            personal_packs_dir: None,
            personal_overrides_builtin: true,
            delivery_scope: None,
        }
    }
}

/// One turn's presented observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    /// The text the model sees: pack prefix (first touch) + tree render.
    pub text: String,
    /// `FULL(first)` / `FULL` / `DIFF` / `NO_CHANGE` / `FULL_FALLBACK` / `DEGRADED`.
    pub tag: String,
    /// `APP_INSTRUCTIONS(first:<file>)` / `(seen:<file>)` / `(none)`.
    pub instr_tag: String,
    /// The delivery key used (bundle id, else app name, else pid).
    pub key: String,
    pub bundle_id: Option<String>,
    pub app_name: Option<String>,
    /// The elements cua flagged `selected`, each as its Sky row line (`<id> <content>`,
    /// no indentation or marker). Reported on every turn, including DIFF / NO_CHANGE
    /// turns whose `text` does not list unchanged elements.
    pub selected_elements: Vec<String>,
    /// The driver's text selection, when non-empty (the `Selected text:` block).
    pub selected_text: Option<String>,
    /// The focused element's Sky row line, when the driver pins exactly one row.
    pub focused_element: Option<String>,
    /// Every id of this revision with the secondary actions its row shows, as (label,
    /// AX action name), from the driver's elements (see `Revision::sky_secondary_actions`).
    pub secondary_actions: std::collections::BTreeMap<u64, Vec<(String, String)>>,
    /// (accessibilityRevision, accessibilitySnapshotCount) after this turn.
    pub revision: (u64, u64),
    /// Set when cua reported a degraded/empty tree; nothing was stepped or delivered.
    pub degraded: Option<String>,
    /// cua `element_index` (this snapshot) -> stable id (the `[id]` the model sees),
    /// so an action on `[id]` can be routed to the element the driver knows.
    pub index_map: std::collections::HashMap<i64, u64>,
}

/// Run `cua-driver call get_window_state` for one window.
pub fn fetch_gws(driver: &str, pid: &str, window: &str, include_screenshot: bool) -> Result<Value, String> {
    let req = format!(
        "{{\"pid\":{},\"window_id\":{},\"include_screenshot\":{},\"include_text\":true}}",
        pid, window, include_screenshot
    );
    let out = Command::new(driver)
        .args(["call", "get_window_state", &req])
        .output()
        .map_err(|e| format!("failed to run {driver}: {e}"))?;
    serde_json::from_slice(&out.stdout)
        .map_err(|_| format!("get_window_state returned non-JSON: {}", String::from_utf8_lossy(&out.stdout)))
}

/// Resolve a pid to its bundle id via cua-driver's `list_apps` (its
/// `get_window_state` reports only `app_name`). None if the driver call fails
/// or the pid is not a listed app.
pub fn bundle_id_for_pid(driver: &str, pid: &str) -> Option<String> {
    // list_apps also lists installed-but-not-running apps with pid 0: never match those
    let pid: i64 = pid.parse().ok().filter(|p| *p > 0)?;
    let out = Command::new(driver).args(["call", "list_apps", "{}"]).output().ok()?;
    let doc: Value = serde_json::from_slice(&out.stdout).ok()?;
    doc.get("apps")?.as_array()?.iter()
        .find(|a| a.get("pid").and_then(|p| p.as_i64()) == Some(pid))
        .and_then(|a| a.get("bundle_id").and_then(|b| b.as_str()))
        .filter(|b| !b.is_empty())
        .map(str::to_string)
}

/// Why a document must not be stepped or delivered:
/// - cua's degraded/empty contract (`degraded: true` with no elements), or
/// - a tool REFUSAL: cua prints an `isError` result's structured content to
///   stdout with exit 0 (stale window id, owner-pid mismatch, permission denied),
///   which has no `elements` array at all. Treating that as an empty snapshot
///   would persist an empty tree and burn the app's first touch (review finding).
pub fn degraded_reason(doc: &Value) -> Option<String> {
    let elements = doc.get("elements").and_then(|e| e.as_array());
    let Some(elements) = elements else {
        let why = doc.get("error").or_else(|| doc.get("message")).or_else(|| doc.get("code"))
            .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
            .unwrap_or_else(|| "no `elements` array in get_window_state result".to_string());
        return Some(format!("refused: {why}"));
    };
    let degraded = doc.get("degraded").and_then(|v| v.as_bool()).unwrap_or(false);
    if degraded && elements.is_empty() {
        Some(doc.get("degraded_reason").and_then(|v| v.as_str()).unwrap_or("degraded").to_string())
    } else {
        None
    }
}

fn load_rev(p: &Path) -> Option<RevState> {
    fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Step one turn for `(pid, window)` from cua's document. `bundle_id` is the
/// resolved app id (see [`bundle_id_for_pid`]); pass None when unresolvable.
pub fn observe(doc: &Value, pid: &str, window: &str, bundle_id: Option<String>, cfg: &LayerConfig) -> Observation {
    let app_name = doc.get("app_name").and_then(|v| v.as_str()).map(str::to_string)
        .filter(|s| !s.is_empty())
        .or_else(|| if cfg.opts.app_name.is_empty() { None } else { Some(cfg.opts.app_name.clone()) });
    let key = bundle_id.clone().or_else(|| app_name.clone()).unwrap_or_else(|| pid.to_string());

    if let Some(reason) = degraded_reason(doc) {
        return Observation {
            text: String::new(), tag: "DEGRADED".into(), instr_tag: "APP_INSTRUCTIONS(none)".into(),
            key, bundle_id, app_name, selected_elements: Vec::new(), selected_text: None, focused_element: None,
            secondary_actions: Default::default(), revision: (0, 0), degraded: Some(reason),
            index_map: Default::default(),
        };
    }

    let forest = sky::forest_from_gws(doc);

    fs::create_dir_all(&cfg.state_dir).ok();
    let stem = format!("{}_{}", pid, window);
    let prev_path = cfg.state_dir.join(format!("{stem}.prev.json"));
    let init_path = cfg.state_dir.join(format!("{stem}.initial.json"));
    let load = |p: &Path| load_rev(p).filter(|s| s.format == LINEAGE_FORMAT);
    let prev = if cfg.reset { None } else { load(&prev_path) };
    let had_prev = prev.is_some();
    let mut opts = SkyOptions {
        disable_diff: cfg.opts.disable_diff, line_budget: cfg.opts.line_budget,
        window_title: String::new(), app_name: app_name.clone().unwrap_or_default(),
    };

    let (rev, out_render, tag, revision, snapshot_count) = match prev {
        None => {
            let rev = Revision::initial_from(&forest, 0);
            opts.window_title = sky::window_title(doc, &rev);
            let r = sky::render_diff(&rev, false, &opts);
            (rev, r, "FULL(first)", 1u64, 1u64)
        }
        Some(pstate) => {
            let prior = Revision::restore(pstate.root, pstate.next_id);
            let rev = prior.next(&forest);
            opts.window_title = sky::window_title(doc, &rev);
            let r = if cfg.cumulative {
                let init = load(&init_path)
                    .map(|s| Revision::restore(s.root, s.next_id))
                    .unwrap_or_else(|| Revision::initial_from(&forest, 0));
                sky::render_cumulative(&rev, &init, &opts)
            } else {
                sky::render_diff(&rev, true, &opts)
            };
            let tag = match &r {
                DiffOutput::Full(_) => "FULL",
                DiffOutput::Diff(_) => "DIFF",
                DiffOutput::NoChange(_) => "NO_CHANGE",
                DiffOutput::FullFallback(_) => "FULL_FALLBACK",
            };
            // the revision counts tree CHANGES, independent of how the turn was
            // rendered (`--disable-diff` renders Full for an identical tree)
            let changed = rev.any_change();
            // counters written before these fields existed load as 0: treat the
            // restored lineage as revision 1 / one snapshot so counts stay >= 1
            let rev_no = pstate.revision.max(1) + if changed { 1 } else { 0 };
            let count = pstate.snapshot_count.max(1) + 1;
            (rev, r, tag, rev_no, count)
        }
    };

    // the selection block, else the focused row (spec: selection and focus)
    let selected_text = doc.get("selected_text").or_else(|| doc.pointer("/structuredContent/selected_text"))
        .and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let focused_element = sky::focused_source(doc).and_then(|s| rev.sky_line_for_source(s));
    let rendered = format!("{}{}", out_render.text(), sky::tail(selected_text.as_deref(), focused_element.as_deref()));

    let (root, next_id) = rev.state();
    let save = RevState { format: LINEAGE_FORMAT, root: root.to_vec(), next_id, revision, snapshot_count };
    fs::write(&prev_path, serde_json::to_vec(&save).unwrap()).ok();
    if !had_prev || cfg.reset || load(&init_path).is_none() {
        fs::write(&init_path, serde_json::to_vec(&save).unwrap()).ok();
    }

    // per-app instruction pack, first touch per session (spec 04). A personal pack
    // (computerUsePersonalInstructionsOverridesBuiltIn) can replace or precede the shipped one.
    let builtin = bundle_id.as_deref().and_then(pack_for_bundle_id).map(|p| p.body.to_string());
    let personal = bundle_id.as_deref().zip(cfg.personal_packs_dir.as_ref()).and_then(|(b, dir)| {
        let f = dir.join(format!("{b}.md"));
        fs::read_to_string(&f).ok().map(|s| s.trim_end().to_string()).filter(|s| !s.is_empty())
    });
    let instructions: Option<String> = match (personal, builtin) {
        (Some(p), Some(b)) if !cfg.personal_overrides_builtin => Some(format!("{p}\n\n{b}")),
        (Some(p), _) => Some(p),
        (None, b) => b,
    };
    let pack_file = bundle_id.as_deref().and_then(pack_for_bundle_id).map(|p| p.file);
    let delivered_path = cfg.delivered_path();
    let mut delivery = fs::read_to_string(&delivered_path)
        .map(|s| DeliveryState::from_json(&s))
        .unwrap_or_default();
    let already = delivery.has(&key);
    let supplement = bundle_id.as_deref().and_then(supplement_for_bundle_id);
    let text = delivery.deliver_with_supplement(&key, instructions.as_deref(), supplement, &rendered);
    if let Some(parent) = delivered_path.parent() { fs::create_dir_all(parent).ok(); }
    fs::write(&delivered_path, delivery.to_json()).ok();
    let instr_tag = match (instructions.is_some(), already) {
        (false, _) => "APP_INSTRUCTIONS(none)".to_string(),
        (true, false) => format!("APP_INSTRUCTIONS(first:{})", pack_file.unwrap_or("personal")),
        (true, true) => format!("APP_INSTRUCTIONS(seen:{})", pack_file.unwrap_or("personal")),
    };

    Observation {
        text, tag: tag.into(), instr_tag, key, bundle_id, app_name,
        selected_elements: rev.sky_selected_lines(), selected_text, focused_element,
        secondary_actions: rev.sky_secondary_actions(doc), revision: (revision, snapshot_count), degraded: None,
        index_map: rev.index_map(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(elements: Vec<Value>) -> Value {
        json!({"app_name": "Fixture", "window_id": 7, "pid": 1, "elements": elements})
    }
    fn cfg() -> LayerConfig {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("axdiff-layer-test-{}-{}-{}", std::process::id(), n,
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        LayerConfig { driver: "unused".into(), state_dir: dir, ..LayerConfig::default() }
    }
    fn e(i: i64, parent: Option<i64>, role: &str, label: &str, selected: bool) -> Value {
        let mut v = json!({"element_index": i, "role": role, "label": label, "depth": if parent.is_some() {1} else {0}, "selected": selected});
        if let Some(p) = parent { v["parent_index"] = json!(p); }
        v
    }

    #[test]
    fn a_deep_tree_renders_bounded_and_keeps_its_lineage() {
        // Round 3 (app-r3 S-D): a parent chain of 1000 overflowed the stack. Past 63
        // levels the saved lineage also failed to load (serde_json's nesting limit), so
        // every turn rendered as the first and ids restarted.
        let chain = |n: i64| doc((0..n).map(|i| e(i, if i == 0 { None } else { Some(i - 1) }, "AXGroup", &format!("g{i}"), false)).collect());
        for n in [1000, 5000, 100] {
            let c = cfg();
            let o1 = observe(&chain(n), "1", "7", None, &c);
            let rows: Vec<&str> = o1.text.lines().skip(1).collect();
            assert_eq!(rows.len(), n as usize, "every row is kept");
            let deepest = rows.iter().map(|r| r.len() - r.trim_start_matches('\t').len()).max().unwrap();
            assert_eq!(deepest, crate::cua::MAX_DEPTH - 1, "rows past the cap sit at the last level");
            assert!(rows.last().unwrap().ends_with(&format!(" container g{}", n - 1)), "in walk order");
            let o2 = observe(&chain(n), "1", "7", None, &c);
            assert_eq!(o2.tag, "NO_CHANGE", "the lineage reloads");
        }
        // Duplicate indexes that form a parent cycle end, each row shown once.
        let c = cfg();
        let o = observe(&doc(vec![e(5, None, "AXGroup", "z", false), e(6, Some(5), "AXGroup", "y", false), e(5, Some(6), "AXGroup", "x", false)]), "1", "7", None, &c);
        assert_eq!(o.text.lines().skip(1).map(|l| l.trim_start_matches('\t')).collect::<Vec<_>>(), ["0 container z", "1 container y", "2 container x"], "{}", o.text);
    }

    #[test]
    fn counters_revision_only_on_change_snapshot_every_turn() {
        let c = cfg();
        let t = doc(vec![e(0, None, "AXWindow", "W", false), e(1, Some(0), "AXRow", "a", true)]);
        let o1 = observe(&t, "1", "7", None, &c);
        assert_eq!((o1.tag.as_str(), o1.revision), ("FULL(first)", (1, 1)));
        assert_eq!(o1.selected_elements, vec!["1 row a".to_string()]);
        let o2 = observe(&t, "1", "7", None, &c);
        assert_eq!((o2.tag.as_str(), o2.revision), ("NO_CHANGE", (1, 2)));
        let t3 = doc(vec![e(0, None, "AXWindow", "W", false), e(1, Some(0), "AXRow", "a", false), e(2, Some(0), "AXRow", "b", true)]);
        let o3 = observe(&t3, "1", "7", None, &c);
        assert_eq!((o3.tag.as_str(), o3.revision), ("DIFF", (2, 3)));
        assert_eq!(o3.selected_elements, vec!["2 row b".to_string()], "stable ids: b is the new id 2");
        let o4 = observe(&t3, "1", "7", None, &c);
        assert_eq!(o4.revision, (2, 4));
    }

    #[test]
    fn a_lineage_persisted_before_the_sky_format_starts_fresh() {
        let c = cfg();
        fs::create_dir_all(&c.state_dir).unwrap();
        // legacy state: ids from 1, `[id]`-era row content, no format field
        let legacy = r#"{"root":[{"id":1,"role":"AXWindow","label":"W","value":null,"depth":0,"children":[]}],"next_id":2,"revision":4,"snapshot_count":9}"#;
        fs::write(c.state_dir.join("1_7.prev.json"), legacy).unwrap();
        fs::write(c.state_dir.join("1_7.initial.json"), legacy).unwrap();
        let o = observe(&doc(vec![e(0, None, "AXWindow", "W", false)]), "1", "7", None, &c);
        assert_eq!((o.tag.as_str(), o.revision), ("FULL(first)", (1, 1)));
        assert_eq!(o.text, "Window: \"W\", App: Fixture.\n0 window W\n");
        let again = observe(&doc(vec![e(0, None, "AXWindow", "W", false)]), "1", "7", None, &c);
        assert_eq!(again.text, "There has been no change in the accessibility tree for Window: \"W\".");
    }

    #[test]
    fn refusal_result_without_elements_steps_nothing_and_delivers_nothing() {
        let c = cfg();
        // what cua prints for an isError tool result (exit 0): no `elements` at all
        let refused = json!({"error": "window_id_not_found", "message": "window 7 is not owned by pid 1"});
        let o = observe(&refused, "1", "7", Some("com.apple.clock".into()), &c);
        assert_eq!(o.degraded.as_deref(), Some("refused: window_id_not_found"));
        assert!(!c.state_dir.join("1_7.prev.json").exists() && !c.state_dir.join("app_instructions_delivered.json").exists());
        let bare = json!({"code": -32602});
        assert!(observe(&bare, "1", "7", None, &c).degraded.unwrap().starts_with("refused: "));
        // and the first real snapshot afterwards is still the first touch
        let good = doc(vec![e(0, None, "AXWindow", "Clock", false)]);
        let o = observe(&good, "1", "7", Some("com.apple.clock".into()), &c);
        assert!(o.instr_tag.starts_with("APP_INSTRUCTIONS(first:") && o.tag == "FULL(first)");
    }

    #[test]
    fn revision_does_not_bump_for_an_identical_tree_under_disable_diff() {
        let mut c = cfg();
        c.opts.disable_diff = true;
        let t = doc(vec![e(0, None, "AXWindow", "W", false), e(1, Some(0), "AXRow", "a", false)]);
        let o1 = observe(&t, "1", "7", None, &c);
        let o2 = observe(&t, "1", "7", None, &c);
        assert_eq!((o2.tag.as_str(), o2.revision), ("FULL", (1, 2)), "Full render of an unchanged tree is not a new revision");
        let t3 = doc(vec![e(0, None, "AXWindow", "W", false), e(1, Some(0), "AXRow", "b", false)]);
        let o3 = observe(&t3, "1", "7", None, &c);
        assert_eq!((o3.tag.as_str(), o3.revision), ("FULL", (2, 3)));
        assert_eq!(o1.revision, (1, 1));
    }

    #[test]
    fn degraded_steps_nothing_and_delivers_nothing() {
        let c = cfg();
        let bad = json!({"app_name": "Clock", "degraded": true, "degraded_reason": "ax_window_unresolved", "elements": []});
        let o = observe(&bad, "1", "7", Some("com.apple.clock".into()), &c);
        assert_eq!(o.degraded.as_deref(), Some("ax_window_unresolved"));
        assert_eq!(o.text, "");
        assert!(!c.state_dir.join("1_7.prev.json").exists());
        assert!(!c.state_dir.join("app_instructions_delivered.json").exists());
        // the first GOOD observation is then the first touch
        let good = doc(vec![e(0, None, "AXWindow", "Clock", false)]);
        let o = observe(&good, "1", "7", Some("com.apple.clock".into()), &c);
        assert!(o.instr_tag.starts_with("APP_INSTRUCTIONS(first:") && o.tag == "FULL(first)");
        assert!(o.text.starts_with("<app_specific_instructions>\n"));
    }

    #[test]
    fn pack_delivered_once_per_app_across_windows_and_key_fallback() {
        let c = cfg();
        let good = doc(vec![e(0, None, "AXWindow", "Clock", false)]);
        let o1 = observe(&good, "1", "7", Some("com.apple.clock".into()), &c);
        let o2 = observe(&good, "1", "8", Some("com.apple.clock".into()), &c); // second window, same app
        assert!(o1.instr_tag.contains("first:") && o2.instr_tag.contains("seen:"));
        assert!(!o2.text.contains("<app_specific_instructions>"));
        // unresolvable bundle id -> key falls back to app_name, then pid
        let o3 = observe(&good, "2", "9", None, &c);
        assert_eq!((o3.key.as_str(), o3.instr_tag.as_str()), ("Fixture", "APP_INSTRUCTIONS(none)"));
        let mut anon = good.clone(); anon["app_name"] = json!("");
        let o4 = observe(&anon, "3", "9", None, &c);
        assert_eq!(o4.key, "3");
    }

    #[test]
    fn pack_first_touch_belongs_to_each_delivery_scope() {
        let base = cfg();
        let good = doc(vec![e(0, None, "AXWindow", "Clock", false)]);
        let scoped = |s: &str| LayerConfig { delivery_scope: Some(s.into()), ..base.clone() };
        let first = observe(&good, "1", "7", Some("com.apple.clock".into()), &scoped("mcp-1/a"));
        let again = observe(&good, "1", "7", Some("com.apple.clock".into()), &scoped("mcp-1/a"));
        let other = observe(&good, "1", "7", Some("com.apple.clock".into()), &scoped("mcp-2"));
        assert!(first.text.starts_with("<app_specific_instructions>\n"));
        assert!(!again.text.contains("<app_specific_instructions>"), "same scope, second touch");
        assert!(other.text.contains("<app_specific_instructions>\n"), "a new session is a new first touch");
        assert!(scoped("mcp-1/a").delivered_path().starts_with(&base.state_dir), "scope names cannot leave the state dir");
    }
}

#[cfg(test)]
mod personal_pack_tests {
    use super::*;
    use serde_json::json;

    fn clock_doc() -> Value {
        json!({"app_name": "Clock", "window_id": 1, "pid": 9, "snapshot_id": "s1",
               "elements": [{"element_index": 0, "role": "AXButton", "label": "Start", "actions": ["AXPress"]}]})
    }

    #[test]
    fn personal_pack_overrides_or_precedes_the_builtin() {
        let dir = std::env::temp_dir().join(format!("icu-personal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir); fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("com.apple.clock.md"), "PERSONAL CLOCK RULES").unwrap();
        // override (default): only the personal pack, wrapped
        let state = std::env::temp_dir().join(format!("icu-personal-state-o-{}", std::process::id()));
        let _ = fs::remove_dir_all(&state);
        let cfg = LayerConfig { state_dir: state.clone(), personal_packs_dir: Some(dir.clone()), personal_overrides_builtin: true, ..LayerConfig::default() };
        let o = observe(&clock_doc(), "9", "1", Some("com.apple.clock".into()), &cfg);
        assert!(o.text.contains("PERSONAL CLOCK RULES"), "personal pack delivered");
        assert!(!o.text.contains("You are viewing the Clock app"), "built-in Clock pack replaced:\n{}", &o.text[..o.text.len().min(120)]);
        // prepend: personal then built-in
        let state2 = std::env::temp_dir().join(format!("icu-personal-state-p-{}", std::process::id()));
        let _ = fs::remove_dir_all(&state2);
        let cfg2 = LayerConfig { state_dir: state2, personal_packs_dir: Some(dir.clone()), personal_overrides_builtin: false, ..LayerConfig::default() };
        let o2 = observe(&clock_doc(), "9", "1", Some("com.apple.clock".into()), &cfg2);
        let p = o2.text.find("PERSONAL CLOCK RULES");
        let b = o2.text.find("Clock");
        assert!(p.is_some() && b.is_some() && p < b, "personal precedes the built-in");
        // no personal pack: built-in only (unchanged behaviour)
        let state3 = std::env::temp_dir().join(format!("icu-personal-state-n-{}", std::process::id()));
        let _ = fs::remove_dir_all(&state3);
        let cfg3 = LayerConfig { state_dir: state3, personal_packs_dir: Some(dir.join("nope")), ..LayerConfig::default() };
        let o3 = observe(&clock_doc(), "9", "1", Some("com.apple.clock".into()), &cfg3);
        assert!(!o3.text.contains("PERSONAL CLOCK RULES"));
        let _ = fs::remove_dir_all(&dir);
    }
}
