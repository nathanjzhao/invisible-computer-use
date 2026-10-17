use icu_messages::{
    Result,
    consent::{Scope, Store},
    contacts::{Native, Person},
    contract::{AddressesOnly, Contacts},
    database::Chat,
    engine::{Engine, Host, ReadDecision},
    worker::Worker,
};
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, atomic::AtomicBool},
};
mod common;

fn native(dir: &std::path::Path) -> Native {
    let helper = dir.join("contacts-fixture");
    std::fs::write(&helper,r#"#!/usr/bin/python3
import json,sys
r=json.load(sys.stdin);q=r.get('query','').lower()
people=[{'identifier':'p1','name':'Alex Doe','handles':['+1 (555) 010-0200','alex@example.invalid']},{'identifier':'p2','name':'Alex Roe','handles':['second@example.invalid']}]
if q=='denied':print(json.dumps({'ok':False,'code':'messagesContactsPermissionDenied','message':'fixture access denied'}))
else:
 matches=[p for p in people if (q in p['name'].lower() if r['operation']=='name' else q in [h.lower() for h in p['handles']])]
 print(json.dumps({'ok':True,'data':{'people':matches,'limited':False}}))
"#).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    Native::new(helper).unwrap()
}
#[test]
fn native_results_preserve_people_and_alternative_addresses() {
    let dir = tempfile::tempdir().unwrap();
    let contacts = native(dir.path());
    let people = contacts.candidates("Alex").unwrap();
    assert_eq!(people.len(), 2);
    assert_eq!(people[0].identifier, "p1");
    assert_eq!(people[0].handles.len(), 2);
    let handles = contacts.resolve("Alex Doe").unwrap();
    assert_eq!(handles.len(), 2);
    assert!(handles.contains(&"+1 (555) 010-0200".into()));
    assert_eq!(
        contacts.name("alex@example.invalid").as_deref(),
        Some("Alex Doe")
    );
    assert!(contacts.name("missing@example.invalid").is_none());
    assert_eq!(
        contacts.resolve("Nobody").unwrap_err().code,
        "messagesContactNotFound"
    );
    assert_eq!(
        contacts.resolve("denied").unwrap_err().code,
        "messagesContactsPermissionDenied"
    );
}
#[test]
fn address_queries_need_no_contacts_access_and_cancellation_reaches_the_helper() {
    let contacts = Native::new("/nonexistent/contacts-helper".into()).unwrap();
    assert_eq!(
        contacts.resolve("+1 (555) 010-0200").unwrap(),
        ["+1 (555) 010-0200"]
    );
    assert_eq!(
        AddressesOnly.candidates("user@example.invalid").unwrap()[0].handles,
        ["user@example.invalid"]
    );
    assert!(Person::direct("not a @ valid address").is_none());
    let dir = tempfile::tempdir().unwrap();
    let contacts = native(dir.path());
    contacts.begin_request(Arc::new(AtomicBool::new(true)));
    assert_eq!(
        contacts.resolve("Alex").unwrap_err().code,
        "messagesRequestCanceled"
    );
}
struct Allow;
impl Host for Allow {
    fn review_read(&mut self, chats: &[Chat], _: &AtomicBool) -> Result<ReadDecision> {
        Ok(ReadDecision::Accept {
            chat_guids: chats.iter().map(|c| c.chat_guid.clone()).collect(),
            scope: Scope::Once,
        })
    }
}
#[test]
fn resolved_names_filter_chats_and_enrich_response_names() {
    let fixture = common::Fixture::new();
    fixture.add(
        1,
        "fixture",
        icu_messages::dates::parse("2026-10-01T00:00:00Z").unwrap(),
    );
    fixture
        .writer()
        .execute(
            "UPDATE handle SET id='alex@example.invalid' WHERE ROWID=1",
            [],
        )
        .unwrap();
    let contacts = native(fixture.dir.path());
    let mut engine = Engine::new(
        Worker::new(
            env!("CARGO_BIN_EXE_icu-messages").into(),
            fixture.dir.path().join("chat.db"),
        )
        .unwrap(),
        Store::open(fixture.dir.path().join("approvals")).unwrap(),
        Box::new(contacts),
        "UTC".into(),
    )
    .unwrap();
    let icu_messages::engine::Output::Json(result) = engine
        .call(
            "find_chats",
            json!({"participants":["Alex Doe"],"exact_participants":true}),
            &mut Allow,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(result["chats"][0]["chat_guid"], "direct");
    assert_eq!(result["participants"][0]["name"], "Alex Doe");
}
