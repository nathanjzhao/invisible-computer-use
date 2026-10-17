//! Public Layer dispatch with real temporary files and a synthetic UI/input provider.
//! Every driver call stays in this fixture; no native driver, input, or replay is used.
use icu::{
    cua::{Cua, CuaError},
    Config, Features, Layer,
};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

type Calls = Arc<Mutex<Vec<Value>>>;
type Verify = Box<dyn Fn(usize, &Value) -> Result<Value, CuaError> + Send>;

struct Fixture {
    calls: Calls,
    verify: Verify,
}
impl Cua for Fixture {
    fn tool_names(&self) -> Result<Vec<String>, CuaError> {
        Ok([
            "list_apps",
            "list_windows",
            "health_report",
            "verify_state",
            "get_window_state",
            "click",
            "press_key",
            "type_text",
        ]
        .map(str::to_owned)
        .to_vec())
    }
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        match tool {
            "health_report" => Ok(
                json!({"platform": "darwin", "driver_version": "0.24.0", "overall": "ok", "checks": []}),
            ),
            "list_apps" => {
                Ok(json!({"apps": [{"pid": 42, "bundle_id": "dev.icu.verify", "name": "Fixture"}]}))
            }
            "list_windows" => {
                Ok(json!({"windows": [{"pid": 42, "window_id": 7, "app_name": "Fixture"}]}))
            }
            "verify_state" => {
                assert_contract_fields(&args, verification_schema());
                let n = {
                    let mut calls = self.calls.lock().unwrap();
                    calls.push(args.clone());
                    calls.len()
                };
                (self.verify)(n, &args)
            }
            "start_session" => Ok(json!({"session": "fixture-session"})),
            "set_agent_cursor_enabled" => Ok(json!({"ok": true})),
            "press_key" => Ok(json!({"fixture_input": true})),
            other => panic!("unexpected fixture tool: {other}"),
        }
    }
}

fn fixture(
    root: &Path,
    verify: impl Fn(usize, &Value) -> Result<Value, CuaError> + Send + 'static,
) -> (Layer, Calls) {
    let mut cfg = Config::from_env();
    cfg.state_dir = root.join("state");
    cfg.memory_root = root.join("memory");
    cfg.personal_packs_dir = root.join("packs");
    cfg.approvals_session = root.join("session.toml");
    cfg.approvals_always = root.join("always.json");
    cfg.require_app_approval = false;
    cfg.denied_apps.clear();
    cfg.forbidden_apps.clear();
    cfg.mode = "headless".into();
    cfg.cursor = Some(false);
    cfg.features = Features::default();
    cfg.features.persistent_predicates = true;
    cfg.features.predicate_roots = vec![root.to_string_lossy().into_owned()];
    let calls = Arc::new(Mutex::new(Vec::new()));
    (
        Layer::with_cua(
            cfg,
            Box::new(Fixture {
                calls: calls.clone(),
                verify: Box::new(verify),
            }),
        ),
        calls,
    )
}

fn ui() -> Value {
    json!({"element": {"selector": {"role": "button", "label_contains": "Ready"}, "exists": true}})
}

fn verification_schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        // Use the driver's generated manifest, so fixture field names cannot silently drift
        // from the production serde(deny_unknown_fields) contract.
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor-cua/libs/cua-driver/contract/manifest.json");
        let manifest: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        manifest["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "verify_state")
            .unwrap()["input_schema"]
            .clone()
    })
}

/// Check object keys and required fields recursively against the real generated schema.
/// This deliberately validates the fixture's wire shape, not UI predicate truth.
fn assert_contract_fields(value: &Value, schema: &Value) {
    if let Some(object) = value.as_object() {
        let properties = schema["properties"]
            .as_object()
            .expect("contract object properties");
        for (key, child) in object {
            let child_schema = properties
                .get(key)
                .unwrap_or_else(|| panic!("fixture field {key:?} is outside the driver contract"));
            assert_contract_fields(child, child_schema);
        }
        if let Some(required) = schema["required"].as_array() {
            for key in required {
                assert!(
                    object.contains_key(key.as_str().unwrap()),
                    "missing contract field {key}"
                );
            }
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            assert_contract_fields(child, &schema["items"]);
        }
    }
}
fn file(path: &Path) -> Value {
    json!({"file": {"path": path, "exists": true}})
}
fn response(status: &str, count: usize) -> Value {
    json!({"status": status, "stable": status == "satisfied", "samples": 1, "elapsed_ms": 0,
        "predicates": (0..count).map(|i| json!({"index": i, "status": status,
            "unknown_reason": if status == "unknown" { Some("observation_unavailable") } else { None },
            "observed_json": "{\"source\":\"fixture\"}"})).collect::<Vec<_>>()})
}
fn verify(layer: &Layer, expect: Value, timeout: u64, stable: u64) -> Value {
    layer
        .dispatch(
            "verify_state",
            &json!({"window": {"app": "dev.icu.verify", "id": 7},
        "expect": expect, "timeout_ms": timeout, "stable_samples": stable}),
        )
        .unwrap()
}

