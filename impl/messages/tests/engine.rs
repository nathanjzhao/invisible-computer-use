mod common;
use common::Fixture;
use icu_messages::{
    Result,
    consent::{Scope, Store},
    database::Chat,
    engine::{Host, NoHost, ReadDecision},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Review<F> {
    calls: usize,
    callback: F,
}
impl<F: FnMut(&[Chat], &AtomicBool) -> ReadDecision> Host for Review<F> {
    fn review_read(&mut self, chats: &[Chat], cancel: &AtomicBool) -> Result<ReadDecision> {
        self.calls += 1;
        Ok((self.callback)(chats, cancel))
    }
}
fn accept(chats: &[Chat], _: &AtomicBool) -> ReadDecision {
    ReadDecision::Accept {
        chat_guids: chats.iter().map(|c| c.chat_guid.clone()).collect(),
        scope: Scope::Once,
    }
}
fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn date() -> i64 {
    icu_messages::dates::parse("2026-10-01T00:00:00Z").unwrap()
}

#[test]
fn content_requires_host_approval_and_once_is_consumed() {
    let f = Fixture::new();
    f.add(1, "Private fixture body", date());
    let mut engine = f.engine();
    let args = json!({"chat_guid":"direct"});
    let error = read(
        &mut engine,
        "read_messages",
        args.clone(),
        &mut NoHost,
        cancel(),
    )
    .unwrap_err();
    assert_eq!(error.code, "messagesApprovalUnavailable");
    assert!(!error.message.contains("Private fixture"));
    let mut host = Review {
        calls: 0,
        callback: accept,
    };
    for _ in 0..2 {
        let result = read(
            &mut engine,
            "read_messages",
            args.clone(),
            &mut host,
            cancel(),
        )
        .unwrap();
        assert_eq!(result["messages"][0]["body"], "Private fixture body");
    }
    assert_eq!(host.calls, 2);
}
#[test]
fn declined_search_page_is_empty_with_cursor_and_later_allowed_page_works() {
    let f = Fixture::new();
    f.add(1, "needle permitted", date());
    f.add(2, "needle withheld", date() + 1_000_000_000);
    let mut engine = f.engine();
    let mut host = Review {
        calls: 0,
        callback: |chats: &[Chat], _: &AtomicBool| {
            if chats[0].chat_guid == "group" {
                ReadDecision::Decline
            } else {
                ReadDecision::Accept {
                    chat_guids: vec!["direct".into()],
                    scope: Scope::Once,
                }
            }
        },
    };
    let args = json!({"text":"needle","limit":1});
    let first = read(
        &mut engine,
        "search_messages",
        args.clone(),
        &mut host,
        cancel(),
    )
    .unwrap();
    assert_eq!(first["permission_filtered"], true);
    assert_eq!(first["messages"], json!([]));
    assert_eq!(first["chats"], json!([]));
    assert_eq!(first["senders"], json!([]));
    assert!(!first.to_string().contains("withheld"));
    let next = read(
        &mut engine,
        "search_messages",
        json!({"text":"needle","limit":1,"cursor":first["next_cursor"]}),
        &mut host,
        cancel(),
    )
    .unwrap();
    assert_eq!(next["messages"][0]["body"], "needle permitted");
    assert!(next.get("permission_filtered").is_none());
    read(&mut engine, "search_messages", args, &mut host, cancel()).unwrap();
    assert_eq!(host.calls, 2, "declined chat was re-prompted");
}
#[test]
fn cancellation_is_terminal_until_a_host_turn_transition() {
    let f = Fixture::new();
    f.add(1, "private", date());
    let mut engine = f.engine();
    let mut host = Review {
        calls: 0,
        callback: |_: &[Chat], _: &AtomicBool| ReadDecision::Cancel,
    };
    let args = json!({"chat_guid":"direct"});
    for _ in 0..2 {
        assert_eq!(
            read(
                &mut engine,
                "read_messages",
                args.clone(),
                &mut host,
                cancel()
            )
            .unwrap_err()
            .code,
            "messagesApprovalCanceled"
        );
    }
    assert_eq!(host.calls, 1);
    engine.end_turn();
    assert_eq!(
        read(&mut engine, "find_chats", json!({}), &mut NoHost, cancel())
            .unwrap_err()
            .code,
        "messagesTurnEnded"
    );
    engine.next_turn().unwrap();
    let mut host = Review {
        calls: 0,
        callback: accept,
    };
    assert_eq!(
        read(&mut engine, "read_messages", args, &mut host, cancel()).unwrap()["messages"][0]["body"],
        "private"
    );
}
#[test]
fn late_host_approval_after_request_cancel_cannot_persist_or_release_data() {
    let f = Fixture::new();
    f.add(1, "private", date());
    let mut engine = f.engine();
    let mut host = Review {
        calls: 0,
        callback: |chats: &[Chat], cancel: &AtomicBool| {
            cancel.store(true, Ordering::SeqCst);
            ReadDecision::Accept {
                chat_guids: vec![chats[0].chat_guid.clone()],
                scope: Scope::Always,
            }
        },
    };
    assert_eq!(
        read(
            &mut engine,
            "read_messages",
            json!({"chat_guid":"direct"}),
            &mut host,
            cancel()
        )
        .unwrap_err()
        .code,
        "messagesRequestCanceled"
    );
    assert!(
        Store::open(f.dir.path().join("approvals"))
            .unwrap()
            .persistent_destinations()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn membership_change_while_review_is_open_invalidates_approval() {
    let f = Fixture::new();
    f.add(1, "private", date());
    let mut engine = f.engine();
    let mut host = Review {
        calls: 0,
        callback: |chats: &[Chat], _: &AtomicBool| {
            f.writer()
                .execute("INSERT INTO chat_handle_join VALUES (1,2)", [])
                .unwrap();
            ReadDecision::Accept {
                chat_guids: vec![chats[0].chat_guid.clone()],
                scope: Scope::Once,
            }
        },
    };
    assert_eq!(
        read(
            &mut engine,
            "read_messages",
            json!({"chat_guid":"direct"}),
            &mut host,
            cancel()
        )
        .unwrap_err()
        .code,
        "messagesApprovalExpired"
    );
}
#[test]
fn persistent_read_scope_is_reused_until_revoked() {
    let f = Fixture::new();
    f.add(1, "private", date());
    let mut engine = f.engine();
    let mut host = Review {
        calls: 0,
        callback: |chats: &[Chat], _: &AtomicBool| ReadDecision::Accept {
            chat_guids: vec![chats[0].chat_guid.clone()],
            scope: Scope::Always,
        },
    };
    read(
        &mut engine,
        "read_messages",
        json!({"chat_guid":"direct"}),
        &mut host,
        cancel(),
    )
    .unwrap();
    drop(engine);
    let mut engine = f.engine();
    read(
        &mut engine,
        "read_messages",
        json!({"chat_guid":"direct"}),
        &mut NoHost,
        cancel(),
    )
    .unwrap();
    let mut store = Store::open(f.dir.path().join("approvals")).unwrap();
    let (op, target) = store.persistent_destinations().unwrap().remove(0);
    store.revoke(op, &target).unwrap();
    assert_eq!(
        read(
            &mut engine,
            "read_messages",
            json!({"chat_guid":"direct"}),
            &mut NoHost,
            cancel()
        )
        .unwrap_err()
        .code,
        "messagesApprovalUnavailable"
    );
}

fn read(
    engine: &mut icu_messages::engine::Engine,
    name: &str,
    args: serde_json::Value,
    host: &mut dyn Host,
    cancel: Arc<AtomicBool>,
) -> Result<serde_json::Value> {
    match engine.call(name, args, host, cancel)? {
        icu_messages::engine::Output::Json(value) => Ok(value),
        _ => panic!("expected JSON output"),
    }
}
