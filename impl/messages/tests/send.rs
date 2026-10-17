mod common;
use common::Fixture;
use icu_messages::{
    Result,
    consent::Scope,
    contacts::Person,
    contract::{AddressesOnly, Contacts},
    database::Chat,
    engine::{Host, Output, ReadDecision, SendDecision},
    send::{self, Delivery, Outbound, Prepared, Request, Target, Transport},
    worker::Worker,
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn worker(f: &Fixture) -> Worker {
    Worker::new(
        env!("CARGO_BIN_EXE_icu-messages").into(),
        f.dir.path().join("chat.db"),
    )
    .unwrap()
}
fn request() -> Request {
    Request {
        chat_guid: Some("direct".into()),
        recipients: None,
        text: Some("original".into()),
        attachments: None,
    }
}
#[derive(Default)]
struct Record {
    bodies: Vec<Option<String>>,
    files: Vec<Vec<Vec<u8>>>,
    paths: Vec<std::path::PathBuf>,
}
struct Capture {
    record: Arc<Mutex<Record>>,
    uncertain: bool,
    cancel_after: bool,
}
impl Transport for Capture {
    fn deliver(&mut self, payload: Outbound<'_>, cancel: &Arc<AtomicBool>) -> Delivery {
        let mut r = self.record.lock().unwrap();
        r.bodies.push(payload.text.map(str::to_owned));
        r.files.push(
            payload
                .attachments
                .iter()
                .map(|f| std::fs::read(&f.staged).unwrap())
                .collect(),
        );
        r.paths
            .extend(payload.attachments.iter().map(|f| f.staged.clone()));
        if self.cancel_after {
            cancel.store(true, Ordering::SeqCst);
        }
        if self.uncertain {
            Delivery::Uncertain("Fixture lost its acknowledgement".into())
        } else {
            Delivery::Accepted {
                chat_guid: payload.target.consent().map(|d| d.chat_guid),
            }
        }
    }
}
#[test]
fn reviewed_text_and_files_are_frozen_consumed_once_and_cleaned() {
    let f = Fixture::new();
    let path = f.dir.path().join("quote ' and unicode 🐾.txt");
    std::fs::write(&path, b"approved bytes").unwrap();
    let plan = Prepared::create(
        Request {
            attachments: Some(vec![path.clone()]),
            ..request()
        },
        &worker(&f),
        &AddressesOnly,
        cancel(),
    )
    .unwrap();
    let preview = plan.preview();
    assert_eq!(preview.attachments[0].path, path);
    assert_eq!(preview.attachments[0].size, 14);
    let record = Arc::new(Mutex::new(Record::default()));
    let mut transport = Capture {
        record: record.clone(),
        uncertain: false,
        cancel_after: false,
    };
    assert!(record.lock().unwrap().bodies.is_empty());
    let body = "Edited \"literal\" text\n🐾; do shell script is data";
    let result = plan
        .approve(body.into())
        .unwrap()
        .commit(&worker(&f), &AddressesOnly, &mut transport, cancel())
        .unwrap();
    assert!(result.user_edited);
    assert_eq!(result.attachment_count, 1);
    let record = record.lock().unwrap();
    assert_eq!(record.bodies, [Some(body.into())]);
    assert_eq!(record.files[0], [b"approved bytes".to_vec()]);
    assert!(record.paths.iter().all(|p| !p.exists()));
}
#[test]
fn file_content_membership_and_contact_changes_refuse_before_transport() {
    let f = Fixture::new();
    let path = f.dir.path().join("file.txt");
    std::fs::write(&path, b"aaaa").unwrap();
    let record = Arc::new(Mutex::new(Record::default()));
    let mut transport = Capture {
        record: record.clone(),
        uncertain: false,
        cancel_after: false,
    };
    let plan = Prepared::create(
        Request {
            attachments: Some(vec![path.clone()]),
            ..request()
        },
        &worker(&f),
        &AddressesOnly,
        cancel(),
    )
    .unwrap()
    .approve("original".into())
    .unwrap();
    std::fs::write(path, b"bbbb").unwrap();
    assert_eq!(
        plan.commit(&worker(&f), &AddressesOnly, &mut transport, cancel())
            .unwrap_err()
            .code,
        "messagesSendPlanChanged"
    );
    let plan = Prepared::create(request(), &worker(&f), &AddressesOnly, cancel())
        .unwrap()
        .approve("original".into())
        .unwrap();
    f.writer()
        .execute("INSERT INTO chat_handle_join VALUES(1,2)", [])
        .unwrap();
    assert_eq!(
        plan.commit(&worker(&f), &AddressesOnly, &mut transport, cancel())
            .unwrap_err()
            .code,
        "messagesSendPlanChanged"
    );
    assert!(record.lock().unwrap().bodies.is_empty());
    struct Changing {
        handles: Mutex<Vec<String>>,
    }
    impl Contacts for Changing {
        fn resolve(&self, _: &str) -> Result<Vec<String>> {
            Ok(self.handles.lock().unwrap().clone())
        }
        fn candidates(&self, _: &str) -> Result<Vec<Person>> {
            Ok(vec![Person {
                identifier: "person".into(),
                name: "Person".into(),
                handles: self.handles.lock().unwrap().clone(),
            }])
        }
    }
    let contacts = Changing {
        handles: Mutex::new(vec!["new@example.invalid".into()]),
    };
    let plan = Prepared::create(
        Request {
            chat_guid: None,
            recipients: Some(vec!["Person".into()]),
            ..request()
        },
        &worker(&f),
        &contacts,
        cancel(),
    )
    .unwrap()
    .approve("original".into())
    .unwrap();
    *contacts.handles.lock().unwrap() = vec!["different@example.invalid".into()];
    assert_eq!(
        plan.commit(&worker(&f), &contacts, &mut transport, cancel())
            .unwrap_err()
            .code,
        "messagesSendPlanChanged"
    );
    assert!(record.lock().unwrap().bodies.is_empty());
}
#[test]
fn exact_groups_empty_existing_chats_and_new_recipients_resolve_without_sending() {
    let f = Fixture::new();
    let direct = Prepared::create(
        Request {
            chat_guid: None,
            recipients: Some(vec!["one@example.invalid".into()]),
            ..request()
        },
        &worker(&f),
        &AddressesOnly,
        cancel(),
    )
    .unwrap();
    assert!(
        matches!(direct.preview().target,Target::Existing{chat_guid,..} if chat_guid=="direct")
    );
    let group = Prepared::create(
        Request {
            chat_guid: None,
            recipients: Some(vec![
                "one@example.invalid".into(),
                "two@example.invalid".into(),
            ]),
            ..request()
        },
        &worker(&f),
        &AddressesOnly,
        cancel(),
    )
    .unwrap();
    assert!(matches!(group.preview().target,Target::Existing{chat_guid,..} if chat_guid=="group"));
    let new = Prepared::create(
        Request {
            chat_guid: None,
            recipients: Some(vec!["+1 (555) 010-0900".into()]),
            ..request()
        },
        &worker(&f),
        &AddressesOnly,
        cancel(),
    )
    .unwrap();
    assert_eq!(
        new.preview().target,
        Target::New {
            recipients: vec!["+15550100900".into()]
        }
    );
    assert!(new.consent().is_none());
}
#[test]
fn ambiguous_people_invalid_files_and_empty_edits_do_not_produce_committable_plans() {
    struct Ambiguous;
    impl Contacts for Ambiguous {
        fn resolve(&self, _: &str) -> Result<Vec<String>> {
            unreachable!()
        }
        fn candidates(&self, _: &str) -> Result<Vec<Person>> {
            Ok(vec![
                Person::direct("a@example.invalid").unwrap(),
                Person::direct("b@example.invalid").unwrap(),
            ])
        }
    }
    let f = Fixture::new();
    assert_eq!(
        Prepared::create(
            Request {
                chat_guid: None,
                recipients: Some(vec!["Alex".into()]),
                ..request()
            },
            &worker(&f),
            &Ambiguous,
            cancel()
        )
        .err()
        .unwrap()
        .code,
        "messagesAmbiguousDestination"
    );
    assert!(
        Prepared::create(
            Request {
                attachments: Some(vec![f.dir.path().into()]),
                ..request()
            },
            &worker(&f),
            &AddressesOnly,
            cancel()
        )
        .is_err()
    );
    assert!(
        Prepared::create(request(), &worker(&f), &AddressesOnly, cancel())
            .unwrap()
            .approve(String::new())
            .is_err()
    );
}
struct Review {
    calls: usize,
    cancel: bool,
}
impl Host for Review {
    fn review_read(&mut self, _: &[Chat], _: &AtomicBool) -> Result<ReadDecision> {
        unreachable!()
    }
    fn review_send(&mut self, _: &send::Preview, _: &AtomicBool) -> Result<SendDecision> {
        self.calls += 1;
        Ok(if self.cancel {
            SendDecision::Cancel
        } else {
            SendDecision::Accept {
                scope: Scope::Once,
                text: "edited".into(),
            }
        })
    }
}
#[test]
fn uncertain_outcomes_and_canceled_reviews_cannot_be_automatically_retried() {
    let f = Fixture::new();
    let record = Arc::new(Mutex::new(Record::default()));
    let mut engine = f.engine().with_sender(Box::new(Capture {
        record: record.clone(),
        uncertain: true,
        cancel_after: false,
    }));
    let mut host = Review {
        calls: 0,
        cancel: false,
    };
    let args = json!({"chat_guid":"direct","text":"original"});
    assert_eq!(
        engine
            .call("send_message", args.clone(), &mut host, cancel())
            .unwrap_err()
            .code,
        "messagesSendOutcomeUncertain"
    );
    assert_eq!(record.lock().unwrap().bodies.len(), 1);
    assert_eq!(
        engine
            .call("send_message", args.clone(), &mut host, cancel())
            .unwrap_err()
            .code,
        "messagesApprovalCanceled"
    );
    assert_eq!(host.calls, 1);
    engine.next_turn().unwrap();
    host.cancel = true;
    assert_eq!(
        engine
            .call("send_message", args.clone(), &mut host, cancel())
            .unwrap_err()
            .code,
        "messagesSendCanceled"
    );
    assert_eq!(record.lock().unwrap().bodies.len(), 1);
    assert_eq!(
        engine
            .call("send_message", args, &mut host, cancel())
            .unwrap_err()
            .code,
        "messagesApprovalCanceled"
    );
    assert_eq!(host.calls, 2);
}
#[test]
fn a_confirmed_send_is_not_relabelled_unsent_when_cancel_arrives_after_acceptance() {
    let f = Fixture::new();
    let record = Arc::new(Mutex::new(Record::default()));
    let mut engine = f.engine().with_sender(Box::new(Capture {
        record: record.clone(),
        uncertain: false,
        cancel_after: true,
    }));
    let mut host = Review {
        calls: 0,
        cancel: false,
    };
    let Output::Json(result) = engine
        .call(
            "send_message",
            json!({"chat_guid":"direct","text":"original"}),
            &mut host,
            cancel(),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result["status"], "sent");
    assert_eq!(result["user_edited"], true);
    assert_eq!(record.lock().unwrap().bodies.len(), 1);
}

#[test]
fn stdio_reviewed_send_preserves_host_edits_files_and_persistent_revocation() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixStream,
    };
    let f = Fixture::new();
    let file = f.dir.path().join("file.txt");
    std::fs::write(&file, "fixture bytes").unwrap();
    let record = Arc::new(Mutex::new(Record::default()));
    let engine = f.engine().with_sender(Box::new(Capture {
        record: record.clone(),
        uncertain: false,
        cancel_after: false,
    }));
    let (mut client, server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(8)))
        .unwrap();
    let output = server.try_clone().unwrap();
    let thread = std::thread::spawn(move || {
        icu_messages::mcp::serve(
            server,
            output,
            engine,
            icu_messages::mcp::Options::default(),
        )
    });
    let mut reader = BufReader::new(client.try_clone().unwrap());
    fn receive(reader: &mut BufReader<UnixStream>) -> serde_json::Value {
        let mut text = String::new();
        reader.read_line(&mut text).unwrap();
        serde_json::from_str(&text).unwrap()
    }
    fn write(client: &mut UnixStream, value: serde_json::Value) {
        writeln!(client, "{value}").unwrap();
    }
    write(
        &mut client,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{"elicitation":{}},"clientInfo":{"name":"synthetic-send-fixture"}}}),
    );
    receive(&mut reader);
    write(
        &mut client,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let args = json!({"chat_guid":"direct","text":"original","attachments":[file]});
    write(
        &mut client,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"send_message","arguments":args}}),
    );
    let form = receive(&mut reader);
    assert_eq!(form["method"], "elicitation/create");
    assert_eq!(
        form["params"]["requestedSchema"]["properties"]["text"]["default"],
        "original"
    );
    assert!(
        form["params"]["message"]
            .as_str()
            .unwrap()
            .contains("one@example.invalid")
    );
    assert!(
        form["params"]["message"]
            .as_str()
            .unwrap()
            .contains("13 bytes")
    );
    write(
        &mut client,
        json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"accept","content":{"scope":"always","text":"human-edited"}}}),
    );
    let result = receive(&mut reader);
    assert_eq!(result["result"]["structuredContent"]["user_edited"], true);
    assert_eq!(result["result"]["structuredContent"]["attachment_count"], 1);
    write(
        &mut client,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"send_message","arguments":{"chat_guid":"direct","text":"next approved by standing grant"}}}),
    );
    assert_eq!(
        receive(&mut reader)["result"]["structuredContent"]["status"],
        "sent"
    );
    let mut store = icu_messages::consent::Store::open(f.dir.path().join("approvals")).unwrap();
    let (op, d) = store.persistent_destinations().unwrap().remove(0);
    store.revoke(op, &d).unwrap();
    write(
        &mut client,
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"send_message","arguments":{"chat_guid":"direct","text":"needs new approval"}}}),
    );
    let form = receive(&mut reader);
    assert_eq!(form["method"], "elicitation/create");
    write(
        &mut client,
        json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"decline"}}),
    );
    assert_eq!(
        receive(&mut reader)["result"]["_meta"]["icu.error_code"],
        "messagesSendCanceled"
    );
    client.shutdown(std::net::Shutdown::Both).unwrap();
    thread.join().unwrap().unwrap();
    let record = record.lock().unwrap();
    assert_eq!(
        record.bodies,
        [
            Some("human-edited".into()),
            Some("next approved by standing grant".into())
        ]
    );
    assert_eq!(record.files[0], [b"fixture bytes".to_vec()]);
}

#[test]
fn grant_revoked_after_validation_prevents_the_transport_call() {
    let f = Fixture::new();
    let record = Arc::new(Mutex::new(Record::default()));
    let mut transport = Capture {
        record: record.clone(),
        uncertain: false,
        cancel_after: false,
    };
    let approved = Prepared::create(request(), &worker(&f), &AddressesOnly, cancel())
        .unwrap()
        .approve("original".into())
        .unwrap();
    let result = approved.commit_authorized(
        &worker(&f),
        &AddressesOnly,
        &mut transport,
        cancel(),
        || {
            Err(icu_messages::Error::new(
                "messagesApprovalExpired",
                "revoked in fixture",
            ))
        },
    );
    assert_eq!(result.unwrap_err().code, "messagesApprovalExpired");
    assert!(record.lock().unwrap().bodies.is_empty());
}
