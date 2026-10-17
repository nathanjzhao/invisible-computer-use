//! The Sky native pipe: framing, the ping gate, and each request type over the fake driver, with
//! the response shapes held against the real service's capture (`sky-ipc.jsonl`).
use super::*;
use crate::skypipe::*;
use crate::tests::{layer_with_log, CallLog};

fn pipe() -> (SkyPipe, tempfile::TempDir, CallLog) {
    let (l, t, log) = layer_with_log(false, false);
    (SkyPipe::new(l), t, log)
}

fn request(pipe: &SkyPipe, request_type: &str, req: Value) -> Value {
    let msg = json!({"id": 7, "jsonrpc": "2.0", "method": "request", "params": {"clientApiVersion": API_VERSION, "deadlineUnixMilliseconds": 9_999_999_999_999u64, "request": req, "requestType": format!("ComputerUseIPC{request_type}Request")}});
    pipe.handle(&msg).expect("a response")
}

fn keys(v: &Value) -> Vec<String> { let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect(); k.sort(); k }

/// The real capture's recv lines, by the request type that produced them (first of each kind).
fn captured() -> HashMap<String, Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../study/audit/golden-trace-prep-2026-09-25/validation/probe/sky-ipc.jsonl");
    let rows: Vec<Value> = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the real service's capture {} is not there ({e}); these tests read it from the checkout, so a copy of impl/ without study/ cannot run them", path.display())).lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let mut by_id: HashMap<u64, String> = HashMap::new();
    let mut out = HashMap::new();
    for r in &rows {
        let raw: Value = serde_json::from_str(r["raw"].as_str().unwrap()).unwrap();
        if r["dir"] == "send" { if let Some(t) = raw["params"]["requestType"].as_str() { by_id.insert(raw["id"].as_u64().unwrap(), t.to_string()); } }
        else if let Some(t) = by_id.get(&raw["id"].as_u64().unwrap_or(0)) {
            let kind = if raw.get("error").is_some() { format!("{t}:error") } else { t.clone() };
            out.entry(kind).or_insert(raw);
        }
    }
    out
}

// ---- framing ----------------------------------------------------------------------------------

#[test]
fn frames_are_a_u32_little_endian_length_then_the_utf8_payload() {
    let f = encode_frame(b"{\"a\":1}").unwrap();
    assert_eq!(&f[..4], &[7, 0, 0, 0]);
    assert_eq!(&f[4..], b"{\"a\":1}");
    let big = vec![b'x'; 300];
    assert_eq!(&encode_frame(&big).unwrap()[..4], &[44, 1, 0, 0], "300 little-endian");
    assert!(encode_frame(&vec![0u8; MAX_FRAME_BYTES + 1]).is_err(), "8 MiB is the ceiling");
    assert!(encode_frame(&vec![0u8; MAX_FRAME_BYTES]).is_ok());
}

#[test]
fn the_decoder_handles_split_and_batched_frames() {
    let (a, b) = (encode_frame(b"one").unwrap(), encode_frame("two \u{2713}".as_bytes()).unwrap());
    let both = [a.clone(), b.clone()].concat();
    let mut d = FrameDecoder::default();
    assert_eq!(d.push(&both).unwrap(), vec!["one", "two \u{2713}"], "two frames in one read");
    let mut d = FrameDecoder::default();
    let mut got = vec![];
    for byte in &both { got.extend(d.push(&[*byte]).unwrap()); }
    assert_eq!(got, vec!["one", "two \u{2713}"], "one byte at a time, including a split length prefix and a split code point");
    let mut d = FrameDecoder::default();
    assert!(d.push(&((MAX_FRAME_BYTES as u32 + 1).to_le_bytes())).is_err(), "an oversize length is refused before the body arrives");
    let mut d = FrameDecoder::default();
    assert!(d.push(&[2, 0, 0, 0, 0xff, 0xfe]).is_err(), "not UTF-8");
}

