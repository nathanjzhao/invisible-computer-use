//! Two-phase prepare / approve / commit over the fake driver's real Clock document.
use super::*;
use crate::mcp::{Io, Server};
use crate::tests::{layer_with_log, CallLog, CLOCK};

fn observe(l: &Layer) -> u64 {
    let mut a = CLOCK(); a["include_text"] = json!(true);
    l.dispatch("get_window_state", &a).unwrap();
    // an actionable element the latest state routes
    let t = l.targets.lock().unwrap();
    *t.get(&25427).unwrap().elements.keys().min().unwrap()
}

fn input_calls(log: &CallLog) -> Vec<(String, Value)> {
    log.lock().unwrap().iter().filter(|(t, _)| matches!(t.as_str(), "click" | "double_click" | "right_click" | "type_text" | "set_value" | "hotkey" | "press_key" | "clipboard_write")).cloned().collect()
}

fn prepare_click(l: &Layer, element: u64) -> Value {
    let mut a = CLOCK(); a["element_index"] = json!(element);
    l.dispatch("prepare_action", &json!({"tool": "click", "arguments": a})).unwrap()
}

#[test]
fn prepare_resolves_the_plan_and_performs_nothing() {
    let (l, _t, log) = layer_with_log(false, false);
    let el = observe(&l);
    let before = input_calls(&log).len();
    let plan = prepare_click(&l, el);
    assert_eq!(plan["tool"], "click");
    assert_eq!(plan["app"]["id"], "com.apple.clock");
    assert_eq!(plan["target"]["element_index"], el);
    assert!(plan["target"]["label"].is_string() || plan["target"]["role"].is_string(), "the plan names the element, not only its id: {plan}");
    assert_eq!(plan["approval"]["required"], true);
    assert!(plan["prepared_id"].as_str().unwrap().len() >= 24);
    assert_eq!(input_calls(&log).len(), before, "no input tool reached the driver");
    // text plans show the text
    let mut a = CLOCK(); a["text"] = json!("Dear Sam, I quit.");
    let p = l.dispatch("prepare_action", &json!({"tool": "type_text", "arguments": a})).unwrap();
    assert_eq!((p["text"].as_str(), p["editable"].as_str()), (Some("Dear Sam, I quit."), Some("text")));
    assert_eq!(input_calls(&log).len(), before);
}

#[test]
fn commit_needs_the_users_approval_and_then_runs_exactly_once() {
    let (l, _t, log) = layer_with_log(false, false);
    let el = observe(&l);
    let id = prepare_click(&l, el)["prepared_id"].as_str().unwrap().to_string();
    let e = l.dispatch("commit_action", &json!({"prepared_id": id})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_PREPARED_APPROVAL_REQUIRED));
    assert_eq!(e.structured["elicitation"]["meta"]["tool_name"], "commit_action");
    assert_eq!(e.structured["elicitation"]["requestedSchema"]["properties"]["persist"]["enum"], json!(["once", "session", "always"]));
    assert!(e.structured["elicitation"]["message"].as_str().unwrap().starts_with("Let Computer Use click"), "{}", e.message);
    assert!(input_calls(&log).is_empty(), "an unapproved commit performs nothing");
    l.dispatch("approve_prepared", &json!({"prepared_id": id, "action": "accept"})).unwrap();
    let r = l.dispatch("commit_action", &json!({"prepared_id": id})).unwrap();
    assert!(matches!(r["outcome"].as_str(), Some("committed" | "outcome_uncertain")), "{r}");
    assert_eq!(r["outcome"], "outcome_uncertain", "the fake driver reports effect: unverifiable, which is not a confirmed delivery");
    assert_eq!(input_calls(&log).len(), 1);
    let again = l.dispatch("commit_action", &json!({"prepared_id": id})).unwrap_err();
    assert_eq!((again.code(), again.structured["reason"].as_str()), (Some(CODE_PLAN_UNAVAILABLE), Some("already_committed")));
    assert_eq!(input_calls(&log).len(), 1, "never replayed");
}

