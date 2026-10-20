//! B2 tests: the fleet Service against a LOCAL mock inbox/status server (std TcpListener,
//! no real server needed). Proves the poll+ingest+stage+status path and invariants
//! I1 (nothing runs), I2 (stage refuses off/expired/withdrawn), I3 (accept is live).
use super::*;
use crate::memory::{self, workflows::Runtime};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const DISPATCH_ID: &str = "dp-9b1d3c5e-7a42-4f08-b6c1-2e8f0a4d7c13";

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dispatch").join(name);
    std::fs::read_to_string(path).unwrap()
}

/// A tiny HTTP/1.1 server that answers the three actor routes from in-memory fixtures.
/// `reject_status` flips the status route to a 409 withdrawn so I3 can be exercised.
struct Mock {
    endpoint: String,
    stop: Arc<AtomicBool>,
    reject_status: Arc<AtomicBool>,
    status_calls: Arc<std::sync::Mutex<Vec<String>>>,
    requests: Arc<std::sync::Mutex<Vec<String>>>,
}
impl Drop for Mock { fn drop(&mut self) { self.stop.store(true, Ordering::SeqCst); } }

fn start_mock() -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let reject_status = Arc::new(AtomicBool::new(false));
    let status_calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (s, r, c, all) = (stop.clone(), reject_status.clone(), status_calls.clone(), requests.clone());
    std::thread::spawn(move || {
        let inbox = fixture("inbox_response.json");
        while !s.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut sock, _)) => {
                    let mut buf = [0u8; 8192];
                    let n = sock.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first = req.lines().next().unwrap_or("");
                    let (method, path) = {
                        let mut it = first.split_whitespace();
                        (it.next().unwrap_or(""), it.next().unwrap_or(""))
                    };
                    all.lock().unwrap().push(format!("{method} {path}"));
                    let (code, body): (u16, String) = if method == "GET" && path.contains("/inbox") {
                        (200, inbox.clone())
                    } else if path.contains("/status") {
                        c.lock().unwrap().push(path.to_string());
                        if r.load(Ordering::SeqCst) {
                            (409, json!({"error":"withdrawn","state":"accepted"}).to_string())
                        } else {
                            (200, json!({"ok":true,"state":"accepted"}).to_string())
                        }
                    } else if path.contains("/exec-consent") {
                        (200, json!({"ok":true,"exec_opt_in_at":null}).to_string())
                    } else if path.contains("/link/start") {
                        (200, json!({"user_code":"ABCD-2345","poll_token":"pt_test","expires_in_s":600,"interval_s":5}).to_string())
                    } else if path.contains("/link/poll") {
                        (200, json!({"actor_token":"tkact_claimed","device_label":"Test Mac","expires_at":null}).to_string())
                    } else {
                        (404, json!({"error":"not_found"}).to_string())
                    };
                    let reason = if code == 200 { "OK" } else if code == 409 { "Conflict" } else { "Error" };
                    let resp = format!("HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = sock.write_all(resp.as_bytes());
                    let _ = sock.flush();
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }
    });
    Mock { endpoint: format!("http://127.0.0.1:{port}"), stop, reject_status, status_calls, requests }
}

fn temp_memory() -> (memory::MemoryService, PathBuf) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!("icu-fleet-svc-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&root);
    (memory::MemoryService::open(root.clone()), root)
}

/// An opted-in, linked Service pointed at the mock, with the fixture offer ingested.
fn ready_service(endpoint: &str) -> Service {
    let service = Service::default();
    service.with_inner(|inner| {
        inner.opted_in = true;
        inner.link = Some(Link { endpoint: endpoint.into(), actor_token: "tkact_test".into(), device_label: "Test Mac".into(), linked_at: memory::now(), expires_at: None });
    });
    let agent = http_agent();
    let inbox = poll_inbox(&agent, endpoint, "tkact_test").unwrap();
    assert_eq!(inbox.poll_after_s, 30);
    let declines = service.with_inner(|inner| ingest(inner, inbox.items, memory::now()));
    assert!(declines.is_empty(), "the v1 fixture offer is not declined");
    service
}

