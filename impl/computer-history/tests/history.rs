use computer_history::{
    mcp::Session,
    now_ms,
    policy::{Behavior, Context, Decision, Rule, Settings, Website},
    store::{Commit, Store},
};
use serde_json::json;
use std::{
    sync::{Arc, Barrier},
    thread,
};
fn context(app: &str, website: Website) -> Context {
    Context {
        application: app.into(),
        process_id: 11,
        window_id: 22,
        website,
    }
}
fn configured() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("history")).unwrap();
    let mut settings = Settings::default();
    settings.observation.default_application_behavior = Behavior::Observe;
    settings.observation.default_url_behavior = Behavior::Observe;
    store.update(settings, None).unwrap();
    store.configure(true, 7, true).unwrap();
    (dir, store)
}
#[test]
fn new_stores_default_to_two_days_and_existing_retention_is_preserved() {
    let dir=tempfile::tempdir().unwrap();let root=dir.path().join("history");
    let store=Store::open(root.clone()).unwrap();assert_eq!(store.state().unwrap().retention_days,2);
    store.configure(true,7,true).unwrap();drop(store);
    let reopened=Store::open(root).unwrap();assert_eq!(reopened.state().unwrap().retention_days,7);
}
#[test]
fn restart_and_clear_remove_only_owned_interrupted_replacement_files() {
    let (dir,store)=configured();let root=store.root().to_owned();
    let temp=root.join("activity").join(format!("replace-{}.tmp","a".repeat(48)));
    let outside=dir.path().join("unrelated.txt");std::fs::write(&outside,b"preserve this").unwrap();
    let unrelated=root.join("activity/user-note.tmp");std::fs::write(&unrelated,b"preserve this too").unwrap();
    std::fs::write(&temp,b"unfinished private activity").unwrap();
    let reopened=Store::open(root.clone()).unwrap();assert!(!temp.exists());
    std::fs::write(&temp,b"another unfinished activity").unwrap();reopened.clear().unwrap();assert!(!temp.exists());
    std::fs::write(&temp,b"an interrupted retention rewrite").unwrap();reopened.prune(now_ms()).unwrap();assert!(!temp.exists());
    assert_eq!(std::fs::read(&outside).unwrap(),b"preserve this");assert!(unrelated.exists());
    #[cfg(unix)]{
        std::os::unix::fs::symlink(&outside,&temp).unwrap();reopened.clear().unwrap();
        assert!(!std::fs::symlink_metadata(&temp).is_ok());assert_eq!(std::fs::read(outside).unwrap(),b"preserve this");
    }
}
#[test]
fn app_and_domain_rules_are_enforced_before_payload_is_written() {
    let (_dir, store) = configured();
    let mut rules = store.state().unwrap().settings;
    rules.observation.blocklist = vec![
        Rule::App {
            bundle_id: "private.app".into(),
        },
        Rule::Url {
            url_domain: "example.com".into(),
        },
    ];
    store.update(rules, None).unwrap();
    for (app, url) in [
        ("private.app", Website::NotWeb),
        (
            "browser",
            Website::Url("https://sub.example.com/path?secret=abc".into()),
        ),
        ("browser", Website::Unknown),
    ] {
        assert!(store.begin(context(app, url), now_ms()).unwrap().is_none());
    }
    let ctx = context(
        "browser",
        Website::Url("https://example.com.evil.invalid/?secret=not-persisted".into()),
    );
    let ticket = store.begin(ctx.clone(), now_ms()).unwrap().unwrap();
    assert_eq!(
        store
            .commit(ticket, &ctx, "focus", json!({"label":"Allowed label"}))
            .unwrap(),
        Commit::Written
    );
    let status = store.status().unwrap();
    let path = status["recent_activity_files"][0].as_str().unwrap();
    let content = std::fs::read_to_string(path).unwrap();
    assert!(!content.contains("secret"));
    assert!(content.contains("example.com.evil.invalid"));
    assert!(content.contains("Allowed label"));
}
#[test]
fn domain_canonicalization_handles_unicode_boundaries_and_rejects_url_rules() {
    use computer_history::policy::{domain, web_host};
    assert_eq!(domain("BÜCHER.Example.").unwrap(), "xn--bcher-kva.example");
    assert_eq!(
        web_host("https://user:pass@BÜCHER.example:8443/a?token=x").as_deref(),
        Some("xn--bcher-kva.example")
    );
    for invalid in [
        "https://example.com",
        "example.com/path",
        "user@example.com",
        "example.com:443",
        " example.com",
        "example..com",
        "*.example.com",
        "example.com?secret",
        "-bad.example",
        "example.com\\evil",
    ] {
        assert!(domain(invalid).is_err(), "{invalid}");
    }
    let mut s = Settings::default();
    s.observation.default_application_behavior = Behavior::Observe;
    s.observation.allowlist.push(Rule::Url {
        url_domain: "example.com".into(),
    });
    let s = s.validated().unwrap();
    assert_eq!(
        s.evaluate(&context(
            "browser",
            Website::Url("https://sub.example.com/a".into())
        )),
        Decision::Allowed
    );
    assert_eq!(
        s.evaluate(&context(
            "browser",
            Website::Url("https://evil-example.com/".into())
        )),
        Decision::WebsiteBlocked
    );
    assert_eq!(
        s.evaluate(&context(
            "browser",
            Website::Url("file:///example.com".into())
        )),
        Decision::UnknownWebsite
    );
}

