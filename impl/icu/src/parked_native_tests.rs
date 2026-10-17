//! The layer's side of the two native features (the patched driver stamps `run_metadata` into every
//! recorded turn and ends a glide early at `close_enough_*`): what `record_start` and `cursor` send.
use super::*;
use crate::parked_tests::tweaked_layer;
use crate::tests::{last, CLOCK};

fn calls(log: &crate::tests::CallLog, tool: &str) -> Vec<Value> {
    log.lock().unwrap().iter().filter(|(t, _)| t == tool).map(|(_, a)| a.clone()).collect()
}

#[test]
fn every_parked_feature_has_the_default_its_source_gives_and_an_environment_switch() {
    let none = |_: &str| None;
    let d = Features::from_env(&none);
    assert_eq!((d.max_value_chars, d.mouse_size_px, d.post_action_sleep_ms), (4096, 12, 100), "Sky's defaults: 12 px pointer, 100 ms settle");
    assert!(!d.persistent_predicates && d.predicate_roots.is_empty(), "file and process predicates are opt-in");
    assert!(d.close_enough_distance_px.is_none() && d.close_enough_progress.is_none(), "close-enough is off unless asked for");
    let env = |k: &str| match k {
        "ICU_MAX_VALUE_CHARS" => Some("0".to_string()), "ICU_MOUSE_SIZE_PX" => Some("24".into()), "ICU_POST_ACTION_SLEEP_MS" => Some("0".into()),
        "ICU_CLOSE_ENOUGH_PX" => Some("30".into()), "ICU_CLOSE_ENOUGH_PROGRESS" => Some("0.9".into()),
        "ICU_PERSISTENT_PREDICATES" => Some("1".into()), "ICU_PREDICATE_ROOTS" => Some("/tmp:/var/data".into()), _ => None };
    let f = Features::from_env(&env);
    assert_eq!((f.max_value_chars, f.mouse_size_px, f.post_action_sleep_ms), (0, 24, 0));
    assert_eq!((f.close_enough_distance_px, f.close_enough_progress), (Some(30.0), Some(0.9)));
    assert!(f.persistent_predicates);
    assert_eq!(f.predicate_roots, vec!["/tmp".to_string(), "/var/data".to_string()]);
    // nonsense does not become a setting
    let junk = |k: &str| match k { "ICU_CLOSE_ENOUGH_PROGRESS" => Some("7".to_string()), "ICU_MAX_VALUE_CHARS" => Some("-5".into()), "ICU_MOUSE_SIZE_PX" => Some("many".into()), "ICU_PERSISTENT_PREDICATES" => Some("off".into()), _ => None };
    let j = Features::from_env(&junk);
    assert_eq!((j.max_value_chars, j.mouse_size_px, j.close_enough_progress, j.persistent_predicates), (4096, 12, None, false));
}

#[test]
fn record_start_tags_the_recording_with_the_run_that_produced_it() {
    let (l, _t, log) = tweaked_layer(0, |_| {});
    l.dispatch("record_start", &json!({"output_dir": "/tmp/rec", "run": {"model": "claude-sonnet-5", "prompt": "abc", "run_id": "r-1", "eval": {"suite": "osworld"}}})).unwrap();
    let sent = last(&log, "start_recording");
    assert_eq!(sent["output_dir"], "/tmp/rec");
    let m = &sent["run_metadata"];
    assert_eq!((m["model"].as_str(), m["run_id"].as_str(), m["eval"]["suite"].as_str()), (Some("claude-sonnet-5"), Some("r-1"), Some("osworld")));
    assert_eq!(m["prompt_sha256"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(m["icu"]["session"], "test");
}

#[test]
fn a_recording_without_a_run_sends_no_metadata_at_all() {
    let (l, _t, log) = tweaked_layer(0, |_| {});
    l.dispatch("record_start", &json!({"output_dir": "/tmp/rec"})).unwrap();
    assert!(last(&log, "start_recording").get("run_metadata").is_none(), "byte-identical to a recording from before");
}

#[test]
fn a_malformed_run_is_refused_before_the_driver_hears_of_it() {
    let (l, _t, log) = tweaked_layer(0, |_| {});
    let e = l.dispatch("record_start", &json!({"output_dir": "/tmp/rec", "run": "gpt"})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_INVALID_ARGUMENT));
    assert!(calls(&log, "start_recording").is_empty());
}

#[test]
fn close_enough_rides_the_motion_preset_only_when_asked_for() {
    let (l, _t, log) = tweaked_layer(0, |_| {});
    // a click syncs the cursor with the default preset: no close-enough fields for a driver that lacks them
    let mut s = CLOCK(); s["include_text"] = json!(false);
    l.dispatch("get_window_state", &s).unwrap();
    let mut c = CLOCK(); c["x"] = json!(5.0); c["y"] = json!(5.0);
    l.dispatch("click", &c).unwrap();
    let motion = last(&log, "set_agent_cursor_motion");
    assert!(motion.get("close_enough_distance_px").is_none() && motion.get("close_enough_progress").is_none(), "{motion}");
    // per call
    l.dispatch("cursor", &json!({"close_enough_distance_px": 24.0, "close_enough_progress": 0.9})).unwrap();
    let motion = last(&log, "set_agent_cursor_motion");
    assert_eq!((motion["close_enough_distance_px"].as_f64(), motion["close_enough_progress"].as_f64()), (Some(24.0), Some(0.9)));
    assert_eq!(motion["session"], last(&log, "get_agent_cursor_state")["session"], "the same driver session");
}

#[test]
fn a_configured_close_enough_applies_to_every_session_the_layer_starts() {
    let (l, _t, log) = tweaked_layer(0, |c| { c.features.close_enough_distance_px = Some(30.0); c.features.close_enough_progress = Some(0.95); });
    let mut s = CLOCK(); s["include_text"] = json!(false);
    l.dispatch("get_window_state", &s).unwrap();
    let motion = last(&log, "set_agent_cursor_motion");
    assert_eq!((motion["close_enough_distance_px"].as_f64(), motion["close_enough_progress"].as_f64()), (Some(30.0), Some(0.95)));
    // a per-call value still wins
    l.dispatch("cursor", &json!({"enabled": true, "close_enough_distance_px": 5.0})).unwrap();
    assert_eq!(last(&log, "set_agent_cursor_motion")["close_enough_distance_px"].as_f64(), Some(5.0));
}

#[test]
fn close_enough_without_a_visible_cursor_does_not_start_one() {
    // headless mode keeps the cursor off; configured close-enough must not turn it on
    let (l, _t, log) = tweaked_layer(0, |c| { c.mode = "headless".into(); c.features.close_enough_distance_px = Some(30.0); });
    let mut s = CLOCK(); s["include_text"] = json!(false);
    l.dispatch("get_window_state", &s).unwrap();
    assert!(calls(&log, "set_agent_cursor_motion").is_empty());
}
