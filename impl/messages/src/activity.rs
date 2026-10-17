use crate::{
    Result,
    cursor::{Cursors, Page, RankPosition},
    database::{Chat, Database, Filter},
    dates::{self, Bucket, Interval},
    invalid,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Breakdown {
    #[default]
    Overall,
    Chat,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RankBy {
    #[default]
    Total,
    Sent,
    Received,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub filter: Filter,
    pub interval: Interval,
    pub time_zone: String,
    pub breakdown: Breakdown,
    pub rank_by: RankBy,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Counts {
    pub total: u64,
    pub sent: u64,
    pub received: u64,
}
impl Counts {
    fn add(&mut self, sent: bool) {
        self.total += 1;
        if sent {
            self.sent += 1
        } else {
            self.received += 1
        }
    }
    fn ranked(&self, rank: RankBy) -> u64 {
        match rank {
            RankBy::Total => self.total,
            RankBy::Sent => self.sent,
            RankBy::Received => self.received,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BucketCounts {
    pub total: Vec<u64>,
    pub sent: Vec<u64>,
    pub received: Vec<u64>,
}
impl BucketCounts {
    fn new(n: usize) -> Self {
        Self {
            total: vec![0; n],
            sent: vec![0; n],
            received: vec![0; n],
        }
    }
    fn add(&mut self, index: usize, sent: bool) {
        self.total[index] += 1;
        if sent {
            self.sent[index] += 1
        } else {
            self.received[index] += 1
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityCount {
    #[serde(flatten)]
    pub counts: Counts,
    pub counts_by_bucket: BucketCounts,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatActivity {
    pub chat: Chat,
    #[serde(flatten)]
    pub activity: ActivityCount,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub from: String,
    pub to: String,
    pub time_zone: String,
    pub interval: Interval,
    pub buckets: Vec<Bucket>,
    pub matching_chat_count: usize,
    pub overall_activity: ActivityCount,
    pub chats: Vec<ChatActivity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl Query {
    pub fn key(&self) -> Result<[u8; 32]> {
        Cursors::query_key(&(
            "count_message_activity",
            self.filter.normalize()?,
            self.interval,
            &self.time_zone,
            self.breakdown,
            self.rank_by,
        ))
    }
    pub fn run(
        &self,
        db: &Database,
        page: Page,
        limit: usize,
        cursors: &mut Cursors,
    ) -> Result<Report> {
        let (mut report, next_page) = self.evaluate(db, page, limit)?;
        report.next_cursor = next_page
            .map(|page| cursors.save(self.key()?, page))
            .transpose()?;
        Ok(report)
    }

    pub fn evaluate(
        &self,
        db: &Database,
        mut page: Page,
        limit: usize,
    ) -> Result<(Report, Option<Page>)> {
        if !(1..=200).contains(&limit) {
            return Err(invalid("Chat page limit must be 1..200"));
        }
        if self.breakdown == Breakdown::Overall && page.rank_after.is_some() {
            return Err(invalid("Cursors are only available for chat breakdowns"));
        }
        let mut filter = self.filter.normalize()?;
        if filter.unread_only || filter.text.is_some() {
            return Err(invalid(
                "Activity filters do not take unread_only or message text",
            ));
        }
        let requested_to = page
            .resolved_range
            .map(|(_, to)| to)
            .or(filter.to)
            .unwrap_or(page.started_ns);
        filter.to = Some(requested_to);
        if let Some((from, _)) = page.resolved_range {
            filter.from = Some(from);
        }
        let rows = db.activity(&filter, &page)?;
        let from = page
            .resolved_range
            .map(|(from, _)| from)
            .or(filter.from)
            .unwrap_or_else(|| rows.first().map(|r| r.date).unwrap_or(requested_to));
        let to = requested_to;
        let buckets = dates::buckets(from, to, self.interval, &self.time_zone)?;
        page.resolved_range = Some((from, to));
        let mut overall = ActivityCount {
            counts: Counts::default(),
            counts_by_bucket: BucketCounts::new(buckets.len()),
        };
        let mut summaries: BTreeMap<i64, (Counts, i64)> = BTreeMap::new();
        let bucket_index = |date: i64| -> Result<usize> {
            let index = buckets.partition_point(|b| b.upper <= date);
            if buckets
                .get(index)
                .is_some_and(|b| b.lower <= date && date < b.upper)
            {
                Ok(index)
            } else {
                Err(invalid(
                    "Message timestamp is outside the resolved activity range",
                ))
            }
        };
        for row in &rows {
            let i = bucket_index(row.date)?;
            overall.counts.add(row.sent);
            overall.counts_by_bucket.add(i, row.sent);
            let entry = summaries
                .entry(row.chat)
                .or_insert((Counts::default(), row.date));
            entry.0.add(row.sent);
            entry.1 = entry.1.max(row.date);
        }
        let matching_chat_count = summaries.len();
        let mut chats = Vec::new();
        let mut next_page = None;
        if self.breakdown == Breakdown::Chat {
            let mut ranked = summaries
                .iter()
                .map(|(&chat, (counts, date))| RankPosition {
                    count: counts.ranked(self.rank_by),
                    last_date: *date,
                    chat,
                })
                .collect::<Vec<_>>();
            ranked.sort_unstable_by(|a, b| b.cmp(a));
            if let Some(after) = page.rank_after {
                ranked.retain(|p| *p < after);
            }
            let more = ranked.len() > limit;
            ranked.truncate(limit);
            let mut selected: BTreeMap<i64, BucketCounts> = ranked
                .iter()
                .map(|p| (p.chat, BucketCounts::new(buckets.len())))
                .collect();
            for row in &rows {
                if let Some(counts) = selected.get_mut(&row.chat) {
                    counts.add(bucket_index(row.date)?, row.sent);
                }
            }
            for position in &ranked {
                chats.push(ChatActivity {
                    chat: db.chat(position.chat)?,
                    activity: ActivityCount {
                        counts: summaries[&position.chat].0.clone(),
                        counts_by_bucket: selected.remove(&position.chat).unwrap(),
                    },
                });
            }
            if more {
                page.rank_after = ranked.last().copied();
                next_page = Some(page);
            }
        }
        Ok((
            Report {
                from: dates::iso(from)?,
                to: dates::iso(to)?,
                time_zone: self.time_zone.clone(),
                interval: self.interval,
                buckets,
                matching_chat_count,
                overall_activity: overall,
                chats,
                next_cursor: None,
            },
            next_page,
        ))
    }
}
