use icu_messages::consent::{Context, Destination, Operation, Scope, Store};

fn chat() -> Destination {
    Destination {
        chat_guid: "fixture-group".into(),
        participants: vec!["alex@example.invalid".into(), "+1 (555) 010-0200".into()],
        service: "iMessage".into(),
    }
}
fn store(dir: &tempfile::TempDir) -> Store {
    Store::open(dir.path().join("approvals")).unwrap()
}

#[test]
fn once_turn_and_session_scopes_have_distinct_lifetimes() {
    for scope in [Scope::Once, Scope::Turn, Scope::Session] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(&dir);
        let context = Context::new_session().unwrap();
        let destination = chat();
        assert!(
            store
                .acquire(&context, Operation::Read, &destination)
                .unwrap()
                .is_none()
        );
        store
            .approve(&context, Operation::Read, &destination, scope)
            .unwrap();
        let lease = store
            .acquire(&context, Operation::Read, &destination)
            .unwrap()
            .unwrap();
        store
            .consume(lease, &context, Operation::Read, &destination)
            .unwrap();
        assert_eq!(
            store
                .acquire(&context, Operation::Read, &destination)
                .unwrap()
                .is_some(),
            scope != Scope::Once
        );
        assert_eq!(
            store
                .acquire(&context.next_turn().unwrap(), Operation::Read, &destination)
                .unwrap()
                .is_some(),
            scope == Scope::Session
        );
        assert!(
            store
                .acquire(
                    &Context::new_session().unwrap(),
                    Operation::Read,
                    &destination
                )
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .acquire(&context, Operation::Send, &destination)
                .unwrap()
                .is_none()
        );
        store.end_session(&context);
        assert_eq!(
            store
                .acquire(&context, Operation::Read, &destination)
                .err()
                .unwrap()
                .code,
            "messagesSessionEnded"
        );
        assert!(store.persistent_destinations().unwrap().is_empty());
    }
}

#[test]
fn persistent_grants_survive_restart_but_revocation_invalidates_pending_work() {
    if let Some(directory) = std::env::var_os("ICU_MESSAGES_TEST_REVOCATION_DIR") {
        let mut child = Store::open(directory.into()).unwrap();
        child.revoke(Operation::Send, &chat()).unwrap();
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut first = store(&dir);
    let context = Context::new_session().unwrap();
    let destination = chat();
    first
        .approve(&context, Operation::Send, &destination, Scope::Always)
        .unwrap();
    drop(first);
    let mut first = store(&dir);
    let lease = first
        .acquire(&context, Operation::Send, &destination)
        .unwrap()
        .unwrap();
    assert_eq!(store(&dir).persistent_destinations().unwrap().len(), 1);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "persistent_grants_survive_restart_but_revocation_invalidates_pending_work",
        ])
        .env(
            "ICU_MESSAGES_TEST_REVOCATION_DIR",
            dir.path().join("approvals"),
        )
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        first
            .consume(lease, &context, Operation::Send, &destination)
            .unwrap_err()
            .code,
        "messagesApprovalExpired"
    );
    assert!(
        first
            .acquire(&context, Operation::Send, &destination)
            .unwrap()
            .is_none()
    );
}

#[test]
fn another_stores_revocation_also_expires_earlier_transient_grants() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = store(&dir);
    let mut second = store(&dir);
    let context = Context::new_session().unwrap();
    let destination = chat();
    first
        .approve(&context, Operation::Send, &destination, Scope::Session)
        .unwrap();
    second.revoke(Operation::Send, &destination).unwrap();
    assert!(
        first
            .acquire(&context, Operation::Send, &destination)
            .unwrap()
            .is_none()
    );
}

