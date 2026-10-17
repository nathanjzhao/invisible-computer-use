use icu_messages::{
    body,
    cursor::Cursors,
    database::{Database, Filter},
    dates::{self, Interval},
};
use rusqlite::{Connection, params};
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

struct Fixture {
    dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("chat.db")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;
   CREATE TABLE chat(guid TEXT,display_name TEXT,service_name TEXT,style INTEGER);
   CREATE TABLE handle(id TEXT);
   CREATE TABLE message(guid TEXT,date INTEGER,text TEXT,attributedBody BLOB,handle_id INTEGER,is_from_me INTEGER,is_read INTEGER);
   CREATE TABLE chat_message_join(chat_id INTEGER,message_id INTEGER,message_date INTEGER);
   CREATE TABLE chat_handle_join(chat_id INTEGER,handle_id INTEGER);
   CREATE TABLE attachment(guid TEXT,filename TEXT,mime_type TEXT,total_bytes INTEGER,transfer_name TEXT);
   CREATE TABLE message_attachment_join(message_id INTEGER,attachment_id INTEGER);
   INSERT INTO handle VALUES('+1 (555) 010-0200'),('alex@example.invalid');
   INSERT INTO chat VALUES('direct','Direct','iMessage',45),('group','Fixture group','RCS',43);
   INSERT INTO chat_handle_join VALUES(1,1),(2,1),(2,2);
   CREATE INDEX cmj_chat ON chat_message_join(chat_id);
   CREATE INDEX chj_chat ON chat_handle_join(chat_id);").unwrap();
        Self { dir }
    }
    fn writer(&self) -> Connection {
        Connection::open(self.dir.path().join("chat.db")).unwrap()
    }
    fn reader(&self) -> Database {
        Database::open(
            &self.dir.path().join("chat.db"),
            Duration::from_secs(3),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
    }
    fn add(
        &self,
        chat: i64,
        guid: &str,
        date: i64,
        text: Option<&str>,
        sent: bool,
        read: bool,
    ) -> i64 {
        let db = self.writer();
        db.execute(
            "INSERT INTO message VALUES(?,?,?,NULL,1,?,?)",
            params![guid, date, text, sent, read],
        )
        .unwrap();
        let id = db.last_insert_rowid();
        db.execute(
            "INSERT INTO chat_message_join VALUES(?,?,?)",
            params![chat, id, date],
        )
        .unwrap();
        id
    }
}
fn date(s: &str) -> i64 {
    dates::parse(s).unwrap()
}

