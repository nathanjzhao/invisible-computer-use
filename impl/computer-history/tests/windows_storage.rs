#![cfg(windows)]

use computer_history::store::Store;
use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt, time::{Duration, Instant}};

#[test]
fn settings_replace_survives_a_transient_external_reader() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("history")).unwrap();
    let reader = OpenOptions::new().read(true).share_mode(3)
        .open(store.root().join("settings.json")).unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(120));
        drop(reader);
    });
    let result = store.configure(true, 7, true);
    release.join().unwrap();
    assert!(result.unwrap().enabled);
    assert!(store.state().unwrap().enabled);
}

#[test]
fn persistent_external_reader_preserves_the_prior_settings() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("history")).unwrap();
    let path = store.root().join("settings.json");
    let before = std::fs::read(&path).unwrap();
    let reader = OpenOptions::new().read(true).share_mode(3).open(&path).unwrap();
    let started = Instant::now();
    let error = store.configure(true, 7, true).unwrap_err();
    assert_eq!(error.code, "computerHistoryStorageUnavailable");
    assert!(started.elapsed() < Duration::from_secs(3), "Replacement must remain bounded");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!std::fs::read_dir(store.root()).unwrap().any(|entry|
        entry.unwrap().path().extension().is_some_and(|extension| extension == "tmp")));
    drop(reader);
    assert!(!store.state().unwrap().enabled);
}
