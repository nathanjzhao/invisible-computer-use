//! Real AppKit observations and dispatch, with a deterministic model substitute.
//! Never run on the developer desktop. See `host` for every required guard.
#![cfg(target_os = "macos")]

use super::*;
use brain::{Brain, Decision};
use std::fs::{self, File};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
fn save(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// These checks run before compiling or launching any AppKit code. An explicit
/// private socket also prevents accidental fallback to the app/vendor daemon.
fn host() -> PathBuf {
    assert_eq!(std::env::var("ICU_DISPOSABLE_HOST").as_deref(), Ok("1"));
    assert_eq!(
        std::env::var("ICU_E2E_NATIVE_READINESS").as_deref(),
        Ok("1")
    );
    assert!(
        !Path::new("/Users/nathanzhao").exists(),
        "never run on Nathan's computer"
    );
    assert!(!std::env::var("HOME")
        .unwrap_or_default()
        .starts_with("/Users/nathanzhao"));
    assert_eq!(
        fs::metadata("/dev/console").unwrap().uid(),
        unsafe { libc::geteuid() },
        "run as the disposable Mac's logged-in console user"
    );
    let work = PathBuf::from(
        std::env::var_os("ICU_E2E_NATIVE_READINESS_WORKDIR")
            .expect("explicit private work directory required"),
    );
    assert!(work.is_absolute());
    assert_eq!(
        fs::canonicalize(&work).unwrap(),
        work,
        "work directory must be canonical"
    );
    let meta = fs::symlink_metadata(&work).unwrap();
    assert!(
        meta.is_dir() && meta.permissions().mode() & 0o077 == 0,
        "private directory required"
    );
    assert_eq!(meta.uid(), unsafe { libc::geteuid() });
    let socket = PathBuf::from(
        std::env::var_os("ICU_CUA_SOCKET").expect("explicit private driver socket required"),
    );
    assert_eq!(
        socket.parent(),
        Some(work.as_path()),
        "socket must belong to this test directory"
    );
    let meta = fs::symlink_metadata(&socket).unwrap();
    assert!(
        meta.file_type().is_socket(),
        "expected an already running owned driver"
    );
    assert_eq!(meta.uid(), unsafe { libc::geteuid() });
    let driver =
        PathBuf::from(std::env::var_os("ICU_CUA_BIN").expect("absolute pinned driver required"));
    assert!(driver.is_absolute() && driver.is_file());
    work
}

struct Fixture {
    child: Child,
    dir: PathBuf,
    pid: i64,
    target: i64,
    decoy: i64,
    front: i64,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Fixture {
    fn launch(mode: &str) -> Self {
        let work = host();
        let dir = work.join(format!("{mode}-{}-{}", std::process::id(), now().to_bits()));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let binary = dir.join("readiness-fixture");
        // Embed the exact source so a copied test executable can run on the
        // disposable Mac without recreating the developer's checkout path.
        let source = dir.join("readiness.swift");
        fs::write(&source, include_str!("../../tests/fixtures/readiness.swift")).unwrap();
        let build = crate::subprocess::output(
            Command::new("/usr/bin/xcrun")
                .arg("swiftc")
                .arg(&source)
                .arg("-o")
                .arg(&binary),
            Duration::from_secs(60),
        )
        .unwrap();
        fs::write(dir.join("swift-build.log"), &build.stderr).unwrap();
        assert!(
            build.status.success(),
            "Swift fixture failed to compile: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let log = File::create(dir.join("fixture.log")).unwrap();
        let child = Command::new(binary)
            .arg(&dir)
            .arg(mode)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let mut fixture = Self {
            child,
            dir,
            pid: 0,
            target: 0,
            decoy: 0,
            front: 0,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !fixture.dir.join("ready.json").is_file() {
            assert!(
                fixture.child.try_wait().unwrap().is_none(),
                "fixture exited before readiness"
            );
            assert!(Instant::now() < deadline, "fixture startup timed out");
            std::thread::sleep(Duration::from_millis(50));
        }
        let ready = read(&fixture.dir.join("ready.json"));
        fixture.pid = ready["pid"].as_i64().unwrap();
        fixture.target = ready["target_window_id"].as_i64().unwrap();
        fixture.decoy = ready["decoy_window_id"].as_i64().unwrap();
        fixture.front = ready["front_pid"].as_i64().unwrap();
        assert_eq!(fixture.pid, i64::from(fixture.child.id()));
        assert_ne!(fixture.target, fixture.decoy);
        assert!(
            fixture.front > 0,
            "no foreground GUI application is present"
        );
        assert_ne!(fixture.front, fixture.pid, "fixture activated itself");
        // Native AX warmup only. This does not issue any input or synthesize state.
        loop {
            if let Ok(state) = cua::get_state(fixture.pid, fixture.target, 200) {
                if resolve_token(&state, -1, Some("Compute")).is_some() {
                    save(
                        &fixture.dir.join("initial-native-state.json"),
                        &without_pixels(state),
                    );
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "native observer cannot resolve fixture's Compute button"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        fixture
    }
    fn state(&self) -> Value {
        read(&self.dir.join("state.json"))
    }
}

fn without_pixels(mut state: Value) -> Value {
    if let Some(object) = state.as_object_mut() {
        object.remove("screenshot_data_url");
        object.remove("screenshot_png_b64");
    }
    state
}

struct ScriptedBrain {
    mode: &'static str,
    calls: Mutex<Vec<Value>>,
}
impl Brain for ScriptedBrain {
    fn name(&self) -> String {
        "native-readiness-script".into()
    }
    fn decide(
        &self,
        _: &str,
        _: Option<&str>,
        tree: &str,
        _: &[String],
    ) -> Result<Decision, String> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(json!({"at":now(), "tree":tree}));
        let action = match calls.len() {
            1 => Action::Click {
                target: Some("Compute".into()),
                element_index: -1,
                button: None,
                count: None,
                action: None,
            },
            2 if self.mode == "delayed" => {
                if !(tree.contains("value=\"42\"") || tree.contains("= \"42\""))
                    || tree.contains("Loading result")
                {
                    return Err("second decision received an unfinished native result".into());
                }
                Action::Done {
                    summary: "native delayed result is 42".into(),
                }
            }
            2 if self.mode == "noeffect" => Action::Done {
                summary: "native control stayed unchanged".into(),
            },
            _ => {
                return Err("brain was called again while the native fixture remained busy".into())
            }
        };
        Ok(Decision {
            thought: String::new(),
            action: Some(action),
            actions: vec![],
            fallback: None,
        })
    }
}

fn exercise(mode: &'static str) {
    let fixture = Fixture::launch(mode);
    let brain = ScriptedBrain {
        mode,
        calls: Mutex::new(Vec::new()),
    };
    let events = Mutex::new(Vec::<Value>::new());
    let action_sample = Mutex::new(None::<Value>);
    let observations = Mutex::new(Vec::<Value>::new());
    let dispatched = std::cell::Cell::new(false);
    let started = Instant::now();
    // Record the observations the production loop actually requests. Capturing
    // synchronously from the act emitter adds an unbudgeted read before settle
    // and can hide a premature second decision by waiting for the fixture.
    run_loop_with(
        "Press Compute once, then finish only after the result is ready.",
        "ICU Readiness Target",
        &brain,
        &|kind, data| events.lock().unwrap().push(json!({"kind":kind,"at":now(),"data":data})),
        &|| {
            let began = now();
            let result = cua::get_state(fixture.pid, fixture.target, 200);
            let finished = now();
            match &result {
                Ok(state) => {
                    let retained = without_pixels(state.clone());
                    observations.lock().unwrap().push(json!({"started_at":began,"at":finished,
                        "elapsed_ms":(finished-began)*1000.,"after_dispatch":dispatched.get(),
                        "busy":ui_readiness::busy(state),"state":retained}));
                    let mut first = action_sample.lock().unwrap();
                    if dispatched.get() && first.is_none() { *first = Some(retained); }
                }
                Err(error) => observations.lock().unwrap().push(json!({"started_at":began,"at":finished,
                    "elapsed_ms":(finished-began)*1000.,"after_dispatch":dispatched.get(),"error":error})),
            }
            result
        },
        &|action, state| {
            let result = dispatch_action(action, state, fixture.pid, fixture.target);
            if result.is_ok() { dispatched.set(true); }
            result
        },
        &|| false,
    );
    // Persist after the loop, so writing evidence does not delay a real poll.
    if let Some(sample) = action_sample.lock().unwrap().as_ref() {
        save(&fixture.dir.join("after-action-native-state.json"),sample);
    }
    let elapsed = started.elapsed().as_secs_f64();
    let state = fixture.state();
    let calls = brain.calls.lock().unwrap();
    let events = events.lock().unwrap();
    let report = json!({"mode":mode,"seconds":elapsed,"fixture":state,"decisions":*calls,
        "events":*events,"observations":*observations.lock().unwrap(),
        "after_action_native":*action_sample.lock().unwrap(),"pid":fixture.pid,
        "target_window_id":fixture.target,"decoy_window_id":fixture.decoy});
    save(&fixture.dir.join("result.json"), &report);
    eprintln!("native readiness evidence: {}", fixture.dir.display());
    assert_eq!(
        state["target_effects"], 1,
        "exactly one owned target action required: {report}"
    );
    assert_eq!(
        state["decoy_effects"], 0,
        "input reached a different window: {report}"
    );
    assert_eq!(
        state["ever_active"], false,
        "fixture took foreground: {report}"
    );
    assert_eq!(
        state["foreground_pids"],
        json!([fixture.front]),
        "foreground changed: {report}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e["kind"] == "act" && e["data"]["ok"] == true)
            .count(),
        1
    );
    assert!(
        elapsed < 10.,
        "readiness did not finish within its bounded budget: {report}"
    );
    if mode != "noeffect" {
        let sample = action_sample.lock().unwrap();
        let sample = sample
            .as_ref()
            .expect("successful action must have a native observation");
        let native_spinner = ["elements", "passive_elements"]
            .into_iter()
            .flat_map(|key| {
                sample
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .any(|row| {
                matches!(
                    row.get("role").and_then(Value::as_str),
                    Some("AXProgressIndicator" | "AXBusyIndicator")
                )
            });
        assert!(
            native_spinner,
            "native driver did not expose the ordinary AppKit spinner: {report}"
        );
    }
    match mode {
        "permanent" => {
            assert_eq!(state["busy"], true);
            assert_eq!(
                calls.len(),
                1,
                "busy state reached another model decision: {report}"
            );
            assert!(
                events.iter().any(|e| e["kind"] == "error"
                    && e["data"]["readiness"]
                        .as_str()
                        .is_some_and(|m| m.contains("budget"))),
                "missing typed readiness timeout: {report}"
            );
            assert!(!events.iter().any(|e| e["kind"] == "done"));
        }
        "delayed" => {
            assert_eq!(state["result"], "42");
            assert_eq!(calls.len(), 2, "unexpected model decisions: {report}");
            assert!(calls[1]["at"].as_f64().unwrap() >= state["completed_at"].as_f64().unwrap());
            assert!(
                events.iter().any(|e| e["kind"] == "done"),
                "unfinished state reached model: {report}"
            );
        }
        "noeffect" => {
            assert_eq!(state["result"], "0");
            assert_eq!(calls.len(), 2);
            assert!(
                events.iter().any(|e| e["kind"] == "done"),
                "unchanged native UI never became ready: {report}"
            );
        }
        _ => unreachable!(),
    }
}

#[test]
#[ignore = "requires an explicitly owned disposable GUI Mac and private driver socket"]
fn delayed_busy_result_precedes_second_decision() {
    exercise("delayed");
}

#[test]
#[ignore = "requires an explicitly owned disposable GUI Mac and private driver socket"]
fn permanent_busy_stops_before_second_decision() {
    exercise("permanent");
}

#[test]
#[ignore = "requires an explicitly owned disposable GUI Mac and private driver socket"]
fn unchanged_ui_finishes_without_touching_decoy() {
    exercise("noeffect");
}
