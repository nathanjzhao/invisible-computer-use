use super::*;
use crate::cua::CuaError;
use crate::tests::{fixture, layer_with_log, tree_ids, CallLog, FakeCua, CLOCK};
use std::cell::Cell;
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Default)]
struct Clock(Cell<Duration>);
impl ui_readiness::Clock for Clock {
    fn now(&self) -> Duration {
        self.0.get()
    }
    fn sleep(&self, d: Duration) {
        self.0.set(self.0.get() + d);
    }
}

struct Sequence {
    docs: Mutex<VecDeque<Value>>,
    fallback: FakeCua,
}
impl Cua for Sequence {
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        if tool == "get_window_state" {
            self.fallback
                .calls
                .lock()
                .unwrap()
                .push((tool.into(), args));
            let mut docs = self.docs.lock().unwrap();
            return Ok(if docs.len() > 1 {
                docs.pop_front().unwrap()
            } else {
                docs.front().unwrap().clone()
            });
        }
        self.fallback.call(tool, args)
    }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> {
        self.fallback.tool_names()
    }
}
fn doc(value: &str, n: usize) -> Value {
    let mut d = fixture("clock_gws.json");
    d["pid"] = json!(26765);
    d["window_id"] = json!(25427);
    d["snapshot_id"] = json!(format!("snap-{n}"));
    d["elements"][7]["value"] = json!(value);
    d["elements"][7]["element_token"] = json!(format!("token-{n}"));
    d
}
fn layer(docs: Vec<Value>) -> (Layer, tempfile::TempDir, CallLog) {
    let (l, tmp, log) = layer_with_log(false, false);
    let next = Layer::with_cua(
        l.cfg.clone(),
        Box::new(Sequence {
            docs: Mutex::new(docs.into()),
            fallback: FakeCua {
                calls: log.clone(),
                locked: false,
            },
        }),
    );
    (next, tmp, log)
}
fn read(l: &Layer) -> Value {
    let mut a = CLOCK();
    a["include_text"] = json!(true);
    a["include_screenshot"] = json!(false);
    l.dispatch("get_window_state", &a).unwrap()
}

#[test]
fn private_settle_preserves_the_next_public_diff_and_invalidates_old_tokens() {
    let (l, _tmp, log) = layer(vec![
        doc("old", 0),
        doc("old", 1),
        doc("old", 2),
        doc("new", 3),
        doc("new", 4),
        doc("new", 5),
    ]);
    let first = read(&l);
    let stable = tree_ids(first["accessibility"]["tree"].as_str().unwrap())[0];
    let before = std::fs::read(l.cfg.state_dir.join("26765_25427.prev.json")).unwrap();
    let pack_path = l.cfg.state_dir.join("app_instructions_delivered/test.json");
    let pack = std::fs::read(&pack_path).unwrap();
    let result = l.settle_with_clock(26765, 25427, 1000, &Clock::default());
    assert_eq!(result["settled"], true);
    assert_eq!(result["polls"], 5);
    assert_eq!(result["requires_fresh_observation"], true);
    assert_eq!(
        std::fs::read(l.cfg.state_dir.join("26765_25427.prev.json")).unwrap(),
        before
    );
    assert_eq!(std::fs::read(pack_path).unwrap(), pack);
    assert!(!l.targets_path(25427).exists());
    assert_eq!(
        l.element_target(25427, &json!({"element_index":stable}))
            .unwrap_err()
            .code(),
        Some(CODE_STALE_ELEMENT)
    );
    let second = read(&l);
    assert_eq!(
        second["accessibilitySnapshotCount"], 2,
        "private polls did not consume public observations"
    );
    assert_eq!(second["accessibilityRevision"], 2);
    assert!(second["accessibility"]["tree"]
        .as_str()
        .unwrap()
        .contains("new"));
    assert!(!second["accessibility"]["tree"]
        .as_str()
        .unwrap()
        .contains("<app_specific_instructions>"));
    let log = log.lock().unwrap();
    let requests: Vec<_> = log
        .iter()
        .filter(|(t, _)| t == "get_window_state")
        .map(|(_, a)| a)
        .collect();
    assert!(requests.iter().all(|a| a["pid"] == 26765
        && a["window_id"] == 25427
        && a["session"] == requests[0]["session"]));
}

#[test]
fn settle_before_first_public_observation_does_not_deliver_packs() {
    let (l, _tmp, _) = layer(vec![doc("same", 0)]);
    assert_eq!(
        l.settle_with_clock(26765, 25427, 1000, &Clock::default())["settled"],
        true
    );
    assert!(!l.cfg.state_dir.join("26765_25427.prev.json").exists());
    assert!(read(&l)["accessibility"]["tree"]
        .as_str()
        .unwrap()
        .starts_with("<app_specific_instructions>"));
}

#[test]
fn exhausted_budget_and_stopped_session_do_not_poll() {
    let (l, _tmp, log) = layer(vec![doc("same", 0)]);
    assert_eq!(
        l.settle_with_clock(26765, 25427, 0, &Clock::default())["polls"],
        0
    );
    l.stopped.store(true, std::sync::atomic::Ordering::Relaxed);
    let outcome = l.settle_with_clock(26765, 25427, 1000, &Clock::default());
    assert_eq!(outcome["settled"], false);
    assert_eq!(outcome["polls"], 0);
    assert!(!log
        .lock()
        .unwrap()
        .iter()
        .any(|(tool, _)| tool == "get_window_state"));
}

#[test]
fn partial_or_wrong_target_observations_never_settle() {
    for field in ["partial", "degraded", "pid", "window_id"] {
        let mut d = doc("same", 0);
        d[field] = if matches!(field, "pid" | "window_id") {
            json!(123)
        } else {
            json!(true)
        };
        let (l, _tmp, _) = layer(vec![d]);
        let outcome = l.settle_with_clock(26765, 25427, 1000, &Clock::default());
        assert_eq!(outcome["settled"], false, "{field}");
        assert_eq!(outcome["polls"], 1);
    }
}

#[test]
fn input_exhausting_deadline_keeps_action_result_without_extra_settle_or_verify() {
    let (l, _tmp, log) = layer_with_log(false, false);
    let mut args = CLOCK();
    args["text"] = json!("one delivery");
    args["deadline_ms"] = json!(0);
    args["settle"] = json!(true);
    args["verify"] = json!([{"kind":"exists"}]);
    let result = l.input("type_text", &args).unwrap();
    assert_eq!(result["tool"], "type_text");
    assert_eq!(result["_settle"]["polls"], 0);
    assert_eq!(result["_verify"]["status"], "unknown");
    assert_eq!(result["_verify"]["reason"], "deadline_exceeded");
    let log = log.lock().unwrap();
    assert_eq!(log.iter().filter(|(t, _)| t == "type_text").count(), 1);
    assert!(!log
        .iter()
        .any(|(t, _)| matches!(t.as_str(), "get_window_state" | "verify_state")));
}
