//! Host-reviewed deletion uses frozen event identities, never a live time query.
use super::*;
use std::collections::BTreeSet;

const MAX_SCAN: usize = 128 * 1024 * 1024;
const MAX_SELECTED: usize = 100_000;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteFilter {
    pub application: Option<String>,
    pub from_ms: Option<u64>,
    pub to_ms: Option<u64>,
    #[serde(default)]
    pub event_ids: BTreeSet<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventIdentity {
    pub id: String,
    pub application: String,
    pub timestamp_unix_ms: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeletePreview {
    pub generation: String,
    pub revision: u64,
    pub events: Vec<EventIdentity>,
}
impl DeleteFilter {
    fn validate(&self) -> Result<()> {
        if self.application.as_ref().is_some_and(|s| s.is_empty() || s.len() > 240 || s.chars().any(char::is_control))
            || self.from_ms.zip(self.to_ms).is_some_and(|(a,b)| a >= b)
            || self.event_ids.len() > MAX_SELECTED
            || self.event_ids.iter().any(|id| !valid_id(id)) {
            return Err(Error::invalid("Invalid history deletion selection"));
        }
        Ok(())
    }
    fn matches(&self, row: &EventIdentity) -> bool {
        self.application.as_ref().is_none_or(|app| app == &row.application)
            && self.from_ms.is_none_or(|from| row.timestamp_unix_ms >= from)
            && self.to_ms.is_none_or(|to| row.timestamp_unix_ms < to)
            && (self.event_ids.is_empty() || self.event_ids.contains(&row.id))
    }
}
fn valid_id(id: &str) -> bool { id.len() == 48 && id.bytes().all(|b| b.is_ascii_hexdigit()) }
fn identity(line: &[u8]) -> Result<EventIdentity> {
    let row: Value = serde_json::from_slice(line).map_err(|_| failed())?;
    let id = row["id"].as_str().ok_or_else(failed)?;
    let application = row["application"].as_str().ok_or_else(failed)?;
    if !valid_id(id) || application.is_empty() || application.len() > 240 || application.chars().any(char::is_control) {
        return Err(failed());
    }
    Ok(EventIdentity { id: id.into(), application: application.into(),
        timestamp_unix_ms: row["timestamp_unix_ms"].as_u64().ok_or_else(failed)? })
}
impl Store {
    pub(super) fn segment(&self, path: &Path) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        file(path, false, false)?.take(MAX_FILE + 1).read_to_end(&mut bytes).map_err(|_| failed())?;
        if bytes.len() as u64 > MAX_FILE || (!bytes.is_empty() && bytes.last() != Some(&b'\n')) {
            return Err(Error::new("computerHistoryIncompleteSegment", "History segment is incomplete or exceeds its bound"));
        }
        Ok(bytes)
    }
    fn segments(&self) -> Result<Vec<(PathBuf, Vec<u8>)>> {
        let mut result = Vec::new();
        let mut total = 0;
        for entry in fs::read_dir(self.root.join("activity")).map_err(|_| failed())? {
            let entry = entry.map_err(|_| failed())?;
            if segment_day(&entry.file_name().to_string_lossy()).is_none() { continue; }
            let bytes = self.segment(&entry.path())?;
            total += bytes.len();
            if total > MAX_SCAN { return Err(Error::invalid("History selection exceeds 128 MiB; shorten retention before retrying")); }
            result.push((entry.path(), bytes));
        }
        result.sort_by(|a,b| a.0.cmp(&b.0));
        Ok(result)
    }
    pub fn preview_delete(&self, filter: &DeleteFilter) -> Result<DeletePreview> {
        filter.validate()?;
        let _guard = self.lock()?;
        let state = self.load()?;
        let mut events = Vec::new();
        let mut seen = BTreeSet::new();
        for (_, bytes) in self.segments()? {
            for line in bytes.split_inclusive(|b| *b == b'\n') {
                let row = identity(line)?;
                if !seen.insert(row.id.clone()) { return Err(Error::invalid("History contains duplicate event identities")); }
                if filter.matches(&row) { events.push(row); }
                if events.len() > MAX_SELECTED { return Err(Error::invalid("Select fewer than 100000 history observations")); }
            }
        }
        Ok(DeletePreview { generation: state.generation, revision: state.revision, events })
    }
    /// The derivative callback executes under the History barrier before raw
    /// removal. It must invalidate in-flight memory writes before deleting any
    /// derivative. A failure never acknowledges deletion or resumes collection.
    pub fn delete_reviewed(&self, preview: &DeletePreview, delete_derived: impl FnOnce(&[EventIdentity]) -> Result<()>) -> Result<usize> {
        if preview.events.len() > MAX_SELECTED { return Err(Error::invalid("History selection is too large")); }
        let _guard = self.lock()?;
        let mut state = self.load()?;
        if state.generation != preview.generation || state.revision != preview.revision {
            return Err(Error::new("computerHistorySettingsChanged", "History changed; review the deletion again"));
        }
        let ids: BTreeSet<_> = preview.events.iter().map(|e| e.id.clone()).collect();
        if ids.len() != preview.events.len() || ids.iter().any(|id| !valid_id(id)) {
            return Err(Error::invalid("History selection has invalid or duplicate identities"));
        }
        let mut actual = Vec::new();
        let mut rewrites = Vec::new();
        for (path, bytes) in self.segments()? {
            let mut kept = Vec::new();
            for line in bytes.split_inclusive(|b| *b == b'\n') {
                let row = identity(line)?;
                if ids.contains(&row.id) { actual.push(row); }
                else { kept.extend_from_slice(line); }
            }
            if kept.len() != bytes.len() { rewrites.push((path, kept)); }
        }
        actual.sort_by(|a,b| a.id.cmp(&b.id));
        let mut expected = preview.events.clone(); expected.sort_by(|a,b| a.id.cmp(&b.id));
        if actual != expected { return Err(Error::invalid("Selected activity changed or expired; review the deletion again")); }
        state.revision = state.revision.checked_add(1).ok_or_else(failed)?;
        state.paused = true;
        if !state.summary_epoch.is_empty() { state.summary_epoch = token()?; }
        self.write(&state)?;
        delete_derived(&actual)?;
        for (path, bytes) in rewrites {
            if bytes.is_empty() { fs::remove_file(path).map_err(|_| failed())?; }
            else { self.replace(&path, &bytes)?; }
        }
        Ok(actual.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("history")).unwrap();
        let path = store.root.join("activity/events-00000010-000.jsonl");
        let rows = [(1,"alpha",10*DAY+1),(2,"beta",10*DAY+2),(3,"alpha",10*DAY+3)];
        let bytes = rows.into_iter().map(|(id,app,ts)| format!("{}\n",json!({"id":format!("{id:048x}"),"application":app,"timestamp_unix_ms":ts}))).collect::<String>();
        store.replace(&path,bytes.as_bytes()).unwrap();
        (dir,store)
    }
    #[test]
    fn reviewed_delete_keeps_other_apps_and_new_appends() {
        let (_dir,store)=fixture();
        let p=store.preview_delete(&DeleteFilter{application:Some("alpha".into()),..Default::default()}).unwrap();
        let path=store.root.join("activity/events-00000010-000.jsonl");
        let extra=format!("{}\n",json!({"id":format!("{:048x}",4),"application":"alpha","timestamp_unix_ms":10*DAY+4}));
        file(&path,false,true).unwrap().write_all(extra.as_bytes()).unwrap();
        let n=store.delete_reviewed(&p,|rows|{assert_eq!(rows.len(),2);Ok(())}).unwrap();
        assert_eq!(n,2);
        let kept=store.preview_delete(&DeleteFilter::default()).unwrap();
        assert_eq!(kept.events.iter().map(|e|e.id.clone()).collect::<Vec<_>>(),vec![format!("{:048x}",2),format!("{:048x}",4)]);
        assert!(store.state().unwrap().paused);
        assert!(store.delete_reviewed(&p,|_|panic!("stale callback")).is_err());
    }
    #[test]
    fn derivative_failure_pauses_and_keeps_raw_for_retry() {
        let (_dir,store)=fixture();
        let p=store.preview_delete(&DeleteFilter::default()).unwrap();
        assert!(store.delete_reviewed(&p,|_|Err(failed())).is_err());
        assert!(store.state().unwrap().paused);
        assert_eq!(store.preview_delete(&DeleteFilter::default()).unwrap().events.len(),3);
    }
    #[test]
    fn exact_retention_boundary_keeps_equal_and_future_events() {
        let (_dir,store)=fixture();
        assert_eq!(store.prune(11*DAY+2).unwrap(),0); // default two days
        let s=store.state().unwrap();store.retention(1,(&s.generation,s.revision)).unwrap();
        assert_eq!(store.prune(11*DAY+2).unwrap(),1);
        let rows=store.preview_delete(&DeleteFilter::default()).unwrap().events;
        assert_eq!(rows.len(),2);assert_eq!(rows[0].timestamp_unix_ms,10*DAY+2);
        assert_eq!(store.prune(9*DAY).unwrap(),0);
        assert_eq!(store.preview_delete(&DeleteFilter::default()).unwrap().events.len(),2);
    }
    #[test]
    fn range_is_half_open_and_forged_identity_is_rejected() {
        let (_dir,store)=fixture();
        let mut p=store.preview_delete(&DeleteFilter{from_ms:Some(10*DAY+1),to_ms:Some(10*DAY+3),..Default::default()}).unwrap();
        assert_eq!(p.events.len(),2);
        p.events[0].application="other".into();
        assert!(store.delete_reviewed(&p,|_|panic!("forged callback")).is_err());
        assert!(!store.state().unwrap().paused);
    }
    #[cfg(unix)]
    #[test]
    fn a_segment_link_never_reads_or_rewrites_an_external_file() {
        let (dir,store)=fixture();let external=dir.path().join("external");
        fs::write(&external,b"private fixture").unwrap();
        std::os::unix::fs::symlink(&external,store.root.join("activity/events-00000010-001.jsonl")).unwrap();
        assert!(store.preview_delete(&DeleteFilter::default()).is_err());
        assert_eq!(fs::read(external).unwrap(),b"private fixture");
    }
}
