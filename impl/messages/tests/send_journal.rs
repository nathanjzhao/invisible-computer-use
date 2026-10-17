use icu_messages::{
    native_delivery::Native,
    send::{Delivery, Outbound, Target, Transport},
    send_journal::{Journal, Outcome},
};
use std::{
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
fn fp() -> String {
    "a".repeat(64)
}
#[test]
fn inflight_and_uncertain_attempts_block_retries_after_process_exit() {
    if let Some(path) = std::env::var_os("ICU_JOURNAL_CRASH_FIXTURE") {
        let journal = Journal::open(path.into()).unwrap();
        let _attempt = journal.begin(&fp(), &AtomicBool::new(false)).unwrap();
        std::process::exit(31);
    }
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("journal");
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inflight_and_uncertain_attempts_block_retries_after_process_exit",
        ])
        .env("ICU_JOURNAL_CRASH_FIXTURE", &state)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(result.code(), Some(31));
    let journal = Journal::open(state.clone()).unwrap();
    assert_eq!(
        journal
            .begin(&fp(), &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        "messagesSendOutcomeUncertain"
    );
    let pending = journal.pending().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].outcome, Outcome::InFlight);
    journal.reconcile(&pending[0].id, Outcome::NotSent).unwrap();
    let attempt = journal.begin(&fp(), &AtomicBool::new(false)).unwrap();
    journal
        .finish(
            attempt,
            Outcome::Uncertain,
            Some("lost acknowledgement".into()),
        )
        .unwrap();
    drop(journal);
    let journal = Journal::open(state).unwrap();
    assert!(journal.begin(&fp(), &AtomicBool::new(false)).is_err());
}
#[test]
fn active_attempt_cannot_be_reconciled_and_completed_outcomes_release_the_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path().join("state")).unwrap();
    let attempt = journal.begin(&fp(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        journal
            .reconcile(attempt.id(), Outcome::NotSent)
            .unwrap_err()
            .code,
        "messagesSendStillActive"
    );
    journal.finish(attempt, Outcome::Accepted, None).unwrap();
    assert!(journal.pending().unwrap().is_empty());
    let attempt = journal.begin(&fp(), &AtomicBool::new(false)).unwrap();
    journal.finish(attempt, Outcome::NotSent, None).unwrap();
    assert!(journal.pending().unwrap().is_empty());
}
#[test]
fn corrupt_or_symlinked_journal_never_starts_an_attempt() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::open(dir.path().join("state")).unwrap();
    let a = journal.begin(&fp(), &AtomicBool::new(false)).unwrap();
    let id = a.id().to_owned();
    journal.finish(a, Outcome::Accepted, None).unwrap();
    let path = journal.directory().join(format!("{id}.json"));
    std::fs::write(&path, "bad json").unwrap();
    assert!(journal.begin(&fp(), &AtomicBool::new(false)).is_err());
    std::fs::remove_file(&path).unwrap();
    let unrelated = dir.path().join("unrelated");
    std::fs::write(&unrelated, "fixture").unwrap();
    symlink(&unrelated, &path).unwrap();
    assert!(journal.begin(&fp(), &AtomicBool::new(false)).is_err());
    assert_eq!(std::fs::read_to_string(unrelated).unwrap(), "fixture");
}
fn helper(dir: &std::path::Path, reply: &str) -> std::path::PathBuf {
    let path = dir.join("fixture-helper");
    let script =
        format!("#!/usr/bin/python3\nimport json,sys,time\nr=json.load(sys.stdin)\n{reply}\n");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn target() -> Target {
    Target::Existing {
        chat_guid: "fixture".into(),
        name: Some("Fixture".into()),
        service: "iMessage".into(),
        recipients: vec!["one@example.invalid".into()],
    }
}
#[test]
fn native_transport_preserves_uncertainty_and_wont_relaunch_the_helper_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let count = dir.path().join("calls");
    let code = format!(
        "open({:?},'a').write('call\\n')\nprint(json.dumps({{'ok':False,'code':'messagesSendOutcomeUncertain','message':'fixture acknowledgement lost'}}))",
        count.to_str().unwrap()
    );
    let path = helper(dir.path(), &code);
    let state = dir.path().join("state");
    let target = target();
    let mut sender = Native::new(path.clone(), state.clone()).unwrap();
    let delivery = sender.deliver(
        Outbound {
            target: &target,
            text: Some("fixture text"),
            attachments: &[],
        },
        &Arc::new(AtomicBool::new(false)),
    );
    assert!(matches!(delivery, Delivery::Uncertain(_)));
    drop(sender);
    let mut sender = Native::new(path, state).unwrap();
    let delivery = sender.deliver(
        Outbound {
            target: &target,
            text: Some("fixture text"),
            attachments: &[],
        },
        &Arc::new(AtomicBool::new(false)),
    );
    assert!(matches!(delivery,Delivery::NotSent(e) if e.code=="messagesSendOutcomeUncertain"));
    assert_eq!(std::fs::read_to_string(count).unwrap(), "call\n");
}
#[test]
fn preflight_failure_is_known_unsent_but_crash_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let path = helper(
        dir.path(),
        "print(json.dumps({'ok':False,'code':'messagesAccountUnavailable','message':'signed out fixture'}))",
    );
    let state = dir.path().join("state");
    let target = target();
    let mut sender = Native::new(path, state.clone()).unwrap();
    assert!(
        matches!(sender.deliver(Outbound{target:&target,text:Some("fixture"),attachments:&[]},&Arc::new(AtomicBool::new(false))),Delivery::NotSent(e) if e.code=="messagesAccountUnavailable")
    );
    assert!(
        Journal::open(state.clone())
            .unwrap()
            .pending()
            .unwrap()
            .is_empty()
    );
    let path = helper(dir.path(), "import os; os._exit(7)");
    let mut sender = Native::new(path, state.clone()).unwrap();
    assert!(matches!(
        sender.deliver(
            Outbound {
                target: &target,
                text: Some("fixture"),
                attachments: &[]
            },
            &Arc::new(AtomicBool::new(false))
        ),
        Delivery::Uncertain(_)
    ));
    assert_eq!(Journal::open(state).unwrap().pending().unwrap().len(), 1);
}
#[test]
fn cancellation_during_native_wait_terminates_helper_and_keeps_uncertain_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let code = format!(
        "import os\nopen({:?},'w').write(str(os.getpid()))\ntime.sleep(30)",
        started.to_str().unwrap()
    );
    let helper = helper(dir.path(), &code);
    let state = dir.path().join("state");
    let mut sender = Native::new(helper, state.clone()).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let path = started.clone();
    let thread = std::thread::spawn(move || {
        let begin = Instant::now();
        while !path.exists() && begin.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(5));
        }
        signal.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let delivery = sender.deliver(
        Outbound {
            target: &target(),
            text: Some("fixture"),
            attachments: &[],
        },
        &cancel,
    );
    thread.join().unwrap();
    assert!(matches!(delivery, Delivery::Uncertain(_)));
    assert!(
        started.exists(),
        "Native fixture did not start: {:?}",
        Journal::open(state.clone()).unwrap().pending().unwrap()
    );
    let pid: libc::pid_t = std::fs::read_to_string(started).unwrap().parse().unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(Journal::open(state).unwrap().pending().unwrap().len(), 1);
}

