use icu_messages::{
    activity::Breakdown,
    contract::{self, AddressesOnly, Invocation},
    worker::Query,
};
use serde_json::json;

#[test]
fn recovered_input_limits_and_exclusive_send_targets_are_enforced() {
    for args in [
        json!({"chat_guid":"fixture","limit":101}),
        json!({"chat_guid":"fixture","limit":0}),
        json!({"chat_guid":"fixture","unread_only":1}),
        json!({"chat_guid":"fixture","extra":true}),
        json!({"chat_guid":"fixture","from":"yesterday"}),
    ] {
        assert!(contract::parse("read_messages", args, "UTC", &AddressesOnly).is_err());
    }
    for args in [
        json!({"chat_guid":"fixture","recipients":["alice@example.invalid"],"text":"hello"}),
        json!({"recipients":[],"text":"hello"}),
        json!({"chat_guid":"fixture"}),
        json!({"chat_guid":"fixture","text":""}),
        json!({"chat_guid":"fixture","attachments":[]}),
    ] {
        assert!(contract::parse("send_message", args, "UTC", &AddressesOnly).is_err());
    }
    assert!(matches!(
        contract::parse(
            "send_message",
            json!({"chat_guid":"fixture","text":"🦀".repeat(10000)}),
            "UTC",
            &AddressesOnly
        )
        .unwrap(),
        Invocation::Send { .. }
    ));
    assert!(
        contract::parse(
            "send_message",
            json!({"chat_guid":"fixture","text":"🦀".repeat(10001)}),
            "UTC",
            &AddressesOnly
        )
        .is_err()
    );
    assert!(
        contract::parse(
            "count_message_activity",
            json!({"interval":"hour"}),
            "UTC",
            &AddressesOnly
        )
        .is_err()
    );
    assert!(
        contract::parse(
            "count_message_activity",
            json!({"time_zone":"UTC"}),
            "UTC",
            &AddressesOnly
        )
        .is_err()
    );
    assert!(
        contract::parse(
            "find_chats",
            json!({"participants":["Unresolved Person"]}),
            "UTC",
            &AddressesOnly
        )
        .is_err()
    );
    assert!(
        contract::parse(
            "find_chats",
            json!({"participants":vec!["person@example.invalid";21]}),
            "UTC",
            &AddressesOnly
        )
        .is_err()
    );
}

