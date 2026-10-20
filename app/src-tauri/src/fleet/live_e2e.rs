//! GATED live device-side end-to-end harness. No-ops unless ICU_FLEET_ENDPOINT is set,
//! so a normal `cargo test` is unaffected. When set, it drives the REAL fleet.rs client
//! (the same poll/link/stage/status functions production uses, not reimplemented HTTP)
//! through offer -> review -> save -> status against a live server, printing STEP markers
//! so the run transcript IS the proof artifact. It STOPS before any real cua run: firing
//! the routine is the consent-gated Send, and there is no signed driver here (B5 already
//! proves only Send flips the run flag).
//!
//! NB: this is a plain `#[test]`, not `#[tokio::test]` — the fleet client is blocking
//! `ureq` and the crate has no tokio dev-dependency, so an async runtime would neither
//! compile nor fit. The harness drives the real synchronous client directly.
use super::*;
use crate::memory;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Manager;

type Mock = tauri::test::MockRuntime;

fn ipc(webview: &tauri::WebviewWindow<Mock>, cmd: &str, body: Value) -> Result<Value, Value> {
    tauri::test::get_ipc_response(webview, tauri::webview::InvokeRequest {
        cmd: cmd.into(), callback: tauri::ipc::CallbackFn(0), error: tauri::ipc::CallbackFn(1),
        // Webview origin is the custom scheme on macOS/Linux but http on Windows; using the
        // wrong one fails the window-scoped capability ACL ("not allowed") for every command.
        url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(), body: tauri::ipc::InvokeBody::Json(body),
        headers: Default::default(), invoke_key: tauri::test::INVOKE_KEY.into(),
    }).map(|r| r.deserialize::<Value>().unwrap())
}

fn env(key: &str) -> Option<String> { std::env::var(key).ok().filter(|v| !v.trim().is_empty()) }