#[test]
fn a_cancelled_approval_is_final_and_says_not_to_retry() {
    let (l, _t, log) = layer_with_log(false, false);
    let el = observe(&l);
    let id = prepare_click(&l, el)["prepared_id"].as_str().unwrap().to_string();
    for action in ["decline", "cancel"] {
        let (l2, _t2, log2) = layer_with_log(false, false);
        let el2 = observe(&l2);
        let id2 = prepare_click(&l2, el2)["prepared_id"].as_str().unwrap().to_string();
        let e = l2.dispatch("approve_prepared", &json!({"prepared_id": id2, "action": action})).unwrap_err();
        assert_eq!(e.code(), Some(CODE_APPROVAL_CANCELED));
        assert!(e.message.contains("Do not ask the user to approve it again or retry"), "{}", e.message);
        for later in [l2.dispatch("commit_action", &json!({"prepared_id": id2})).unwrap_err(), l2.dispatch("approve_prepared", &json!({"prepared_id": id2})).unwrap_err()] {
            assert!(matches!(later.code(), Some(CODE_APPROVAL_CANCELED | CODE_PLAN_UNAVAILABLE)), "{action}: {}", later.message);
        }
        assert!(input_calls(&log2).is_empty(), "{action}: nothing was performed");
    }
    let _ = (&l, &log, id);
}

#[test]
fn the_user_can_edit_the_text_before_it_is_sent() {
    let (l, _t, log) = layer_with_log(false, false);
    observe(&l);
    let mut a = CLOCK(); a["text"] = json!("original");
    let id = l.dispatch("prepare_action", &json!({"tool": "type_text", "arguments": a})).unwrap()["prepared_id"].as_str().unwrap().to_string();
    l.dispatch("approve_prepared", &json!({"prepared_id": id, "action": "accept", "text": "edited by the user"})).unwrap();
    let r = l.dispatch("commit_action", &json!({"prepared_id": id})).unwrap();
    assert_eq!(r["edited"], true);
    assert_eq!(input_calls(&log).iter().find(|(t, _)| t == "type_text").unwrap().1["text"], "edited by the user");
    // a click has no text to edit
    let el = observe(&l);
    let click = prepare_click(&l, el)["prepared_id"].as_str().unwrap().to_string();
    assert_eq!(l.dispatch("approve_prepared", &json!({"prepared_id": click, "text": "x"})).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT));
}

#[test]
fn the_plan_is_bound_to_what_the_model_saw() {
    let (l, _t, log) = layer_with_log(false, false);
    let el = observe(&l);
    let id = prepare_click(&l, el)["prepared_id"].as_str().unwrap().to_string();
    l.dispatch("approve_prepared", &json!({"prepared_id": id})).unwrap();
    // the app re-renders: that id is now a different element
    l.targets.lock().unwrap().get_mut(&25427).unwrap().labels.insert(el, ("AXButton".into(), "Delete everything".into()));
    let e = l.dispatch("commit_action", &json!({"prepared_id": id})).unwrap_err();
    assert_eq!((e.code(), e.structured["reason"].as_str()), (Some(CODE_PLAN_UNAVAILABLE), Some("target_changed")));
    assert!(input_calls(&log).is_empty(), "never a click on whatever took its place");
}

#[test]
fn plans_expire_and_ids_cannot_walk_the_filesystem() {
    let (l, _t, _) = layer_with_log(false, false);
    let el = observe(&l);
    let id = prepare_click(&l, el)["prepared_id"].as_str().unwrap().to_string();
    let path = l.prepared_path(&id);
    let mut p: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    p["plan"]["expires_unix"] = json!(1);
    std::fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
    let e = l.dispatch("commit_action", &json!({"prepared_id": id})).unwrap_err();
    assert_eq!((e.code(), e.structured["reason"].as_str()), (Some(CODE_PLAN_UNAVAILABLE), Some("expired")));
    for bad in ["../x", "a/b", "", "zz", "..%2f..%2fetc"] {
        assert_eq!(l.dispatch("commit_action", &json!({"prepared_id": bad})).unwrap_err().code(), Some(CODE_PLAN_UNAVAILABLE), "{bad:?}");
    }
}