#[test]
fn pages_are_stable_across_ties_backdated_arrivals_and_new_joins() {
    let f = Fixture::new();
    let d = date("2026-10-09T12:00:00Z");
    for i in 1..=5 {
        f.add(1, &format!("m{i}"), d, Some("fixture"), false, false);
    }
    let mut cursors = Cursors::default();
    let filter = Filter::default();
    let db = f.reader();
    let page = db.first_page().unwrap();
    let first = db.read_messages(&filter, &page, 2, &mut cursors).unwrap();
    assert_eq!(
        first
            .messages
            .iter()
            .map(|m| m.guid.as_str())
            .collect::<Vec<_>>(),
        ["m5", "m4"]
    );
    drop(db);
    f.add(1, "new", d + 10, Some("new arrival"), false, false);
    f.add(
        1,
        "backdated",
        d - 10,
        Some("backdated arrival"),
        false,
        false,
    );
    f.writer()
        .execute("INSERT INTO chat_message_join VALUES(2,1,?)", [d])
        .unwrap();
    let key = Cursors::query_key(&filter.normalize().unwrap()).unwrap();
    let next = cursors
        .load(first.next_cursor.as_ref().unwrap(), key)
        .unwrap();
    let db = f.reader();
    let second = db.read_messages(&filter, &next, 3, &mut cursors).unwrap();
    assert_eq!(
        second
            .messages
            .iter()
            .map(|m| m.guid.as_str())
            .collect::<Vec<_>>(),
        ["m3", "m2", "m1"]
    );
    assert!(second.next_cursor.is_none());
    let other = Filter {
        unread_only: true,
        ..Default::default()
    };
    assert!(
        cursors
            .load(
                first.next_cursor.as_ref().unwrap(),
                Cursors::query_key(&other).unwrap()
            )
            .is_err()
    );
}
#[test]
fn participant_filters_require_all_people_and_exact_rejects_extra_members() {
    let f = Fixture::new();
    let d = date("2026-10-09T12:00:00Z");
    f.add(1, "direct-m", d, Some("direct"), false, false);
    f.add(2, "group-m", d, Some("group"), false, false);
    let db = f.reader();
    let filter = Filter {
        participants: vec![vec!["+1-555-010-0200".into()]],
        ..Default::default()
    };
    assert_eq!(db.find_chats(&filter, None, 20).unwrap().chats.len(), 2);
    assert_eq!(db.participants(1).unwrap(), ["+1 (555) 010-0200"]);
    let mut cursors = Cursors::default();
    let page = db
        .read_messages(
            &Filter::default(),
            &db.first_page().unwrap(),
            20,
            &mut cursors,
        )
        .unwrap();
    assert_eq!(
        page.messages[0].sender.as_deref(),
        Some("+1 (555) 010-0200")
    );
    let exact = Filter {
        exact_participants: true,
        ..filter.clone()
    };
    assert_eq!(
        db.find_chats(&exact, None, 20).unwrap().chats[0].chat_guid,
        "direct"
    );
    let both = Filter {
        participants: vec![
            vec!["15550100200".into()],
            vec!["ALEX@example.invalid".into()],
        ],
        exact_participants: true,
        ..Default::default()
    };
    assert_eq!(
        db.find_chats(&both, None, 20).unwrap().chats[0].chat_guid,
        "group"
    );
    assert!(
        db.find_chats(
            &Filter {
                exact_participants: true,
                ..Default::default()
            },
            None,
            20
        )
        .is_err()
    );
}
#[test]
fn unicode_attributed_text_is_searchable_and_attributes_are_not_body_text() {
    let f = Fixture::new();
    let d = date("2026-10-09T12:00:00Z");
    let id = f.add(1, "rich", d, None, false, false);
    let bytes = include_bytes!("fixtures/attributed-body.bin");
    f.writer()
        .execute(
            "UPDATE message SET attributedBody=? WHERE ROWID=?",
            params![&bytes[..], id],
        )
        .unwrap();
    assert_eq!(
        body::decode(None, Some(bytes)).unwrap().as_deref(),
        Some("Fixture café 🐾\nSecond line")
    );
    let db = f.reader();
    let page = db.first_page().unwrap();
    let mut cursors = Cursors::default();
    let matches = db
        .read_messages(
            &Filter {
                text: Some("CAFÉ 🐾".into()),
                ..Default::default()
            },
            &page,
            20,
            &mut cursors,
        )
        .unwrap();
    assert_eq!(matches.messages.len(), 1);
    let metadata = db
        .read_messages(
            &Filter {
                text: Some("metadata is not".into()),
                ..Default::default()
            },
            &page,
            20,
            &mut cursors,
        )
        .unwrap();
    assert!(metadata.messages.is_empty());
}
#[test]
fn unread_and_half_open_ranges_and_legacy_seconds_are_consistent() {
    let f = Fixture::new();
    let lo = date("2004-01-01T00:00:00Z");
    let hi = lo + 1_000_000_000;
    f.add(
        1,
        "received",
        lo / 1_000_000_000,
        Some("legacy seconds"),
        false,
        false,
    );
    f.add(1, "sent", lo, Some("sent"), true, false);
    f.add(1, "read", lo, Some("read"), false, true);
    f.add(1, "outside", hi, Some("to is exclusive"), false, false);
    let db = f.reader();
    let filter = Filter {
        from: Some(lo),
        to: Some(hi),
        unread_only: true,
        ..Default::default()
    };
    let mut cursors = Cursors::default();
    let page = db.first_page().unwrap();
    let read = db.read_messages(&filter, &page, 20, &mut cursors).unwrap();
    assert_eq!(read.messages.len(), 1);
    assert_eq!(read.messages[0].guid, "received");
    assert_eq!(read.messages[0].date, "2004-01-01T00:00:00Z");
    let chats = db.find_chats(&filter, None, 20).unwrap();
    assert_eq!(chats.chats[0].unread_count, 1);
}
#[test]
fn malformed_body_is_explicit_and_does_not_become_a_silent_search_miss() {
    let f = Fixture::new();
    let id = f.add(1, "bad", date("2026-01-01T00:00:00Z"), None, false, false);
    f.writer()
        .execute(
            "UPDATE message SET attributedBody=? WHERE ROWID=?",
            params![b"bad archive".as_slice(), id],
        )
        .unwrap();
    let db = f.reader();
    let page = db.first_page().unwrap();
    let mut cursors = Cursors::default();
    let messages = db
        .read_messages(&Filter::default(), &page, 20, &mut cursors)
        .unwrap();
    assert_eq!(
        messages.messages[0].body_error.as_deref(),
        Some("messagesBodyUnsupported")
    );
    assert!(
        db.read_messages(
            &Filter {
                text: Some("anything".into()),
                ..Default::default()
            },
            &page,
            20,
            &mut cursors
        )
        .is_err()
    );
}
#[test]
fn filters_are_data_not_sql_and_unknown_chat_is_not_empty_success() {
    let f = Fixture::new();
    f.add(
        1,
        "m",
        date("2026-01-01T00:00:00Z"),
        Some("quote ' OR 1=1 --"),
        false,
        false,
    );
    let db = f.reader();
    let page = db.first_page().unwrap();
    let mut cursors = Cursors::default();
    assert!(
        db.read_messages(
            &Filter {
                chat_guids: vec!["' OR 1=1 --".into()],
                ..Default::default()
            },
            &page,
            20,
            &mut cursors
        )
        .is_err()
    );
    assert_eq!(
        db.read_messages(
            &Filter {
                text: Some("' OR 1=1 --".into()),
                ..Default::default()
            },
            &page,
            20,
            &mut cursors
        )
        .unwrap()
        .messages
        .len(),
        1
    );
    assert_eq!(
        f.writer()
            .query_row("SELECT count(*) FROM message", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn calendar_buckets_handle_dst_and_repeated_hours() {
    let spring = dates::buckets(
        date("2026-03-08T00:00:00-08:00"),
        date("2026-03-10T00:00:00-07:00"),
        Interval::Day,
        "America/Los_Angeles",
    )
    .unwrap();
    assert_eq!(spring.len(), 2);
    assert_eq!(
        (spring[0].upper - spring[0].lower) / 1_000_000_000,
        23 * 3600
    );
    let fall = dates::buckets(
        date("2026-11-01T00:00:00-07:00"),
        date("2026-11-01T03:00:00-08:00"),
        Interval::Hour,
        "America/Los_Angeles",
    )
    .unwrap();
    assert_eq!(fall.len(), 4);
    assert!(fall.iter().all(|b| b.upper - b.lower == 3_600_000_000_000));
    let mid = dates::buckets(
        date("2026-11-01T01:30:00-08:00"),
        date("2026-11-01T03:00:00-08:00"),
        Interval::Hour,
        "America/Los_Angeles",
    )
    .unwrap();
    assert_eq!(mid.len(), 2);
    assert_eq!(mid[0].upper - mid[0].lower, 1_800_000_000_000);
}

#[test]
fn activity_totals_cover_the_full_range_and_buckets_are_zero_filled() {
    use icu_messages::activity::{Breakdown, Counts, Query, RankBy};
    let f = Fixture::new();
    let lo = date("2026-10-01T00:00:00Z");
    let day = 86_400_000_000_000;
    f.add(1, "direct-sent", lo, Some("a"), true, true);
    let id = f.add(1, "direct-received", lo + 2 * day, Some("b"), false, false);
    f.add(2, "group-received", lo + 2 * day, Some("c"), false, false);
    f.add(2, "excluded-upper", lo + 3 * day, Some("d"), true, true);
    // A repeated join must not double count a message within its chat.
    f.writer()
        .execute(
            "INSERT INTO chat_message_join VALUES(1,?,?)",
            params![id, lo + 2 * day],
        )
        .unwrap();
    let q = Query {
        filter: Filter {
            from: Some(lo),
            to: Some(lo + 3 * day),
            ..Default::default()
        },
        interval: Interval::Day,
        time_zone: "UTC".into(),
        breakdown: Breakdown::Chat,
        rank_by: RankBy::Total,
    };
    let db = f.reader();
    let mut cursors = Cursors::default();
    let report = q
        .run(&db, db.first_page().unwrap(), 1, &mut cursors)
        .unwrap();
    assert_eq!(report.matching_chat_count, 2);
    assert_eq!(
        report.overall_activity.counts,
        Counts {
            total: 3,
            sent: 1,
            received: 2
        }
    );
    assert_eq!(report.overall_activity.counts_by_bucket.total, [1, 0, 2]);
    assert_eq!(report.overall_activity.counts_by_bucket.sent, [1, 0, 0]);
    assert_eq!(report.overall_activity.counts_by_bucket.received, [0, 0, 2]);
    assert_eq!(report.chats[0].chat.chat_guid, "direct");
    assert_eq!(report.chats[0].activity.counts_by_bucket.total, [1, 0, 1]);
    let page = cursors
        .load(report.next_cursor.as_ref().unwrap(), q.key().unwrap())
        .unwrap();
    let second = q.run(&db, page, 1, &mut cursors).unwrap();
    assert_eq!(second.chats[0].chat.chat_guid, "group");
    assert_eq!(
        second.overall_activity.counts,
        report.overall_activity.counts
    );
    assert_eq!(second.matching_chat_count, 2);
    assert!(second.next_cursor.is_none());
}

#[test]
fn activity_omitted_bounds_ranking_and_watermarks_survive_new_arrivals() {
    use icu_messages::activity::{Breakdown, Query, RankBy};
    let f = Fixture::new();
    let lo = date("2026-01-01T00:00:00Z");
    f.add(1, "one", lo, None, false, false);
    f.add(2, "two", lo, None, true, true);
    let q = Query {
        filter: Filter::default(),
        interval: Interval::Total,
        time_zone: "UTC".into(),
        breakdown: Breakdown::Chat,
        rank_by: RankBy::Sent,
    };
    let mut cursors = Cursors::default();
    let db = f.reader();
    let first = q
        .run(&db, db.first_page().unwrap(), 1, &mut cursors)
        .unwrap();
    assert_eq!(first.chats[0].chat.chat_guid, "group");
    let token = first.next_cursor.as_ref().unwrap();
    drop(db);
    f.add(1, "older-arrival", lo - 1_000_000_000, None, true, true);
    f.add(1, "newer-arrival", lo + 1_000_000_000, None, true, true);
    let db = f.reader();
    let second = q
        .run(
            &db,
            cursors.load(token, q.key().unwrap()).unwrap(),
            1,
            &mut cursors,
        )
        .unwrap();
    assert_eq!(second.from, first.from);
    assert_eq!(second.to, first.to);
    assert_eq!(second.overall_activity.counts.total, 2);
    assert_eq!(second.chats[0].chat.chat_guid, "direct");
    assert!(second.next_cursor.is_none());
    let changed = Query {
        rank_by: RankBy::Received,
        ..q
    };
    assert!(cursors.load(token, changed.key().unwrap()).is_err());
}

#[test]
fn activity_chat_types_empty_ranges_and_invalid_queries_are_explicit() {
    use icu_messages::{
        activity::{Breakdown, Query, RankBy},
        database::ChatType,
    };
    let f = Fixture::new();
    let lo = date("2026-01-01T00:00:00Z");
    f.add(1, "one", lo, None, true, true);
    f.add(2, "two", lo, None, false, false);
    let db = f.reader();
    let mut cursors = Cursors::default();
    for kind in [ChatType::Direct, ChatType::Group] {
        let q = Query {
            filter: Filter {
                from: Some(lo),
                to: Some(lo + 1_000_000_000),
                chat_type: kind,
                ..Default::default()
            },
            interval: Interval::Day,
            time_zone: "UTC".into(),
            breakdown: Breakdown::Overall,
            rank_by: RankBy::Total,
        };
        let report = q
            .run(&db, db.first_page().unwrap(), 20, &mut cursors)
            .unwrap();
        assert_eq!(report.overall_activity.counts.total, 1);
        assert_eq!(
            report.overall_activity.counts.sent,
            u64::from(kind == ChatType::Direct)
        );
        assert_eq!(report.matching_chat_count, 1);
        assert!(report.chats.is_empty());
        let empty = Query {
            filter: Filter {
                from: Some(lo),
                to: Some(lo),
                ..q.filter.clone()
            },
            ..q.clone()
        };
        let report = empty
            .run(&db, db.first_page().unwrap(), 20, &mut cursors)
            .unwrap();
        assert_eq!(report.overall_activity.counts.total, 0);
        assert!(report.buckets.is_empty());
        assert!(
            Query {
                time_zone: "Not/AZone".into(),
                ..q.clone()
            }
            .run(&db, db.first_page().unwrap(), 20, &mut cursors)
            .is_err()
        );
        assert!(
            q.run(&db, db.first_page().unwrap(), 0, &mut cursors)
                .is_err()
        );
    }
}

#[test]
fn cancel_deadline_and_attachment_metadata_do_not_silently_succeed() {
    use std::sync::atomic::Ordering;
    let f = Fixture::new();
    let id = f.add(
        1,
        "attached",
        date("2026-01-01T00:00:00Z"),
        None,
        false,
        false,
    );
    f.writer().execute_batch("INSERT INTO attachment VALUES('image-guid','/private/fixture/image.png','image/png',42,'photo.png');").unwrap();
    f.writer()
        .execute("INSERT INTO message_attachment_join VALUES(?,1)", [id])
        .unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let db = Database::open(
        &f.dir.path().join("chat.db"),
        Duration::from_secs(3),
        cancel.clone(),
    )
    .unwrap();
    let mut cursors = Cursors::default();
    let page = db.first_page().unwrap();
    let result = db
        .read_messages(&Filter::default(), &page, 20, &mut cursors)
        .unwrap();
    let attachment = &result.messages[0].attachments[0];
    assert_eq!(attachment.id, "image-guid");
    assert_eq!(attachment.filename.as_deref(), Some("photo.png"));
    assert_eq!(attachment.mime_type.as_deref(), Some("image/png"));
    assert_eq!(attachment.size, Some(42));
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("/private/fixture")
    );
    cancel.store(true, Ordering::SeqCst);
    assert_eq!(
        db.read_messages(&Filter::default(), &page, 20, &mut cursors)
            .unwrap_err()
            .code,
        "messagesRequestCanceled"
    );
    assert!(
        Database::open(
            &f.dir.path().join("chat.db"),
            Duration::from_secs(3),
            cancel
        )
        .is_err()
    );
    let expired = Database::open(
        &f.dir.path().join("chat.db"),
        Duration::ZERO,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(
        expired.first_page().unwrap_err().code,
        "messagesQueryTimedOut"
    );
}

#[test]
fn real_workers_decode_and_continue_across_processes_without_writing_the_database() {
    use icu_messages::worker::{Query, Worker};
    let f = Fixture::new();
    let d = date("2026-01-01T00:00:00Z");
    let id = f.add(1, "rich", d, None, false, false);
    f.writer()
        .execute(
            "UPDATE message SET attributedBody=? WHERE ROWID=?",
            params![&include_bytes!("fixtures/attributed-body.bin")[..], id],
        )
        .unwrap();
    f.add(1, "newest", d + 1_000_000_000, Some("latest"), true, true);
    let path = f.dir.path().join("chat.db");
    let before = std::fs::read(&path).unwrap();
    let worker = Worker::new(env!("CARGO_BIN_EXE_icu-messages").into(), path.clone()).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let first = worker
        .query(
            Query::Read {
                filter: Filter::default(),
                page: None,
                limit: 1,
            },
            cancel.clone(),
        )
        .unwrap();
    assert_eq!(first.data["messages"][0]["guid"], "newest");
    let second = worker
        .query(
            Query::Read {
                filter: Filter::default(),
                page: first.next_page,
                limit: 1,
            },
            cancel,
        )
        .unwrap();
    assert_eq!(
        second.data["messages"][0]["text"],
        "Fixture café 🐾\nSecond line"
    );
    assert!(second.next_page.is_none());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn malformed_body_error_crosses_worker_boundary_and_next_query_still_works() {
    use icu_messages::worker::{Query, Worker};
    let f = Fixture::new();
    let id = f.add(1, "bad", date("2026-01-01T00:00:00Z"), None, false, false);
    f.writer()
        .execute(
            "UPDATE message SET attributedBody=? WHERE ROWID=?",
            params![b"bad archive".as_slice(), id],
        )
        .unwrap();
    let worker = Worker::new(
        env!("CARGO_BIN_EXE_icu-messages").into(),
        f.dir.path().join("chat.db"),
    )
    .unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let error = worker
        .query(
            Query::Read {
                filter: Filter {
                    text: Some("needle".into()),
                    ..Default::default()
                },
                page: None,
                limit: 20,
            },
            cancel.clone(),
        )
        .unwrap_err();
    assert_eq!(error.code, "messagesBodyUnsupported");
    let read = worker
        .query(
            Query::Read {
                filter: Filter::default(),
                page: None,
                limit: 20,
            },
            cancel,
        )
        .unwrap();
    assert_eq!(
        read.data["messages"][0]["body_error"],
        "messagesBodyUnsupported"
    );
}

#[test]
fn unfinished_and_system_messages_are_not_counted_as_unread() {
    let f = Fixture::new();
    let date = date("2026-10-01T00:00:00Z");
    f.add(1, "normal", date, Some("normal"), false, false);
    let unfinished = f.add(1, "unfinished", date, Some("unfinished"), false, false);
    let system = f.add(1, "system", date, Some("system"), false, false);
    let event = f.add(1, "event", date, Some("group event"), false, false);
    let writer = f.writer();
    writer.execute_batch("ALTER TABLE message ADD COLUMN is_finished INTEGER DEFAULT 1; ALTER TABLE message ADD COLUMN is_system_message INTEGER DEFAULT 0; ALTER TABLE message ADD COLUMN item_type INTEGER DEFAULT 0;").unwrap();
    writer
        .execute("UPDATE message SET item_type=1 WHERE ROWID=?", [event])
        .unwrap();
    writer
        .execute(
            "UPDATE message SET is_finished=0 WHERE ROWID=?",
            [unfinished],
        )
        .unwrap();
    writer
        .execute(
            "UPDATE message SET is_system_message=1 WHERE ROWID=?",
            [system],
        )
        .unwrap();
    let db = f.reader();
    let page = db.first_page().unwrap();
    let mut cursors = Cursors::default();
    let read = db
        .read_messages(&Filter::default(), &page, 20, &mut cursors)
        .unwrap();
    assert_eq!(
        read.messages
            .iter()
            .filter(|m| m.is_unread)
            .map(|m| m.guid.as_str())
            .collect::<Vec<_>>(),
        ["normal"]
    );
    let unread = Filter {
        unread_only: true,
        ..Default::default()
    };
    assert_eq!(
        db.read_messages(&unread, &page, 20, &mut cursors)
            .unwrap()
            .messages
            .len(),
        1
    );
    assert_eq!(
        db.find_chats(&unread, None, 20).unwrap().chats[0].unread_count,
        1
    );
    assert_eq!(db.chat(1).unwrap().unread_count, 1);
}

#[test]
fn valid_search_text_is_not_truncated_or_rejected_by_an_extra_character_cap() {
    let f = Fixture::new();
    let d = date("2026-10-01T00:00:00Z");
    let needle = format!("{}needle", "x".repeat(5000));
    f.add(1, "long", d, Some(&needle), false, false);
    f.add(1, "prefix", d, Some(&"x".repeat(5000)), false, false);
    f.add(1, "space", d, Some("has space"), false, false);
    let db = f.reader();
    let page = db.first_page().unwrap();
    let mut cursors = Cursors::default();
    let result = db
        .read_messages(
            &Filter {
                text: Some(needle),
                ..Default::default()
            },
            &page,
            20,
            &mut cursors,
        )
        .unwrap();
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].guid, "long");
    let result = db
        .read_messages(
            &Filter {
                text: Some(" ".into()),
                ..Default::default()
            },
            &page,
            20,
            &mut cursors,
        )
        .unwrap();
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].guid, "space");
}