#[test]
fn live_device_roundtrip() {
    let Some(endpoint) = env("ICU_FLEET_ENDPOINT") else {
        eprintln!("SKIP live_device_roundtrip: set ICU_FLEET_ENDPOINT to run it against a live server");
        return;
    };
    let app_name = env("ICU_FLEET_APP_NAME").unwrap_or_else(|| "Test".into());
    // The production command registry, with a no-op task body (we never reach Send).
    let runner: crate::TaskBody<Mock> = Arc::new(|_app, _choice, lease| lease.finish());
    let app = crate::configure_commands_with(tauri::test::mock_builder(), runner)
        .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let service = app.state::<Service>();
    let agent = http_agent();
    let running = || matches!(app.state::<crate::run_control::RunManager>().status(), Ok(Some(_)));

    // Resolve the fleet dir first (ensure_root) so later state loads/persists coherently.
    let _ = ipc(&webview, "fleet_status", json!({}));

    // A pre-provisioned actor token lets the operator skip the pairing handshake.
    let provided_token = env("ICU_FLEET_ACTOR_TOKEN");
    if let Some(token) = &provided_token {
        service.with_inner(|inner| {
            inner.link = Some(Link {
                endpoint: endpoint.clone(), actor_token: token.clone(),
                device_label: env("ICU_FLEET_DEVICE_LABEL").unwrap_or_else(|| "device".into()),
                linked_at: memory::now(), expires_at: None,
            });
            inner.poll_token = None; inner.link_code = None;
        });
    }

    // STEP 1 — opt in (endpoint from ICU_FLEET_ENDPOINT). If already linked (token mode)
    // this posts exec-consent; otherwise it does the real link/start.
    let status = ipc(&webview, "fleet_set_exec_optin", json!({"enabled": true, "endpoint": endpoint}))
        .unwrap_or_else(|e| panic!("STEP 1 fleet_set_exec_optin failed: {e}"));
    println!("STEP 1 opt-in ON  endpoint={endpoint}");
    println!("  status: optedIn={} endpointConfigured={} linked={} linkCode={}",
        status["optedIn"], status["endpointConfigured"], status["linked"], status["linkCode"]);
    // opt-in starts the background worker. Stop it so the harness drives link/poll
    // deterministically (every 2s) and the one-time pairing claim can't race the worker.
    // The operator cannot approve before STEP 2 relays the code, so no claim is lost.
    service.shutdown();

    // STEP 2 — actor link. Print what the operator needs to approve + relay a dispatch to.
    if let Some(token) = &provided_token {
        let shown: String = token.chars().take(10).collect();
        println!("STEP 2 actor link: using provided token {shown}...  DEVICE_ID={}",
            env("ICU_FLEET_DEVICE_ID").unwrap_or_else(|| "(set ICU_FLEET_DEVICE_ID to echo it)".into()));
    } else {
        let user_code = status["linkCode"].as_str().unwrap_or("").to_string();
        println!("STEP 2 actor link: RELAY TO OPERATOR -> USER_CODE={user_code}  DEVICE_ID={}",
            env("ICU_FLEET_DEVICE_ID").unwrap_or_else(|| "(operator picks the device in the dashboard)".into()));
        let poll_token = service.with_inner(|i| i.poll_token.clone())
            .expect("link/start should have set a poll token; is the server reachable?");
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            assert!(Instant::now() < deadline, "the operator did not approve the link within 300s");
            match claim_link(&agent, &endpoint, &poll_token) {
                Ok(Some(link)) => { println!("  linked: device_label={}", link.device_label);
                    service.with_inner(|i| i.link = Some(link)); break; }
                Ok(None) => { println!("  link pending; retrying in 5s"); std::thread::sleep(Duration::from_secs(5)); }
                Err(e) => panic!("STEP 2 link poll error: {e:?}"),
            }
        }
    }
    let token = service.with_inner(|i| i.link.as_ref().expect("linked").actor_token.clone());

    // STEP 2b -- PUT exec-consent=true. The real app posts this in the background worker
    // right after linking; the harness stopped the worker for deterministic pairing, so it
    // sends it here to match the operator-expected sequence (link -> approve -> exec-consent
    // -> offer). Uses the same call the production fleet_set_exec_optin path uses.
    match post_exec_consent(&agent, &endpoint, &service.shared, true) {
        Ok(()) => println!("STEP 2b exec-consent=true PUT OK (server now sees the device opted in)"),
        Err(e) => println!("STEP 2b exec-consent PUT error: {e:?} (continuing; operator may still offer)"),
    }

    // STEP 3 — poll the real inbox until the operator's dispatch arrives.
    println!("STEP 3 polling inbox every 2s for up to 120s (operator creates the dispatch out of band)");
    let deadline = Instant::now() + Duration::from_secs(120);
    let dispatch_id = loop {
        assert!(Instant::now() < deadline, "no offer arrived within 120s");
        match poll_inbox(&agent, &endpoint, &token) {
            Ok(inbox) => {
                let item_count = inbox.items.len();
                let declines = service.with_inner(|i| { i.last_checked = Some(memory::now()); ingest(i, inbox.items, memory::now()) });
                for (id, report) in &declines { let _ = post_status(&agent, &endpoint, &token, id, report);
                    println!("  declined {id} ({:?})", report.result_code); }
                let offers = service.status().offers;
                println!("  poll: {item_count} item(s), {} live offer(s)", offers.len());
                if let Some(offer) = offers.into_iter().next() {
                    println!("  OFFER dispatch_id={} title={:?} issuer={}", offer.dispatch_id, offer.title, offer.issuer_label);
                    break offer.dispatch_id;
                }
            }
            Err(e) => println!("  poll error (retrying): {e:?}"),
        }
        std::thread::sleep(Duration::from_secs(2));
    };

    // STEP 4 — verify the digest over the exact received spec_json (the client did this in
    // ingest; re-check and print for the transcript).
    let (spec_json, expected_sha) = service.with_inner(|i| {
        let offer = i.offers.get(&dispatch_id).expect("ingested offer");
        (offer.spec_json.clone(), offer.spec_sha256.clone())
    });
    let computed = sha256_hex(spec_json.as_bytes());
    println!("STEP 4 digest  computed={computed}  expected={expected_sha}  match={}", computed == expected_sha);
    assert_eq!(computed, expected_sha, "spec_sha256 must verify over the received spec_json");

    // STEP 5 — stage into a draft; assert NO run begins.
    let pid = std::process::id() as i64;
    let staged = ipc(&webview, "fleet_stage", json!({"dispatchId": dispatch_id, "pid": pid, "appName": app_name}))
        .unwrap_or_else(|e| panic!("STEP 5 fleet_stage failed: {e}"));
    let request_id = staged["request_id"].as_str().expect("request_id").to_string();
    println!("STEP 5 staged  request_id={request_id}  running={}", running());
    assert!(!running(), "I1: staging an offer must not begin a run");

    // STEP 6 — the live accepted check against the server.
    crate::fleet::confirm_dispatch_accept(app.handle(), &request_id)
        .unwrap_or_else(|e| panic!("STEP 6 confirm_dispatch_accept refused: {e}"));
    println!("STEP 6 confirm_dispatch_accept OK (server accepted the offer; it is live)");

    // STEP 7 — save the dispatch draft (re-confirms accepted idempotently, then persists).
    let saved = ipc(&webview, "workflow_save", json!({"request": {
        "request_id": request_id, "expected_proposal_sha256": staged["proposal_sha256"],
        "plan": staged["plan"], "approved": true,
    }})).unwrap_or_else(|e| panic!("STEP 7 workflow_save failed: {e}"));
    println!("STEP 7 workflow_save OK  id={} sha256={}", saved["workflow"]["id"], saved["sha256"]);
    assert!(!running(), "saving must not begin a run");

    // STEP 8 — report a status via the real reporter and print the server ack, then STOP.
    // Default "started" (the legal accepted->started transition agent.rs would post at run
    // start). report_status_detached wraps this exact post_status; we call it synchronously
    // to surface the ack. No real cua run follows.
    let state = env("ICU_FLEET_FINAL_STATE").unwrap_or_else(|| "started".into());
    let report = if state == "started" || state == "accepted" { StatusReport::new(&state, None) }
        else { StatusReport::new(&state, env("ICU_FLEET_RESULT_CODE").as_deref()) };
    match post_status(&agent, &endpoint, &token, &dispatch_id, &report) {
        Ok(ack) => println!("STEP 8 status reported state={state} -> server ack: ok={} state={}", ack.ok, ack.state),
        Err(e) => panic!("STEP 8 status report failed: {e:?}"),
    }
    assert!(!running(), "the harness must not have started a run");
    println!("DONE: offer->digest->stage->accept->save->status round-trip verified against {endpoint}.");
    println!("      Harness stops before any real run (Send is the only run trigger; see B5).");
}

