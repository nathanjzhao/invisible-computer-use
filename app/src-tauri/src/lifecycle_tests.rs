// App task lifecycle through the production command registry.
// MockRuntime supplies the webview. The task body, admission, cua client and
// owned driver lifetime are production code; only the model is scripted and the
// driver is the socket fixture. Children are re-executed per scenario because
// driver shutdown is process-global.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn fixture_with_metadata() -> (CaptureFile, String, String) {
    warm_python_shim();
    let directory = CaptureFile::new().unwrap();
    let binary = directory.0.join("lifetime-driver");
    std::fs::write(&binary, include_str!("../../tests/fixtures/fake_lifetime_driver.py")).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.0.join("lifetime.sock");
    use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
    std::fs::write(directory.0.join("metadata.json"), json!({
        "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
        "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
        "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
    }).to_string()).unwrap();
    (directory, binary.to_string_lossy().into(), socket.to_string_lossy().into())
}

fn fixture_rows(directory: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(directory.join("requests.jsonl")).unwrap_or_default()
        .lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

const INPUT_TOOLS: [&str; 8] = ["click", "type_text", "press_key", "hotkey", "scroll", "drag", "set_value", "double_click"];
fn is_input(row: &Value) -> bool { INPUT_TOOLS.contains(&row["name"].as_str().unwrap_or("")) }

fn process_gone(pid: i64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        // A zombie still answers kill(0); ESRCH means it was also reaped.
        let gone = unsafe { libc::kill(pid as i32, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if gone || Instant::now() >= deadline { return gone; }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The scripted model. Its first request waits (model/reload phases) until the
/// test releases it and then asks for two inputs; later requests fail the task.
static MODEL_GATE: (Mutex<(bool, bool)>, std::sync::Condvar) = (Mutex::new((false, false)), std::sync::Condvar::new());
struct LifecycleBrain;
impl crate::brain::Brain for LifecycleBrain {
    fn name(&self) -> String { "lifecycle-fixture".into() }
    fn decide(&self, _: &str, _memory: Option<&str>, _: &str, _: &[String]) -> Result<crate::brain::Decision, String> {
        use crate::brain::{Action, Decision};
        let phase = std::env::var("ICU_LIFECYCLE_CHILD").unwrap_or_default();
        let (lock, wake) = &MODEL_GATE;
        let mut gate = lock.lock().unwrap();
        if gate.0 {
            return Ok(Decision { thought: String::new(), action: None, actions: vec![Action::Fail { reason: "fixture finished".into() }], fallback: None });
        }
        gate.0 = true;
        wake.notify_all();
        if matches!(phase.as_str(), "model" | "reload") {
            while !gate.1 { gate = wake.wait(gate).unwrap(); }
        }
        Ok(Decision { thought: String::new(), action: None, actions: vec![
            Action::Type { text: "one".into() }, Action::Type { text: "two".into() },
            Action::Done { summary: "must not be reported".into() },
        ], fallback: None })
    }
}
fn release_model() {
    let (lock, wake) = &MODEL_GATE;
    lock.lock().unwrap().1 = true;
    wake.notify_all();
}
fn model_entered() -> bool { MODEL_GATE.0.lock().unwrap().0 }
fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

type Mock = tauri::test::MockRuntime;
fn lifecycle_app() -> (tauri::App<Mock>, std::sync::mpsc::Receiver<Value>) {
    lifecycle_app_with(|| Box::new(LifecycleBrain))
}
fn lifecycle_app_with(make: fn() -> Box<dyn crate::brain::Brain>) -> (tauri::App<Mock>, std::sync::mpsc::Receiver<Value>) {
    use tauri::Listener;
    let runner: crate::TaskBody<Mock> = std::sync::Arc::new(move |app, _choice, lease| {
        crate::agent::run_with_brain(app, lease, make)
    });
    let app = crate::configure_commands_with(tauri::test::mock_builder(), runner)
        .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    app.listen_any("agent", move |event| { let _ = tx.send(serde_json::from_str::<Value>(event.payload()).unwrap()); });
    (app, rx)
}
fn ipc(webview: &tauri::WebviewWindow<Mock>, cmd: &str, body: Value) -> Result<Value, Value> {
    tauri::test::get_ipc_response(webview, tauri::webview::InvokeRequest {
        cmd: cmd.into(), callback: tauri::ipc::CallbackFn(0), error: tauri::ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(), body: tauri::ipc::InvokeBody::Json(body),
        headers: Default::default(), invoke_key: tauri::test::INVOKE_KEY.into(),
    }).map(|response| response.deserialize::<Value>().unwrap())
}
fn start(webview: &tauri::WebviewWindow<Mock>, run_id: &str) -> Result<Value, Value> {
    // Linux task admission resolves /proc identity even for a scripted driver.
    // Use the live test executable; PID 42 need not exist or belong to the test.
    #[cfg(target_os = "linux")]
    let pid = std::process::id();
    #[cfg(not(target_os = "linux"))]
    let pid = 42;
    ipc(webview, "agent_run", json!({"goal":"type one then two","pid":pid,"windowId":7,"appName":"fixture","brain":"local","runId":run_id}))
}
/// Every event until the run's terminal event, which must arrive promptly.
fn until_terminal(events: &std::sync::mpsc::Receiver<Value>, seen: &mut Vec<Value>, run_id: &str) -> Value {
    loop {
        let event = events.recv_timeout(Duration::from_secs(25)).unwrap_or_else(|_| panic!("no terminal event for {run_id}: {seen:?}"));
        seen.push(event.clone());
        if event["runId"] == run_id && matches!(event["kind"].as_str(), Some("done" | "fail" | "error" | "stopped")) { return event; }
    }
}
fn child_directory() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ICU_CUA_BIN").unwrap()).parent().unwrap().to_path_buf()
}
fn lifecycle_child(test: &str, phase: &str) -> (CaptureFile, Vec<Value>, String) {
    let (directory, binary, socket) = fixture_with_metadata();
    if phase == "observe" { std::fs::write(directory.0.join("observe-gate"), "").unwrap(); }
    if phase == "action" { std::fs::write(directory.0.join("input-gate"), "").unwrap(); }
    if phase == "refusal" {
        std::fs::write(directory.0.join("type-refusal.json"), json!({"isError":true,
            "content":[{"type":"text","text":"refused (type_text_target_not_focused): no element of window 7 has keyboard focus; click the field first"}],
            "structuredContent":{"code":"type_text_target_not_focused","window_id":7,"effect":"none","delivered_chars":0}}).to_string()).unwrap();
    }
    let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
        .args([format!("cua::lifecycle_tests::{test}").as_str(), "--exact", "--nocapture"])
        .env("ICU_LIFECYCLE_CHILD", phase).env("ICU_CUA_BIN", &binary)
        .env("ICU_TEST_OWN_SOCKET", &socket).env("ICU_OWN_DRIVER", "1")
        .env_remove("ICU_CUA_SOCKET"), Duration::from_secs(60)).unwrap();
    let output = format!("{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    assert!(result.status.success(), "{test}/{phase}: {output}\nrequests: {:?}", fixture_rows(&directory.0));
    let rows = fixture_rows(&directory.0);
    (directory, rows, output)
}
fn summary(output: &str) -> &str {
    output.lines().find(|l| l.starts_with("LIFECYCLE")).unwrap_or_else(|| panic!("no summary: {output}"))
}

#[test]
fn exit_during_each_task_phase_stops_input_and_the_owned_driver() {
    if let Ok(phase) = std::env::var("ICU_LIFECYCLE_CHILD") {
        let directory = child_directory();
        let (app, events) = lifecycle_app();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let run_id = format!("exit-{phase}");
        assert_eq!(start(&webview, &run_id), Ok(json!(run_id)));
        match phase.as_str() {
            // The daemon has received the observation and holds its reply.
            "observe" => wait_until("observation in flight", || fixture_rows(&directory)
                .iter().any(|r| r["name"] == "get_window_state")),
            "model" => wait_until("model request", model_entered),
            "action" => wait_until("input in flight", || fixture_rows(&directory).iter().any(|r| r["name"] == "type_text")),
            other => panic!("unknown phase {other}"),
        }
        let driver = PROCESS.get().unwrap().lock().unwrap().child.as_ref().expect("owned driver").id() as i64;
        let before = fixture_rows(&directory).len();
        // The production RunEvent::Exit handler.
        let exited = Instant::now();
        crate::close_for_exit(app.handle());
        assert!(PROCESS.get().unwrap().lock().unwrap().child.is_none(), "owned driver handle retained");
        assert!(process_gone(driver), "owned driver {driver} survived exit");
        if phase == "observe" {
            assert!(!directory.join("cli-observe.pid").exists(), "direct observation must not fork a CLI child");
        }
        release_model();
        let mut seen = Vec::new();
        let terminal = until_terminal(&events, &mut seen, &run_id);
        let settled = exited.elapsed();
        assert!(settled < Duration::from_secs(5), "{phase}: task ended {settled:?} after exit");
        assert_eq!(terminal["kind"], "stopped", "{phase}: {seen:?}");
        assert!(!seen.iter().any(|e| e["kind"] == "done"), "{phase}: completion reported after exit: {seen:?}");
        std::thread::sleep(Duration::from_millis(300));
        let late: Vec<_> = fixture_rows(&directory).into_iter().skip(before).collect();
        assert!(late.is_empty(), "{phase}: driver contacted after exit: {late:?}");
        assert_eq!(ipc(&webview, "agent_status", json!({})), Ok(Value::Null));
        assert_eq!(start(&webview, "after-exit"), Err(json!("the app is shutting down")));
        assert_eq!(ipc(&webview, "cua_click", json!({"pid":42,"windowId":7,"x":1.0,"y":1.0})), Err(json!("the app is shutting down")));
        println!("LIFECYCLE {}", json!({"phase":phase,"driver":driver,"model_called":model_entered(),"terminal_after_exit_ms":settled.as_millis() as u64,"events":seen}));
        return;
    }
    for phase in ["observe", "model", "action"] {
        let (directory, rows, output) = lifecycle_child("exit_during_each_task_phase_stops_input_and_the_owned_driver", phase);
        let typed: Vec<_> = rows.iter().filter(|r| r["name"] == "type_text").collect();
        match phase {
            // Exit before any decision: no model output reaches the driver.
            "observe" | "model" => assert!(!rows.iter().any(is_input), "{phase}: {rows:?}"),
            // The admitted input may be lost with the driver; the next one never starts.
            _ => {
                assert_eq!(typed.len(), 1, "{rows:?}");
                assert_eq!(typed[0]["args"]["text"], "one");
                assert_eq!(rows.iter().filter(|r| is_input(r)).count(), 1, "{rows:?}");
            }
        }
        if phase == "observe" { assert!(summary(&output).contains("\"model_called\":false"), "{output}"); }
        assert!(!std::fs::read_to_string(directory.0.join("received-text.txt")).unwrap_or_default().contains("two"));
        eprintln!("{}", summary(&output));
    }
}

#[test]
fn reloaded_renderer_adopts_the_backend_run_and_its_terminal_event() {
    if std::env::var_os("ICU_LIFECYCLE_CHILD").is_some() {
        let directory = child_directory();
        let (app, events) = lifecycle_app();
        let first = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        assert_eq!(start(&first, "run-a"), Ok(json!("run-a")));
        wait_until("model request", model_entered);
        // A reload discards all renderer state. The replacement knows no run ID.
        first.destroy().unwrap();
        let reloaded = tauri::WebviewWindowBuilder::new(&app, "reloaded", Default::default()).build().unwrap();
        let status = ipc(&reloaded, "agent_status", json!({})).unwrap();
        assert_eq!((status["runId"].clone(), status["stopping"].clone()), (json!("run-a"), json!(false)), "{status}");
        assert_eq!(start(&reloaded, "run-b"), Err(json!("a task is already running; stop it before starting another")));
        assert_eq!(ipc(&reloaded, "cua_click", json!({"pid":42,"windowId":7,"x":1.0,"y":1.0})),
            Err(json!("stop the running task before sending manual input")));
        assert_eq!(ipc(&reloaded, "agent_stop", json!({"runId":"run-b"})), Ok(json!(false)));
        assert_eq!(ipc(&reloaded, "agent_stop", json!({"runId":"run-a"})), Ok(json!(true)));
        assert_eq!(ipc(&reloaded, "agent_status", json!({})).unwrap()["stopping"], true);
        release_model();
        let mut seen = Vec::new();
        let terminal = until_terminal(&events, &mut seen, "run-a");
        assert_eq!(terminal["kind"], "stopped", "{seen:?}");
        // Admission is released before the terminal event is published.
        assert_eq!(ipc(&reloaded, "agent_status", json!({})), Ok(Value::Null));
        assert!(!fixture_rows(&directory).iter().any(is_input), "stopped run delivered input after reload");
        assert_eq!(start(&reloaded, "run-c"), Ok(json!("run-c")));
        let terminal = until_terminal(&events, &mut seen, "run-c");
        assert_eq!(terminal["kind"], "fail", "{seen:?}");
        let first_c = seen.iter().position(|e| e["runId"] == "run-c").unwrap();
        assert!(seen[..first_c].iter().all(|e| e["runId"] == "run-a") && seen[first_c..].iter().all(|e| e["runId"] == "run-c"), "{seen:?}");
        crate::close_for_exit(app.handle());
        println!("LIFECYCLE {}", json!({"phase":"reload","events":seen}));
        return;
    }
    let (_directory, rows, output) = lifecycle_child("reloaded_renderer_adopts_the_backend_run_and_its_terminal_event", "reload");
    assert!(!rows.iter().any(is_input), "{rows:?}");
    eprintln!("{}", summary(&output));
}

// CLI subprocess failures through the app's cua::call, the read fallback the
// packaged app uses. Each mode runs in its own child with the fixture binary.
#[test]
fn cli_failures_are_bounded_and_reap_the_process_tree() {
    if std::env::var_os("ICU_CLI_FAILURE_CHILD").is_some() {
        let started = Instant::now();
        let result = call("get_window_state", &json!({"pid":42,"window_id":7}));
        println!("CLI_RESULT {}", json!({"result":format!("{result:?}"),"ok":result.is_ok(),"elapsed_ms":started.elapsed().as_millis() as u64}));
        return;
    }
    for mode in ["flood", "hang", "fail", "missing"] {
        let directory = CaptureFile::new().unwrap();
        let binary = directory.0.join("cua-driver");
        std::fs::write(&binary, include_str!("../../tests/fixtures/fake_failing_cli.sh")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let bin = if mode == "missing" { directory.0.join("absent-driver") } else { binary };
        let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
            .args(["cua::lifecycle_tests::cli_failures_are_bounded_and_reap_the_process_tree", "--exact", "--nocapture"])
            .env("ICU_CLI_FAILURE_CHILD", mode).env("ICU_CUA_BIN", &bin)
            .env("ICU_CUA_SOCKET", directory.0.join("unused.sock")).env_remove("ICU_OWN_DRIVER"),
            Duration::from_secs(40)).unwrap();
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(result.status.success(), "{mode}: {stdout}\n{}", String::from_utf8_lossy(&result.stderr));
        let line = stdout.lines().find_map(|l| l.strip_prefix("CLI_RESULT ")).unwrap_or_else(|| panic!("{mode}: {stdout}"));
        let outcome: Value = serde_json::from_str(line).unwrap();
        let message = outcome["result"].as_str().unwrap();
        let elapsed = outcome["elapsed_ms"].as_u64().unwrap();
        assert_eq!(outcome["ok"], false, "{mode}: {outcome}");
        let expected = match mode {
            "flood" => "subprocess output exceeds 32 MiB",
            "hang" => "subprocess deadline exceeded",
            "fail" => "fixture refused get_window_state",
            _ => "spawn cua-driver: spawn:",
        };
        assert!(message.contains(expected), "{mode}: {message}");
        // The message check above is what proves each failure path fired; the
        // bound only shows the non-hang modes return before the 15 s call
        // deadline would have ended them. A tighter bound measured host load
        // (flood took 11 s at load average 190), not the product.
        let bound = if mode == "hang" { 17_000 } else { 14_000 };
        assert!(elapsed < bound, "{mode}: took {elapsed} ms");
        if mode == "hang" { assert!(elapsed >= 14_900, "hang returned before the call deadline: {elapsed} ms"); }
        for file in ["cli.pid", "descendant.pid"] {
            let Ok(pid) = std::fs::read_to_string(directory.0.join(file)) else {
                assert!(mode == "missing" || (mode == "fail" && file == "descendant.pid"), "{mode}: no {file}");
                continue;
            };
            let pid: i64 = pid.trim().parse().unwrap();
            assert!(process_gone(pid), "{mode}: {file} {pid} was not killed and reaped");
        }
        eprintln!("CLI {mode}: {elapsed} ms, {message}");
    }
}

/// The model's turns in the refusal scenario: the history each one was shown.
static RECOVERY_TURNS: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());
struct RecoveryBrain;
impl crate::brain::Brain for RecoveryBrain {
    fn name(&self) -> String { "recovery-fixture".into() }
    fn decide(&self, _: &str, _memory: Option<&str>, _: &str, history: &[String]) -> Result<crate::brain::Decision, String> {
        use crate::brain::{Action, Decision};
        let mut turns = RECOVERY_TURNS.lock().unwrap();
        turns.push(history.to_vec());
        let editor = Action::Click { target: Some("Editor".into()), element_index: -1, button: None, count: None, action: None };
        let actions = match turns.len() {
            1 => vec![Action::Type { text: "one".into() }, Action::Type { text: "two".into() }],
            // Recovery: the refusal said to click the field first.
            2 => vec![editor, Action::Type { text: "one".into() }, Action::Type { text: "two".into() }],
            _ => vec![Action::Done { summary: "typed one then two".into() }],
        };
        Ok(Decision { thought: String::new(), action: None, actions, fallback: None })
    }
}

// Through the production task body, admission, cua client and socket transport:
// the fixture driver refuses the first type_text with a typed isError answer.
// The model gets the refusal with a fresh observation and recovers.
#[test]
fn a_driver_refusal_reaches_the_model_and_the_run_recovers() {
    if std::env::var("ICU_LIFECYCLE_CHILD").as_deref() == Ok("refusal") {
        let (app, events) = lifecycle_app_with(|| Box::new(RecoveryBrain));
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        assert_eq!(start(&webview, "refusal"), Ok(json!("refusal")));
        let mut seen = Vec::new();
        until_terminal(&events, &mut seen, "refusal");
        let turns = RECOVERY_TURNS.lock().unwrap().clone();
        crate::close_for_exit(app.handle());
        println!("LIFECYCLE {}", json!({"phase":"refusal","events":seen,"turns":turns}));
        return;
    }
    let (directory, rows, output) = lifecycle_child("a_driver_refusal_reaches_the_model_and_the_run_recovers", "refusal");
    let summary: Value = serde_json::from_str(summary(&output).strip_prefix("LIFECYCLE ").unwrap()).unwrap();
    let events = summary["events"].as_array().unwrap();
    assert_eq!(events.last().unwrap()["kind"], "done", "{events:?}");
    let acts: Vec<_> = events.iter().filter(|e| e["kind"] == "act").map(|e| e["data"].clone()).collect();
    assert_eq!(acts[0]["status"], "refused", "{acts:?}");
    assert_eq!(acts[0]["code"], "type_text_target_not_focused");
    assert_eq!(acts[1], json!({"action":"type \"two\"","ok":false,"status":"not_run","step":2,"of":2}));
    assert_eq!(acts[2..].iter().map(|a| a["status"].as_str().unwrap()).collect::<Vec<_>>(), ["delivered"; 3]);
    let turns = summary["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 3, "{turns:?}");
    assert_eq!(turns[1], json!([
        "type \"one\" -> REFUSED, step 1 of 2 (type_text_target_not_focused): no element of window 7 has keyboard focus; click the field first",
        "type \"two\" -> NOT RUN, step 2 of 2; skipped because step 1 was refused",
    ]));
    // The driver saw the refused input once and the skipped one never; the
    // recovery landed exactly once.
    let inputs: Vec<_> = rows.iter().filter(|r| is_input(r)).map(|r| (r["name"].as_str().unwrap().to_owned(), r["args"]["text"].clone())).collect();
    assert_eq!(inputs, [("type_text".into(), json!("one")), ("click".into(), Value::Null), ("type_text".into(), json!("one")), ("type_text".into(), json!("two"))]);
    assert_eq!(std::fs::read_to_string(directory.0.join("received-text.txt")).unwrap(), "onetwo");
    // Public observations remain one per decision. Readiness may poll privately.
    assert_eq!(events.iter().filter(|e| e["kind"] == "observe").count(), 3);
    let reads: Vec<_> = rows.iter().filter(|r| r["name"] == "get_window_state").collect();
    assert!((3..=5).contains(&reads.len()) && reads.iter().all(|r| r["transport"].is_null()), "{reads:?}");
    eprintln!("{}", summary.to_string().chars().take(400).collect::<String>());
}

/// Opt-in (`ICU_REAL_CLAUDE_CODE=1`): the GUI's own `claude-code` brain choice, end to end below
/// the webview. `agent_run` is called with `"brain":"claude-code"` exactly as `main.ts` sends it;
/// the production task body (`agent::run`, not a stand-in) builds the brain with `make_brain_for`,
/// which runs the real `claude -p` (through whatever proxy the environment names, ccroute here),
/// and the actions go through the cua client and socket transport to the fixture driver. No
/// desktop is touched. The record goes to `ICU_REAL_CLAUDE_CODE_OUT` when set.
#[test]
fn real_claude_code_choice_drives_the_production_task_body() {
    if let Ok(phase) = std::env::var("ICU_LIFECYCLE_CHILD") {
        if phase != "real-claude-code" { return; }
        let (app, events) = {
            use tauri::Listener;
            let app = crate::configure_commands(tauri::test::mock_builder())
                .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            app.listen_any("agent", move |event| { let _ = tx.send(serde_json::from_str::<Value>(event.payload()).unwrap()); });
            (app, rx)
        };
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let started = ipc(&webview, "agent_run", json!({"goal":"Type the word hello into the Editor text area, then finish.",
            "pid":42,"windowId":7,"appName":"fixture","brain":"claude-code","runId":"real-cc"}));
        assert_eq!(started, Ok(json!("real-cc")));
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(50);
        loop {
            let event = events.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap_or_else(|_| panic!("no terminal event: {seen:?}"));
            seen.push(event.clone());
            if matches!(event["kind"].as_str(), Some("done" | "fail" | "error" | "stopped")) { break; }
        }
        crate::close_for_exit(app.handle());
        println!("LIFECYCLE {}", json!({"phase":"real-claude-code","events":seen}));
        return;
    }
    if std::env::var("ICU_REAL_CLAUDE_CODE").is_err() {
        eprintln!("skipped (set ICU_REAL_CLAUDE_CODE=1 to run the real claude CLI)");
        return;
    }
    let (directory, rows, output) = lifecycle_child("real_claude_code_choice_drives_the_production_task_body", "real-claude-code");
    let run: Value = serde_json::from_str(summary(&output).strip_prefix("LIFECYCLE ").unwrap()).unwrap();
    if let Ok(path) = std::env::var("ICU_REAL_CLAUDE_CODE_OUT") {
        std::fs::write(path, serde_json::to_string_pretty(&json!({"run": run, "driver_rows": rows})).unwrap()).unwrap();
    }
    let events = run["events"].as_array().unwrap();
    let start = events.iter().find(|e| e["kind"] == "start").expect("start event");
    assert!(start["data"]["brain"].as_str().unwrap().starts_with("claude-code:"), "{start}");
    assert_eq!(events.last().unwrap()["kind"], "done", "{events:?}");
    let typed = std::fs::read_to_string(directory.0.join("received-text.txt")).unwrap_or_default();
    assert!(typed.contains("hello"), "the model's text reached the driver: {typed:?}");
}

/// Raw model replies, one per decision, read by the real parser. The set depends on the phase.
/// This is the model's side of the malformed-reply path: what a model that sends an empty key, a
/// null target, a `set_value` with no target or a kind used as a key would send, with the real
/// parser, the production task body, the cua client and the socket transport behind it.
static MALFORMED_TURNS: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());
struct MalformedBrain;
impl crate::brain::Brain for MalformedBrain {
    fn name(&self) -> String { "malformed-fixture".into() }
    fn decide(&self, _: &str, _memory: Option<&str>, _: &str, history: &[String]) -> Result<crate::brain::Decision, String> {
        let mut turns = MALFORMED_TURNS.lock().unwrap();
        turns.push(history.to_vec());
        let replies: &[&str] = match std::env::var("ICU_LIFECYCLE_CHILD").unwrap_or_default().as_str() {
            "malformed" => &[
                r#"{"thought":"t","action":{"kind":"key","key":""}}"#,
                r#"{"thought":"t","action":{"kind":"type","text":"ok"}}"#,
                // The first step is valid; nothing of the batch may run.
                r#"{"thought":"t","actions":[{"kind":"type","text":"never"},{"kind":"set_value","value":"v"}]}"#,
                r#"{"thought":"t","action":{"kind":"type","text":"two"}}"#,
                r#"{"thought":"t","action":{"kind":"click","target":null}}"#,
                r#"{"thought":"t","action":{"kind":"type","text":"three"}}"#,
                r#"{"thought":"done","action":{"done":true,"summary":"typed"}}"#,
                r#"{"thought":"t","action":{"kind":"done","summary":"typed ok two three"}}"#,
            ],
            _ => &[
                r#"{"thought":"t","action":{"kind":"click","target":""}}"#,
                r#"{"thought":"t","actions":[{"kind":"type","text":"never"},{"kind":"click","target":null}]}"#,
                r#"{"thought":"t","action":{"kind":"type","text":"never either"}}"#,
            ],
        };
        crate::brain::parse_decision(replies.get(turns.len() - 1).copied().unwrap_or(r#"{"action":{"kind":"fail","reason":"script ended"}}"#))
    }
}

#[test]
fn malformed_replies_never_reach_the_driver_and_the_run_recovers_or_ends() {
    if let Ok(phase) = std::env::var("ICU_LIFECYCLE_CHILD") {
        let (app, events) = lifecycle_app_with(|| Box::new(MalformedBrain));
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        assert_eq!(start(&webview, &phase), Ok(json!(phase)));
        let mut seen = Vec::new();
        until_terminal(&events, &mut seen, &phase);
        let turns = MALFORMED_TURNS.lock().unwrap().clone();
        crate::close_for_exit(app.handle());
        println!("LIFECYCLE {}", json!({"phase":phase,"events":seen,"turns":turns}));
        return;
    }
    let shape = crate::brain::REPLY_SHAPE;
    // A malformed reply, a valid one, a malformed batch, a valid one, a malformed one, a valid one,
    // `{"done":true}` and the finish: the run recovers each time and ends done.
    let (directory, rows, output) = lifecycle_child("malformed_replies_never_reach_the_driver_and_the_run_recovers_or_ends", "malformed");
    let first_run: Value = serde_json::from_str(summary(&output).strip_prefix("LIFECYCLE ").unwrap()).unwrap();
    let events = first_run["events"].as_array().unwrap();
    assert_eq!(events.last().unwrap()["kind"], "done", "{events:?}");
    assert_eq!(events.iter().filter(|e| e["kind"] == "malformed").count(), 4, "{events:?}");
    let inputs: Vec<_> = rows.iter().filter(|r| is_input(r)).map(|r| (r["name"].as_str().unwrap().to_owned(), r["args"]["text"].clone())).collect();
    assert_eq!(inputs, [("type_text".into(), json!("ok")), ("type_text".into(), json!("two")), ("type_text".into(), json!("three"))],
        "only the valid replies reached the driver; nothing of a malformed one, its valid first step included");
    assert_eq!(std::fs::read_to_string(directory.0.join("received-text.txt")).unwrap(), "oktwothree");
    let turns = first_run["turns"].as_array().unwrap();
    assert_eq!(turns.len(), 8, "{turns:?}");
    // The model read the parser's exact error and the expected shape, once, on its next turn.
    let first = turns[1][0].as_str().unwrap();
    assert!(first.starts_with("(your reply) -> MALFORMED, nothing of it ran: bad action: key: `key` must be a non-empty string, got \"\"")
        && first.ends_with(shape), "{first}");
    assert!(turns[3].as_array().unwrap().last().unwrap().as_str().unwrap().contains("bad action: step 2: set_value needs `target` or `element_index`"), "{turns:?}");
    assert!(turns[5].as_array().unwrap().last().unwrap().as_str().unwrap().contains("bad action: click needs `target` or `element_index`"), "{turns:?}");
    assert!(turns[7].as_array().unwrap().last().unwrap().as_str().unwrap().contains("no kind and an unknown field `done`"), "{turns:?}");
    // Private readiness polls must not become additional public decisions.
    assert_eq!(events.iter().filter(|e| e["kind"] == "observe").count(), 8);
    let reads: Vec<_> = rows.iter().filter(|r| r["name"] == "get_window_state").collect();
    assert!((8..=14).contains(&reads.len()) && reads.iter().all(|r| r["transport"].is_null()), "{reads:?}");

    // Two malformed replies in a row end the run, with no input of any kind.
    let (directory, rows, output) = lifecycle_child("malformed_replies_never_reach_the_driver_and_the_run_recovers_or_ends", "malformed-twice");
    let summary: Value = serde_json::from_str(summary(&output).strip_prefix("LIFECYCLE ").unwrap()).unwrap();
    let last = summary["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["kind"], "error", "{last}");
    assert_eq!(last["data"]["malformed_in_a_row"], 2);
    assert!(last["data"]["msg"].as_str().unwrap().starts_with("brain: bad action: step 2: click needs `target` or `element_index`"), "{last}");
    assert!(!rows.iter().any(is_input), "no input reached the driver: {rows:?}");
    assert_eq!(summary["turns"].as_array().unwrap().len(), 2, "the third scripted reply was never asked for");
    assert!(!directory.0.join("received-text.txt").exists());
}