#[test]
fn disjoint_file_and_ui_states_never_verify() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let remove = path.clone();
    let (layer, calls) = fixture(root.path(), move |_, args| {
        assert_eq!(args["expect"], json!([ui()]));
        // The UI becomes true precisely after the file becomes false, with no overlap.
        std::fs::remove_file(&remove).unwrap();
        assert!(!remove.exists());
        Ok(response("satisfied", 1))
    });
    let result = verify(&layer, json!([file(&path), ui()]), 0, 1);
    assert_eq!(result["status"], "unsatisfied", "{result}");
    assert_eq!(result["stable"], false);
    assert_eq!(result["predicates"][0]["status"], "unsatisfied");
    assert_eq!(result["predicates"][1]["status"], "satisfied");
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn coexistence_uses_shared_samples_and_preserves_predicate_order() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let (layer, calls) = fixture(root.path(), |_, args| {
        assert_eq!(args["pid"], 42);
        assert_eq!(args["window_id"], 7);
        assert_eq!(
            args["timeout_ms"], 0,
            "a cycle must not start a nested polling budget"
        );
        assert_eq!(args["stable_samples"], 1);
        assert_eq!(args["expect"], json!([ui(), {"window": {"exists": true}}]));
        let mut r = response("satisfied", 2);
        r["samples"] = json!(99); // Driver counters cannot satisfy the layer's shared stability.
        r["elapsed_ms"] = json!(9999);
        Ok(r)
    });
    let start = Instant::now();
    let result = verify(
        &layer,
        json!([ui(), file(&path), {"window": {"exists": true}}]),
        2000,
        3,
    );
    assert_eq!(result["status"], "satisfied", "{result}");
    assert_eq!(result["stable"], true);
    assert_eq!(result["samples"], 3);
    assert_eq!(calls.lock().unwrap().len(), 3);
    assert!(result["elapsed_ms"].as_u64().unwrap() <= start.elapsed().as_millis() as u64);
    for i in 0..3 {
        assert_eq!(result["predicates"][i]["index"], i);
    }
}

#[test]
fn unknown_ui_sample_resets_the_shared_streak() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let (layer, calls) = fixture(root.path(), |n, _| {
        Ok(response(if n == 2 { "unknown" } else { "satisfied" }, 1))
    });
    let result = verify(&layer, json!([file(&path), ui()]), 2000, 2);
    assert_eq!(result["status"], "satisfied", "{result}");
    assert_eq!(result["samples"], 4);
    assert_eq!(calls.lock().unwrap().len(), 4);
}

#[test]
fn persistent_change_resets_stability_even_when_ui_stays_true() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let change = path.clone();
    let (layer, calls) = fixture(root.path(), move |n, _| {
        if n == 2 {
            std::fs::remove_file(&change).unwrap();
        }
        if n == 3 {
            std::fs::write(&change, "saved").unwrap();
        }
        Ok(response("satisfied", 1))
    });
    let result = verify(&layer, json!([file(&path), ui()]), 2000, 2);
    assert_eq!(result["status"], "satisfied", "{result}");
    assert_eq!(
        result["samples"], 5,
        "neither the deleted nor newly created file is a coherent cycle"
    );
    assert_eq!(calls.lock().unwrap().len(), 5);
}

