#![cfg(windows)]
#[path = "../src/subprocess.rs"]
mod subprocess;
use std::fs;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE};

fn fixture_command(role: &str, path: &std::path::Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--ignored", "--exact", "subprocess_fixture", "--nocapture"])
        .env("ICU_SUBPROCESS_FIXTURE_ROLE", role)
        .env("ICU_SUBPROCESS_FIXTURE_PATH", path);
    command
}

#[test]
#[ignore = "helper process, invoked only by the containment tests"]
fn subprocess_fixture() {
    let Ok(role) = std::env::var("ICU_SUBPROCESS_FIXTURE_ROLE") else { return; };
    let path = std::path::PathBuf::from(std::env::var_os("ICU_SUBPROCESS_FIXTURE_PATH").unwrap());
    if role == "child" {
        fs::write(&path, std::process::id().to_string()).unwrap();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let mut child = fixture_command("child", &path);
    if role == "detached" {
        // Null standard streams alone do not clear the original handles'
        // inheritance flags in this process. Make the fixture truly detached.
        unsafe {
            assert_ne!(SetHandleInformation(std::io::stdout().as_raw_handle(), HANDLE_FLAG_INHERIT, 0), 0);
            assert_ne!(SetHandleInformation(std::io::stderr().as_raw_handle(), HANDLE_FLAG_INHERIT, 0), 0);
        }
        child.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let _child = child.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.with_extension("ack").exists() {
        assert!(Instant::now() < deadline, "observer never retained the child handle");
        std::thread::sleep(Duration::from_millis(10));
    }
    if role == "waiting" { std::thread::sleep(Duration::from_secs(30)); }
}

fn check_tree(role: &str, expected_timeout: bool) {
    let path = std::env::temp_dir().join(format!("icu-tree-{}-{role}.pid", std::process::id()));
    assert!(!path.exists(), "stale probe artifact");
    let observed_path = path.clone();
    let observer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let pid: u32 = loop {
            if let Ok(text) = fs::read_to_string(&observed_path) {
                if let Ok(pid) = text.parse() { break pid; }
            }
            assert!(Instant::now() < deadline, "descendant never ran");
            std::thread::sleep(Duration::from_millis(5));
        };
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid) };
        assert!(!raw.is_null(), "cannot retain live descendant handle");
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        // Keep the original kernel object alive across cleanup. Looking up the
        // PID afterward could observe an unrelated process that reused it.
        fs::write(observed_path.with_extension("ack"), b"ready").unwrap();
        handle
    });
    let started = Instant::now();
    let result = subprocess::output(&mut fixture_command(role, &path), Duration::from_secs(2));
    let handle = observer.join().unwrap();
    fs::remove_file(&path).unwrap();
    fs::remove_file(path.with_extension("ack")).unwrap();
    // Preserve failure evidence, but clean an escaped child before asserting.
    let escaped = unsafe {
        {
            // Job termination is asynchronous. Require the original process
            // object to signal within a bounded second, not just an empty job.
            let wait = WaitForSingleObject(handle.as_raw_handle(), 1000);
            assert!(wait == 0 || wait == 0x102, "cannot determine descendant state");
            if wait == 0x102 {
                assert_ne!(TerminateProcess(handle.as_raw_handle(), 9), 0);
                assert_eq!(WaitForSingleObject(handle.as_raw_handle(), 5000), 0);
            }
            wait == 0x102
        }
    };
    assert!(started.elapsed() < Duration::from_secs(5), "cleanup exceeded its bound");
    if expected_timeout { assert!(result.unwrap_err().contains("deadline")); }
    else { assert!(result.unwrap().status.success()); }
    assert!(!escaped, "descendant survived command return ({role})");
}

#[test]
fn timeout_kills_descendant_with_live_parent() { check_tree("waiting", true); }
#[test]
fn exited_parent_cannot_leave_inherited_pipes_open() { check_tree("exited", true); }
#[test]
fn success_cannot_leave_a_detached_descendant() { check_tree("detached", false); }
#[test]
fn output_and_nonzero_status_are_preserved() {
    let result = subprocess::output(Command::new("cmd.exe").args(["/d", "/c", "echo result & echo problem 1>&2 & exit /b 7"]), Duration::from_secs(5)).unwrap();
    assert_eq!(result.status.code(), Some(7));
    assert_eq!(String::from_utf8(result.stdout).unwrap().trim(), "result");
    assert_eq!(String::from_utf8(result.stderr).unwrap().trim(), "problem");
}