#[test]
fn membership_service_operation_and_context_are_rechecked_at_consumption() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(&dir);
    let context = Context::new_session().unwrap();
    let destination = chat();
    store
        .approve(&context, Operation::Send, &destination, Scope::Always)
        .unwrap();
    let mut normalized = destination.clone();
    normalized.participants = vec!["15550100200".into(), "ALEX@EXAMPLE.INVALID".into()];
    let lease = store
        .acquire(&context, Operation::Send, &normalized)
        .unwrap()
        .unwrap();
    store
        .consume(lease, &context, Operation::Send, &destination)
        .unwrap();
    let mut changed = destination.clone();
    changed.participants.push("third@example.invalid".into());
    assert!(
        store
            .acquire(&context, Operation::Send, &changed)
            .unwrap()
            .is_none()
    );
    let lease = store
        .acquire(&context, Operation::Send, &destination)
        .unwrap()
        .unwrap();
    assert!(
        store
            .consume(lease, &context, Operation::Send, &changed)
            .is_err()
    );
    let lease = store
        .acquire(&context, Operation::Send, &destination)
        .unwrap()
        .unwrap();
    assert!(
        store
            .consume(lease, &context, Operation::Read, &destination)
            .is_err()
    );
    let lease = store
        .acquire(&context, Operation::Send, &destination)
        .unwrap()
        .unwrap();
    assert!(
        store
            .consume(
                lease,
                &context.next_turn().unwrap(),
                Operation::Send,
                &destination
            )
            .is_err()
    );
    let lease = store
        .acquire(&context, Operation::Send, &destination)
        .unwrap()
        .unwrap();
    let mut changed = destination;
    changed.service = "RCS".into();
    assert!(
        store
            .consume(lease, &context, Operation::Send, &changed)
            .is_err()
    );
}

#[test]
fn canceled_reads_cannot_reprompt_or_retry_in_the_same_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(&dir);
    let context = Context::new_session().unwrap();
    let destination = chat();
    store
        .approve(&context, Operation::Read, &destination, Scope::Session)
        .unwrap();
    let lease = store
        .acquire(&context, Operation::Read, &destination)
        .unwrap()
        .unwrap();
    store.cancel(&context, Operation::Read);
    assert!(
        store
            .consume(lease, &context, Operation::Read, &destination)
            .is_err()
    );
    assert_eq!(
        store
            .acquire(&context, Operation::Read, &destination)
            .err()
            .unwrap()
            .code,
        "messagesApprovalCanceled"
    );
    assert_eq!(
        store
            .approve(&context, Operation::Read, &destination, Scope::Always)
            .unwrap_err()
            .code,
        "messagesApprovalCanceled"
    );
    assert!(
        store
            .acquire(&context.next_turn().unwrap(), Operation::Read, &destination)
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .acquire(&context, Operation::Send, &destination)
            .unwrap()
            .is_none()
    );
}

#[test]
fn corrupt_or_symlinked_storage_never_grants_permission() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(&dir);
    let context = Context::new_session().unwrap();
    let destination = chat();
    store
        .approve(&context, Operation::Send, &destination, Scope::Always)
        .unwrap();
    let path = dir.path().join("approvals/approvals.json");
    let lease = store
        .acquire(&context, Operation::Send, &destination)
        .unwrap()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    let replacement = Store::open(dir.path().join("approvals")).unwrap();
    assert!(
        store
            .consume(lease, &context, Operation::Send, &destination)
            .is_err()
    );
    assert!(
        store
            .acquire(&context, Operation::Send, &destination)
            .is_err()
    );
    store = replacement;
    store
        .approve(&context, Operation::Send, &destination, Scope::Always)
        .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
        0
    );
    std::fs::write(&path, b"{broken}").unwrap();
    assert!(
        store
            .acquire(&context, Operation::Send, &destination)
            .is_err()
    );
    std::fs::remove_file(&path).unwrap();
    let unrelated = dir.path().join("unrelated");
    std::fs::write(&unrelated, "private fixture").unwrap();
    symlink(&unrelated, &path).unwrap();
    assert!(
        store
            .acquire(&context, Operation::Send, &destination)
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(unrelated).unwrap(),
        "private fixture"
    );
}
