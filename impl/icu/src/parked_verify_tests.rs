//! File and process predicates through `Layer::verify_state` and the input tools' `verify`.
use super::*;
use crate::parked_tests::tweaked_layer;
use crate::tests::CLOCK;

fn verify(l: &Layer, expect: Value) -> Value { l.dispatch("verify_state", &json!({"window": CLOCK()["window"], "expect": expect, "timeout_ms": 300})).unwrap() }

fn file_pred(path: &std::path::Path, extra: Value) -> Value {
    let mut o = json!({"path": path.to_str().unwrap()});
    for (k, v) in extra.as_object().unwrap() { o[k] = v.clone(); }
    json!({"file": o})
}

#[test]
fn persistent_predicates_are_unknown_until_the_operator_enables_the_capability() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("out.txt"); std::fs::write(&f, "x").unwrap();
    let (l, _t, log) = tweaked_layer(0, |_| {});
    let r = verify(&l, json!([file_pred(&f, json!({}))]));
    assert_eq!(r["status"], "unknown", "{r}");
    assert_eq!(r["predicates"][0]["unknown_reason"], "capability_disabled");
    assert!(log.lock().unwrap().iter().all(|(t, _)| t != "verify_state"), "never forwarded to the driver, which would reject it");
}

#[test]
fn a_file_predicate_needs_no_window_and_reports_the_tri_state_result() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("saved.txt");
    let (l, _t, log) = tweaked_layer(0, |c| c.features.persistent_predicates = true);
    let ask = |extra: Value| l.dispatch("verify_state", &json!({"expect": [file_pred(&f, extra)], "timeout_ms": 0})).unwrap();
    let before = ask(json!({}));
    assert_eq!((before["status"].as_str(), before["predicates"][0]["status"].as_str()), (Some("unsatisfied"), Some("unsatisfied")), "{before}");
    std::fs::write(&f, "hello").unwrap();
    let after = ask(json!({"contains": "ell", "size_bytes": 5}));
    assert_eq!(after["status"], "satisfied", "{after}");
    assert_eq!(after["stable"], true);
    assert!(log.lock().unwrap().iter().all(|(t, _)| t != "verify_state"), "no window, no driver call");
}

#[test]
fn window_and_file_predicates_combine_with_and_and_keep_their_positions() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("saved.txt"); std::fs::write(&f, "x").unwrap();
    let (l, _t, log) = tweaked_layer(0, |c| c.features.persistent_predicates = true);
    let r = verify(&l, json!([{"window": {"exists": true}}, file_pred(&f, json!({})), {"element": {"selector": {"role": "button"}, "exists": true}}]));
    let sent = crate::tests::last(&log, "verify_state");
    assert_eq!(sent["expect"], json!([{"window": {"exists": true}}, {"element": {"selector": {"role": "button"}, "exists": true}}]), "only the driver's predicates reach it");
    let idx: Vec<u64> = r["predicates"].as_array().unwrap().iter().map(|p| p["index"].as_u64().unwrap()).collect();
    assert_eq!(idx, vec![0, 1, 2], "the caller's positions, not the split lists'");
    assert_eq!(r["predicates"][1]["status"], "satisfied");
    // the unit-test fake answers with the old shape (`verified`, no `predicates`): unknown, never a guess
    assert_eq!(r["predicates"][0]["status"], "unknown");
    assert_eq!(r["status"], "unknown");
}

#[test]
fn an_action_can_verify_that_it_wrote_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("exported.txt");
    let (l, _t, _) = tweaked_layer(0, |c| { c.features.persistent_predicates = true; c.features.predicate_roots = vec![dir.path().display().to_string()]; });
    let mut s = CLOCK(); s["include_text"] = json!(false);
    l.dispatch("get_window_state", &s).unwrap();
    let mut c = CLOCK(); c["x"] = json!(10.0); c["y"] = json!(10.0);
    c["verify"] = json!([file_pred(&f, json!({}))]); c["verify_timeout_ms"] = json!(0);
    assert_eq!(l.dispatch("click", &c).unwrap()["_verify"]["status"], "unsatisfied");
    std::fs::write(&f, "done").unwrap();
    assert_eq!(l.dispatch("click", &c).unwrap()["_verify"]["status"], "satisfied");
    // a path outside the configured roots is refused as a malformed predicate
    c["verify"] = json!([{"file": {"path": "/etc/hosts"}}]);
    assert_eq!(l.dispatch("click", &c).unwrap()["_verify"]["predicates"][0]["unknown_reason"], "invalid_predicate");
}

#[test]
fn a_process_predicate_sees_this_test_process() {
    let (l, _t, _) = tweaked_layer(0, |c| c.features.persistent_predicates = true);
    let me = std::process::id();
    let r = l.dispatch("verify_state", &json!({"expect": [{"process": {"pid": me}}, {"process": {"pid": 2_000_000_000u64, "exists": false}}], "timeout_ms": 0})).unwrap();
    assert_eq!(r["status"], "satisfied", "{r}");
}

#[test]
fn predicates_without_a_driver_window_are_still_bounded() {
    let (l, _t, _) = tweaked_layer(0, |c| c.features.persistent_predicates = true);
    assert_eq!(l.dispatch("verify_state", &json!({"expect": []})).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT));
    let nine = vec![json!({"process": {"pid": 1}}); 9];
    assert_eq!(l.dispatch("verify_state", &json!({"expect": nine})).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT));
}