#[test]
fn polls_and_ingests_the_fixture_offer() {
    let mock = start_mock();
    let service = ready_service(&mock.endpoint);
    let status = service.status();
    assert_eq!(status.offers.len(), 1);
    let offer = &status.offers[0];
    assert_eq!(offer.dispatch_id, DISPATCH_ID);
    assert_eq!(offer.title, "Export the weekly support report");
    assert_eq!(offer.issuer_label, "ops@acme.example");
    assert_eq!(offer.app_hint, "Google Chrome");
    assert_eq!(offer.steps_preview.len(), 3);
    assert_eq!(offer.state, "delivered");
    assert!(offer.deadline_unix_s > memory::now());
}

#[test]
fn stage_from_offer_builds_a_draft_and_starts_no_run() { // I1
    let mock = start_mock();
    let service = ready_service(&mock.endpoint);
    let (memory, _root) = temp_memory();
    let runtime = Runtime::default();
    // A RunManager exists but the fleet path must never touch it.
    let runs = crate::run_control::RunManager::default();
    assert!(matches!(runs.status(), Ok(None)));
    let generated = service.stage(&memory, &runtime, DISPATCH_ID, "com.google.Chrome".into(), "Google Chrome".into()).unwrap();
    assert_eq!(generated.source.format, "fleet-dispatch/v1");
    assert_eq!(generated.plan.steps.len(), 3);
    // Staging produced a reviewable draft; nothing is running.
    assert!(matches!(runs.status(), Ok(None)), "I1: staging an offer must not start a run");
    assert_eq!(runtime.draft_dispatch_id(&generated.request_id).as_deref(), Some(DISPATCH_ID));
}

#[test]
fn stage_refuses_when_opt_in_off_or_offer_gone() { // I2
    let mock = start_mock();
    let (memory, _root) = temp_memory();
    let runtime = Runtime::default();

    // opt-in off
    let service = ready_service(&mock.endpoint);
    service.with_inner(|i| i.opted_in = false);
    assert!(service.stage(&memory, &runtime, DISPATCH_ID, "com.google.Chrome".into(), "Google Chrome".into()).is_err());

    // locally expired
    let service = ready_service(&mock.endpoint);
    service.with_inner(|i| i.offers.get_mut(DISPATCH_ID).unwrap().deadline_unix_s = memory::now() - 1);
    assert!(service.stage(&memory, &runtime, DISPATCH_ID, "com.google.Chrome".into(), "Google Chrome".into()).is_err());

    // withdrawn locally
    let service = ready_service(&mock.endpoint);
    service.with_inner(|i| i.offers.get_mut(DISPATCH_ID).unwrap().state = "withdrawn".into());
    assert!(service.stage(&memory, &runtime, DISPATCH_ID, "com.google.Chrome".into(), "Google Chrome".into()).is_err());

    // unknown dispatch id
    let service = ready_service(&mock.endpoint);
    assert!(service.stage(&memory, &runtime, "dp-does-not-exist", "com.google.Chrome".into(), "Google Chrome".into()).is_err());
}

#[test]
fn confirm_accept_is_a_live_check() { // I3
    let mock = start_mock();
    let service = ready_service(&mock.endpoint);

    // Offer is live: the server accepts.
    assert!(service.confirm_accept(DISPATCH_ID).is_ok());
    assert_eq!(service.with_inner(|i| i.offers.get(DISPATCH_ID).map(|o| o.state.clone())), Some("accepted".into()));

    // Offer withdrawn server-side: a 409 blocks the save and drops the offer locally.
    let service = ready_service(&mock.endpoint);
    mock.reject_status.store(true, Ordering::SeqCst);
    assert!(service.confirm_accept(DISPATCH_ID).is_err());
    assert!(service.with_inner(|i| i.offers.get(DISPATCH_ID).is_none()), "a withdrawn offer is discarded");
    // The app did POST accepted (the live check actually hit the server).
    assert!(mock.status_calls.lock().unwrap().iter().any(|p| p.contains(DISPATCH_ID)));
}