#[test]
fn a_session_scope_approves_the_same_kind_of_action_on_the_same_app_later() {
    let (l, _t, log) = layer_with_log(false, false);
    let el = observe(&l);
    let first = prepare_click(&l, el)["prepared_id"].as_str().unwrap().to_string();
    l.dispatch("approve_prepared", &json!({"prepared_id": first, "persist": "session"})).unwrap();
    l.dispatch("commit_action", &json!({"prepared_id": first})).unwrap();
    let second = prepare_click(&l, el);
    assert_eq!(second["approval"]["required"], false, "keyed by tool and app");
    l.dispatch("commit_action", &json!({"prepared_id": second["prepared_id"]})).unwrap();
    assert_eq!(input_calls(&log).len(), 2);
    // another tool on the same app still asks
    let mut a = CLOCK(); a["text"] = json!("hi");
    let typed = l.dispatch("prepare_action", &json!({"tool": "type_text", "arguments": a})).unwrap();
    assert_eq!(typed["approval"]["required"], true);
    assert_eq!(l.dispatch("commit_action", &json!({"prepared_id": typed["prepared_id"]})).unwrap_err().code(), Some(CODE_PREPARED_APPROVAL_REQUIRED));
    // a once approval is not a scope
    let once = prepare_click(&l, el);
    assert!(once["prepared_id"].is_string());
    let (l2, _t2, _) = layer_with_log(false, false);
    let el2 = observe(&l2);
    let p = prepare_click(&l2, el2)["prepared_id"].as_str().unwrap().to_string();
    l2.dispatch("approve_prepared", &json!({"prepared_id": p, "persist": "once"})).unwrap();
    l2.dispatch("commit_action", &json!({"prepared_id": p})).unwrap();
    assert_eq!(prepare_click(&l2, el2)["approval"]["required"], true);
}

#[test]
fn refusals_that_stop_the_real_call_stop_the_plan() {
    let (l, _t, _) = layer_with_log(false, false);
    observe(&l);
    // Safari is in the fake's denied list
    let e = l.dispatch("prepare_action", &json!({"tool": "click", "arguments": {"window": {"app": "com.apple.Safari", "id": 900}, "x": 1, "y": 1}})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_APP_NOT_ALLOWED));
    assert_eq!(l.dispatch("prepare_action", &json!({"tool": "get_window_state", "arguments": CLOCK()})).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT));
    assert_eq!(l.dispatch("prepare_action", &json!({"tool": "type_text", "arguments": CLOCK()})).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT), "type_text needs text");
    // an element the latest state does not show is a plan that cannot be made
    let mut a = CLOCK(); a["element_index"] = json!(99999);
    let e = l.dispatch("prepare_action", &json!({"tool": "click", "arguments": a})).unwrap_err();
    assert_eq!((e.code(), e.structured["reason"].as_str()), (Some(CODE_PLAN_UNAVAILABLE), Some("target_missing")));
}

#[test]
fn typing_into_a_password_field_is_masked_in_the_plan_and_still_needs_the_secret_confirmation() {
    let (l, _t, log) = layer_with_log(false, false);
    let mut s = json!({"window": {"app": "com.apple.clock", "id": 31337}, "include_text": true});
    s["include_screenshot"] = json!(true);
    l.dispatch("get_window_state", &s).unwrap();
    let mut a = json!({"window": {"app": "com.apple.clock", "id": 31337}, "text": "hunter2-secret"});
    let plan = l.dispatch("prepare_action", &json!({"tool": "type_text", "arguments": a.clone()})).unwrap();
    assert_eq!(plan["sensitive"], true);
    assert!(!plan.to_string().contains("hunter2"), "the secret is not echoed in the plan: {plan}");
    let id = plan["prepared_id"].as_str().unwrap().to_string();
    l.dispatch("approve_prepared", &json!({"prepared_id": id})).unwrap();
    // the approval of the plan does not replace the policy-[14] confirmation for the secret
    let e = l.dispatch("commit_action", &json!({"prepared_id": id})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_CONFIRMATION_REQUIRED), "{}", e.message);
    assert!(input_calls(&log).is_empty());
    // nothing was performed, so the plan is not spent: once the human confirms the secret it commits
    l.dispatch("confirm_secure_input", &json!({"window_id": 31337u64, "action": "accept"})).unwrap();
    let r = l.dispatch("commit_action", &json!({"prepared_id": id})).unwrap();
    assert!(matches!(r["outcome"].as_str(), Some("committed" | "outcome_uncertain")), "{r}");
    assert_eq!(input_calls(&log).iter().filter(|(t, _)| t == "type_text").count(), 1);
    a["text"] = json!("x");
    let _ = a;
}

