//! Captures approved sends to a local receipt. Refuses any database without the synthetic marker.
use icu_messages::{
    consent::Store,
    contract::AddressesOnly,
    engine::Engine,
    mcp,
    send::{Delivery, Outbound, Transport},
    worker::Worker,
};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
struct Capture {
    path: PathBuf,
    uncertain: bool,
}
impl Transport for Capture {
    fn deliver(&mut self, payload: Outbound<'_>, cancel: &Arc<AtomicBool>) -> Delivery {
        if cancel.load(Ordering::SeqCst) {
            return Delivery::NotSent(icu_messages::Error::new(
                "messagesSendCanceled",
                "Fixture send canceled",
            ));
        }
        let files=payload.attachments.iter().map(|f|std::fs::read(&f.staged).map(|bytes|serde_json::json!({"name":f.original.file_name().unwrap().to_string_lossy(),"size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}))).collect::<std::io::Result<Vec<_>>>();
        let result = files.and_then(|files| {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            writeln!(
                file,
                "{}",
                serde_json::json!({"target":payload.target,"text":payload.text,"files":files})
            )?;
            file.sync_all()
        });
        match result {
            Err(_) => Delivery::Uncertain("Fixture receipt failed after dispatch".into()),
            Ok(()) if self.uncertain => {
                Delivery::Uncertain("Fixture discarded acknowledgement".into())
            }
            Ok(()) => Delivery::Accepted {
                chat_guid: payload.target.consent().map(|d| d.chat_guid),
            },
        }
    }
}
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--query-worker") {
        std::process::exit(icu_messages::worker::entry());
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        (3..=4).contains(&args.len()),
        "database, state directory and receipt path are required"
    );
    let database = PathBuf::from(&args[0]);
    let c = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let marker: String = c
        .query_row("SELECT marker FROM icu_synthetic_fixture", [], |r| r.get(0))
        .expect("refusing non-fixture database");
    assert_eq!(marker, "icu-messages-round5-synthetic-v1");
    drop(c);
    let engine = Engine::new(
        Worker::new(std::env::current_exe().unwrap(), database).unwrap(),
        Store::open(args[1].clone().into()).unwrap(),
        Box::new(AddressesOnly),
        "UTC".into(),
    )
    .unwrap()
    .with_sender(Box::new(Capture {
        path: args[2].clone().into(),
        uncertain: args.get(3).is_some_and(|s| s == "uncertain"),
    }));
    mcp::serve(
        std::io::stdin(),
        std::io::stdout(),
        engine,
        mcp::Options::default(),
    )
    .unwrap();
}
