//! Real pipe backpressure, independent of desktop / daemon permissions.
#[cfg(unix)]
#[test]
fn blocked_stdin_deadline_kills_child_and_connection_stays_failed() {
    use icu::cua::{Cua, McpChild};
    use serde_json::json;
    use std::{sync::mpsc, time::{Duration, Instant}};
    let tmp = tempfile::tempdir().unwrap();
    let pidfile = tmp.path().join("pid");
    std::env::set_var("AUDIT_DRIVER_PID", &pidfile);
    std::env::set_var("AUDIT_DRIVER_MODE", "blocked_write");
    // The same setting covers interpreter startup and the blocked write. A
    // 200 ms cold-start budget can expire before this fixture acknowledges
    // initialize on a busy host, never exercising pipe backpressure at all.
    // One second still distinguishes bounded cleanup from its 60-second stall.
    std::env::set_var("ICU_DRIVER_TIMEOUT_MS", "1000");
    let driver = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/fixtures/audit_driver.py");
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let child = McpChild::spawn(driver.to_str().unwrap(), None).unwrap();
        let start = Instant::now();
        let first = child.call("audit", json!({"padding": "x".repeat(1_000_000)}));
        let second = child.tool_names();
        tx.send((first, second, start.elapsed())).unwrap();
    });
    let result = rx.recv_timeout(Duration::from_secs(3));
    if result.is_err() {
        // External RED guard: only this fixture's PID, never a real driver.
        if let Ok(pid) = std::fs::read_to_string(&pidfile) {
            let _ = std::process::Command::new("/bin/kill").args(["-KILL", pid.trim()]).status();
        }
    }
    worker.join().unwrap();
    let (first, second, elapsed) = result.expect("transport exceeded external guard (blocked write)");
    for err in [first.unwrap_err(), second.unwrap_err()] {
        assert_eq!(icu::ToolError::from_cua(err).code(), Some(icu::CODE_TRANSPORT_ERROR));
    }
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    // McpChild must reap the fixture; a zombie is not cleanup.
    let pid = std::fs::read_to_string(pidfile).unwrap();
    assert!(!std::process::Command::new("/bin/kill").args(["-0", pid.trim()]).status().unwrap().success());
}