#[test]
fn a_refusal_before_the_driver_is_asked_leaves_the_plan_committable_but_a_driver_attempt_spends_it() {
    // the app still needs approval: the commit is refused at the gate and nothing is performed
    let (l, _t, log) = layer_with_log(false, true);
    let el = observe(&l);
    let id = prepare_click(&l, el)["prepared_id"].as_str().unwrap().to_string();
    l.dispatch("approve_prepared", &json!({"prepared_id": id})).unwrap();
    let e = l.dispatch("commit_action", &json!({"prepared_id": id})).unwrap_err();
    assert_eq!(e.code(), Some(CODE_APP_APPROVAL_REQUIRED));
    assert!(input_calls(&log).is_empty());
    l.dispatch("approve_app", &json!({"app": "com.apple.clock", "action": "accept"})).unwrap();
    assert!(l.dispatch("commit_action", &json!({"prepared_id": id})).is_ok(), "not spent by the refused attempt");
    assert_eq!(input_calls(&log).len(), 1);
    // a locked host refuses at the driver: that attempt reached it, so the plan is spent
    let (locked, _t2, _) = layer_with_log(true, false);
    let el = observe(&locked);
    let id = prepare_click(&locked, el)["prepared_id"].as_str().unwrap().to_string();
    locked.dispatch("approve_prepared", &json!({"prepared_id": id})).unwrap();
    assert!(locked.dispatch("commit_action", &json!({"prepared_id": id})).is_err());
    assert_eq!(locked.dispatch("commit_action", &json!({"prepared_id": id})).unwrap_err().code(), Some(CODE_PLAN_UNAVAILABLE));
}

#[test]
fn over_mcp_the_server_asks_the_human_and_the_model_cannot_approve_for_itself() {
    let (l, _t, log) = layer_with_log(false, false);
    let el = observe(&l);
    let mut a = CLOCK(); a["text"] = json!("original");
    let id = l.dispatch("prepare_action", &json!({"tool": "type_text", "arguments": a})).unwrap()["prepared_id"].as_str().unwrap().to_string();
    let _ = el;
    let mut srv = Server::new(&l);
    let input = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {"elicitation": {}}, "clientInfo": {"name": "t", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "approve_prepared", "arguments": {"prepared_id": id}}}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "commit_action", "arguments": {"prepared_id": id}}}),
        // the server's elicitation is request id 1 on its own counter
        json!({"jsonrpc": "2.0", "id": 1, "result": {"action": "accept", "content": {"persist": "once", "text": "edited in the dialog"}}}),
    ].iter().map(|m| format!("{m}\n")).collect::<String>();
    let mut reader = std::io::Cursor::new(input.into_bytes());
    let mut out: Vec<u8> = Vec::new();
    { let mut io = Io { reader: &mut reader, writer: &mut out }; srv.serve(&mut io).unwrap(); }
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[1]["result"]["isError"], true, "a model cannot approve for itself: {}", lines[1]);
    assert!(lines[1]["result"]["structuredContent"]["message"].as_str().unwrap().contains("not callable over MCP"));
    let elicit = &lines[2];
    assert_eq!(elicit["method"], "elicitation/create");
    assert!(elicit["params"]["message"].as_str().unwrap().contains("type_text \"original\""), "{}", elicit["params"]["message"]);
    assert_eq!(elicit["params"]["requestedSchema"]["properties"]["text"]["type"], "string", "the text is editable in the dialog");
    assert_eq!(lines[3]["id"], 3);
    let outcome = &lines[3]["result"]["structuredContent"];
    assert_eq!((outcome["outcome"].as_str(), outcome["edited"].as_bool()), (Some("outcome_uncertain"), Some(true)), "{outcome}");
    assert_eq!(input_calls(&log).iter().find(|(t, _)| t == "type_text").unwrap().1["text"], "edited in the dialog");
}
