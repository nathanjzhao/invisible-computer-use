use crate::{
    Error, Result, body,
    cursor::{Cursors, Page, Position, Watermark},
    dates, invalid,
};
use rusqlite::{Connection, OpenFlags, params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_PAGE: usize = 200;
const MAX_ATTACHMENTS: usize = 500;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChatType {
    #[default]
    All,
    Direct,
    Group,
}

/// Each inner group represents one resolved person and their alternative addresses.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    #[serde(default)]
    pub chat_guids: Vec<String>,
    #[serde(default)]
    pub participants: Vec<Vec<String>>,
    #[serde(default)]
    pub exact_participants: bool,
    pub from: Option<i64>,
    pub to: Option<i64>,
    #[serde(default)]
    pub unread_only: bool,
    pub text: Option<String>,
    #[serde(default)]
    pub chat_type: ChatType,
}
impl Filter {
    pub fn normalize(&self) -> Result<Self> {
        let mut f = self.clone();
        if f.from.zip(f.to).is_some_and(|(a, b)| a > b) {
            return Err(invalid("from must not be later than to"));
        }
        if f.chat_guids.len() > 200 || f.participants.len() > 100 {
            return Err(invalid("Too many chat or participant filters"));
        }
        if f.chat_guids
            .iter()
            .any(|g| g.trim().is_empty() || g.len() > 1000)
        {
            return Err(invalid(
                "chat_guids must not contain empty or oversized identifiers",
            ));
        }
        f.chat_guids.sort();
        f.chat_guids.dedup();
        for person in &mut f.participants {
            if person.is_empty() || person.len() > 100 {
                return Err(invalid(
                    "Every participant must resolve to at least one address",
                ));
            }
            for handle in person.iter_mut() {
                *handle = normalize_handle(handle);
                if handle.is_empty() || handle.len() > 1000 {
                    return Err(invalid("Invalid participant address"));
                }
            }
            person.sort();
            person.dedup();
        }
        f.participants.sort();
        f.participants.dedup();
        if f.exact_participants && f.participants.is_empty() {
            return Err(invalid(
                "exact_participants requires at least one participant",
            ));
        }
        if let Some(text) = f.text.as_mut() {
            if text.is_empty() {
                return Err(invalid("Search text must be nonempty"));
            }
            *text = text.to_lowercase();
        }
        Ok(f)
    }
}
pub fn normalize_handle(value: &str) -> String {
    let value = value.trim();
    if value.contains('@') {
        return value.to_lowercase();
    }
    if value
        .chars()
        .all(|c| c.is_ascii_digit() || "+()- .".contains(c))
    {
        return value.chars().filter(char::is_ascii_digit).collect();
    }
    value.to_lowercase()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    #[serde(skip)]
    pub path: Option<PathBuf>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub guid: String,
    pub chat_guid: String,
    pub sender: Option<String>,
    pub from_me: bool,
    pub is_unread: bool,
    pub date: String,
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_error: Option<String>,
    pub attachments: Vec<Attachment>,
    #[serde(skip)]
    pub position: Position,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessagePage {
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chat {
    pub chat_guid: String,
    pub name: Option<String>,
    pub service: Option<String>,
    pub is_group: bool,
    pub participants: Vec<String>,
    pub unread_count: u64,
    pub last_message_date: Option<String>,
    #[serde(skip)]
    pub row_id: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatPage {
    pub chats: Vec<Chat>,
    pub has_more: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Activity {
    pub date: i64,
    pub sent: bool,
    pub chat: i64,
}

/// Read-only database access. The host gates release of message and attachment contents.
/// Chat metadata is used to present resolved conversations in the approval UI.
/// Untrusted attributed bodies are parsed in the service's bounded query worker.
pub struct Database {
    connection: Connection,
    columns: BTreeMap<String, BTreeSet<String>>,
    budget: Duration,
    cancel: Arc<AtomicBool>,
    started: Instant,
}
impl Database {
    pub fn open(path: &Path, budget: Duration, cancel: Arc<AtomicBool>) -> Result<Self> {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::new(
                "messagesRequestCanceled",
                "Messages request was canceled",
            ));
        }
        if !path.is_absolute() {
            return Err(invalid("The database path must be absolute"));
        }
        let connection=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY|OpenFlags::SQLITE_OPEN_NO_MUTEX)
   .map_err(|_|Error::new("messagesDatabaseUnavailable","The Messages database could not be opened read-only. Check the configured path and macOS Full Disk Access."))?;
        connection.pragma_update(None, "query_only", true)?;
        connection.busy_timeout(budget.min(Duration::from_millis(250)))?;
        connection.set_limit(
            rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH,
            8 * 1024 * 1024,
        );
        connection.create_scalar_function(
            "icu_handle",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| Ok(normalize_handle(&context.get::<String>(0)?)),
        )?;
        let mut columns = BTreeMap::new();
        for table in [
            "chat",
            "message",
            "handle",
            "chat_message_join",
            "chat_handle_join",
            "attachment",
            "message_attachment_join",
        ] {
            let mut statement = connection.prepare("SELECT name FROM pragma_table_info(?1)")?;
            let names = statement
                .query_map([table], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<BTreeSet<_>, _>>()?;
            if names.is_empty() {
                return Err(Error::new(
                    "messagesSchemaUnsupported",
                    format!("Missing Messages table: {table}"),
                ));
            }
            columns.insert(table.into(), names);
        }
        for (table, required) in [
            ("chat", &["guid"][..]),
            (
                "message",
                &["guid", "date", "handle_id", "is_from_me", "is_read"][..],
            ),
            ("handle", &["id"][..]),
            ("chat_message_join", &["chat_id", "message_id"][..]),
            ("chat_handle_join", &["chat_id", "handle_id"][..]),
            (
                "message_attachment_join",
                &["message_id", "attachment_id"][..],
            ),
            ("attachment", &["guid", "filename"][..]),
        ] {
            for name in required {
                if !columns[table].contains(*name) {
                    return Err(Error::new(
                        "messagesSchemaUnsupported",
                        format!("Missing {table}.{name}"),
                    ));
                }
            }
        }
        let mut result = Self {
            connection,
            columns,
            budget,
            cancel,
            started: Instant::now(),
        };
        result.begin()?;
        Ok(result)
    }
    fn begin(&mut self) -> Result<()> {
        self.started = Instant::now();
        let start = self.started;
        let budget = self.budget;
        let cancel = self.cancel.clone();
        self.connection.progress_handler(
            1000,
            Some(move || cancel.load(Ordering::SeqCst) || start.elapsed() >= budget),
        );
        // One consistent SQLite snapshot covers message rows and attachment/participant metadata.
        self.connection.execute_batch("BEGIN DEFERRED")?;
        Ok(())
    }
    fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            Err(Error::new(
                "messagesRequestCanceled",
                "Messages request was canceled",
            ))
        } else if self.started.elapsed() >= self.budget {
            Err(Error::new(
                "messagesQueryTimedOut",
                "The Messages query exceeded its deadline",
            ))
        } else {
            Ok(())
        }
    }
    fn column(&self, table: &str, alias: &str, name: &str, fallback: &str) -> String {
        if self.columns[table].contains(name) {
            format!("{alias}.\"{name}\"")
        } else {
            fallback.into()
        }
    }
    fn date_sql(&self) -> String {
        let base = if self.columns["chat_message_join"].contains("message_date") {
            "COALESCE(NULLIF(m.date,0),j.message_date,0)"
        } else {
            "COALESCE(m.date,0)"
        };
        format!("CASE WHEN ABS({base}) < 1000000000000 THEN {base} * 1000000000 ELSE {base} END")
    }
    fn unread_sql(&self, alias: &str) -> String {
        let finished = self.column("message", alias, "is_finished", "1");
        let system = self.column("message", alias, "is_system_message", "0");
        let item_type = self.column("message", alias, "item_type", "0");
        format!(
            "(COALESCE({alias}.is_from_me,0)=0 AND COALESCE({alias}.is_read,0)=0 AND COALESCE({finished},0)=1 AND COALESCE({system},0)=0 AND COALESCE({item_type},0)=0)"
        )
    }
    pub fn watermark(&self) -> Result<Watermark> {
        Ok(Watermark {
            message: self.connection.query_row(
                "SELECT COALESCE(MAX(ROWID),0) FROM message",
                [],
                |r| r.get(0),
            )?,
            chat: self.connection.query_row(
                "SELECT COALESCE(MAX(ROWID),0) FROM chat",
                [],
                |r| r.get(0),
            )?,
            join: self.connection.query_row(
                "SELECT COALESCE(MAX(ROWID),0) FROM chat_message_join",
                [],
                |r| r.get(0),
            )?,
        })
    }
    pub fn participants(&self, chat: i64) -> Result<Vec<String>> {
        let mut q=self.connection.prepare("SELECT DISTINCT h.id FROM chat_handle_join ch JOIN handle h ON h.ROWID=ch.handle_id WHERE ch.chat_id=? ORDER BY h.id")?;
        let mut handles = q
            .query_map([chat], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        // Normalize for matching/identity only. Preserve routable/display addresses in responses.
        let mut seen = BTreeSet::new();
        handles.retain(|h| seen.insert(normalize_handle(h)));
        Ok(handles)
    }
    fn conditions(
        &self,
        filter: &Filter,
        page: &Page,
        after: bool,
    ) -> Result<(String, Vec<SqlValue>)> {
        self.check()?;
        let mut sql = "m.ROWID <= ? AND c.ROWID <= ? AND j.ROWID <= ?".to_string();
        let mut values = vec![
            page.watermark.message.into(),
            page.watermark.chat.into(),
            page.watermark.join.into(),
        ];
        if !filter.chat_guids.is_empty() {
            let encoded = serde_json::to_string(&filter.chat_guids)
                .map_err(|_| invalid("Invalid chat identifiers"))?;
            let count:i64=self.connection.query_row("SELECT COUNT(DISTINCT guid) FROM chat WHERE ROWID<=? AND guid IN (SELECT value FROM json_each(?))",params![page.watermark.chat,&encoded],|r|r.get(0))?;
            if count != filter.chat_guids.len() as i64 {
                return Err(Error::new(
                    "messagesChatNotFound",
                    "One or more message chats were not found",
                ));
            }
            sql.push_str(" AND c.guid IN (SELECT value FROM json_each(?))");
            values.push(encoded.into());
        }
        for person in &filter.participants {
            sql.push_str(" AND EXISTS(SELECT 1 FROM chat_handle_join ch JOIN handle h ON h.ROWID=ch.handle_id WHERE ch.chat_id=c.ROWID AND icu_handle(h.id) IN (SELECT value FROM json_each(?)))");
            values.push(
                serde_json::to_string(person)
                    .map_err(|_| invalid("Invalid participant"))?
                    .into(),
            );
        }
        if filter.exact_participants {
            let addresses: BTreeSet<_> = filter.participants.iter().flatten().collect();
            sql.push_str(" AND NOT EXISTS(SELECT 1 FROM chat_handle_join ch JOIN handle h ON h.ROWID=ch.handle_id WHERE ch.chat_id=c.ROWID AND icu_handle(h.id) NOT IN (SELECT value FROM json_each(?)))");
            values.push(
                serde_json::to_string(&addresses)
                    .map_err(|_| invalid("Invalid participants"))?
                    .into(),
            );
        }
        if filter.chat_type != ChatType::All {
            let style = self.column("chat", "c", "style", "0");
            let group = format!(
                "({style}=43 OR (SELECT COUNT(DISTINCT icu_handle(h.id)) FROM chat_handle_join ch JOIN handle h ON h.ROWID=ch.handle_id WHERE ch.chat_id=c.ROWID)>1)"
            );
            sql.push_str(if filter.chat_type == ChatType::Group {
                " AND "
            } else {
                " AND NOT "
            });
            sql.push_str(&group);
        }
        let date = self.date_sql();
        if let Some(from) = filter.from {
            sql.push_str(&format!(" AND ({date}) >= ?"));
            values.push(from.into());
        }
        if let Some(to) = filter.to {
            sql.push_str(&format!(" AND ({date}) < ?"));
            values.push(to.into());
        }
        if filter.unread_only {
            sql.push_str(&format!(" AND {}", self.unread_sql("m")));
        }
        if after && let Some(p) = page.after {
            sql.push_str(&format!(" AND (({date}),m.ROWID,c.ROWID) < (?,?,?)"));
            values.extend([p.date.into(), p.message.into(), p.chat.into()]);
        }
        Ok((sql, values))
    }
    pub fn read_messages(
        &self,
        filter: &Filter,
        page: &Page,
        limit: usize,
        cursors: &mut Cursors,
    ) -> Result<MessagePage> {
        let (messages, next_page) = self.read_batch(filter, page, limit)?;
        let next_cursor = next_page
            .map(|page| cursors.save(Cursors::query_key(&filter.normalize()?)?, page))
            .transpose()?;
        Ok(MessagePage {
            messages,
            next_cursor,
        })
    }

    /// Internal worker boundary. Only the parent turns continuation state into opaque tokens.
    pub fn read_batch(
        &self,
        filter: &Filter,
        page: &Page,
        limit: usize,
    ) -> Result<(Vec<Message>, Option<Page>)> {
        if !(1..=MAX_PAGE).contains(&limit) {
            return Err(invalid("Page limit must be between 1 and 200"));
        }
        let filter = filter.normalize()?;
        let (conditions, values) = self.conditions(&filter, page, true)?;
        let date = self.date_sql();
        let text = self.column("message", "m", "text", "NULL");
        let attributed = self.column("message", "m", "attributedBody", "NULL");
        let unread = self.unread_sql("m");
        let sql = format!(
            "SELECT DISTINCT m.ROWID,c.ROWID,m.guid,c.guid,h.id,COALESCE(m.is_from_me,0),COALESCE(m.is_read,0),({date}) AS sort_date,CASE WHEN length({text})<=4194304 THEN {text} ELSE NULL END,CASE WHEN length({attributed})<=4194304 THEN {attributed} ELSE NULL END,COALESCE(length({text}),0)>4194304 OR COALESCE(length({attributed}),0)>4194304,{unread} FROM chat_message_join j JOIN message m ON m.ROWID=j.message_id JOIN chat c ON c.ROWID=j.chat_id LEFT JOIN handle h ON h.ROWID=m.handle_id WHERE {conditions} ORDER BY sort_date DESC,m.ROWID DESC,c.ROWID DESC"
        );
        let mut q = self.connection.prepare(&sql)?;
        let mut rows = q.query(params_from_iter(values))?;
        let mut messages = Vec::new();
        let mut more = false;
        while let Some(row) = rows.next()? {
            self.check()?;
            let row_id: i64 = row.get(0)?;
            let chat_id: i64 = row.get(1)?;
            let date: i64 = row.get(7)?;
            let raw_text: Option<String> = row.get(8)?;
            let raw_body: Option<Vec<u8>> = row.get(9)?;
            let decoded = if row.get::<_, bool>(10)? {
                Err(Error::new(
                    "messagesBodyTooLarge",
                    "Message body exceeds its decode limit",
                ))
            } else {
                body::decode(raw_text.as_deref(), raw_body.as_deref())
            };
            self.check()?;
            if let Some(needle) = &filter.text {
                match &decoded {
                    Ok(Some(text)) if text.to_lowercase().contains(needle) => {}
                    Ok(_) => continue,
                    Err(error) => return Err(error.clone()),
                }
            }
            if messages.len() == limit {
                more = true;
                break;
            }
            let from_me = row.get::<_, bool>(5)?;
            let (text, body_error) = match decoded {
                Ok(s) => (s, None),
                Err(e) => (None, Some(e.code)),
            };
            messages.push(Message {
                guid: row.get(2)?,
                chat_guid: row.get(3)?,
                sender: row.get(4)?,
                from_me,
                is_unread: row.get(11)?,
                date: dates::iso(date)?,
                text,
                body_error,
                attachments: self.attachments(row_id)?,
                position: Position {
                    date,
                    message: row_id,
                    chat: chat_id,
                },
            });
        }
        let next_page = if more {
            Some(Page {
                after: messages.last().map(|m| m.position),
                ..page.clone()
            })
        } else {
            None
        };
        Ok((messages, next_page))
    }
    pub fn first_page(&self) -> Result<Page> {
        self.check()?;
        Ok(Page {
            watermark: self.watermark()?,
            after: None,
            started_ns: dates::now(),
            rank_after: None,
            resolved_range: None,
        })
    }
    fn attachments(&self, message: i64) -> Result<Vec<Attachment>> {
        let mime = self.column("attachment", "a", "mime_type", "NULL");
        let size = self.column("attachment", "a", "total_bytes", "NULL");
        let name = self.column("attachment", "a", "transfer_name", "NULL");
        let mut q=self.connection.prepare(&format!("SELECT a.guid,a.filename,{mime},{size},{name} FROM message_attachment_join ma JOIN attachment a ON a.ROWID=ma.attachment_id WHERE ma.message_id=? ORDER BY a.ROWID LIMIT ?"))?;
        let data = q
            .query_map(params![message, (MAX_ATTACHMENTS + 1) as i64], |r| {
                let path = r.get::<_, Option<String>>(1)?.map(PathBuf::from);
                let filename = r.get::<_, Option<String>>(4)?.or_else(|| {
                    path.as_ref()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                });
                Ok(Attachment {
                    id: r.get(0)?,
                    path,
                    filename,
                    mime_type: r.get(2)?,
                    size: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if data.len() > MAX_ATTACHMENTS {
            return Err(Error::new(
                "messagesTooManyAttachments",
                "This message has too many attachments for one response",
            ));
        }
        Ok(data)
    }
    pub fn find_chats(
        &self,
        filter: &Filter,
        name: Option<&str>,
        limit: usize,
    ) -> Result<ChatPage> {
        if !(1..=MAX_PAGE).contains(&limit) {
            return Err(invalid("Chat limit must be between 1 and 200"));
        }
        let filter = filter.normalize()?;
        if filter.text.is_some() {
            return Err(invalid("find_chats does not take message-body search text"));
        }
        let page = self.first_page()?;
        let (conditions, values) = self.conditions(&filter, &page, false)?;
        let date = self.date_sql();
        let display = self.column("chat", "c", "display_name", "NULL");
        let service = self.column("chat", "c", "service_name", "NULL");
        let style = self.column("chat", "c", "style", "0");
        let unread = self.unread_sql("m");
        let mut q=self.connection.prepare(&format!("SELECT c.ROWID,c.guid,{display},{service},{style},MAX({date}) AS last_date,COUNT(DISTINCT CASE WHEN {unread} THEN m.ROWID END) FROM chat c JOIN chat_message_join j ON j.chat_id=c.ROWID JOIN message m ON m.ROWID=j.message_id WHERE {conditions} GROUP BY c.ROWID ORDER BY last_date DESC,c.ROWID DESC"))?;
        let mut rows = q.query(params_from_iter(values))?;
        let mut chats = Vec::new();
        let mut more = false;
        while let Some(row) = rows.next()? {
            self.check()?;
            let display_name: Option<String> = row.get(2)?;
            if let Some(name) = name
                && !display_name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&name.to_lowercase())
            {
                continue;
            }
            if chats.len() == limit {
                more = true;
                break;
            }
            let id = row.get(0)?;
            let participants = self.participants(id)?;
            let is_group = row.get::<_, i64>(4)? == 43 || participants.len() > 1;
            chats.push(Chat {
                row_id: id,
                chat_guid: row.get(1)?,
                name: display_name,
                service: row.get(3)?,
                is_group,
                participants,
                unread_count: row.get(6)?,
                last_message_date: Some(dates::iso(row.get(5)?)?),
            });
        }
        Ok(ChatPage {
            chats,
            has_more: more,
        })
    }
    pub fn activity(&self, filter: &Filter, page: &Page) -> Result<Vec<Activity>> {
        let filter = filter.normalize()?;
        if filter.text.is_some() {
            return Err(invalid("Activity counts do not take body search text"));
        }
        let (conditions, values) = self.conditions(&filter, page, false)?;
        let date = self.date_sql();
        let mut q=self.connection.prepare(&format!("SELECT DISTINCT m.ROWID,c.ROWID,({date}) AS sort_date,COALESCE(m.is_from_me,0) FROM chat_message_join j JOIN message m ON m.ROWID=j.message_id JOIN chat c ON c.ROWID=j.chat_id WHERE {conditions} ORDER BY sort_date"))?;
        let mut rows = q.query(params_from_iter(values))?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            self.check()?;
            result.push(Activity {
                chat: row.get(1)?,
                date: row.get(2)?,
                sent: row.get(3)?,
            });
        }
        Ok(result)
    }
    pub fn chat(&self, id: i64) -> Result<Chat> {
        self.check()?;
        let display = self.column("chat", "c", "display_name", "NULL");
        let service = self.column("chat", "c", "service_name", "NULL");
        let style = self.column("chat", "c", "style", "0");
        let (guid, name, service, style) = self.connection.query_row(
            &format!("SELECT guid,{display},{service},{style} FROM chat c WHERE ROWID=?"),
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )?;
        let participants = self.participants(id)?;
        let unread=self.connection.query_row(&format!("SELECT COUNT(DISTINCT m.ROWID) FROM message m JOIN chat_message_join j ON m.ROWID=j.message_id WHERE j.chat_id=? AND {}",self.unread_sql("m")),[id],|r|r.get(0))?;
        let last:Option<i64>=self.connection.query_row(&format!("SELECT MAX({}) FROM message m JOIN chat_message_join j ON m.ROWID=j.message_id WHERE j.chat_id=?",self.date_sql()),[id],|r|r.get(0))?;
        Ok(Chat {
            row_id: id,
            chat_guid: guid,
            name,
            service,
            is_group: style == 43 || participants.len() > 1,
            participants,
            unread_count: unread,
            last_message_date: last.map(dates::iso).transpose()?,
        })
    }

    pub fn chat_by_guid(&self, guid: &str) -> Result<Chat> {
        self.check()?;
        let row = self
            .connection
            .query_row("SELECT ROWID FROM chat WHERE guid=?", [guid], |r| r.get(0))
            .map_err(|error| {
                if matches!(error, rusqlite::Error::QueryReturnedNoRows) {
                    Error::new(
                        "messagesChatNotFound",
                        "The requested Messages chat was not found",
                    )
                } else {
                    error.into()
                }
            })?;
        self.chat(row)
    }
    pub fn destination_candidates(&self, groups: &[Vec<String>]) -> Result<Vec<Chat>> {
        let filter = Filter {
            participants: groups.to_vec(),
            exact_participants: true,
            ..Default::default()
        }
        .normalize()?;
        let mut conditions = Vec::new();
        let mut values = Vec::<SqlValue>::new();
        for person in &filter.participants {
            conditions.push("EXISTS(SELECT 1 FROM chat_handle_join j JOIN handle h ON h.ROWID=j.handle_id WHERE j.chat_id=c.ROWID AND icu_handle(h.id) IN (SELECT value FROM json_each(?)))".to_string());
            values.push(
                serde_json::to_string(person)
                    .map_err(|_| invalid("Invalid recipients"))?
                    .into(),
            );
        }
        conditions.push("(SELECT COUNT(DISTINCT icu_handle(h.id)) FROM chat_handle_join j JOIN handle h ON h.ROWID=j.handle_id WHERE j.chat_id=c.ROWID)=?".into());
        values.push((filter.participants.len() as i64).into());
        let group = if groups.len() == 1 { "<>43" } else { "=43" };
        if self.columns["chat"].contains("style") {
            conditions.push(format!("COALESCE(c.style,45){group}"));
        }
        let sql = format!(
            "SELECT c.ROWID FROM chat c WHERE {} LIMIT 201",
            conditions.join(" AND ")
        );
        let ids = self
            .connection
            .prepare(&sql)?
            .query_map(params_from_iter(values), |r| r.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if ids.len() > 200 {
            return Err(Error::new(
                "messagesAmbiguousDestination",
                "Too many matching chats; use an exact chat_guid",
            ));
        }
        ids.into_iter().map(|id| self.chat(id)).collect()
    }
    pub fn image_attachment_path(
        &self,
        chat_guid: &str,
        message_guid: &str,
        attachment_id: &str,
    ) -> Result<PathBuf> {
        self.check()?;
        let mut statement=self.connection.prepare("SELECT DISTINCT a.filename FROM attachment a JOIN message_attachment_join ma ON ma.attachment_id=a.ROWID JOIN message m ON m.ROWID=ma.message_id JOIN chat_message_join cm ON cm.message_id=m.ROWID JOIN chat c ON c.ROWID=cm.chat_id WHERE a.guid=? AND m.guid=? AND c.guid=? LIMIT 2")?;
        let rows = statement
            .query_map(params![attachment_id, message_guid, chat_guid], |r| {
                r.get::<_, Option<String>>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if rows.len() != 1 {
            return Err(Error::new(
                "messagesAttachmentNotFound",
                "This attachment is no longer associated with the message that returned it",
            ));
        }
        rows.into_iter()
            .next()
            .flatten()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                Error::new(
                    "messagesAttachmentUnavailable",
                    "The attachment has not been downloaded to this Mac",
                )
            })
    }
}