#[test]
fn tool_defaults_and_cursors_bind_to_the_original_filter_and_operation() {
    let Invocation::Query { query, cursor, key } = contract::parse(
        "read_messages",
        json!({"chat_guid":"fixture"}),
        "UTC",
        &AddressesOnly,
    )
    .unwrap() else {
        panic!()
    };
    assert!(cursor.is_none());
    let Query::Read { filter, limit, .. } = *query else {
        panic!()
    };
    assert_eq!(limit, 50);
    assert_eq!(filter.chat_guids, ["fixture"]);
    let Invocation::Query { key: next, .. } = contract::parse(
        "read_messages",
        json!({"chat_guid":"fixture","limit":1,"cursor":"opaque"}),
        "UTC",
        &AddressesOnly,
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(key, next);
    let Invocation::Query { key: other, .. } = contract::parse(
        "read_messages",
        json!({"chat_guid":"fixture","unread_only":true}),
        "UTC",
        &AddressesOnly,
    )
    .unwrap() else {
        panic!()
    };
    assert_ne!(key, other);
    let Invocation::Query { query, .. } = contract::parse(
        "count_message_activity",
        json!({}),
        "America/Los_Angeles",
        &AddressesOnly,
    )
    .unwrap() else {
        panic!()
    };
    let Query::Count { query, limit, .. } = *query else {
        panic!()
    };
    assert_eq!(limit, 20);
    assert_eq!(query.time_zone, "America/Los_Angeles");
    assert_eq!(query.breakdown, Breakdown::Overall);
}

#[test]
fn message_wire_output_uses_page_local_references_and_excludes_private_fields() {
    let message = |guid: &str, chat: &str, sender: Option<&str>, from_me: bool| json!({"guid":guid,"chat_guid":chat,"sender":sender,"from_me":from_me,"is_unread":!from_me,"date":"2026-10-09T00:00:00Z","text":"Synthetic body","attachments":[{"id":"attachment-id","filename":"fixture.png","mime_type":"image/png","size":42}]});
    let data = json!({"messages":[message("m1","chat-one",None,true),message("m2","chat-two",Some("person@example.invalid"),false),message("m3","chat-two",Some("person@example.invalid"),false)],"next_cursor":"opaque"});
    let result =
        contract::render("read_messages", data, &AddressesOnly, Breakdown::Overall).unwrap();
    assert_eq!(result["messages"][0]["sender"], "me");
    assert_eq!(result["messages"][1]["sender"], "s1");
    assert_eq!(result["messages"][2]["sender"], "s1");
    assert_eq!(result["messages"][0]["chat"], "c1");
    assert_eq!(result["messages"][1]["chat"], "c2");
    assert_eq!(result["senders"].as_array().unwrap().len(), 1);
    assert_eq!(result["messages"][0]["attachments"][0]["size_bytes"], 42);
    assert!(result["messages"][0].get("guid").is_none());
    assert!(result["messages"][0].get("text").is_none());
    assert!(result["messages"][0].get("is_unread").is_none());
    assert_eq!(result["next_cursor"], "opaque");
    let next = contract::render(
        "read_messages",
        json!({"messages":[message("m4","chat-two",None,false)]}),
        &AddressesOnly,
        Breakdown::Overall,
    )
    .unwrap();
    assert_eq!(next["messages"][0]["chat"], "c1");
    assert_eq!(next["messages"][0]["sender"], "unknown");
    let mut broken = message("m5", "chat-two", None, false);
    broken["body_error"] = json!("messagesBodyUnsupported");
    assert!(
        contract::render(
            "read_messages",
            json!({"messages":[broken]}),
            &AddressesOnly,
            Breakdown::Overall
        )
        .is_err()
    );
}

#[test]
fn activity_wire_output_distinguishes_overall_from_chat_breakdowns() {
    let activity = json!({"total":1,"sent":1,"received":0,"counts_by_bucket":{"total":[1],"sent":[1],"received":[0]}});
    let mut data = json!({"from":"2026-10-01T00:00:00Z","to":"2026-10-02T00:00:00Z","time_zone":"UTC","interval":"day","buckets":[{"from":"2026-10-01T00:00:00Z","to":"2026-10-02T00:00:00Z"}],"matching_chat_count":1,"overall_activity":activity,"chats":[]});
    let overall = contract::render(
        "count_message_activity",
        data.clone(),
        &AddressesOnly,
        Breakdown::Overall,
    )
    .unwrap();
    assert_eq!(overall["range"]["from"], "2026-10-01T00:00:00Z");
    assert!(overall.get("chats").is_none());
    assert_eq!(overall["overall_activity"]["total"], 1);
    let mut chat = activity;
    chat["chat"] = json!({"chat_guid":"fixture","name":"Fixture group","service":"RCS","is_group":true,"participants":["person@example.invalid"],"unread_count":0,"last_message_date":null});
    data["chats"] = json!([chat]);
    data["next_cursor"] = json!("opaque");
    let result = contract::render(
        "count_message_activity",
        data,
        &AddressesOnly,
        Breakdown::Chat,
    )
    .unwrap();
    assert!(result.get("overall_activity").is_none());
    assert_eq!(result["chats"][0]["type"], "group");
    assert_eq!(result["chats"][0]["participants"][0], "p1");
    assert_eq!(
        result["participants"][0]["handle"],
        "person@example.invalid"
    );
    assert_eq!(result["next_cursor"], "opaque");
}
