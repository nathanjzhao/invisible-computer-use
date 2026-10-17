//! Persistent dispatch boundary. Unknown outcomes require explicit human reconciliation.
use crate::{Error, Result, invalid};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    InFlight,
    Accepted,
    NotSent,
    Uncertain,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub version: u32,
    pub id: String,
    pub fingerprint: String,
    pub outcome: Outcome,
    pub created: u64,
    pub updated: u64,
    pub reason: Option<String>,
    #[serde(default)]
    pub destination: Option<serde_json::Value>,
}
pub struct Journal {
    directory: PathBuf,
}
pub struct Attempt {
    entry: Entry,
    active_lock: File,
}
impl Attempt {
    pub fn id(&self) -> &str {
        &self.entry.id
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn storage() -> Error {
    Error::new(
        "messagesSendJournalUnavailable",
        "The send journal could not be checked or saved; no new send was started",
    )
}
fn unknown() -> Error {
    Error::new(
        "messagesSendOutcomeUncertain",
        "A matching send is still active or has an uncertain outcome. Do not retry automatically. Review the pending send before reconciling it.",
    )
}
fn private_file(path: &Path, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(create).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let file = options.open(path).map_err(|_| storage())?;
    let m = file.metadata().map_err(|_| storage())?;
    if !m.is_file() {
        return Err(storage());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            return Err(storage());
        }
    }
    Ok(file)
}
fn lock(file: &File, cancel: &AtomicBool) -> Result<()> {
    let start = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::new(
                "messagesSendCanceled",
                "The send was canceled before dispatch",
            ));
        }
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    && start.elapsed() < Duration::from_secs(1) =>
            {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(_) => return Err(storage()),
        }
    }
}
impl Journal {
    pub fn open(directory: PathBuf) -> Result<Self> {
        if !directory.is_absolute() {
            return Err(invalid("Send journal directory must be absolute"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, MetadataExt};
            let mut b = fs::DirBuilder::new();
            b.recursive(true).mode(0o700);
            b.create(&directory).map_err(|_| storage())?;
            let m = fs::symlink_metadata(&directory).map_err(|_| storage())?;
            if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
                return Err(storage());
            }
        }
        #[cfg(not(unix))]
        {
            return Err(Error::new(
                "messagesPlatformUnsupported",
                "Native send journals require macOS or Linux",
            ));
        }
        Ok(Self { directory })
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    fn global(&self, cancel: &AtomicBool) -> Result<File> {
        let file = private_file(&self.directory.join("journal.lock"), true)?;
        lock(&file, cancel)?;
        Ok(file)
    }
    fn entries(&self) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();
        for path in fs::read_dir(&self.directory).map_err(|_| storage())? {
            let path = path.map_err(|_| storage())?.path();
            if path.extension().is_none_or(|x| x != "json") {
                continue;
            }
            if entries.len() >= 10_000 {
                return Err(storage());
            }
            let mut bytes = Vec::new();
            private_file(&path, false)?
                .take(65537)
                .read_to_end(&mut bytes)
                .map_err(|_| storage())?;
            if bytes.len() > 65536 {
                return Err(storage());
            }
            let entry: Entry = serde_json::from_slice(&bytes).map_err(|_| storage())?;
            if entry.version != 1
                || !valid_id(&entry.id, 48)
                || !valid_id(&entry.fingerprint, 64)
                || path.file_stem().and_then(|s| s.to_str()) != Some(&entry.id)
            {
                return Err(storage());
            }
            entries.push(entry);
        }
        Ok(entries)
    }
    fn save(&self, entry: &Entry) -> Result<()> {
        let bytes = serde_json::to_vec(entry).map_err(|_| storage())?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.directory).map_err(|_| storage())?;
        temp.write_all(&bytes)
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|_| storage())?;
        temp.persist(self.directory.join(format!("{}.json", entry.id)))
            .map_err(|_| storage())?;
        File::open(&self.directory)
            .and_then(|f| f.sync_all())
            .map_err(|_| storage())
    }
    pub fn begin(&self, fingerprint: &str, cancel: &AtomicBool) -> Result<Attempt> {
        self.begin_for(fingerprint, None, cancel)
    }
    pub fn begin_for(
        &self,
        fingerprint: &str,
        destination: Option<serde_json::Value>,
        cancel: &AtomicBool,
    ) -> Result<Attempt> {
        if !valid_id(fingerprint, 64) {
            return Err(invalid("Invalid send fingerprint"));
        }
        let _guard = self.global(cancel)?;
        let entries = self.entries()?;
        if entries.iter().any(|e| {
            e.fingerprint == fingerprint
                && matches!(e.outcome, Outcome::InFlight | Outcome::Uncertain)
        }) {
            return Err(unknown());
        }
        let mut random = [0u8; 24];
        getrandom::fill(&mut random).map_err(|_| storage())?;
        let id = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let active_lock = private_file(&self.directory.join(format!("{id}.active")), true)?;
        lock(&active_lock, cancel)?;
        let entry = Entry {
            version: 1,
            id,
            fingerprint: fingerprint.into(),
            outcome: Outcome::InFlight,
            created: now(),
            updated: now(),
            reason: None,
            destination,
        };
        self.save(&entry)?;
        Ok(Attempt { entry, active_lock })
    }
    pub fn finish(
        &self,
        mut attempt: Attempt,
        outcome: Outcome,
        reason: Option<String>,
    ) -> Result<()> {
        if outcome == Outcome::InFlight {
            return Err(invalid("A completed attempt needs a terminal outcome"));
        }
        let _guard = self.global(&AtomicBool::new(false))?;
        attempt.entry.outcome = outcome;
        attempt.entry.reason = reason.map(|s| s.chars().take(2048).collect());
        attempt.entry.updated = now();
        self.save(&attempt.entry)?;
        drop(attempt.active_lock);
        Ok(())
    }
    pub fn pending(&self) -> Result<Vec<Entry>> {
        let _guard = self.global(&AtomicBool::new(false))?;
        Ok(self
            .entries()?
            .into_iter()
            .filter(|e| matches!(e.outcome, Outcome::InFlight | Outcome::Uncertain))
            .collect())
    }
    /// Administrative host action, never exposed through a model tool.
    pub fn reconcile(&self, id: &str, outcome: Outcome) -> Result<()> {
        if !valid_id(id, 48) || !matches!(outcome, Outcome::Accepted | Outcome::NotSent) {
            return Err(invalid(
                "Specify a send id and sent or not_sent after checking the Messages conversation",
            ));
        }
        let _guard = self.global(&AtomicBool::new(false))?;
        let mut entry = self
            .entries()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| invalid("Unknown send id"))?;
        let file = private_file(&self.directory.join(format!("{id}.active")), true)?;
        file.try_lock_exclusive().map_err(|_| {
            Error::new(
                "messagesSendStillActive",
                "The sending process is still active; stop it before reconciling its outcome",
            )
        })?;
        entry.outcome = outcome;
        entry.updated = now();
        entry.reason = Some("Outcome explicitly reconciled by the host".into());
        self.save(&entry)
    }
    pub fn forget_attachments(&self, id: &str) -> Result<()> {
        if !valid_id(id, 48) {
            return Err(invalid("Invalid send id"));
        }
        let _guard = self.global(&AtomicBool::new(false))?;
        let entry = self
            .entries()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| invalid("Unknown send id"))?;
        if matches!(entry.outcome, Outcome::InFlight | Outcome::Uncertain) {
            return Err(unknown());
        }
        let active = private_file(&self.directory.join(format!("{id}.active")), true)?;
        active
            .try_lock_exclusive()
            .map_err(|_| Error::new("messagesSendStillActive", "The send is still active"))?;
        let path = self.directory.join("attachments").join(id);
        match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(storage()),
        }
    }
}
fn valid_id(id: &str, len: usize) -> bool {
    id.len() == len && id.bytes().all(|b| b.is_ascii_hexdigit())
}
