//! Explicit fixture harness. Launch only on an owned disposable test desktop.
#[cfg(target_os = "macos")]
fn main() {
    use computer_history::{
        capture::{Recorder, Source},
        macos::MacSource,
        policy::Behavior,
        store::{CaptureMode, Store},
    };
    use serde_json::{json, Value};
    use std::{path::PathBuf, sync::atomic::AtomicBool, time::Duration};
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("private fixture directory"),
    );
    assert!(path.is_absolute());
    let store = Store::open(path.join("history")).unwrap();
    let owner = store.attach_collector("native_fixture").unwrap();
    let mut recorder = Recorder::default();
    let mut source = MacSource::default();
    let mut last = Value::Null;
    loop {
        let command: Value = std::fs::read(path.join("request.json"))
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or(Value::Null);
        if !command.is_null() && command["id"] != last {
            last = command["id"].clone();
            let result = (|| -> computer_history::Result<Value> {
                match command["op"].as_str().unwrap_or("") {
                    "setup" => {
                        let mut settings = store.state()?.settings;
                        settings.observation.default_application_behavior = Behavior::Observe;
                        settings.observation.default_url_behavior = Behavior::Observe;
                        store.update(settings, None)?;
                        store.configure(true, 7, true)?;
                        Ok(json!(true))
                    }
                    "mode" => {
                        let mode: CaptureMode =
                            serde_json::from_value(command["mode"].clone()).unwrap();
                        let s = store.state()?;
                        store.capture_mode(mode, true, (&s.generation, s.revision))?;
                        Ok(json!(true))
                    }
                    "settings" => {
                        store.update(
                            serde_json::from_value(command["settings"].clone()).unwrap(),
                            None,
                        )?;
                        Ok(json!(true))
                    }
                    "sample" => {
                        let mut sink = Value::Null;
                        let outcome = recorder.sample(
                            &store,
                            &mut source,
                            &AtomicBool::new(false),
                            |c, _, p| {
                                sink = json!({"application":c.application,"payload":p});
                                Ok(())
                            },
                        )?;
                        Ok(json!({"outcome":outcome,"sink":sink,"skip":source.last_skip}))
                    }
                    "status" => store.status(),
                    "focus_status" => Ok(computer_history::macos::focus_status()),
                    "browser" => {
                        let state = store.state()?;
                        store.browser_capture(
                            command["enabled"].as_bool().unwrap(),
                            true,
                            (&state.generation, state.revision),
                        )?;
                        Ok(json!(true))
                    }
                    "browser_permission" => {
                        // The native permission API must run off the main thread.
                        std::thread::spawn(|| {
                            computer_history::macos::request_browser_permission("com.google.Chrome")
                        })
                        .join()
                        .expect("permission thread")?;
                        Ok(json!(true))
                    }
                    "identity" => Ok(match source.identify()? {
                        Some(c) => {
                            json!({"application":c.application,"pid":c.process_id,"window":c.window_id})
                        }
                        None => Value::Null,
                    }),
                    "pause" => {
                        store.pause(command["paused"].as_bool().unwrap())?;
                        Ok(json!(true))
                    }
                    "quit" => std::process::exit(0),
                    _ => Err(computer_history::Error::invalid("Unknown probe command")),
                }
            })();
            owner
                .heartbeat(
                    "native_fixture",
                    result.as_ref().err().map(|e| e.message.as_str()),
                )
                .unwrap();
            let reply = match result {
                Ok(v) => json!({"id":last,"result":v}),
                Err(e) => json!({"id":last,"error":e}),
            };
            let pending = path.join("reply.tmp");
            std::fs::write(&pending, reply.to_string()).unwrap();
            std::fs::rename(pending, path.join("reply.json")).unwrap();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("Native probe requires macOS")
}