#[test]
fn changed_file_observation_cannot_count_as_a_stable_cycle() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let change = path.clone();
    let (layer, _) = fixture(root.path(), move |_, _| {
        // Both reads satisfy contains, but refer to different observed file state.
        std::fs::write(&change, "saved again").unwrap();
        Ok(response("satisfied", 1))
    });
    let result = verify(
        &layer,
        json!([{"file": {"path": path, "contains": "saved"}}, ui()]),
        0,
        1,
    );
    assert_eq!(result["status"], "unknown", "{result}");
    assert_eq!(result["stable"], false);
    assert_eq!(
        result["predicates"][0]["unknown_reason"],
        "stability_unproven"
    );
}

#[test]
fn zero_timeout_takes_one_cycle_and_unproven_stability_is_unknown() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let (layer, calls) = fixture(root.path(), |_, _| Ok(response("satisfied", 1)));
    for need in [0, 1, 2] {
        let result = verify(&layer, json!([file(&path), ui()]), 0, need);
        assert_eq!(result["samples"], 1);
        assert_eq!(result["stable"], need <= 1);
        assert_eq!(
            result["status"],
            if need <= 1 { "satisfied" } else { "unknown" }
        );
        if need > 1 {
            assert_eq!(
                result["predicates"][0]["unknown_reason"],
                "stability_unproven"
            );
        }
    }
    assert_eq!(calls.lock().unwrap().len(), 3);
    let result = layer
        .dispatch(
            "verify_state",
            &json!({"expect": [file(&path)], "timeout_ms": 0, "stable_samples": 2}),
        )
        .unwrap();
    assert_eq!(
        result["status"], "unknown",
        "persistent-only uses the same stability contract"
    );
    assert_eq!(result["samples"], 1);
    assert_eq!(calls.lock().unwrap().len(), 3);
}

#[test]
fn a_late_successful_callback_cannot_verify_after_the_deadline() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let (layer, calls) = fixture(root.path(), |_, _| {
        std::thread::sleep(Duration::from_millis(60));
        Ok(response("satisfied", 1))
    });
    let result = verify(&layer, json!([file(&path), ui()]), 20, 1);
    assert_eq!(result["status"], "unknown", "{result}");
    assert_eq!(result["stable"], false);
    assert_eq!(result["samples"], 1);
    assert!(result["elapsed_ms"].as_u64().unwrap() >= 60);
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "an in-flight read may overrun, but never gets another cycle"
    );
}

#[test]
fn mixed_polling_has_one_budget_and_never_reuses_completed_file_success() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    let (layer, calls) = fixture(root.path(), |_, args| {
        assert_eq!(args["timeout_ms"], 0);
        assert_eq!(args["stable_samples"], 1);
        std::thread::sleep(Duration::from_millis(15));
        Ok(response("unsatisfied", 1))
    });
    let start = Instant::now();
    let result = verify(&layer, json!([file(&path), ui()]), 180, 2);
    assert_eq!(result["status"], "unsatisfied");
    assert_eq!(result["stable"], false);
    assert_eq!(
        result["samples"].as_u64().unwrap() as usize,
        calls.lock().unwrap().len()
    );
    assert!(
        start.elapsed() < Duration::from_millis(700),
        "no sequential full-timeout polls"
    );
    assert!(result["elapsed_ms"].as_u64().unwrap() >= 180);
    assert!(
        calls.lock().unwrap().len() <= 2,
        "no cycle begins after the deadline"
    );
}

#[test]
fn unknowns_and_authority_limits_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let (mut layer, calls) = fixture(root.path(), |_, _| Ok(response("unknown", 1)));
    let result = verify(&layer, json!([ui(), file(&path)]), 2000, 1);
    assert_eq!(result["status"], "unknown");
    assert_eq!(
        result["predicates"][0]["unknown_reason"],
        "observation_unavailable"
    );
    assert_eq!(
        result["predicates"][1]["unknown_reason"],
        "invalid_predicate"
    );
    assert_eq!(
        result["samples"], 1,
        "permanent authority problems do not burn the budget"
    );
    layer.cfg.features.persistent_predicates = false;
    let result = verify(&layer, json!([file(&path), ui()]), 2000, 1);
    assert_eq!(
        result["predicates"][0]["unknown_reason"],
        "capability_disabled"
    );
    assert_eq!(result["stable"], false);
    assert_eq!(calls.lock().unwrap().len(), 2);
}

