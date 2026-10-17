// Each integration-test binary uses a different subset of these shared fixtures.
#![allow(dead_code)]
use icu_messages::{consent::Store, contract::AddressesOnly, engine::Engine, worker::Worker};
use rusqlite::{Connection, params};

pub struct Fixture {
    pub dir: tempfile::TempDir,
}
impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let c = Connection::open(dir.path().join("chat.db")).unwrap();
        c.execute_batch("PRAGMA journal_mode=WAL;
          CREATE TABLE chat(guid TEXT,display_name TEXT,service_name TEXT,style INTEGER);
          CREATE TABLE handle(id TEXT);
          CREATE TABLE message(guid TEXT,date INTEGER,text TEXT,attributedBody BLOB,handle_id INTEGER,is_from_me INTEGER,is_read INTEGER);
          CREATE TABLE chat_message_join(chat_id INTEGER,message_id INTEGER,message_date INTEGER);
          CREATE TABLE chat_handle_join(chat_id INTEGER,handle_id INTEGER);
          CREATE TABLE attachment(guid TEXT,filename TEXT,mime_type TEXT,total_bytes INTEGER,transfer_name TEXT);
          CREATE TABLE message_attachment_join(message_id INTEGER,attachment_id INTEGER);
          INSERT INTO handle VALUES('one@example.invalid'),('two@example.invalid');
          INSERT INTO chat VALUES('direct','Fixture direct','iMessage',45),('group','Fixture group','iMessage',43);
          INSERT INTO chat_handle_join VALUES(1,1),(2,1),(2,2);").unwrap();
        Self { dir }
    }
    pub fn writer(&self) -> Connection {
        Connection::open(self.dir.path().join("chat.db")).unwrap()
    }
    pub fn add(&self, chat: i64, text: &str, date: i64) {
        let c = self.writer();
        c.execute(
            "INSERT INTO message VALUES(?,?,?,NULL,1,0,0)",
            params![format!("m-{date}"), date, text],
        )
        .unwrap();
        let id = c.last_insert_rowid();
        c.execute(
            "INSERT INTO chat_message_join VALUES(?,?,?)",
            params![chat, id, date],
        )
        .unwrap();
    }
    pub fn engine(&self) -> Engine {
        Engine::new(
            Worker::new(
                env!("CARGO_BIN_EXE_icu-messages").into(),
                self.dir.path().join("chat.db"),
            )
            .unwrap(),
            Store::open(self.dir.path().join("approvals")).unwrap(),
            Box::new(AddressesOnly),
            "UTC".into(),
        )
        .unwrap()
    }
    pub fn attach(&self, id: &str, path: &std::path::Path) {
        let c = self.writer();
        c.execute(
            "INSERT INTO attachment VALUES (?,?, 'image/png',NULL, 'fixture.png')",
            params![id, path.to_str().unwrap()],
        )
        .unwrap();
        let attachment = c.last_insert_rowid();
        c.execute(
            "INSERT INTO message_attachment_join VALUES(1,?)",
            [attachment],
        )
        .unwrap();
    }
    pub fn png(&self) -> std::path::PathBuf {
        let dir = self.dir.path().join("Attachments");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fixture.png");
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(file, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .add_text_chunk(
                "Comment".into(),
                "fixture metadata must not be returned".into(),
            )
            .unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[255, 0, 0, 255, 0, 0, 255, 128])
            .unwrap();
        writer.finish().unwrap();
        path
    }
}
