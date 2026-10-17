use computer_history::{
    capture::{Content, Recorder, Source},
    policy::{Behavior, Context, Rule, Website},
    store::{CaptureMode, Commit, Store},
    Result,
};
use std::sync::atomic::AtomicBool;
struct Fixture {
    context: Option<Context>,
    reads: usize,
    content: Option<Content>,
    valid: bool,
    during_identity: Option<Box<dyn FnOnce()>>,
    during_read: Option<Box<dyn FnOnce()>>,
    during_verify: Option<Box<dyn FnOnce()>>,
}
impl Default for Fixture {
    fn default() -> Self {
        Self {
            context: Some(Context {
                application: "fixture.app".into(),
                process_id: 12,
                window_id: 34,
                website: Website::NotWeb,
            }),
            reads: 0,
            valid: true,
            content: Some(Content {
                role: "AXTextField".into(),
                label: Some("Draft".into()),
                identifier: Some("entry".into()),
                value: Some("Fixture content".into()),
            }),
            during_identity: None,
            during_read: None,
            during_verify: None,
        }
    }
}
impl Source for Fixture {
    fn identify(&mut self) -> Result<Option<Context>> {
        if let Some(f) = self.during_identity.take() {
            f()
        }
        Ok(self.context.clone())
    }
    fn read(&mut self, _: &Context, _: CaptureMode) -> Result<Option<Content>> {
        self.reads += 1;
        if let Some(f) = self.during_read.take() {
            f()
        }
        Ok(self.content.clone())
    }
    fn verify(&mut self, _: &Context) -> Result<bool> {
        if let Some(f) = self.during_verify.take() {
            f()
        }
        Ok(self.valid)
    }
}
fn setup(mode: CaptureMode) -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("history")).unwrap();
    let mut settings = s.state().unwrap().settings;
    settings.observation.default_application_behavior = Behavior::Observe;
    settings.observation.default_url_behavior = Behavior::Observe;
    s.update(settings, None).unwrap();
    s.configure(true, 7, true).unwrap();
    let state = s.state().unwrap();
    s.capture_mode(mode, true, (&state.generation, state.revision))
        .unwrap();
    (d, s)
}
fn bytes(s: &Store) -> String {
    s.status().unwrap()["recent_activity_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| std::fs::read_to_string(p.as_str().unwrap()).unwrap())
        .collect()
}
fn sample(r: &mut Recorder, s: &Store, f: &mut Fixture) -> Option<Commit> {
    r.sample(s, f, &AtomicBool::new(false), |_, _, _| Ok(()))
        .unwrap()
}
#[test]
fn pre_discovery_pause_resume_invalidates_observation() {
    let (_d, s) = setup(CaptureMode::Values);
    let other = s.clone();
    let mut f = Fixture {
        during_identity: Some(Box::new(move || {
            other.pause(true).unwrap();
            other.pause(false).unwrap();
        })),
        ..Default::default()
    };
    assert_eq!(sample(&mut Recorder::default(), &s, &mut f), None);
    assert_eq!(f.reads, 0);
    assert!(bytes(&s).is_empty());
}
#[test]
fn blocked_apps_and_unknown_browsers_never_read_content() {
    for web in [false, true] {
        let (_d, s) = setup(CaptureMode::Values);
        let mut f = Fixture::default();
        if web {
            f.context.as_mut().unwrap().website = Website::Unknown;
        } else {
            let mut settings = s.state().unwrap().settings;
            settings.observation.blocklist.push(Rule::App {
                bundle_id: "fixture.app".into(),
            });
            s.update(settings, None).unwrap();
        }
        assert_eq!(sample(&mut Recorder::default(), &s, &mut f), None);
        assert_eq!(f.reads, 0);
        assert!(bytes(&s).is_empty());
    }
}
#[test]
fn modes_limit_payload_and_approved_content_feeds_the_sink() {
    for mode in [
        CaptureMode::ApplicationMetadata,
        CaptureMode::Semantic,
        CaptureMode::Values,
    ] {
        let (_d, s) = setup(mode);
        let mut f = Fixture::default();
        let mut seen = None;
        let result = Recorder::default()
            .sample(
                &s,
                &mut f,
                &AtomicBool::new(false),
                |context, _, payload| {
                    seen = Some((context.clone(), payload.clone()));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(result, Some(Commit::Written));
        let data = bytes(&s);
        assert_eq!(
            data.contains("Draft"),
            mode != CaptureMode::ApplicationMetadata
        );
        assert_eq!(
            data.contains("Fixture content"),
            mode == CaptureMode::Values
        );
        assert_eq!(
            f.reads,
            usize::from(mode != CaptureMode::ApplicationMetadata)
        );
        assert_eq!(seen.unwrap().0.application, "fixture.app");
    }
}
#[test]
fn consent_upgrades_are_separate_stale_safe_and_migrate_to_metadata() {
    let (_d, s) = setup(CaptureMode::ApplicationMetadata);
    let old = s.state().unwrap();
    assert!(s
        .capture_mode(CaptureMode::Values, false, (&old.generation, old.revision))
        .is_err());
    s.capture_mode(CaptureMode::Semantic, true, (&old.generation, old.revision))
        .unwrap();
    assert!(s
        .capture_mode(CaptureMode::Values, true, (&old.generation, old.revision))
        .is_err());
    let current = s.state().unwrap();
    s.capture_mode(
        CaptureMode::ApplicationMetadata,
        false,
        (&current.generation, current.revision),
    )
    .unwrap();
    let mut json = serde_json::to_value(s.state().unwrap()).unwrap();
    json.as_object_mut().unwrap().remove("captureMode");
    std::fs::write(s.root().join("settings.json"), json.to_string()).unwrap();
    assert_eq!(
        s.state().unwrap().capture_mode,
        CaptureMode::ApplicationMetadata
    );
}
#[test]
fn pause_clear_disable_or_scope_downgrade_during_content_read_blocks_both_sinks() {
    for action in ["pause", "clear", "disable", "scope"] {
        let (_d, s) = setup(CaptureMode::Values);
        let other = s.clone();
        let mut f = Fixture {
            during_read: Some(Box::new(move || match action {
                "pause" => {
                    other.pause(true).unwrap();
                    other.pause(false).unwrap();
                }
                "clear" => {
                    other.clear().unwrap();
                }
                "disable" => {
                    other.configure(false, 7, false).unwrap();
                }
                _ => {
                    let old = other.state().unwrap();
                    other
                        .capture_mode(
                            CaptureMode::ApplicationMetadata,
                            false,
                            (&old.generation, old.revision),
                        )
                        .unwrap();
                }
            })),
            ..Default::default()
        };
        let mut called = false;
        let result = Recorder::default()
            .sample(&s, &mut f, &AtomicBool::new(false), |_, _, _| {
                called = true;
                Ok(())
            })
            .unwrap();
        assert_eq!(result, Some(Commit::Stale));
        assert!(!called);
        assert!(bytes(&s).is_empty());
    }
}
#[test]
fn locks_secure_fields_changed_focus_and_shutdown_suppress_content() {
    for case in ["locked", "secure", "changed", "stop"] {
        let (_d, s) = setup(CaptureMode::Values);
        let mut f = Fixture::default();
        match case {
            "locked" => f.context = None,
            "secure" => f.content = None,
            "changed" => f.valid = false,
            _ => {}
        }
        let mut sink = false;
        let result = Recorder::default()
            .sample(&s, &mut f, &AtomicBool::new(case == "stop"), |_, _, _| {
                sink = true;
                Ok(())
            })
            .unwrap();
        assert_ne!(result, Some(Commit::Written));
        assert!(!sink);
        assert!(bytes(&s).is_empty());
    }
}
#[test]
fn dedup_does_not_hide_changes_after_pause_or_secure_focus() {
    let (_d, s) = setup(CaptureMode::Values);
    let mut f = Fixture::default();
    let mut r = Recorder::default();
    assert_eq!(sample(&mut r, &s, &mut f), Some(Commit::Written));
    assert_eq!(sample(&mut r, &s, &mut f), None);
    s.pause(true).unwrap();
    s.pause(false).unwrap();
    assert_eq!(sample(&mut r, &s, &mut f), Some(Commit::Written));
    let normal = f.content.take();
    assert_eq!(sample(&mut r, &s, &mut f), None);
    f.content = normal;
    assert_eq!(sample(&mut r, &s, &mut f), Some(Commit::Written));
    assert_eq!(bytes(&s).lines().count(), 3);
}
#[test]
fn values_are_bounded_by_unicode_characters() {
    let (_d, s) = setup(CaptureMode::Values);
    let mut f = Fixture::default();
    f.content.as_mut().unwrap().value = Some("日".repeat(4000));
    sample(&mut Recorder::default(), &s, &mut f);
    let value: serde_json::Value = serde_json::from_str(bytes(&s).trim()).unwrap();
    assert_eq!(
        value["payload"]["value"].as_str().unwrap().chars().count(),
        2048
    );
    assert_eq!(value["payload"]["truncated"]["value"], true);
    assert_eq!(value["payload"]["secure"], false);
}
#[test]
fn storage_caps_mark_partial_fields_but_not_complete_boundary_values() {
    for extra in [0, 1] {
        let (_d, s) = setup(CaptureMode::Values);
        let mut f = Fixture::default();
        let c = f.content.as_mut().unwrap();
        c.role = "R".repeat(128 + extra);
        c.label = Some("L".repeat(512 + extra));
        c.identifier = Some("I".repeat(256 + extra));
        c.value = Some("日".repeat(2048 + extra));
        sample(&mut Recorder::default(), &s, &mut f);
        let row: serde_json::Value = serde_json::from_str(bytes(&s).trim()).unwrap();
        for field in ["role", "label", "identifier", "value"] {
            assert_eq!(row["payload"]["truncated"][field], extra == 1);
        }
    }
}
#[test]
fn persisted_domain_rules_are_normalized_when_loaded() {
    let (_d, s) = setup(CaptureMode::Values);
    let mut state = s.state().unwrap();
    state.settings.observation.blocklist.push(Rule::Url {
        url_domain: "BÜCHER.Example.".into(),
    });
    std::fs::write(
        s.root().join("settings.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let mut f = Fixture::default();
    f.context.as_mut().unwrap().website =
        Website::Url("https://xn--bcher-kva.example/private".into());
    assert_eq!(sample(&mut Recorder::default(), &s, &mut f), None);
    assert_eq!(f.reads, 0);
}
#[test]
fn legacy_sink_ticket_rechecks_the_barrier_after_binding() {
    let (_d, s) = setup(CaptureMode::ApplicationMetadata);
    let context = Fixture::default().context.unwrap();
    let ticket = s
        .bind(s.observation().unwrap().unwrap(), context.clone())
        .unwrap()
        .unwrap();
    s.pause(true).unwrap();
    s.pause(false).unwrap();
    assert_eq!(
        s.with_ticket(ticket, &context, || panic!("stale sink called"))
            .unwrap(),
        None::<()>
    );
}
#[test]
fn history_and_memory_share_the_pre_read_timestamp() {
    let (_d, s) = setup(CaptureMode::Values);
    let mut timestamp = 0;
    let mut source = Fixture {
        during_identity: Some(Box::new(|| {
            std::thread::sleep(std::time::Duration::from_millis(25))
        })),
        ..Default::default()
    };
    Recorder::default()
        .sample(&s, &mut source, &AtomicBool::new(false), |_, ts, _| {
            timestamp = ts;
            Ok(())
        })
        .unwrap();
    let row: serde_json::Value = serde_json::from_str(bytes(&s).trim()).unwrap();
    assert_eq!(row["timestamp_unix_ms"], timestamp);
}
#[test]
fn shutdown_during_native_verification_does_not_reach_either_sink() {
    use std::sync::{atomic::Ordering, Arc};
    let (_d, s) = setup(CaptureMode::Values);
    let stop = Arc::new(AtomicBool::new(false));
    let sender = stop.clone();
    let mut source = Fixture {
        during_verify: Some(Box::new(move || sender.store(true, Ordering::SeqCst))),
        ..Default::default()
    };
    assert_eq!(
        Recorder::default()
            .sample(&s, &mut source, &stop, |_, _, _| panic!(
                "shutdown reached sink"
            ))
            .unwrap(),
        Some(Commit::Stale)
    );
    assert!(bytes(&s).is_empty());
}

#[test]
fn producer_read_keeps_its_ticket_timestamp_and_rechecks_later_commit() {
    let (_d, store) = setup(CaptureMode::Values);
    let context = Fixture::default().context.unwrap();
    let ticket = store.bind(store.observation().unwrap().unwrap(), context.clone()).unwrap().unwrap();
    let at = ticket.timestamp();
    let value = store.read_with_ticket(&ticket, &context, || Ok("Public source value")).unwrap().unwrap();
    store.commit(ticket, &context, "ui.value", serde_json::json!({"value":value})).unwrap();
    let row: serde_json::Value = serde_json::from_str(bytes(&store).trim()).unwrap();
    assert_eq!(row["timestamp_unix_ms"], at);
    let ticket = store.bind(store.observation().unwrap().unwrap(), context.clone()).unwrap().unwrap();
    assert_eq!(store.read_with_ticket(&ticket, &context, || Ok("Previously allowed")).unwrap(), Some("Previously allowed"));
    store.pause(true).unwrap();
    let before = bytes(&store);
    assert!(matches!(store.commit(ticket, &context, "ui.value", serde_json::json!({"value":"Previously allowed"})).unwrap(), Commit::Stale | Commit::Suppressed));
    assert_eq!(bytes(&store), before);
}

#[test]
fn producer_read_does_not_open_a_revoked_or_wrong_window_value() {
    let (_d, store) = setup(CaptureMode::Values);
    let context = Fixture::default().context.unwrap();
    let ticket = store.bind(store.observation().unwrap().unwrap(), context.clone()).unwrap().unwrap();
    let mut other = context.clone();other.window_id += 1;
    assert_eq!(store.read_with_ticket(&ticket, &other, || panic!("wrong window read")).unwrap(), None::<()>);
    store.pause(true).unwrap();store.pause(false).unwrap();
    assert_eq!(store.read_with_ticket(&ticket, &context, || panic!("revoked value read")).unwrap(), None::<()>);
}

#[test]
fn notification_sample_uses_read_time_without_renewing_consent() {
    let (_d, store) = setup(CaptureMode::Values);
    let context = Fixture::default().context.unwrap();
    let mut ticket = store.bind(store.observation().unwrap().unwrap(), context.clone()).unwrap().unwrap();
    let before = ticket.timestamp();
    let actual_read = store.sample_with_ticket(&mut ticket, &context, || {
        std::thread::sleep(std::time::Duration::from_millis(5));
        Ok(computer_history::now_ms())
    }).unwrap().unwrap();
    assert!(ticket.timestamp() >= actual_read && ticket.timestamp() > before);
    store.pause(true).unwrap();
    store.pause(false).unwrap();
    assert_eq!(store.sample_with_ticket(&mut ticket, &context, || panic!("revoked sample read")).unwrap(), None::<()>);
    assert_eq!(store.commit(ticket, &context, "ui.value", serde_json::json!({"value":"stale sample"})).unwrap(), Commit::Stale);
    assert!(bytes(&store).is_empty());
}