#[test]
fn unknown_version_offer_is_declined_not_surfaced() { // I9 at the fleet layer
    let service = Service::default();
    let item = InboxItem {
        dispatch_id: "dp-v2".into(),
        spec_json: r#"{"v":2,"dispatch_id":"dp-v2"}"#.into(),
        spec_sha256: sha256_hex(br#"{"v":2,"dispatch_id":"dp-v2"}"#),
        expires_in_s: 1000,
    };
    let declines = service.with_inner(|inner| ingest(inner, vec![item], memory::now()));
    assert_eq!(declines.len(), 1);
    assert_eq!(declines[0].0, "dp-v2");
    assert_eq!(declines[0].1, StatusReport::new("declined", Some("unsupported_version")));
    assert_eq!(service.status().offers.len(), 0, "a v2 offer is never shown to the user");
}

#[test]
fn digest_mismatch_is_dropped() {
    let service = Service::default();
    let item = InboxItem {
        dispatch_id: "dp-tampered".into(),
        spec_json: r#"{"v":1,"dispatch_id":"dp-tampered"}"#.into(),
        spec_sha256: "0".repeat(64), // wrong digest
        expires_in_s: 1000,
    };
    let declines = service.with_inner(|inner| ingest(inner, vec![item], memory::now()));
    assert!(declines.is_empty());
    assert_eq!(service.status().offers.len(), 0);
}

#[test]
fn agent_kinds_map_to_dispatch_statuses() { // B3
    assert_eq!(terminal_status("done"), Some(("finished", Some("reported_done"))));
    assert_eq!(terminal_status("fail"), Some(("failed", Some("agent_failed"))));
    assert_eq!(terminal_status("error"), Some(("failed", Some("agent_error"))));
    assert_eq!(terminal_status("stopped"), Some(("stopped", Some("stopped_by_user"))));
    assert_eq!(terminal_status("think"), None);
    assert_eq!(terminal_status("observe"), None);
    assert_eq!(terminal_status("start"), None);
}

#[test]
fn https_is_required_except_loopback() {
    // Allowed: https to any host; cleartext http ONLY to a loopback literal.
    assert!(https_ok("https://fleet.example.com"));
    assert!(https_ok("https://fleet.example.com/path?q=1"));
    assert!(https_ok("http://127.0.0.1:8080"));
    assert!(https_ok("http://localhost:8080"));
    assert!(https_ok("http://[::1]:7077"), "::1 ipv6 loopback");
    // Rejected cleartext to non-loopback.
    assert!(!https_ok("http://fleet.example.com"));
    assert!(!https_ok("ftp://fleet.example.com"));
    assert!(!https_ok("https://"));
    // HIGH-A: prefix/substring bypasses the old check accepted — the real host is evil.com.
    assert!(!https_ok("http://127.0.0.1.evil.com"), "prefix trick: host is evil.com");
    assert!(!https_ok("http://127.0.0.1.evil.com/x"));
    assert!(!https_ok("http://127.0.0.1@evil.com"), "userinfo trick: host is evil.com");
    assert!(!https_ok("http://user@evil.com"));
    assert!(!https_ok("http://localhost.evil.com"), "localhost is a label, not the host");
    assert!(!https_ok("http://127.0.0.2:8080"), "only the 127.0.0.1 literal, not all of 127/8");
    assert!(!https_ok("not a url"));
}

#[test]
fn worker_pins_linked_endpoint_ignoring_later_changes() { // HIGH-B
    let mock_a = start_mock();
    let mock_b = start_mock(); // the redirect target: must NEVER receive the tkact_ token
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!("icu-fleet-pin-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&root);
    let service = Service::default();
    service.with_inner(|i| {
        i.root = Some(root.clone());
        i.opted_in = true;
        // The token was minted against A at link time.
        i.link = Some(Link { endpoint: mock_a.endpoint.clone(), actor_token: "tkact_pinned".into(), device_label: "A".into(), linked_at: memory::now(), expires_at: None });
    });
    // A later endpoint change points the configured endpoint at B.
    service.with_inner(|i| i.endpoint = Some(mock_b.endpoint.clone()));
    // The worker stays pinned to A; resolve_endpoint WOULD redirect to B (that is the trap).
    assert_eq!(service.with_inner(|i| i.worker_endpoint()), Some(mock_a.endpoint.clone()));
    assert_eq!(service.with_inner(|i| i.resolve_endpoint()), Some(mock_b.endpoint.clone()));
    // The authenticated poll targets the pinned A; B is never contacted.
    let endpoint = service.with_inner(|i| i.worker_endpoint()).unwrap();
    let token = service.with_inner(|i| i.link.as_ref().unwrap().actor_token.clone());
    let _ = poll_inbox(&http_agent(), &endpoint, &token);
    std::thread::sleep(Duration::from_millis(50));
    assert!(mock_a.requests.lock().unwrap().iter().any(|r| r.contains("/inbox")), "A (the linked endpoint) must receive the poll");
    assert!(mock_b.requests.lock().unwrap().is_empty(), "B must NEVER be contacted with the token after an endpoint change while linked");
    let _ = std::fs::remove_dir_all(&root);
}

// ── B5: lifecycle through the production command registry (ipc(webview, ...)). ──
// offer -> stage -> save -> Send; nothing runs before Send; a withdrawn offer cannot
// be saved. Uses MockRuntime and a scripted task body (no real cua driver).
mod lifecycle {
    use super::*;
    use tauri::Manager;

    type Mock = tauri::test::MockRuntime;

    fn ipc(webview: &tauri::WebviewWindow<Mock>, cmd: &str, body: serde_json::Value) -> Result<serde_json::Value, serde_json::Value> {
        tauri::test::get_ipc_response(webview, tauri::webview::InvokeRequest {
            cmd: cmd.into(), callback: tauri::ipc::CallbackFn(0), error: tauri::ipc::CallbackFn(1),
            // Webview origin is the custom scheme on macOS/Linux but http on Windows; using the
            // wrong one fails the window-scoped capability ACL ("not allowed") for every command.
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(), body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(), invoke_key: tauri::test::INVOKE_KEY.into(),
        }).map(|r| r.deserialize::<serde_json::Value>().unwrap())
    }

    fn build() -> (tauri::App<Mock>, tauri::WebviewWindow<Mock>, Arc<AtomicBool>) {
        let ran = Arc::new(AtomicBool::new(false));
        let ran2 = ran.clone();
        // The only run trigger: agent_run spawns this. A dispatch that "ran" flips the flag.
        let runner: crate::TaskBody<Mock> = Arc::new(move |_app, _choice, lease| { ran2.store(true, Ordering::SeqCst); lease.finish(); });
        let app = crate::configure_commands_with(tauri::test::mock_builder(), runner)
            .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        // Isolate each test's fleet dir. The mock app_local_data_dir is a FIXED path shared by
        // every build()-based test, so otherwise one test's persisted flag.json (e.g. an opt-out
        // writing optedIn:false) gets loaded by the next test's ensure_root and clobbers its
        // seeded state. A unique empty root makes ensure_root a no-op and the tests order-independent.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!("icu-fleet-life-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&root);
        app.state::<Service>().with_inner(|i| i.root = Some(root));
        (app, webview, ran)
    }

    fn seed_offer(app: &tauri::App<Mock>, endpoint: &str) {
        let inbox: Value = serde_json::from_str(&fixture("inbox_response.json")).unwrap();
        let spec_json = inbox["items"][0]["spec_json"].as_str().unwrap().to_string();
        let item = InboxItem { dispatch_id: DISPATCH_ID.into(), spec_sha256: sha256_hex(spec_json.as_bytes()), spec_json, expires_in_s: 1000 };
        let service = app.state::<Service>();
        service.with_inner(|inner| {
            inner.opted_in = true;
            inner.link = Some(Link { endpoint: endpoint.into(), actor_token: "tkact_test".into(), device_label: "Test".into(), linked_at: memory::now(), expires_at: None });
            ingest(inner, vec![item], memory::now());
        });
    }

    fn running(app: &tauri::App<Mock>) -> bool {
        matches!(app.state::<crate::run_control::RunManager>().status(), Ok(Some(_)))
    }

    #[test]
    fn offer_stage_save_then_send_is_the_only_run_trigger() {
        let mock = start_mock(); // status POSTs answer 200 accepted
        let (app, webview, ran) = build();
        seed_offer(&app, &mock.endpoint);
        // Linux admission resolves /proc identity for a real pid; use this test process.
        let pid = std::process::id() as i64;

        // Stage from the offer. Nothing runs.
        let staged = ipc(&webview, "fleet_stage", json!({"dispatchId": DISPATCH_ID, "pid": pid, "appName": "Test"}))
            .expect("fleet_stage should build a draft");
        assert_eq!(staged["source"]["format"], "fleet-dispatch/v1");
        assert!(!running(&app) && !ran.load(Ordering::SeqCst), "staging an offer must not run anything");
        let request_id = staged["request_id"].as_str().unwrap().to_string();

        // Save (approved). The command posts the live `accepted` first (mock 200), then saves.
        let saved = ipc(&webview, "workflow_save", json!({"request": {
            "request_id": request_id, "expected_proposal_sha256": staged["proposal_sha256"], "plan": staged["plan"], "approved": true,
        }})).expect("a live-accepted dispatch draft should save");
        assert_eq!(saved["workflow"]["source"]["format"], "fleet-dispatch/v1");
        assert!(!running(&app) && !ran.load(Ordering::SeqCst), "saving must not run anything");
        let (id, sha) = (saved["workflow"]["id"].as_str().unwrap().to_string(), saved["sha256"].as_str().unwrap().to_string());

        // Use -> Send: agent_run is the ONLY thing that starts the task.
        ipc(&webview, "agent_run", json!({"goal": "run the saved routine", "pid": pid, "windowId": 1, "appName": "Test",
            "workflow": {"id": id, "expected_sha256": sha, "values": {}, "approved": true}})).expect("Send should start the run");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !ran.load(Ordering::SeqCst) && std::time::Instant::now() < deadline { std::thread::sleep(Duration::from_millis(10)); }
        assert!(ran.load(Ordering::SeqCst), "Send (agent_run) must be what finally runs the routine");
    }

    #[test]
    fn a_withdrawn_offer_cannot_be_saved() {
        let mock = start_mock();
        mock.reject_status.store(true, Ordering::SeqCst); // the server withdrew it: 409 on accept
        let (app, webview, ran) = build();
        seed_offer(&app, &mock.endpoint);
        let pid = std::process::id() as i64;

        let staged = ipc(&webview, "fleet_stage", json!({"dispatchId": DISPATCH_ID, "pid": pid, "appName": "Test"}))
            .expect("staging is local and still succeeds");
        let request_id = staged["request_id"].as_str().unwrap().to_string();

        // The live accept check fails -> the save is refused and the draft discarded.
        let result = ipc(&webview, "workflow_save", json!({"request": {
            "request_id": request_id, "expected_proposal_sha256": staged["proposal_sha256"], "plan": staged["plan"], "approved": true,
        }}));
        assert!(result.is_err(), "a withdrawn offer must not save");
        assert!(!running(&app) && !ran.load(Ordering::SeqCst), "a refused save runs nothing");
        // The draft was discarded, so a second save attempt cannot succeed either.
        let again = ipc(&webview, "workflow_save", json!({"request": {
            "request_id": staged["request_id"], "expected_proposal_sha256": staged["proposal_sha256"], "plan": staged["plan"], "approved": true,
        }}));
        assert!(again.is_err(), "the discarded draft is gone");
    }

    #[test]
    fn exec_consent_posts_only_to_the_linked_endpoint() { // HIGH-B: exec-consent (opt-in-while-linked + opt-out)
        let mock_a = start_mock();
        let mock_b = start_mock(); // the redirect target: must receive NOTHING
        let (app, webview, ran) = build();
        // Linked to A (the token was minted against A).
        app.state::<Service>().with_inner(|i| {
            i.link = Some(Link { endpoint: mock_a.endpoint.clone(), actor_token: "tkact_pinned".into(), device_label: "A".into(), linked_at: memory::now(), expires_at: None });
        });
        // A later endpoint change points the configured (stored) endpoint at B.
        app.state::<Service>().with_inner(|i| i.endpoint = Some(mock_b.endpoint.clone()));
        // Both exec-consent toggles carry the tkact_ bearer and must target the pinned A,
        // never the reconfigured B.
        ipc(&webview, "fleet_set_exec_optin", json!({"enabled": true})).expect("opt-in");
        ipc(&webview, "fleet_set_exec_optin", json!({"enabled": false})).expect("opt-out");
        std::thread::sleep(Duration::from_millis(100));
        let a = mock_a.requests.lock().unwrap().clone();
        let b = mock_b.requests.lock().unwrap().clone();
        assert!(a.iter().any(|r| r.starts_with("PUT") && r.contains("/exec-consent")), "exec-consent must PUT to the linked endpoint A: {a:?}");
        assert!(b.is_empty(), "B must NEVER be contacted with the token after an endpoint change: {b:?}");
        assert!(!running(&app) && !ran.load(Ordering::SeqCst), "toggling opt-in must not run anything");
    }
}
