//! Public consumer paths executed on a Tokio task, with actual subprocess pipes.
use icu::cua::{Cua, McpChild};
use serde_json::json;
use std::{time::{Duration, Instant}, path::Path};

#[test]
fn runtime_consumer() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let pid = tmp.path().join("pid");
    std::env::set_var("AUDIT_DRIVER_PID", &pid);
    let driver = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/fixtures/audit_driver.py");
    runtime.block_on(async {
        for mode in ["healthy", "silent_init", "silent_health", "blocked_write", "notifications", "eof"] {
            std::env::set_var("AUDIT_DRIVER_MODE", mode);
            // A mode that must connect needs the Python fixture to start and answer
            // `initialize` within the deadline, which a cold start under load missed at
            // 200 ms; it gets the 1500 ms the product allows initialization at most. The
            // modes that must fail to connect keep 200 ms, and every failing exchange is
            // still timed against its own deadline.
            let must_connect = matches!(mode, "healthy" | "silent_health" | "blocked_write");
            let timeout = Duration::from_millis(if must_connect { 1500 } else { 200 });
            std::env::set_var("ICU_DRIVER_TIMEOUT_MS", timeout.as_millis().to_string());
            let start = Instant::now();
            match McpChild::spawn(driver.to_str().unwrap(), None) {
                Ok(child) => {
                    if mode == "healthy" {
                        assert!(child.tool_names().unwrap().contains(&"click".into()));
                        assert_eq!(child.call("health_report", json!({})).unwrap()["overall"], "ok");
                    } else {
                        let call = Instant::now();
                        let e = child.call("health_report", json!({"padding": "x".repeat(1_000_000)})).unwrap_err();
                        assert!(call.elapsed() < timeout + Duration::from_millis(1000), "{mode}: call deadline exceeded ({:?})", call.elapsed());
                        assert_eq!(icu::ToolError::from_cua(e).code(), Some(icu::CODE_TRANSPORT_ERROR));
                        assert!(child.tool_names().is_err(), "failed connection must not replay/reconnect");
                    }
                    drop(child); // teardown inside caller runtime is part of the contract
                }
                Err(e) => {
                    assert!(!must_connect, "{mode}: {e:?}");
                    assert_eq!(icu::ToolError::from_cua(e).code(), Some(icu::CODE_TRANSPORT_ERROR));
                }
            }
            // Connecting, then (for a failing mode) one exchange, each within its deadline.
            let bound = if must_connect { 2 * timeout } else { timeout } + Duration::from_millis(1000);
            assert!(start.elapsed() < bound, "{mode}: deadline exceeded ({:?})", start.elapsed());
            #[cfg(unix)] {
                let child_pid = std::fs::read_to_string(&pid).unwrap();
                assert!(!std::process::Command::new("/bin/kill").args(["-0", child_pid.trim()]).output().unwrap().status.success(), "{mode}: child not reaped");
            }
        }
        assert!(McpChild::spawn("/no/such/icu-test-driver", None).is_err());
    });
}