#[test]
fn malformed_driver_rows_are_unknown_and_unsatisfied_still_wins() {
    let root = tempfile::tempdir().unwrap();
    let (layer, _) = fixture(root.path(), |_, _| {
        Ok(json!({"stable": true, "predicates": [42]}))
    });
    let result = verify(
        &layer,
        json!([ui(), file(&root.path().join("absent"))]),
        0,
        1,
    );
    assert_eq!(result["status"], "unsatisfied");
    assert_eq!(result["predicates"][0]["status"], "unknown");
    assert_eq!(
        result["predicates"][0]["unknown_reason"],
        "observation_unavailable"
    );
}

#[test]
fn driver_failure_propagates_without_retry() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let (layer, calls) = fixture(root.path(), |_, _| {
        Err(CuaError {
            message: "fixture failure".into(),
            structured: None,
        })
    });
    let result = layer.dispatch(
        "verify_state",
        &json!({"window": {"id": 7}, "expect": [file(&path), ui()], "timeout_ms": 2000}),
    );
    assert!(result.unwrap_err().message.contains("fixture failure"));
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn driver_only_preserves_its_request_and_response_contract() {
    let root = tempfile::tempdir().unwrap();
    let answer = json!({"legacy": true, "samples": 11, "elapsed_ms": 23});
    let expected = answer.clone();
    let (layer, calls) = fixture(root.path(), move |_, _| Ok(answer.clone()));
    assert_eq!(verify(&layer, json!([ui()]), 1234, 4), expected);
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            json!({"pid": 42, "window_id": 7, "expect": [ui()], "timeout_ms": 1234, "stable_samples": 4})
        ]
    );
}

#[test]
fn post_action_mixed_verification_keeps_the_drivers_implicit_two_sample_default() {
    assert_eq!(
        verification_schema()["properties"]["stable_samples"]["default"],
        2
    );
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("saved.txt");
    std::fs::write(&path, "saved").unwrap();
    let (layer, calls) = fixture(root.path(), |n, _| {
        Ok(response(
            if n == 1 { "satisfied" } else { "unsatisfied" },
            1,
        ))
    });
    let result = layer
        .dispatch(
            "press_key",
            &json!({
                "window": {"app": "dev.icu.verify", "id": 7}, "key": "Return",
                "verify": [file(&path), ui()], "verify_timeout_ms": 350
            }),
        )
        .unwrap();
    assert_eq!(result["fixture_input"], true);
    assert_eq!(result["_verify"]["status"], "unsatisfied", "{result}");
    assert_eq!(result["_verify"]["stable"], false);
    assert!(
        calls.lock().unwrap().len() >= 2,
        "one transient UI success cannot establish stability"
    );
}

#[test]
fn post_action_mixed_verification_allows_one_sample_only_for_zero_timeout() {
    for timeout in [0, 2000] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("saved.txt");
        std::fs::write(&path, "saved").unwrap();
        let (layer, calls) = fixture(root.path(), |_, _| Ok(response("satisfied", 1)));
        let result = layer
            .dispatch(
                "press_key",
                &json!({
                    "window": {"app": "dev.icu.verify", "id": 7}, "key": "Return",
                    "verify": [file(&path), ui()], "verify_timeout_ms": timeout
                }),
            )
            .unwrap();
        assert_eq!(result["_verify"]["status"], "satisfied", "{result}");
        assert_eq!(result["_verify"]["stable"], true);
        assert_eq!(
            calls.lock().unwrap().len(),
            if timeout == 0 { 1 } else { 2 }
        );
    }
}

#[test]
fn explicit_verify_state_keeps_its_one_sample_default_and_override() {
    for stable in [None, Some(1)] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("saved.txt");
        std::fs::write(&path, "saved").unwrap();
        let (layer, calls) = fixture(root.path(), |n, _| {
            Ok(response(
                if n == 1 { "satisfied" } else { "unsatisfied" },
                1,
            ))
        });
        let mut args =
            json!({"window": {"id": 7}, "expect": [file(&path), ui()], "timeout_ms": 350});
        if let Some(n) = stable {
            args["stable_samples"] = json!(n);
        }
        let result = layer.dispatch("verify_state", &args).unwrap();
        assert_eq!(result["status"], "satisfied", "{result}");
        assert_eq!(result["stable"], true);
        assert_eq!(calls.lock().unwrap().len(), 1);
    }
}
