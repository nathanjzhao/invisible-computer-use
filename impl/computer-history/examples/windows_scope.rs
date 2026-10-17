//! Component preflight only. Production signed-app acceptance is separate.
#[cfg(windows)]
fn main() {
    use computer_history::{
        capture::Recorder,
        policy::{Rule, Settings},
        store::{CaptureMode, Store},
    };
    use std::{
        path::PathBuf,
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };
    if std::env::var("ICU_DISPOSABLE_HOST").as_deref() != Ok("1") {
        panic!("Owned Windows test host required")
    }
    let root = PathBuf::from(std::env::args().nth(1).expect("Owned output root"));
    assert!(root.starts_with(r"C:\ICUAudit\round18"));
    let store = Store::open(root.join("history")).unwrap();
    let mut settings = Settings::default();
    settings.observation.allowlist.push(Rule::App {
        bundle_id: "HistoryFixture.exe".into(),
    });
    store.update(settings, None).unwrap();
    let state = store.state().unwrap();
    store
        .capture_mode(
            CaptureMode::Values,
            true,
            (&state.generation, state.revision),
        )
        .unwrap();
    store.configure(true, 7, true).unwrap();
    let _owner = store.attach_collector("windows_uia_component").unwrap();
    let stop = AtomicBool::new(false);
    let mut source = computer_history::windows::WindowsSource::default();
    let mut recorder = Recorder::default();
    let deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < deadline {
        match recorder.sample(&store, &mut source, &stop, |_, _, _| Ok(())) {
            Ok(value) => {
                if let Some(value) = value {
                    println!("{value:?}")
                }
            }
            Err(error) => eprintln!("{}", error.code),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("{}", store.status().unwrap());
}
#[cfg(not(windows))]
fn main() {
    panic!("Windows-only native preflight")
}
