//! Actual app-loop readiness on an explicitly owned GTK3/X11 or WinForms desktop.
//! Every ignored case performs native input. The tests never provision a host,
//! start a driver, activate a window, or inject readiness metadata into a snapshot.
use super::*;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
#[path = "../windows_pipe.rs"]
mod windows_pipe;

fn now() -> f64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() }
fn save(path: &Path, value: &Value) { fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap(); }
fn read(path: &Path) -> Value { serde_json::from_slice(&fs::read(path).unwrap()).unwrap() }
fn digest(path: &Path) -> String { format!("{:x}", Sha256::digest(fs::read(path).unwrap())) }

struct Host { work: PathBuf, socket: String, driver: PathBuf, driver_pid: u32 }
impl Host {
    fn checked() -> Self {
        assert!(cfg!(any(target_os = "linux", target_os = "windows")), "portable native acceptance requires Linux or Windows");
        assert_eq!(std::env::var("ICU_DISPOSABLE_HOST").as_deref(), Ok("1"));
        assert_eq!(std::env::var("ICU_E2E_PORTABLE_READINESS").as_deref(), Ok("1"));
        assert!(!Path::new("/Users/nathanzhao").exists(), "never run on Nathan's working desktop");
        for key in ["HOME", "USERPROFILE", "USERNAME", "USER"] {
            assert!(!std::env::var(key).unwrap_or_default().to_ascii_lowercase().contains("nathanzhao"), "never run on Nathan's working desktop");
        }
        let work = PathBuf::from(std::env::var_os("ICU_E2E_PORTABLE_WORKDIR").expect("explicit private work directory required"));
        assert!(work.is_absolute() && work.is_dir());
        assert!(!fs::symlink_metadata(&work).unwrap().file_type().is_symlink());
        assert!(!work.components().any(|p| p == std::path::Component::ParentDir));
        let socket = std::env::var("ICU_CUA_SOCKET").expect("explicit scoped socket or pipe required");
        let driver = PathBuf::from(std::env::var_os("ICU_CUA_BIN").expect("absolute pinned driver required"));
        assert!(driver.is_absolute() && driver.is_file());
        let driver_pid = std::env::var("ICU_E2E_DRIVER_PID").expect("owned daemon pid required").parse::<u32>().unwrap();
        assert!(driver_pid > 0 && driver_pid != std::process::id());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
            assert_eq!(fs::canonicalize(&work).unwrap(), work, "work directory must be canonical");
            assert_eq!(std::env::var("XDG_SESSION_TYPE").as_deref(), Ok("x11"), "this fixture covers GTK3/X11 only");
            assert!(std::env::var_os("DISPLAY").is_some() && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some());
            let meta = fs::symlink_metadata(&work).unwrap();
            assert_eq!(meta.uid(), unsafe { libc::geteuid() });
            assert_eq!(meta.permissions().mode() & 0o077, 0, "work directory must be private");
            assert_eq!(Path::new(&socket).parent(), Some(work.as_path()));
            let meta = fs::symlink_metadata(&socket).unwrap();
            assert!(meta.file_type().is_socket());
            assert_eq!(meta.uid(), unsafe { libc::geteuid() });
            assert_eq!(fs::canonicalize(format!("/proc/{driver_pid}/exe")).unwrap(), fs::canonicalize(&driver).unwrap(), "daemon is not the pinned executable");
        }
        #[cfg(windows)]
        {
            assert_eq!(std::env::var("SESSIONNAME").as_deref(), Ok("Console"), "owned console session required; RDP/service sessions are not acceptance");
            let expected = format!(r"\\.\pipe\icu-readiness-{}", work.file_name().unwrap().to_str().unwrap());
            assert_eq!(socket, expected, "pipe must be scoped to this test directory; kernel ownership is checked separately");
            let script = format!("(Get-Process -Id {driver_pid} -ErrorAction Stop).Path");
            let out = crate::subprocess::output(Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &script]), Duration::from_secs(5)).unwrap();
            assert!(out.status.success(), "cannot establish daemon executable identity");
            assert_eq!(fs::canonicalize(String::from_utf8(out.stdout).unwrap().trim()).unwrap(), fs::canonicalize(&driver).unwrap(), "daemon is not the pinned executable");
        }
        let host = Self { work, socket, driver, driver_pid };
        host.assert_endpoint();
        host
    }

    fn assert_endpoint(&self) {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let stream = crate::transport::connect(&self.socket).unwrap();
            unsafe {
                let mut peer: libc::ucred = std::mem::zeroed();
                let mut size = std::mem::size_of_val(&peer) as libc::socklen_t;
                assert_eq!(libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut peer as *mut libc::ucred).cast(), &mut size), 0);
                assert_eq!(peer.pid as u32, self.driver_pid, "private socket owner changed");
                assert_eq!(peer.uid, libc::geteuid(), "private socket belongs to another user");
            }
        }
        #[cfg(windows)]
        assert_eq!(windows_pipe::listener_pid(&self.socket, Duration::from_millis(500)), Some(self.driver_pid), "private pipe owner changed");
    }
}