/// GATED live invite enrollment (Decision A). No-ops unless ICU_FLEET_ENROLL_URL is set to a
/// real `icu://enroll?invite=...&h=...` minted by the server lane. Feeds that URL through the
/// REAL ingest path (`fleet_enroll`, the same command the Connect screen and deep link call),
/// then proves the device auto-linked with NO device-code handshake: managed.json is written
/// with {endpoint, deviceToken, tenantId}, the Link is pinned to the returned endpoint, and the
/// inbox is reachable with the returned token. STOPS before any run.
///
/// This is the headless trigger for a manual round trip: set ICU_FLEET_ENROLL_URL and run
///   cargo test -p app --lib fleet::live_e2e::live_enroll_roundtrip -- --nocapture
/// On a real desktop the equivalent is `open 'icu://enroll?...'` (macOS) or
/// `xdg-open 'icu://enroll?...'` (Linux), which single-instance forwards to on_open_url.
#[test]
fn live_enroll_roundtrip() {
    let Some(enroll_url) = env("ICU_FLEET_ENROLL_URL") else {
        eprintln!("SKIP live_enroll_roundtrip: set ICU_FLEET_ENROLL_URL to an icu://enroll?invite=...&h=... link to run it");
        return;
    };
    let runner: crate::TaskBody<Mock> = Arc::new(|_app, _choice, lease| lease.finish());
    let app = crate::configure_commands_with(tauri::test::mock_builder(), runner)
        .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let service = app.state::<Service>();
    let running = || matches!(app.state::<crate::run_control::RunManager>().status(), Ok(Some(_)));

    // Resolve the fleet dir first so managed.json / link.json land coherently.
    let _ = ipc(&webview, "fleet_status", json!({}));

    // STEP 1 — feed the invite link through the real ingest path.
    println!("STEP 1 enroll via {enroll_url}");
    let status = ipc(&webview, "fleet_enroll", json!({"input": enroll_url}))
        .unwrap_or_else(|e| panic!("STEP 1 fleet_enroll failed: {e}"));
    println!("  status: optedIn={} linked={} endpointConfigured={} tenantId={} linkCode={}",
        status["optedIn"], status["linked"], status["endpointConfigured"], status["tenantId"], status["linkCode"]);
    assert_eq!(status["optedIn"].as_bool(), Some(true), "redeem must opt the device in");
    assert_eq!(status["linked"].as_bool(), Some(true), "redeem must link directly (Decision A: no device-code)");
    assert!(status["linkCode"].is_null(), "the invite path must NEVER show a pairing code");

    // STEP 2 — managed.json was written atomically with the three keys, and the Link is pinned.
    let (pinned, token, tenant) = service.with_inner(|i| {
        let link = i.link.as_ref().expect("enroll must set the Link");
        (link.endpoint.clone(), link.actor_token.clone(), i.tenant_id.clone())
    });
    let root = service.with_inner(|i| i.root.clone()).expect("fleet dir resolved");
    let managed = skysight::private::read_bounded(&root.join("managed.json"), 8192).expect("managed.json written");
    let managed: Value = serde_json::from_str(&managed).unwrap();
    println!("STEP 2 managed.json endpoint={} tenantId={} deviceToken={}...",
        managed["endpoint"], managed["tenantId"],
        managed["deviceToken"].as_str().unwrap_or("").chars().take(10).collect::<String>());
    assert_eq!(managed["endpoint"].as_str(), Some(pinned.as_str()), "managed.json endpoint == pinned link endpoint");
    assert_eq!(managed["tenantId"].as_str(), tenant.as_deref(), "managed.json tenantId == stored tenant");
    assert!(managed["deviceToken"].as_str().is_some_and(|t| !t.is_empty()), "managed.json carries the device token");
    assert_eq!(service.with_inner(|i| i.worker_endpoint()).as_deref(), Some(pinned.as_str()),
        "worker_endpoint() pins to the returned endpoint");

    // STEP 3 — the returned token reaches the real inbox (device is live for the admin to offer to).
    let agent = http_agent();
    match poll_inbox(&agent, &pinned, &token) {
        Ok(inbox) => println!("STEP 3 inbox reachable with the enrolled token: {} item(s), poll_after_s={}", inbox.items.len(), inbox.poll_after_s),
        Err(e) => panic!("STEP 3 inbox unreachable with the enrolled token: {e:?}"),
    }

    assert!(!running(), "enrollment must not start a run");
    println!("DONE: invite -> redeem -> managed.json -> pinned link -> live inbox, no device-code, no run.");
}
