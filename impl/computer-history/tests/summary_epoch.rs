use computer_history::{
    now_ms,
    policy::{Behavior, Context, Website},
    store::{Commit, Store},
    Error,
};
use serde_json::{json, Value};

fn setup() -> (tempfile::TempDir, Store, Context) {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path().join("history")).unwrap();
    let mut settings = store.state().unwrap().settings;
    settings.observation.default_application_behavior = Behavior::Observe;
    store.update(settings, None).unwrap();
    store.configure(true, 7, true).unwrap();
    let context = Context {
        application: "fixture.app".into(),
        process_id: 12,
        window_id: 34,
        website: Website::NotWeb,
    };
    (root, store, context)
}
fn rows(store: &Store) -> Vec<Value> {
    std::fs::read_dir(store.root().join("activity"))
        .unwrap()
        .map(Result::unwrap)
        .flat_map(|p| {
            std::fs::read_to_string(p.path())
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn sharing_approval_is_a_capture_boundary_not_a_wall_clock_cutoff() {
    let (_root, store, context) = setup();
    let now = now_ms();
    let first = store.begin(context.clone(), now).unwrap().unwrap();
    store
        .commit(
            first,
            &context,
            "ui.snapshot",
            json!({"value":"before approval","summary_epoch":"forged"}),
        )
        .unwrap();
    let pending = store.begin(context.clone(), now).unwrap().unwrap();
    let before = store.state().unwrap();
    let epoch = store
        .renew_summary_epoch((&before.generation, before.revision), |_, epoch| {
            Ok(epoch.to_owned())
        })
        .unwrap();
    assert_eq!(
        store
            .commit(pending, &context, "ui.snapshot", json!({}))
            .unwrap(),
        Commit::Stale
    );
    let after = store
        .begin(context.clone(), now.saturating_sub(1000))
        .unwrap()
        .unwrap();
    store
        .commit(
            after,
            &context,
            "ui.snapshot",
            json!({"value":"after approval with earlier timestamp","summary_epoch":"forged"}),
        )
        .unwrap();
    let records = rows(&store);
    assert_eq!(records.len(), 2);
    let first = records
        .iter()
        .find(|r| r["payload"]["value"] == "before approval")
        .unwrap();
    assert!(first["payload"].get("summary_epoch").is_none());
    let second = records
        .iter()
        .find(|r| r["payload"]["value"] == "after approval with earlier timestamp")
        .unwrap();
    assert_eq!(second["payload"]["summary_epoch"], epoch);
    assert!(
        second["timestamp_unix_ms"].as_u64().unwrap()
            < first["timestamp_unix_ms"].as_u64().unwrap()
    );
}

#[test]
fn failed_or_stale_sharing_approval_never_changes_the_epoch() {
    let (_root, store, _) = setup();
    let state = store.state().unwrap();
    let error: computer_history::Result<()> = store
        .renew_summary_epoch((&state.generation, state.revision), |_, _| {
            Err(Error::invalid("policy write failed"))
        });
    assert!(error.is_err());
    assert!(store.state().unwrap().summary_epoch.is_empty());
    store.pause(true).unwrap();
    assert!(store
        .renew_summary_epoch::<()>((&state.generation, state.revision), |_, _| panic!(
            "stale approval reached its writer"
        ))
        .is_err());
}

#[test]
fn clear_invalidates_derived_work_and_acknowledges_after_cleanup() {
    let (_root, store, context) = setup();
    let before = store.state().unwrap();
    let epoch = store
        .renew_summary_epoch((&before.generation, before.revision), |_, epoch| {
            Ok(epoch.to_owned())
        })
        .unwrap();
    let ticket = store.begin(context.clone(), now_ms()).unwrap().unwrap();
    store
        .commit(ticket, &context, "ui.snapshot", json!({"value":"clear me"}))
        .unwrap();
    let derived = store.root().join("derived-test.txt");
    std::fs::write(&derived, "clear me").unwrap();
    store
        .clear_with(|| {
            assert!(rows(&store).is_empty());
            std::fs::remove_file(&derived).unwrap();
            Ok(())
        })
        .unwrap();
    assert!(!derived.exists());
    assert!(store.state().unwrap().paused);
    assert!(store.begin(context.clone(), now_ms()).unwrap().is_none());
    store
        .with_state(|state| {
            assert_ne!(state.summary_epoch, epoch);
            Ok(())
        })
        .unwrap();
}

#[test]
fn recording_pause_preserves_sharing_but_scope_changes_revoke_it() {
    let (_root, store, _) = setup();
    let before = store.state().unwrap();
    let epoch = store
        .renew_summary_epoch((&before.generation, before.revision), |_, epoch| {
            Ok(epoch.to_owned())
        })
        .unwrap();
    store.pause(true).unwrap();
    store.pause(false).unwrap();
    assert_eq!(store.state().unwrap().summary_epoch, epoch);
    let before = store.state().unwrap();
    store
        .capture_mode(
            computer_history::store::CaptureMode::Values,
            true,
            (&before.generation, before.revision),
        )
        .unwrap();
    assert_ne!(store.state().unwrap().summary_epoch, epoch);
    let before = store.state().unwrap();
    let epoch = before.summary_epoch.clone();
    let mut settings = before.settings;
    settings.observation.default_application_behavior = Behavior::DoNotObserve;
    store.update(settings, None).unwrap();
    assert_ne!(store.state().unwrap().summary_epoch, epoch);
}
