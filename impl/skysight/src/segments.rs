//! Raw event-stream storage, as Codex's `SkysightSegmentWriter` lays it out:
//! `segments/<segment_timestamp>/events.jsonl` (append-only records),
//! `metadata.json` (segment timestamps and event counts) and `suppressed.jsonl`
//! (markers for what a rule or guard kept out, with no content). Folders are 0700
//! and files 0600 ([`crate::private`]).
//!
//! Also here: clear-history by scope (Codex's `last_ten_minutes`, `last_hour`,
//! `last_day`, `all`, `interval`, `application_session`), which cascades to the
//! 10min/6h summaries derived from the cleared span, raw-segment expiry, and the
//! recent-application sessions a clear can target.

use crate::stream::{Output, Suppressed};
use crate::{format_ts, parse_ts, private, EventRecord, Store, Ts};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const EVENTS_FILE: &str = "events.jsonl";
pub const METADATA_FILE: &str = "metadata.json";
pub const SUPPRESSED_FILE: &str = "suppressed.jsonl";
/// A segment is closed and a new one opened after this long (INFERRED; bounds file
/// sizes and lets expiry drop whole folders).
pub const SEGMENT_SECS: i64 = 3600;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SegmentMeta {
    pub version: u32,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub event_count: u64,
    pub suppressed_count: u64,
    pub end_reason: Option<String>,
}

impl SegmentMeta {
    pub fn start(&self) -> Ts { self.started_at_ms.div_euclid(1000) }
    /// The end, or `now` for an open segment.
    pub fn end_or(&self, now: Ts) -> Ts { self.ended_at_ms.map(|e| e.div_euclid(1000)).unwrap_or(now) }
}

/// The `segments/` folder.
#[derive(Clone, Debug)]
pub struct Segments { pub root: PathBuf }

/// The segment being written.
pub struct SegmentWriter {
    pub dir: PathBuf,
    pub meta: SegmentMeta,
    events: std::fs::File,
    suppressed: Option<std::fs::File>,
    /// Held while this writer lives: a segment whose lock can be taken has no writer left.
    _lock: Option<std::fs::File>,
}

pub const WRITER_LOCK_FILE: &str = "writer.lock";

/// An exclusive lock on `path` (created 0600), or None when another holder has it.
pub fn try_lock_file(path: &Path) -> Option<std::fs::File> {
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)); }
    f.try_lock().ok().map(|()| f)
}

impl SegmentWriter {
    pub fn events_path(&self) -> PathBuf { self.dir.join(EVENTS_FILE) }
    pub fn metadata_path(&self) -> PathBuf { self.dir.join(METADATA_FILE) }
    pub fn suppressed_path(&self) -> PathBuf { self.dir.join(SUPPRESSED_FILE) }

    /// Whether the folder still exists (a clear may have removed it).
    pub fn alive(&self) -> bool { self.dir.is_dir() }

