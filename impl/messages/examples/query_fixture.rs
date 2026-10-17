//! Audit harness only. No default database path; requires an explicit synthetic-fixture marker.
use icu_messages::{
    Result,
    activity::Breakdown,
    contract::{self, AddressesOnly, Invocation},
    cursor::Cursors,
    worker::{self, Worker},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--query-worker") {
        std::process::exit(worker::entry());
    }
    let database = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("explicit synthetic database path required"),
    );
    let connection = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("fixture database");
    let marker: String = connection
        .query_row("SELECT marker FROM icu_synthetic_fixture", [], |row| {
            row.get(0)
        })
        .expect("missing synthetic-fixture marker; refusing database");
    assert_eq!(marker, "icu-messages-round5-synthetic-v1");
    drop(connection);
    let worker = Worker::new(std::env::current_exe().unwrap(), database).unwrap();
    let mut cursors = Cursors::default();
    for line in std::io::stdin().lock().lines() {
        let result = (|| -> Result<Value> {
            let value: Value = serde_json::from_str(&line.unwrap()).unwrap();
            let name = value["name"].as_str().unwrap();
            let invocation =
                contract::parse(name, value["arguments"].clone(), "UTC", &AddressesOnly)?;
            let Invocation::Query {
                mut query,
                cursor,
                key,
            } = invocation
            else {
                return Err(icu_messages::Error::new(
                    "fixtureReadOnly",
                    "This audit harness only reads synthetic fixtures",
                ));
            };
            let page = cursor
                .as_deref()
                .map(|c| cursors.load(c, key))
                .transpose()?;
            let mut breakdown = Breakdown::Overall;
            match query.as_mut() {
                worker::Query::Read { page: state, .. } => *state = page,
                worker::Query::Count {
                    page: state, query, ..
                } => {
                    *state = page;
                    breakdown = query.breakdown;
                }
                _ => {}
            }
            let mut result = worker.query(*query, Arc::new(AtomicBool::new(false)))?;
            if let Some(page) = result.next_page {
                result.data["next_cursor"] = json!(cursors.save(key, page)?);
            }
            contract::render(name, result.data, &AddressesOnly, breakdown)
        })();
        println!("{}", serde_json::to_string(&result).unwrap());
        std::io::stdout().flush().unwrap();
    }
}