#[test]
fn settings_wire_keys_match_the_reference_acronym_case() {
    let settings = serde_json::to_value(Settings::default()).unwrap();
    assert_eq!(
        settings["observation"]["defaultURLBehavior"],
        "do_not_observe"
    );
    assert!(settings["observation"].get("defaultUrlBehavior").is_none());
    let input = json!({"observation":{"defaultApplicationBehavior":"observe","defaultURLBehavior":"do_not_observe","allowlist":[],"blocklist":[]}});
    let parsed: Settings = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), input);
}
#[test]
fn pause_and_settings_updates_are_barriers_to_already_started_capture() {
    let (_dir, store) = configured();
    let ctx = context("fixture.app", Website::NotWeb);
    let ticket = store.begin(ctx.clone(), now_ms()).unwrap().unwrap();
    let other = Store::open(store.root().into()).unwrap();
    other.pause(true).unwrap();
    assert_eq!(
        store
            .commit(
                ticket,
                &ctx,
                "focus",
                json!({"private":"must not reach disk"})
            )
            .unwrap(),
        Commit::Stale
    );
    assert!(store.status().unwrap()["recent_activity_files"]
        .as_array()
        .unwrap()
        .is_empty());
    other.pause(false).unwrap();
    let ticket = store.begin(ctx.clone(), now_ms()).unwrap().unwrap();
    let mut settings = other.state().unwrap().settings;
    settings.observation.blocklist.push(Rule::App {
        bundle_id: "fixture.app".into(),
    });
    other.update(settings, None).unwrap();
    assert_eq!(
        store
            .commit(
                ticket,
                &ctx,
                "focus",
                json!({"private":"still not recorded"})
            )
            .unwrap(),
        Commit::Stale
    );
}
#[test]
fn disabled_is_distinct_from_paused_and_models_cannot_enable_capture() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("history")).unwrap();
    let mut api = Session::new(store.clone());
    assert_eq!(
        api.call("computer_history_status", json!({})).unwrap()["state"],
        "disabled"
    );
    assert_eq!(
        api.call("computer_history_resume", json!({}))
            .unwrap_err()
            .code,
        "computerHistoryDisabled"
    );
    assert!(api.call("enable", json!({"approved":true})).is_err());
    assert!(store.configure(true, 7, false).is_err());
    store.configure(true, 7, true).unwrap();
    api.call("computer_history_pause", json!({})).unwrap();
    assert_eq!(store.status().unwrap()["state"], "paused");
    store.configure(false, 7, false).unwrap();
    assert_eq!(store.status().unwrap()["state"], "disabled");
    assert!(api.call("computer_history_resume", json!({})).is_err());
}
#[test]
fn settings_replacement_requires_a_fresh_read_and_preserves_no_partial_updates() {
    let (_dir, store) = configured();
    let mut api = Session::new(store.clone());
    let settings = serde_json::to_value(store.state().unwrap().settings).unwrap();
    assert_eq!(
        api.call("computer_history_update_settings", settings.clone())
            .unwrap_err()
            .code,
        "computerHistorySettingsNotRead"
    );
    api.call("computer_history_get_settings", json!({}))
        .unwrap();
    store.pause(true).unwrap();
    assert_eq!(
        api.call("computer_history_update_settings", settings.clone())
            .unwrap_err()
            .code,
        "computerHistorySettingsChanged"
    );
    api.call("computer_history_get_settings", json!({}))
        .unwrap();
    assert!(api
        .call(
            "computer_history_update_settings",
            json!({"observation":{"allowlist":[]}})
        )
        .is_err());
    assert_eq!(
        serde_json::to_value(store.state().unwrap().settings).unwrap(),
        settings
    );
}
#[test]
fn changed_context_and_recreated_store_cannot_reuse_capture_tickets() {
    let (_dir, store) = configured();
    let ctx = context("fixture.app", Website::NotWeb);
    let ticket = store.begin(ctx.clone(), now_ms()).unwrap().unwrap();
    let mut changed = ctx.clone();
    changed.window_id = 23;
    assert_eq!(
        store
            .commit(ticket, &changed, "focus", json!({"no":"cross-window data"}))
            .unwrap(),
        Commit::Stale
    );
    let ticket = store.begin(ctx.clone(), now_ms()).unwrap().unwrap();
    std::fs::remove_file(store.root().join("settings.json")).unwrap();
    let replacement = Store::open(store.root().into()).unwrap();
    replacement.configure(true, 7, true).unwrap();
    let mut settings = Settings::default();
    settings.observation.default_application_behavior = Behavior::Observe;
    replacement.update(settings, None).unwrap();
    assert_eq!(
        store
            .commit(ticket, &ctx, "focus", json!({"no":"stale data"}))
            .unwrap(),
        Commit::Stale
    );
}
#[test]
fn retention_removes_only_expired_owned_segments() {
    let (_dir, store) = configured();
    let activity = store.root().join("activity");
    std::fs::write(activity.join("events-00000001-000.jsonl"), "fixture").unwrap();
    std::fs::write(activity.join("user-notes.jsonl"), "keep").unwrap();
    assert_eq!(store.prune(now_ms()).unwrap(), 1);
    assert_eq!(
        std::fs::read_to_string(activity.join("user-notes.jsonl")).unwrap(),
        "keep"
    );
}
#[test]
fn concurrent_settings_and_capture_never_write_after_pause_acknowledgement() {
    let (_dir, store) = configured();
    let barrier = Arc::new(Barrier::new(2));
    let writer = store.clone();
    let ready = barrier.clone();
    let ctx = context("fixture.app", Website::NotWeb);
    let thread = thread::spawn(move || {
        let ticket = writer.begin(ctx.clone(), now_ms()).unwrap().unwrap();
        ready.wait();
        ready.wait();
        writer
            .commit(ticket, &ctx, "focus", json!({"no":"late write"}))
            .unwrap()
    });
    barrier.wait();
    store.pause(true).unwrap();
    barrier.wait();
    assert_eq!(thread.join().unwrap(), Commit::Stale);
    assert!(store.status().unwrap()["recent_activity_files"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[cfg(unix)]
#[test]
fn private_files_and_symlink_refusal() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (dir, store) = configured();
    let settings = store.root().join("settings.json");
    assert_eq!(
        std::fs::metadata(&settings).unwrap().permissions().mode() & 0o077,
        0
    );
    std::fs::remove_file(&settings).unwrap();
    let outside = dir.path().join("outside");
    std::fs::write(&outside, "do not touch").unwrap();
    symlink(&outside, &settings).unwrap();
    assert!(store.pause(true).is_err());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "do not touch");
}

#[test]
fn retention_edits_preserve_disabled_state_and_refuse_stale_revisions() {
    let (_dir, store) = configured();
    let s = store.state().unwrap();
    store.configure(false, 7, false).unwrap();
    assert_eq!(
        store
            .retention(2, (&s.generation, s.revision))
            .unwrap_err()
            .code,
        "computerHistorySettingsChanged"
    );
    let s = store.state().unwrap();
    let s = store.retention(2, (&s.generation, s.revision)).unwrap();
    assert!(!s.enabled);
    assert_eq!(s.retention_days, 2);
}

#[test]
fn collector_status_tracks_real_ownership_and_clear_invalidates_pending_capture() {
    let (_dir, store) = configured();
    assert_eq!(store.status().unwrap()["capturing"], false);
    let owner = store.attach_collector("fixture").unwrap();
    assert_eq!(store.status().unwrap()["capturing"], true);
    assert!(store.attach_collector("second").is_err());
    owner
        .heartbeat("fixture", Some("fixture sensor unavailable"))
        .unwrap();
    assert_eq!(store.status().unwrap()["capturing"], false);
    drop(owner);
    assert!(store.status().unwrap()["collector"].is_null());
    let ctx = context("fixture.app", Website::NotWeb);
    let ticket = store.begin(ctx.clone(), now_ms()).unwrap().unwrap();
    store.clear().unwrap();
    assert_eq!(
        store.commit(ticket, &ctx, "focus", json!({})).unwrap(),
        Commit::Stale
    );
}
#[test]
fn a_pause_waits_for_a_contended_store_lock_without_losing_the_update() {
    use fs2::FileExt;
    use std::{fs::OpenOptions, sync::mpsc, thread, time::Duration};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("history")).unwrap();
    let held = OpenOptions::new()
        .read(true)
        .write(true)
        .open(store.root().join("history.lock"))
        .unwrap();
    held.lock_exclusive().unwrap();
    let contender = OpenOptions::new()
        .read(true)
        .write(true)
        .open(store.root().join("history.lock"))
        .unwrap();
    let busy = contender.try_lock_exclusive().unwrap_err();
    assert_eq!(
        busy.raw_os_error(),
        fs2::lock_contended_error().raw_os_error()
    );
    eprintln!(
        "native lock contention: raw={:?}, kind={:?}",
        busy.raw_os_error(),
        busy.kind()
    );
    let (started, ready) = mpsc::channel();
    let (done, result) = mpsc::channel();
    let peer = store.clone();
    let worker = thread::spawn(move || {
        started.send(()).unwrap();
        done.send(peer.pause(true)).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        result.recv_timeout(Duration::from_millis(80)).is_err(),
        "a contended update must wait, not return a storage error"
    );
    FileExt::unlock(&held).unwrap();
    result
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert!(store.state().unwrap().paused);
}