    pub fn write(&mut self, out: &Output) -> Result<(), String> {
        match out {
            Output::Record(r) => {
                writeln!(self.events, "{}", serde_json::to_string(r).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                self.meta.event_count += 1;
            }
            Output::Suppressed(s) => {
                if self.suppressed.is_none() { self.suppressed = Some(private::append(&self.suppressed_path())?); }
                writeln!(self.suppressed.as_mut().unwrap(), "{}", serde_json::to_string(s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                self.meta.suppressed_count += 1;
            }
        }
        Ok(())
    }

    pub fn sync_meta(&self) -> Result<(), String> { write_meta(&self.dir, &self.meta) }

    pub fn close(mut self, now_ms: i64, reason: &str) -> Result<(), String> {
        self.meta.ended_at_ms = Some(now_ms);
        self.meta.end_reason = Some(reason.into());
        if self.alive() { self.sync_meta() } else { Ok(()) }
    }
}

fn write_meta(dir: &Path, meta: &SegmentMeta) -> Result<(), String> {
    let tmp = dir.join("metadata.json.tmp");
    private::write(&tmp, serde_json::to_vec_pretty(meta).map_err(|e| e.to_string())?)?;
    std::fs::rename(&tmp, dir.join(METADATA_FILE)).map_err(|e| e.to_string())
}

/// Parsed segment event files, by path, with the length and modification time they were read at.
#[derive(Default)]
pub struct EventCache {
    files: std::collections::HashMap<PathBuf, ((u64, Option<std::time::SystemTime>), Vec<EventRecord>)>,
    /// How many files were parsed (for tests).
    pub reads: usize,
}

/// What a clear removed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cleared { pub records: usize, pub segments: usize, pub summaries: Vec<String>,
    /// Summaries removed that held more than what was cleared but start before the rebuild window
    /// ([`SUMMARY_BACKFILL_SECS`]): nothing will write them again (re-review 3).
    #[serde(default)]
    pub not_rebuilt: usize,
    /// Later summaries kept with their "Relevant prior context" section emptied, because it was
    /// written from a removed one (fresh review 3, item 2).
    #[serde(default)]
    pub stripped: Vec<String> }

/// Codex's clear-history scopes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClearScope { LastTenMinutes, LastHour, LastDay, All, Interval, ApplicationSession }

impl ClearScope {
    pub fn parse(s: &str) -> Option<ClearScope> { serde_json::from_value(serde_json::Value::String(s.into())).ok() }
    /// The span `[from, to)` a scope clears at `now` (`interval` and `application_session`
    /// take theirs from the caller, in ms as Codex sends them).
    pub fn span(self, now: Ts, interval: Option<(i64, i64)>) -> Result<(Ts, Ts), String> {
        let to = now + 1;
        Ok(match self {
            ClearScope::LastTenMinutes => (now - 600, to),
            ClearScope::LastHour => (now - 3600, to),
            ClearScope::LastDay => (now - 86_400, to),
            ClearScope::All => (i64::MIN / 4, i64::MAX / 4),
            ClearScope::Interval | ClearScope::ApplicationSession => {
                let (s, e) = interval.ok_or("this scope needs an interval {startMs, endMs}")?;
                if e <= s { return Err("interval must end after it starts".into()); }
                (s.div_euclid(1000), (e + 999).div_euclid(1000))
            }
        })
    }
}

/// One app's stretch in front, from the event stream (what `application_session` clears).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSession { pub bundle_id: Option<String>, pub name: Option<String>, pub started_at_ms: i64, pub ended_at_ms: i64 }

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn record_ts(line: &str) -> Option<Ts> {
    serde_json::from_str::<serde_json::Value>(line).ok()?.get("ts")?.as_str().and_then(parse_ts)
}

impl Segments {
    pub fn new(root: impl Into<PathBuf>) -> Segments { Segments { root: root.into() } }

    /// Close what a writer left open: the app quit or crashed while recording (seen on the real
    /// Mac run, where quitting the app left `endedAtMs: null`). The end is the last line written,
    /// the counts are what the files hold, and the reason is `interrupted`.
    pub fn close_interrupted(&self) -> usize { self.close_interrupted_idle(None) }

    /// [`Segments::close_interrupted`], leaving alone any segment written to in the last
    /// `idle.1` seconds before `idle.0` (prune runs while a recorder may be writing; re-review 7).
    pub fn close_interrupted_idle(&self, idle: Option<(Ts, Ts)>) -> usize {
        let mut n = 0;
        for (dir, mut meta) in self.list() {
            if meta.ended_at_ms.is_some() { continue; }
            // A writer still holds it (another recorder process, or this one): not interrupted.
            let lock = dir.join(WRITER_LOCK_FILE);
            if lock.exists() && try_lock_file(&lock).is_none() { continue; }
            let lines = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect::<Vec<_>>();
            let (events, suppressed) = (lines(EVENTS_FILE), lines(SUPPRESSED_FILE));
            let last = events.iter().chain(&suppressed).filter_map(|l| record_ts(l)).max();
            // written to lately (by its record times): a recorder may still have it open
            if idle.is_some_and(|(now, secs)| last.unwrap_or(meta.start()).max(meta.start()) > now - secs) { continue; }
            meta.ended_at_ms = Some(last.map(|t| t * 1000).unwrap_or(meta.started_at_ms).max(meta.started_at_ms));
            meta.end_reason = Some("interrupted".into());
            meta.event_count = events.len() as u64;
            meta.suppressed_count = suppressed.len() as u64;
            if write_meta(&dir, &meta).is_ok() { n += 1; }
        }
        n
    }

    /// Open a new segment at `now_ms`, closing any segment a previous writer left open.
    pub fn open_segment(&self, now_ms: i64) -> Result<SegmentWriter, String> {
        private::create_dir(&self.root)?;
        self.close_interrupted();
        let base = format_ts(now_ms.div_euclid(1000));
        let mut dir = self.root.join(&base);
        let mut n = 1;
        while dir.exists() { dir = self.root.join(format!("{base}-{n}")); n += 1; }
        private::create_dir(&dir)?;
        let meta = SegmentMeta { version: 1, started_at_ms: now_ms, ..Default::default() };
        write_meta(&dir, &meta)?;
        let _lock = try_lock_file(&dir.join(WRITER_LOCK_FILE));
        let events = private::append(&dir.join(EVENTS_FILE))?;
        Ok(SegmentWriter { dir, meta, events, suppressed: None, _lock })
    }

    /// Every segment with its metadata, oldest first. A segment without readable
    /// metadata takes its start from the folder name.
    pub fn list(&self) -> Vec<(PathBuf, SegmentMeta)> {
        let mut v: Vec<(PathBuf, SegmentMeta)> = std::fs::read_dir(&self.root).into_iter().flatten().filter_map(|e| e.ok())
            .map(|e| e.path()).filter(|p| p.is_dir() && !p.is_symlink())
            .filter_map(|p| {
                let meta = std::fs::read(p.join(METADATA_FILE)).ok().and_then(|b| serde_json::from_slice::<SegmentMeta>(&b).ok())
                    .or_else(|| parse_ts(p.file_name()?.to_str()?.get(..19)?).map(|t| SegmentMeta { version: 1, started_at_ms: t * 1000, ..Default::default() }))?;
                Some((p, meta))
            }).collect();
        v.sort_by_key(|(_, m)| m.started_at_ms);
        v
    }

    /// Records with `from <= ts < to`, in time order.
    pub fn events_between(&self, from: Ts, to: Ts, now: Ts) -> Vec<EventRecord> {
        let mut out: Vec<EventRecord> = self.list().into_iter()
            .filter(|(_, m)| m.start() < to && m.end_or(now) >= from)
            .flat_map(|(p, _)| read_jsonl::<EventRecord>(&p.join(EVENTS_FILE)))
            .filter(|r| r.timestamp().is_some_and(|t| t >= from && t < to)).collect();
        out.sort_by_key(|r| r.timestamp());
        out
    }

    /// [`Segments::events_between`] reading only the segment files that changed since the last
    /// call: a pass every minute over two days of segments parsed every file every time (review
    /// finding 11). A file is re-read when its length or modification time differs.
    pub fn events_between_cached(&self, cache: &mut EventCache, from: Ts, to: Ts, now: Ts) -> Vec<EventRecord> {
        let live: Vec<(PathBuf, SegmentMeta)> = self.list().into_iter().filter(|(_, m)| m.start() < to && m.end_or(now) >= from).collect();
        cache.files.retain(|p, _| live.iter().any(|(d, _)| d.join(EVENTS_FILE) == *p));
        let mut out = Vec::new();
        for (dir, _) in live {
            let path = dir.join(EVENTS_FILE);
            let stamp = std::fs::metadata(&path).ok().map(|m| (m.len(), m.modified().ok()));
            let fresh = cache.files.get(&path).is_some_and(|(s, _)| Some(s) == stamp.as_ref());
            if !fresh {
                cache.reads += 1;
                let Some(st) = stamp else { continue };
                cache.files.insert(path.clone(), (st, read_jsonl::<EventRecord>(&path)));
            }
            if let Some((_, evs)) = cache.files.get(&path) {
                out.extend(evs.iter().filter(|r| r.timestamp().is_some_and(|t| t >= from && t < to)).cloned());
            }
        }
        out.sort_by_key(|r| r.timestamp());
        out
    }

    pub fn suppressed_between(&self, from: Ts, to: Ts, now: Ts) -> Vec<Suppressed> {
        self.list().into_iter().filter(|(_, m)| m.start() < to && m.end_or(now) >= from)
            .flat_map(|(p, _)| read_jsonl::<Suppressed>(&p.join(SUPPRESSED_FILE)))
            .filter(|s| parse_ts(&s.ts).is_some_and(|t| t >= from && t < to)).collect()
    }

    /// Remove records with `from <= ts < to` (only `bundle`'s, when given) from every
    /// segment, deleting a segment left with nothing. Lines whose time cannot be read are
    /// removed by `all` only.
    pub fn clear(&self, from: Ts, to: Ts, bundle: Option<&str>, now: Ts) -> Result<Cleared, String> {
        let mut c = Cleared::default();
        for (dir, mut meta) in self.list() {
            if meta.start() >= to || meta.end_or(now) < from { continue; }
            let mut kept_events = 0u64;
            for (file, count) in [(EVENTS_FILE, true), (SUPPRESSED_FILE, false)] {
                let path = dir.join(file);
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let hit = |l: &str| -> bool {
                    let in_span = record_ts(l).map_or(bundle.is_none() && from <= i64::MIN / 4 + 1, |t| t >= from && t < to);
                    in_span && bundle.is_none_or(|b| serde_json::from_str::<EventRecord>(l).ok().and_then(|r| r.bundle_id).as_deref() == Some(b))
                };
                let removed = text.lines().filter(|l| !l.trim().is_empty() && hit(l)).count();
                let kept: String = text.lines().filter(|l| !l.trim().is_empty() && !hit(l)).map(|l| format!("{l}\n")).collect();
                if count { c.records += removed; kept_events = kept.lines().count() as u64; }
                if removed > 0 {
                    let tmp = path.with_extension("jsonl.tmp");
                    private::write(&tmp, &kept)?;
                    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
                }
                if !count { meta.suppressed_count = kept.lines().count() as u64; }
            }
            let whole = meta.start() >= from && meta.end_or(now) < to && bundle.is_none();
            if whole || (kept_events == 0 && meta.ended_at_ms.is_some()) {
                std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
                c.segments += 1;
            } else {
                meta.event_count = kept_events;
                write_meta(&dir, &meta)?;
            }
        }
        Ok(c)
    }

    /// Delete closed segments that ended before `now - keep_secs`. Returns how many.
    pub fn prune(&self, now: Ts, keep_secs: i64) -> usize {
        // A segment left open by a recorder that quit or crashed is closed first, so it can expire
        // even if recording is never turned on again (fresh review, F6).
        self.close_interrupted_idle(Some((now, 600)));
        let cutoff = now.saturating_sub(keep_secs);
        self.list().into_iter().filter(|(_, m)| m.ended_at_ms.is_some() && m.end_or(now) < cutoff)
            .filter(|(p, _)| std::fs::remove_dir_all(p).is_ok()).count()
    }

    /// The apps that were in front since `since`, as sessions (newest first): a session
    /// runs from one `window.changed` into an app to the next record from another app.
    pub fn recent_applications(&self, since: Ts, now: Ts, limit: usize) -> Vec<AppSession> {
        let events = self.events_between(since, now + 1, now);
        let mut out: Vec<AppSession> = Vec::new();
        for e in events.iter().filter(|e| e.bundle_id.is_some() || e.app_name.is_some()) {
            let t = e.timestamp().unwrap_or(now) * 1000;
            match out.last_mut() {
                Some(s) if s.bundle_id == e.bundle_id => s.ended_at_ms = t.max(s.ended_at_ms),
                _ => {
                    if let Some(s) = out.last_mut() { s.ended_at_ms = s.ended_at_ms.max(t); }
                    out.push(AppSession { bundle_id: e.bundle_id.clone(), name: e.app_name.clone(), started_at_ms: t, ended_at_ms: t });
                }
            }
        }
        out.reverse();
        out.truncate(limit);
        out
    }
}

/// How far back the app writes summaries it has not written yet (the rolling refresh's backfill).
pub const SUMMARY_BACKFILL_SECS: Ts = 2 * 86_400;

/// Delete the 10min/6h summaries whose window overlaps `[from, to)` (and, with
/// `bundle`, that name it), so the rolling refresh rebuilds them from what is left and
/// phase 2 sees them deleted. Returns their file names.
pub fn clear_summaries(store: &Store, from: Ts, to: Ts, bundle: Option<&str>) -> Result<Vec<String>, String> {
    Ok(clear_summaries_with(store, from, to, bundle, Ts::MIN)?.removed)
}

/// What [`clear_summaries_with`] did to the summaries folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SummaryClear {
    /// Deleted: the summaries over the cleared span, and every 6-hour rollup over a removed or
    /// stripped window (the rolling refresh writes them again: 10-minute ones from what is left,
    /// rollups from the stored 10-minute summaries, at any age).
    pub removed: Vec<String>,
    /// Kept, with their "Relevant prior context" section emptied: every later 10-minute summary.
    pub stripped: Vec<String>,
    /// How many 10-minute summaries in `removed` start before the rebuild window and still had
    /// something outside the cleared span (another app's records, or time outside it): nothing
    /// writes those again.
    pub not_rebuilt: usize,
}

/// The text a stripped summary's "Relevant prior context" section is left with.
pub const PRIOR_CLEARED: &str = "Removed when history was cleared.";

/// [`clear_summaries`], keeping later summaries and only taking out what they may restate.
///
/// A summary is written with the three summaries that end before its window as "Relevant prior
/// context", given whole (their own prior context included), so what was cleared can live on in
/// the summaries after the span, at any distance and through any chain of them. The summaries over
/// the span are deleted and rebuilt. Every later 10-minute summary keeps its own window and loses
/// that section (round 4, item 2: stripping only the ones written directly on a removed one left
/// the second ring, written on those, restating it); this costs no model calls. A 6-hour rollup
/// is written from its children's whole text, so one over a removed or stripped window may hold
/// the cleared facts in its own body: it is deleted and written again from the stored children.
pub fn clear_summaries_with(store: &Store, from: Ts, to: Ts, bundle: Option<&str>, rebuild_from: Ts) -> Result<SummaryClear, String> {
    let all = store.list();
    let names_bundle = |n: &crate::SummaryName| bundle.is_none_or(|b| store.read(n).map(|s| s.frontmatter.applications.iter().any(|a| a == b)).unwrap_or(true));
    let mut gone: std::collections::BTreeSet<usize> = all.iter().enumerate()
        .filter(|(_, n)| { let w = n.window(); w.start < to && w.end() > from && names_bundle(n) }).map(|(i, _)| i).collect();
    let ten = |i: usize| all[i].cadence == crate::Cadence::TenMin;
    // every later 10-minute summary, from the first one removed on
    let first = gone.iter().filter(|&&i| ten(i)).map(|&i| all[i].start).min();
    let strip: std::collections::BTreeSet<usize> = match first {
        Some(f) => (0..all.len()).filter(|&i| ten(i) && !gone.contains(&i) && all[i].start >= f).collect(),
        None => Default::default(),
    };
    // and every rollup over a removed or stripped window
    let rollups: Vec<usize> = (0..all.len()).filter(|&i| !ten(i) && !gone.contains(&i)
        && gone.iter().chain(strip.iter()).any(|&j| ten(j) && all[i].window().contains(all[j].start))).collect();
    gone.extend(rollups);
    let mut out = SummaryClear::default();
    for &i in &gone {
        let w = all[i].window();
        let others = bundle.is_some_and(|b| store.read(&all[i]).map(|s| s.frontmatter.applications.iter().any(|a| a != b)).unwrap_or(false));
        std::fs::remove_file(store.path(&all[i])).map_err(|e| e.to_string())?;
        if ten(i) && w.start < rebuild_from && (others || w.start < from || w.end() > to) { out.not_rebuilt += 1; }
        out.removed.push(all[i].file_name());
    }
    for &i in &strip {
        match store.read(&all[i]) {
            Ok(mut s) => {
                if s.relevant_prior_context.trim() == PRIOR_CLEARED { continue; }
                s.relevant_prior_context = PRIOR_CLEARED.into();
                store.write(all[i].window(), &s)?;
                out.stripped.push(all[i].file_name());
            }
            // not readable as a summary: nothing to keep, and it may hold what was cleared
            Err(_) => { std::fs::remove_file(store.path(&all[i])).map_err(|e| e.to_string())?; out.removed.push(all[i].file_name()); }
        }
    }
    Ok(out)
}
