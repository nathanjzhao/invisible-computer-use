//! The unified `get_state` / `get_app` facade over the fake driver's real Clock document.
use super::*;
use crate::tests::{layer_with_log, tree_ids};

#[test]
fn get_state_is_the_whole_inventory_in_one_call() {
    let (l, _t, log) = layer_with_log(false, false);
    let s = l.dispatch("get_state", &json!({})).unwrap();
    let ids: Vec<&str> = s["apps"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"com.apple.clock") && ids.contains(&"notion.id"), "{ids:?}");
    let clock = s["apps"].as_array().unwrap().iter().find(|a| a["id"] == "com.apple.clock").unwrap();
    assert_eq!(clock["windows"].as_array().unwrap().len(), 2, "its open windows ride with it");
    assert_eq!(s["frontmost"]["id"], 25427, "the first on-screen window of the driver's z-order");
    assert_eq!(s["frontmost"]["app"], "com.apple.clock");
    // honest about what is not covered: not an empty list that reads as 'no tabs open'
    assert_eq!(s["surfaces"], json!({"native_apps": true, "browsers": false, "tabs": false}));
    assert!(s["_gaps"][0].as_str().unwrap().contains("not listed"));
    assert!(s.get("tabs").is_none() && s.get("browsers").is_none());
    let tools: Vec<String> = log.lock().unwrap().iter().map(|(t, _)| t.clone()).collect();
    assert!(!tools.iter().any(|t| t == "get_window_state"), "an inventory does not observe any window");
}

#[test]
fn get_app_returns_the_app_its_window_and_the_initial_state_by_name_id_or_path() {
    for wanted in ["Clock", "clock", "com.apple.clock", "/System/Applications/Clock.app"] {
        let (l, _t, _) = layer_with_log(false, false);
        let r = l.dispatch("get_app", &json!({"app": wanted})).unwrap_or_else(|e| panic!("{wanted}: {}", e.message));
        assert_eq!(r["app"]["id"], "com.apple.clock", "{wanted}");
        assert_eq!(r["window"]["id"], 25427, "{wanted}: the frontmost of the app's two windows");
        assert_eq!(r["selected"], "frontmost");
        assert_eq!(r["windows"].as_array().unwrap().len(), 2);
        assert_eq!(r["launched"], false);
        let tree = r["state"]["accessibility"]["tree"].as_str().unwrap();
        assert!(!tree_ids(tree).is_empty(), "the accessibility tree is part of the initial state");
        assert_eq!(r["state"]["screenshots"].as_array().unwrap().len(), 1, "and so is the screenshot");
        assert_eq!(r["state"]["window"]["id"], 25427);
    }
}

#[test]
fn get_app_state_is_the_same_state_get_window_state_returns() {
    let (l, _t, _) = layer_with_log(false, false);
    let via_app = l.dispatch("get_app", &json!({"app": "Clock"})).unwrap();
    let mut a = json!({"window": {"app": "com.apple.clock", "id": 25427}, "include_text": true});
    a["include_screenshot"] = json!(true);
    let direct = l.dispatch("get_window_state", &a).unwrap();
    assert_eq!(via_app["state"]["window"], direct["window"]);
    assert_eq!((via_app["state"]["accessibilitySnapshotCount"].as_u64(), direct["accessibilitySnapshotCount"].as_u64()), (Some(1), Some(2)), "one lineage: the facade took snapshot 1, the direct call continued it");
    assert!(direct["accessibility"]["tree"].as_str().unwrap().starts_with("There has been no change in the accessibility tree"));
}

#[test]
fn an_app_with_no_window_says_so_instead_of_inventing_one() {
    let (l, _t, log) = layer_with_log(false, false);
    let r = l.dispatch("get_app", &json!({"app": "Notion"})).unwrap();
    assert_eq!(r["app"]["id"], "notion.id");
    // the real driver lists installed apps that are not open (isRunning false): getApp opens them (found on a real Mac)
    assert_eq!(r["launched"], true);
    assert!(log.lock().unwrap().iter().any(|(t, a)| t == "launch_app" && a["bundle_id"] == "notion.id"));
    assert!(r["window"].is_null() && r["state"].is_null());
    assert!(r["hint"].as_str().unwrap().contains("no open window"));
}

#[test]
fn policy_and_invalid_apps_are_refused_like_everywhere_else() {
    let (l, _t, log) = layer_with_log(false, false);
    // Safari is in the fake's denied list
    let e = l.dispatch("get_app", &json!({"app": "Safari"})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_APP_NOT_ALLOWED), "{}", e.message);
    assert!(!log.lock().unwrap().iter().any(|(t, a)| t == "get_window_state" && a["pid"] == 500), "a denied app is never observed");
    // an app that is neither listed nor installed: launched, still absent, Sky's Invalid app
    let e = l.dispatch("get_app", &json!({"app": "Nope Pad"})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_INVALID_APP));
    assert!(e.message.contains("Invalid app: Nope Pad"));
    assert_eq!(l.dispatch("get_app", &json!({})).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT));
}