struct Fixture { child: Child, dir: PathBuf, ready: Value }
impl Drop for Fixture {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}
impl Fixture {
    fn launch(host: &Host, mode: &str) -> Self {
        let dir = host.work.join(format!("{mode}-{}-{}", std::process::id(), now().to_bits()));
        fs::create_dir(&dir).unwrap();
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap(); }
        let source;
        let mut command;
        if cfg!(target_os = "windows") {
            source = dir.join("readiness_windows.cs");
            fs::write(&source, include_str!("../../tests/fixtures/readiness_windows.cs")).unwrap();
            let compiler = PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot required")).join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
            let binary = dir.join("readiness-fixture.exe");
            let out = crate::subprocess::output(Command::new(compiler).args(["/nologo", "/target:winexe", "/reference:System.Windows.Forms.dll", "/reference:System.Drawing.dll", "/reference:System.Web.Extensions.dll"])
                .arg(format!("/out:{}", binary.display())).arg(&source), Duration::from_secs(45)).unwrap();
            fs::write(dir.join("build.stdout"), out.stdout).unwrap(); fs::write(dir.join("build.stderr"), &out.stderr).unwrap();
            assert!(out.status.success(), "WinForms receiver failed to compile: {}", String::from_utf8_lossy(&out.stderr));
            command = Command::new(binary);
        } else {
            source = dir.join("readiness_gtk.py");
            fs::write(&source, include_str!("../../tests/fixtures/readiness_gtk.py")).unwrap();
            command = Command::new("/usr/bin/python3"); command.arg(&source);
        }
        let log = File::create(dir.join("fixture.log")).unwrap();
        let child = command.arg(&dir).arg(mode).stdin(Stdio::null()).stdout(log.try_clone().unwrap()).stderr(log).spawn().unwrap();
        let mut fixture = Self { child, dir, ready: Value::Null };
        save(&fixture.dir.join("binding.json"), &json!({"platform":std::env::consts::OS,"driver":host.driver,"driver_pid":host.driver_pid,
            "driver_sha256":digest(&host.driver),"fixture_source_sha256":digest(&source),"test_binary_sha256":digest(&std::env::current_exe().unwrap()),
            "scope":"production app loop with native fixture and scripted Brain; not signed-package acceptance"}));
        let deadline = Instant::now() + Duration::from_secs(12);
        while !fixture.dir.join("ready.json").is_file() {
            assert!(fixture.child.try_wait().unwrap().is_none(), "receiver exited before ready; inspect {}", fixture.dir.display());
            assert!(Instant::now() < deadline, "receiver startup timed out; inspect {}", fixture.dir.display());
            std::thread::sleep(Duration::from_millis(40));
        }
        fixture.ready = read(&fixture.dir.join("ready.json"));
        assert_eq!(fixture.ready["pid"].as_u64(), Some(u64::from(fixture.child.id())));
        assert_ne!(fixture.ready["target_window_id"], fixture.ready["decoy_window_id"]);
        assert!(fixture.ready["front_window_id"].as_u64().unwrap() > 0);
        assert_ne!(fixture.ready["front_window_id"], fixture.ready["target_window_id"]);
        assert_ne!(fixture.ready["front_window_id"], fixture.ready["decoy_window_id"]);
        fixture
    }
}

