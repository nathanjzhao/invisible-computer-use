use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// Captures all three append boundaries so a new join cannot reintroduce an old message.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Watermark {
    pub message: i64,
    pub chat: i64,
    pub join: i64,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub date: i64,
    pub message: i64,
    pub chat: i64,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct RankPosition {
    pub count: u64,
    pub last_date: i64,
    pub chat: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub watermark: Watermark,
    pub after: Option<Position>,
    pub started_ns: i64,
    pub rank_after: Option<RankPosition>,
    pub resolved_range: Option<(i64, i64)>,
}
struct Entry {
    query: [u8; 32],
    page: Page,
    created: Instant,
}
#[derive(Default)]
pub struct Cursors {
    entries: HashMap<String, Entry>,
}
impl Cursors {
    pub fn query_key<T: Serialize>(query: &T) -> Result<[u8; 32]> {
        let bytes =
            serde_json::to_vec(query).map_err(|_| crate::invalid("invalid cursor query"))?;
        Ok(Sha256::digest(bytes).into())
    }
    pub fn load(&mut self, token: &str, query: [u8; 32]) -> Result<Page> {
        self.expire();
        let entry=self.entries.get(token).filter(|e|e.query==query).ok_or_else(||Error::new("messagesInvalidCursor","Cursor is expired or belongs to different filters; use the preceding next_cursor unchanged."))?;
        Ok(entry.page.clone())
    }
    pub fn save(&mut self, query: [u8; 32], page: Page) -> Result<String> {
        self.expire();
        if self.entries.len() >= 256
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.created)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
        let mut bytes = [0u8; 24];
        getrandom::fill(&mut bytes)
            .map_err(|_| Error::new("internalError", "Secure cursor generation failed"))?;
        let token = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        self.entries.insert(
            token.clone(),
            Entry {
                query,
                page,
                created: Instant::now(),
            },
        );
        Ok(token)
    }
    fn expire(&mut self) {
        self.entries
            .retain(|_, e| e.created.elapsed() < Duration::from_secs(1800));
    }
}