#[test]
fn accepted_attachments_outlive_the_plan_and_require_explicit_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("photo.txt");
    std::fs::write(
        &input,
        b"fixture bytes retained until Messages consumes them",
    )
    .unwrap();
    let staging = tempfile::tempdir().unwrap();
    std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let files = icu_messages::send::stage_files(&[input], staging.path()).unwrap();
    let path = helper(
        dir.path(),
        "print(json.dumps({'ok':True,'data':{'status':'accepted','chat_guid':'fixture'}}))",
    );
    let state = dir.path().join("state");
    let mut sender = Native::new(path, state.clone()).unwrap();
    assert!(matches!(
        sender.deliver(
            Outbound {
                target: &target(),
                text: None,
                attachments: &files
            },
            &Arc::new(AtomicBool::new(false))
        ),
        Delivery::Accepted { .. }
    ));
    drop(staging);
    let mut entries = std::fs::read_dir(&state)
        .unwrap()
        .filter_map(|p| p.ok().map(|p| p.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    let entry: icu_messages::send_journal::Entry =
        serde_json::from_slice(&std::fs::read(entries.remove(0)).unwrap()).unwrap();
    assert_eq!(entry.outcome, Outcome::Accepted);
    let retained = state
        .join("attachments")
        .join(&entry.id)
        .join("0/photo.txt");
    assert_eq!(
        std::fs::read(&retained).unwrap(),
        b"fixture bytes retained until Messages consumes them"
    );
    Journal::open(state)
        .unwrap()
        .forget_attachments(&entry.id)
        .unwrap();
    assert!(!retained.exists());
}

#[test]
fn fingerprints_do_not_change_when_a_chat_is_renamed_or_recipients_reordered() {
    let first = Target::Existing {
        chat_guid: "fixture".into(),
        name: Some("Old name".into()),
        service: "iMessage".into(),
        recipients: vec!["first@example.invalid".into(), "+1 (555) 010-0200".into()],
    };
    let second = Target::Existing {
        chat_guid: "fixture".into(),
        name: Some("New name".into()),
        service: "iMessage".into(),
        recipients: vec!["15550100200".into(), "FIRST@EXAMPLE.INVALID".into()],
    };
    let a = icu_messages::send::fingerprint(&Outbound {
        target: &first,
        text: Some("identical"),
        attachments: &[],
    })
    .unwrap();
    let b = icu_messages::send::fingerprint(&Outbound {
        target: &second,
        text: Some("identical"),
        attachments: &[],
    })
    .unwrap();
    assert_eq!(a, b);
}