// ---- the ping version gate -------------------------------------------------------------------------

#[test]
fn ping_answers_with_the_clients_version_and_refuses_another_with_incompatible_client_version() {
    let (p, _t, _) = pipe();
    let ok = p.handle(&json!({"id": 1, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": "CodexComputerUseIPC-5"}})).unwrap();
    assert_eq!(ok, json!({"jsonrpc": "2.0", "id": 1, "result": {"serverApiVersion": "CodexComputerUseIPC-5"}}), "the real service's answer, byte for byte after parsing");
    let old = p.handle(&json!({"id": 2, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": "CodexComputerUseIPC-3"}})).unwrap();
    assert_eq!(old["error"]["code"], -10013, "ServerErrorCode.incompatibleClientVersion");
    let m = old["error"]["message"].as_str().unwrap();
    assert!(!m.contains("Some(") && !m.contains("String("), "no Rust debug formatting in a message the model may read: {m}");
    assert!(old.get("result").is_none());
    let req = p.handle(&json!({"id": 3, "jsonrpc": "2.0", "method": "request", "params": {"clientApiVersion": "nope", "request": {}, "requestType": "ComputerUseIPCListAppsRequest"}})).unwrap();
    assert_eq!(req["error"]["code"], -10013, "a request under another version is refused too");
    assert_eq!(p.handle(&json!({"id": 4, "jsonrpc": "2.0", "method": "shutdown"})).unwrap()["error"]["code"], -10003);
    assert!(p.handle(&json!({"jsonrpc": "2.0", "method": "ping"})).is_none(), "no id, no answer");
}

// ---- request types --------------------------------------------------------------------------------

#[test]
fn list_apps_and_app_policy_have_the_real_services_shapes() {
    let real = captured();
    let (p, _t, _) = pipe();
    let apps = request(&p, "ListApps", json!({}));
    let list = apps["result"].as_array().unwrap();
    assert!(list.iter().any(|a| a["bundleIdentifier"] == "com.apple.clock" && a["displayName"] == "Clock"));
    let real_keys: Vec<String> = real["ComputerUseIPCListAppsRequest"]["result"][0].as_object().unwrap().keys().cloned().filter(|k| ["bundleIdentifier", "displayName", "isRunning", "isFrontmost"].contains(&k.as_str())).collect();
    for k in &real_keys { assert!(list[0].get(k).is_some(), "the real service's {k} is in ours"); }

    let pol = request(&p, "AppPolicy", json!({"app": "com.apple.clock"}));
    let real_pol = &real["ComputerUseIPCAppPolicyRequest"]["result"];
    assert_eq!(keys(&pol["result"]), keys(real_pol), "{pol}");
    assert_eq!(pol["result"]["decision"], "allowed");
    for k in ["bundleIdentifier", "displayName", "risk"] { assert!(pol["result"]["target"].get(k).is_some(), "{k}"); }
    assert_eq!(request(&p, "AppPolicy", json!({"app": "com.apple.Safari"}))["result"]["decision"], "denied", "denied apps report denied; the client refuses, as Codex's does");
    // the real service's refusal for an app that is not there
    let bad = request(&p, "AppPolicy", json!({"app": "dev.icu.golden.does-not-exist"}));
    let real_bad = &real["ComputerUseIPCAppPolicyRequest:error"];
    assert_eq!((bad["error"]["code"].clone(), bad["error"]["message"].clone()), (real_bad["error"]["code"].clone(), real_bad["error"]["message"].clone()));
}

#[test]
fn get_skyshot_returns_text_screenshot_file_and_the_pack_beside_it() {
    let real = captured();
    let (p, _t, _) = pipe();
    let first = request(&p, "AppGetSkyshot", json!({"app": "com.apple.clock"}));
    let r = &first["result"];
    let real_keys = keys(&real["ComputerUseIPCAppGetSkyshotRequest"]["result"]);
    for k in ["app", "skyshot"] { assert!(real_keys.contains(&k.to_string()) && r.get(k).is_some(), "{k}"); }
    assert_eq!(r["app"]["bundleIdentifier"], "com.apple.clock");
    assert_eq!(r["app"]["pid"], 26765);
    let text = r["skyshot"]["text"].as_str().unwrap();
    assert!(text.starts_with("Window: \"Clock\", App: Clock.\n"), "Sky's header: {}", &text[..60.min(text.len())]);
    assert!(!text.contains("<app_specific_instructions>"), "the pack is its own field, as in the real result");
    assert!(r["appSpecificInstructions"].as_str().unwrap().contains("Clock Computer Use"), "first touch of a pack app");
    let shot = &r["skyshot"]["screenshot"];
    assert_eq!(keys(shot), keys(&real["ComputerUseIPCAppGetSkyshotRequest"]["result"]["skyshot"]["screenshot"]), "url and mimeType");
    let file = shot["url"].as_str().unwrap().strip_prefix("file://").unwrap().replace("%20", " ");
    assert!(std::fs::metadata(&file).unwrap().len() > 0, "the URL names a real file: {file}");
    // the second read is the diff/no-change line, like the real service's second turn
    let second = request(&p, "AppGetSkyshot", json!({"app": "com.apple.clock"}));
    assert!(second["result"]["skyshot"]["text"].as_str().unwrap().starts_with("There has been no change in the accessibility tree for Window"), "{second}");
    assert!(second["result"].get("appSpecificInstructions").is_none(), "the pack comes once per session");
    // disableDiff: the full tree again
    let full = request(&p, "AppGetSkyshot", json!({"app": "com.apple.clock", "disableDiff": true}));
    assert!(full["result"]["skyshot"]["text"].as_str().unwrap().starts_with("Window: \"Clock\""));
    // an app that is not there
    assert_eq!(request(&p, "AppGetSkyshot", json!({"app": "dev.nope"}))["error"]["code"], -10010);
    // a denied app is never read
    assert_eq!(request(&p, "AppGetSkyshot", json!({"app": "com.apple.Safari"}))["error"]["code"], -10006);
}

#[test]
fn actions_translate_to_the_layers_tools_and_answer_with_the_apps_identity_only() {
    let real = captured();
    let (p, _t, log) = pipe();
    let sky = request(&p, "AppGetSkyshot", json!({"app": "com.apple.clock"}));
    let tree = sky["result"]["skyshot"]["text"].as_str().unwrap();
    let el = crate::tests::tree_ids(tree)[1];
    let act = |a: Value| request(&p, "AppPerformAction", json!({"app": "com.apple.clock", "action": a}));
    let last = |tool: &str| crate::tests::last(&log, tool);
    // click by element id: the wire shape is {at:{elementID:{_0:"N"}}, clickCount, mouseButton}
    let r = act(json!({"click": {"at": {"elementID": {"_0": el.to_string()}}, "clickCount": 1, "mouseButton": 0}}));
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(keys(&r["result"]), keys(&real["ComputerUseIPCAppPerformActionRequest"]["result"]), "the real answer is {{app}} only");
    assert_eq!(keys(&r["result"]["app"]).iter().filter(|k| *k != "appPath").count(), 2, "bundleIdentifier and pid");
    assert_eq!(last("click")["window_id"], 25427, "the window the skyshot read");
    // type, pressKey, setValue
    act(json!({"type": {"_0": "hello"}}));
    assert_eq!(last("type_text")["text"], "hello");
    act(json!({"pressKey": {"_0": "super+a"}}));
    assert!(log.lock().unwrap().iter().any(|(t, _)| t == "hotkey" || t == "press_key"), "a chord reached the driver");
    let sv = act(json!({"setValue": {"elementID": el.to_string(), "value": "123"}}));
    assert!(sv.get("result").is_some() || sv["error"]["code"] == -10005, "{sv}");
    // errors keep the real service's numbers and wording
    let bad = act(json!({"click": {"at": {"elementID": {"_0": "99999"}}, "clickCount": 1, "mouseButton": 0}}));
    let real_bad = &real["ComputerUseIPCAppPerformActionRequest:error"];
    assert_eq!(bad["error"]["code"], -10005);
    assert_eq!(bad["error"]["message"], "Computer Use server error -10005: 99999 is an invalid element ID");
    assert_eq!(real_bad["error"]["code"], -10005);
    let nosuch = act(json!({"performSecondaryAction": {"action": "NoSuchAction", "elementID": el.to_string()}}));
    assert_eq!(nosuch["error"]["code"], -10005);
    assert!(nosuch["error"]["message"].as_str().unwrap().contains("NoSuchAction is not a valid secondary action"), "{nosuch}");
    // coordinates, drag, scroll with direction and pages
    act(json!({"click": {"at": {"coordinate": {"_0": [10.0, 20.0]}}, "clickCount": 2, "mouseButton": 1}}));
    let c = log.lock().unwrap().iter().rev().find(|(t, _)| t == "right_click" || t == "double_click" || t == "click").cloned().unwrap();
    assert!(c.1.get("x").is_some() || c.1.get("element_token").is_some(), "{c:?}");
    act(json!({"drag": {"from": [1.0, 2.0], "to": [30.0, 40.0]}}));
    assert_eq!(last("drag")["window_id"], 25427);
    act(json!({"scroll": {"at": {"coordinate": {"_0": [5.0, 5.0]}}, "direction": "down", "pages": 2}}));
    let s = last("scroll");
    assert_eq!((s["direction"].as_str(), s["amount"].as_u64()), (Some("down"), Some(20)), "two pages are twenty notches: {s}");
    // unknown actions and request types
    assert_eq!(act(json!({"teleport": {}}))["error"]["code"], -10005);
    assert_eq!(request(&p, "StartAudioRecording", json!({}))["error"]["code"], -10003, "audio is a non-goal");
    assert_eq!(request(&p, "Nonsense", json!({}))["error"]["code"], -10003);
}

#[test]
fn an_app_start_launches_and_names_the_app() {
    let (p, _t, log) = pipe();
    let r = request(&p, "AppStart", json!({"app": "com.apple.clock"}));
    assert_eq!(r["result"]["app"]["bundleIdentifier"], "com.apple.clock", "{r}");
    assert!(log.lock().unwrap().iter().any(|(t, _)| t == "launch_app"));
    assert_eq!(request(&p, "AppStart", json!({"app": "com.apple.Safari"}))["error"]["code"], -10006, "policy applies to a launch too");
}

#[test]
fn runtime_expired_and_malformed_deadlines_do_not_reach_the_driver() {
    let (p, _t, log) = pipe();
    for deadline in [json!(0), json!(1), json!(-1), json!("later"), json!(null), json!(1.5)] {
        log.lock().unwrap().clear();
        let reply = p.handle(&json!({"id": 1, "method": "request", "params": {
            "clientApiVersion": API_VERSION, "deadlineUnixMilliseconds": deadline,
            "requestType": "ComputerUseIPCAppPerformActionRequest",
            "request": {"app": "com.apple.clock", "action": {"type": {"_0": "must not type"}}}
        }})).unwrap();
        assert!(reply.get("error").is_some(), "{deadline}: {reply}");
        assert!(log.lock().unwrap().is_empty(), "{deadline}: no driver work after a refused deadline");
    }
    // Older callers may omit the deadline; the normal layer budget still applies.
    let reply = p.handle(&json!({"id": 2, "method": "request", "params": {
        "clientApiVersion": API_VERSION, "requestType": "ComputerUseIPCListAppsRequest", "request": {}
    }})).unwrap();
    assert!(reply.get("result").is_some(), "{reply}");
}

#[test]
fn runtime_paste_formats_are_never_silently_changed() {
    let (p, _t, log) = pipe();
    for format in [json!("html"), json!("md"), json!("unknown"), json!(null), json!(7)] {
        log.lock().unwrap().clear();
        let reply = request(&p, "AppPerformAction", json!({"app": "com.apple.clock",
            "action": {"paste": {"text": "<b>hello</b>", "format": format}}}));
        assert!(reply.get("error").is_some(), "{format}: {reply}");
        assert!(!log.lock().unwrap().iter().any(|(t, _)| t == "clipboard_write" || t == "hotkey"), "unsupported paste must not change the clipboard or app");
    }
    let reply = request(&p, "AppPerformAction", json!({"app": "com.apple.clock",
        "action": {"paste": {"text": "hello", "format": "text"}}}));
    assert!(reply.get("result").is_some(), "{reply}");
    let writes: Vec<_> = log.lock().unwrap().iter().filter(|(t, _)| t == "clipboard_write").map(|(_, a)| a["text"].clone()).collect();
    assert_eq!(writes, vec![json!("hello"), json!("PRIOR")]);
}

#[test]
fn runtime_cursor_placement_is_not_misreported_as_text_selection() {
    let (p, _t, log) = pipe();
    let sky = request(&p, "AppGetSkyshot", json!({"app": "com.apple.clock"}));
    let el = crate::tests::tree_ids(sky["result"]["skyshot"]["text"].as_str().unwrap())[0];
    for selection in [json!("cursor_before"), json!("cursor_after"), json!("unknown"), json!(null)] {
        log.lock().unwrap().clear();
        let reply = request(&p, "AppPerformAction", json!({"app": "com.apple.clock", "action": {
            "selectText": {"elementID": el.to_string(), "text": "cat", "selection": selection}
        }}));
        assert!(reply.get("error").is_some(), "{selection}: {reply}");
        assert!(!log.lock().unwrap().iter().any(|(t, _)| t == "select_text"), "must not select text when cursor placement was requested");
    }
    let reply = request(&p, "AppPerformAction", json!({"app": "com.apple.clock", "action": {
        "selectText": {"elementID": el.to_string(), "text": "cat", "prefix": "dog ", "suffix": " bird", "selection": "text"}
    }}));
    assert!(reply.get("result").is_some(), "{reply}");
    let sent = crate::tests::last(&log, "select_text");
    assert_eq!((sent["text"].as_str(), sent["prefix"].as_str(), sent["suffix"].as_str()), (Some("cat"), Some("dog "), Some(" bird")));
}

#[test]
fn runtime_a_deadline_that_expires_during_settling_prevents_input() {
    let (layer, _t, log) = layer_with_log(false, false);
    let deadline = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64 + 100;
    layer.settler.defer(200);
    let result = layer.input_before("type_text", &json!({"window": {"app": "com.apple.clock", "id": 25427}, "text": "expired"}), Some(deadline));
    assert_eq!(result.unwrap_err().code(), Some(CODE_DEADLINE_EXCEEDED));
    assert!(!log.lock().unwrap().iter().any(|(t, _)| t == "type_text"));
}

#[test]
fn runtime_a_closed_window_or_restarted_app_requires_a_new_observation_before_input() {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    struct ClosingWindow {
        fake: crate::tests::FakeCua,
        closed: Arc<AtomicBool>,
        restart: bool,
    }
    impl cua::Cua for ClosingWindow {
        fn call(&self, tool: &str, args: Value) -> Result<Value, cua::CuaError> {
            let mut result = self.fake.call(tool, args)?;
            if self.closed.load(Ordering::SeqCst) {
                if tool == "list_windows" {
                    if self.restart {
                        for w in result["windows"].as_array_mut().unwrap() {
                            if w["pid"] == 26765 { w["pid"] = json!(26766); }
                        }
                    } else {
                        result["windows"].as_array_mut().unwrap().retain(|w| w["window_id"] != 25427);
                    }
                } else if tool == "list_apps" && self.restart {
                    result["apps"][0]["pid"] = json!(26766);
                }
            }
            Ok(result)
        }
        fn tool_names(&self) -> Result<Vec<String>, cua::CuaError> { self.fake.tool_names() }
    }
    for restart in [false, true] {
        let (layer, _t, log) = layer_with_log(false, false);
        let closed = Arc::new(AtomicBool::new(false));
        let p = SkyPipe::new(Layer::with_cua(layer.cfg.clone(), Box::new(ClosingWindow {
            fake: crate::tests::FakeCua { calls: log.clone(), locked: false }, closed: closed.clone(), restart,
        })));
        assert!(request(&p, "AppGetSkyshot", json!({"app": "com.apple.clock"})).get("result").is_some());
        closed.store(true, Ordering::SeqCst);
        log.lock().unwrap().clear();
        let action = || request(&p, "AppPerformAction", json!({"app": "com.apple.clock", "action": {"pressKey": {"_0": "Return"}}}));
        let refused = action();
        assert_eq!(refused["error"]["code"], ERR_RUNNING_APPLICATION_NOT_FOUND, "{refused}");
        assert!(!log.lock().unwrap().iter().any(|(t, _)| t == "press_key" || t == "hotkey"), "input must not be redirected to the remaining sign-in window");
        // Explicit observation recovers by selecting and showing the remaining window.
        assert!(request(&p, "AppGetSkyshot", json!({"app": "com.apple.clock"})).get("result").is_some());
        assert!(action().get("result").is_some());
        assert_eq!(crate::tests::last(&log, "press_key")["window_id"], if restart { 25427 } else { 31337 });
    }
}

#[cfg(unix)]
#[test]
fn over_a_real_socket_a_client_pings_requests_and_a_stranger_is_hung_up_on() {
    use std::io::{Read, Write};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sock").join("computeruse.sock");
    let (l, _t, _) = layer_with_log(false, false);
    let pipe = std::sync::Arc::new(SkyPipe::new(l));
    let listener = bind(&path).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700, "the socket's directory is private");
    }
    let server = { let pipe = pipe.clone(); std::thread::spawn(move || { for s in listener.incoming().take(2) { let s = s.unwrap(); let pipe = pipe.clone(); std::thread::spawn(move || serve_connection(&pipe, s)); } }) };
    let roundtrip = |stream: &mut std::os::unix::net::UnixStream, msg: Value| -> Option<Value> {
        stream.write_all(&encode_frame(msg.to_string().as_bytes()).unwrap()).unwrap();
        let mut d = FrameDecoder::default(); let mut buf = [0u8; 4096];
        loop { let n = stream.read(&mut buf).ok()?; if n == 0 { return None; } if let Some(m) = d.push(&buf[..n]).unwrap().pop() { return serde_json::from_str(&m).ok(); } }
    };
    let mut c = std::os::unix::net::UnixStream::connect(&path).unwrap();
    c.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    assert_eq!(roundtrip(&mut c, json!({"id": 1, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": API_VERSION}})).unwrap()["result"]["serverApiVersion"], API_VERSION);
    let apps = roundtrip(&mut c, json!({"id": 2, "jsonrpc": "2.0", "method": "request", "params": {"clientApiVersion": API_VERSION, "request": {}, "requestType": "ComputerUseIPCListAppsRequest"}})).unwrap();
    assert!(apps["result"].as_array().is_some_and(|a| !a.is_empty()));
    // garbage with no JSON in it: the connection is closed without a framed reply
    let mut stranger = std::os::unix::net::UnixStream::connect(&path).unwrap();
    stranger.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    assert!(roundtrip(&mut stranger, json!("not a request object")).is_none());
    server.join().unwrap();
    // a stale socket file (nobody listening) is replaced; a live one is refused
    let first = bind(&path).expect("a stale file is replaced");
    let second = bind(&path);
    assert_eq!(second.err().map(|e| e.kind()), Some(std::io::ErrorKind::AddrInUse), "a socket somebody serves is not stolen");
    drop(first);
}