fn without_pixels(mut state: Value) -> Value {
    if let Some(fields) = state.as_object_mut() {
        for key in ["screenshot_data_url", "screenshot_png_b64"] { fields.remove(key); }
        if let Some(shots) = fields.get_mut("screenshots").and_then(Value::as_array_mut) {
            for shot in shots { if let Some(fields) = shot.as_object_mut() { fields.remove("png_b64"); } }
        }
    }
    state
}

struct ScriptedBrain { mode: &'static str, calls: Mutex<Vec<Value>> }
impl brain::Brain for ScriptedBrain {
    fn name(&self) -> String { "portable-native-readiness".into() }
    fn decide(&self, _: &str, _: Option<&str>, tree: &str, _: &[String]) -> Result<brain::Decision, String> {
        let mut calls = self.calls.lock().unwrap(); calls.push(json!({"at":now(),"tree":tree}));
        let action = match calls.len() {
            1 => Action::Click { target: Some("Compute".into()), element_index: -1, button: None, count: None, action: None },
            2 if self.mode == "noeffect" => Action::Done { summary: "native controls unchanged".into() },
            2 if self.mode == "delayed" && (tree.contains("value=\"42\"") || tree.contains("= \"42\"")) && !tree.contains("Working") => Action::Done { summary: "native result is 42".into() },
            _ => return Err("readiness allowed a model decision before the native fixture completed".into()),
        };
        Ok(brain::Decision { thought: String::new(), action: Some(action), actions: vec![], fallback: None })
    }
}

fn exercise(mode: &'static str) {
    let host = Host::checked();
    let mut fixture = Fixture::launch(&host, mode);
    let pid = fixture.ready["pid"].as_i64().unwrap();
    let target = fixture.ready["target_window_id"].as_i64().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        host.assert_endpoint();
        if let Ok(state) = cua::get_state(pid, target, 200) {
            if resolve_token(&state, -1, Some("Compute")).is_some() {
                save(&fixture.dir.join("initial-native-state.json"), &without_pixels(state)); break;
            }
        }
        assert!(fixture.child.try_wait().unwrap().is_none(), "fixture exited during AX warmup");
        assert!(Instant::now() < deadline, "native observer cannot resolve Compute; inspect {}", fixture.dir.display());
        std::thread::sleep(Duration::from_millis(60));
    }
    let brain = ScriptedBrain { mode, calls: Mutex::new(Vec::new()) };
    let events = Mutex::new(Vec::new());
    let after_action = Mutex::new(Value::Null);
    let native_busy = Mutex::new(None);
    let observations = Mutex::new(Vec::new());
    let dispatched = std::cell::Cell::new(false);
    let started = Instant::now();
    // Instrument the production observation callback, never the act event.
    // An additional capture after act would delay the next model decision and
    // could hide the premature-read regression this fixture is meant to catch.
    run_loop_with("Press Compute once, then finish only after the result is ready.",
        "ICU Portable Readiness Target", &brain,
        &|kind, data| events.lock().unwrap().push(json!({"kind":kind,"at":now(),"data":data})),
        &|| {
            let state = cua::get_state(pid, target, 200)?;
            let busy = ui_readiness::busy(&state);
            let retained = without_pixels(state.clone());
            observations.lock().unwrap().push(json!({"at":now(),"after_dispatch":dispatched.get(),"busy":busy,"state":retained}));
            let mut first_busy = native_busy.lock().unwrap();
            if dispatched.get() && first_busy.is_none() {
                *first_busy = Some(busy);
                *after_action.lock().unwrap() = retained;
            }
            Ok(state)
        },
        &|action, state| {
            host.assert_endpoint();
            let result = dispatch_action(action, state, pid, target);
            dispatched.set(true);
            result
        }, &|| false);
    host.assert_endpoint();
    let state = read(&fixture.dir.join("state.json"));
    let calls = brain.calls.lock().unwrap();
    let events = events.lock().unwrap();
    let report = json!({"mode":mode,"seconds":started.elapsed().as_secs_f64(),"ready":fixture.ready,"fixture":state,
        "decisions":*calls,"events":*events,"observations":*observations.lock().unwrap(),
        "native_busy_detected":*native_busy.lock().unwrap(),"after_action_native":*after_action.lock().unwrap()});
    save(&fixture.dir.join("result.json"), &report);
    eprintln!("portable native readiness evidence: {}", fixture.dir.display());
    assert_eq!(state["target_effects"], 1, "exactly one target input required: {report}");
    assert_eq!(state["decoy_effects"], 0, "wrong target changed: {report}");
    assert_eq!(state["ever_active"], false, "fixture took foreground: {report}");
    assert_eq!(state["foreground_windows"], json!([fixture.ready["front_window_id"]]), "foreground owner changed: {report}");
    assert!(state.get("ownership_error").is_none(), "ownership sampler failed: {report}");
    assert!(state["ownership_samples"].as_u64().unwrap() >= 2, "ownership sampler did not run: {report}");
    assert!(now() - state["ownership_sampled_at"].as_f64().unwrap() < 0.5, "ownership sampler stopped: {report}");
    assert!(started.elapsed() < Duration::from_secs(12), "bounded readiness did not finish: {report}");
    if mode == "busy-static" {
        assert_eq!(*native_busy.lock().unwrap(), Some(true), "CAPABILITY_MISSING: native busy signal was not exported or recognized; constant-tree busy is unproven: {report}");
    }
    match mode {
        "delayed" => {
            assert_eq!(state["result"], "42"); assert_eq!(calls.len(), 2, "{report}");
            assert!(calls[1]["at"].as_f64().unwrap() >= state["completed_at"].as_f64().unwrap(), "{report}");
            assert!(events.iter().any(|e| e["kind"] == "done"), "{report}");
        }
        "noeffect" => {
            assert_eq!(state["result"], "0"); assert_eq!(calls.len(), 2, "{report}");
            assert!(events.iter().any(|e| e["kind"] == "done"), "{report}");
        }
        "permanent" | "busy-static" => {
            assert_eq!(calls.len(), 1, "unready state reached a model decision: {report}");
            assert!(events.iter().any(|e| e["kind"] == "error" && e["data"]["readiness"].as_str().is_some_and(|s| s.contains("budget"))), "missing readiness timeout: {report}");
            assert!(!events.iter().any(|e| e["kind"] == "done"), "{report}");
        }
        _ => unreachable!(),
    }
}

#[test]
#[ignore = "native input on explicitly owned GTK3/X11 or Windows console only"]
fn portable_delayed_result_precedes_second_decision() { exercise("delayed"); }

#[test]
#[ignore = "native input on explicitly owned GTK3/X11 or Windows console only"]
fn portable_changing_state_stops_before_second_decision() { exercise("permanent"); }

#[test]
#[ignore = "native input on explicitly owned GTK3/X11 or Windows console only"]
fn portable_unchanged_ui_finishes_without_touching_decoy() { exercise("noeffect"); }

#[test]
#[ignore = "native capability probe; fails when the real busy signal is absent, never injects one"]
fn portable_static_busy_requires_native_signal() { exercise("busy-static"); }
