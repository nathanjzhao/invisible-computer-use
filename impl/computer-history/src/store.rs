use crate::{
    now_ms,
    policy::{web_host, Context, Decision, Settings, Website},
    Error, Result,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DAY: u64 = 86_400_000;
const MAX_FILE: u64 = 16 * 1024 * 1024;
#[path = "store_deletion.rs"]
mod deletion;
pub use deletion::{DeleteFilter, DeletePreview, EventIdentity};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub version: u32,
    pub generation: String,
    pub revision: u64,
    pub enabled: bool,
    pub paused: bool,
    pub retention_days: u32,
    pub settings: Settings,
    #[serde(default)]
    pub capture_mode: CaptureMode,
    #[serde(default)]
    pub browser_capture: bool,
    /// Host-approved sharing epoch. Model observation tools cannot change it.
    #[serde(default)]
    pub summary_epoch: String,
}
/// Content scope is host consent, deliberately absent from the model control tools.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    #[default]
    ApplicationMetadata,
    Semantic,
    Values,
    InteractionEvents,
}
/// Created before any native observation, including identity discovery.
pub struct Observation {
    generation: String,
    revision: u64,
    started: Instant,
    timestamp: u64,
    pub mode: CaptureMode,
    pub browser_capture: bool,
    settings: Settings,
}
impl Observation {
    pub fn matches_revision(&self, generation: &str, revision: u64) -> bool {
        self.generation == generation && self.revision == revision
    }
    pub fn allows_application(&self, context: &Context) -> bool {
        self.settings.evaluate_application(context) == Decision::Allowed
    }
}
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}
pub struct Ticket {
    generation: String,
    revision: u64,
    context: Context,
    started: Instant,
    timestamp: u64,
}
impl Ticket {
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }
    /// Build a ticket with a chosen source timestamp for ordering tests. The
    /// other bindings are placeholders; only `timestamp` is observed by the
    /// value ordering buffer.
    #[cfg(test)]
    pub(crate) fn for_test(timestamp: u64) -> Self {
        Self {
            generation: String::new(),
            revision: 0,
            context: Context {
                application: "test.app".into(),
                process_id: 0,
                window_id: 0,
                website: Website::NotWeb,
            },
            started: Instant::now(),
            timestamp,
        }
    }
}
pub struct Collector {
    store: Store,
    _lock: File,
}
impl Collector {
    pub fn heartbeat(&self, mode: &str, error: Option<&str>) -> Result<()> {
        let _guard = self.store.lock()?;
        self.heartbeat_locked(mode, error)
    }
    fn heartbeat_locked(&self, mode: &str, error: Option<&str>) -> Result<()> {
        let mut output = file(&self.store.root.join("collector.json"), true, false)?;
        output.set_len(0).map_err(|_| failed())?;
        let value = json!({"pid":std::process::id(),"mode":mode,"last_seen_unix_ms":now_ms(),"error":error.map(|s|s.chars().take(300).collect::<String>())});
        output
            .write_all(&serde_json::to_vec(&value).map_err(|_| failed())?)
            .and_then(|_| output.sync_data())
            .map_err(|_| failed())
    }
}
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Commit {
    Written,
    Stale,
    Suppressed,
}
fn failed() -> Error {
    Error::new(
        "computerHistoryStorageUnavailable",
        "Computer History storage could not be read or updated",
    )
}
fn lock_contended(error: &std::io::Error) -> bool {
    // Windows reports ERROR_LOCK_VIOLATION, which is not necessarily WouldBlock.
    // fs2 exposes the platform's actual contention error for this comparison.
    error.kind() == std::io::ErrorKind::WouldBlock
        || error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
}
fn token() -> Result<String> {
    let mut bytes = [0; 24];
    getrandom::fill(&mut bytes).map_err(|_| failed())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn private_dir(path: &Path) -> Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(path).map_err(|_| failed())?;
    let metadata = fs::symlink_metadata(path).map_err(|_| failed())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(failed());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(failed());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(failed());
        }
    }
    Ok(())
}
fn file(path: &Path, write: bool, append: bool) -> Result<File> {
    let mut o = OpenOptions::new();
    o.read(true).write(write).create(write).append(append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.custom_flags(0x00200000);
    }
    let f = o.open(path).map_err(|_| failed())?;
    let m = f.metadata().map_err(|_| failed())?;
    if !m.is_file() {
        return Err(failed());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            return Err(failed());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(failed());
        }
    }
    Ok(f)
}
impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        if !root.is_absolute() {
            return Err(Error::invalid("History root must be absolute"));
        }
        private_dir(&root)?;
        private_dir(&root.join("activity"))?;
        let store = Self { root };
        let _guard = store.lock()?;
        if !store
            .root
            .join("settings.json")
            .try_exists()
            .map_err(|_| failed())?
        {
            store.write(&State {
                version: 1,
                generation: token()?,
                revision: 0,
                enabled: false,
                paused: false,
                retention_days: 2,
                settings: Settings::default(),
                capture_mode: CaptureMode::default(),
                browser_capture: false,
                summary_epoch: String::new(),
            })?;
        }
        store.load()?;
        store.cleanup_temporary_files()?;
        Ok(store)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn attach_collector(&self, mode: &str) -> Result<Collector> {
        let _guard = self.lock()?;
        let lock = file(&self.root.join("collector.lock"), true, false)?;
        match lock.try_lock_exclusive() {
            Ok(()) => {}
            Err(e) if lock_contended(&e) => {
                return Err(Error::new(
                    "computerHistoryCollectorActive",
                    "Another history collector already owns this store",
                ))
            }
            Err(_) => return Err(failed()),
        }
        let collector = Collector {
            store: self.clone(),
            _lock: lock,
        };
        collector.heartbeat_locked(mode, None)?;
        Ok(collector)
    }
    fn lock(&self) -> Result<File> {
        let f = file(&self.root.join("history.lock"), true, false)?;
        let start = Instant::now();
        loop {
            match f.try_lock_exclusive() {
                Ok(()) => return Ok(f),
                Err(e) if lock_contended(&e) && start.elapsed() < Duration::from_secs(2) => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return Err(failed()),
            }
        }
    }
    fn load(&self) -> Result<State> {
        let mut bytes = Vec::new();
        file(&self.root.join("settings.json"), false, false)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| failed())?;
        if bytes.len() > 1024 * 1024 {
            return Err(failed());
        }
        let mut s: State = serde_json::from_slice(&bytes).map_err(|_| failed())?;
        if s.version != 1 || s.generation.len() != 48 || !(1..=3650).contains(&s.retention_days) {
            return Err(failed());
        }
        s.settings = s.settings.validated()?;
        Ok(s)
    }
    fn write(&self, state: &State) -> Result<()> {
        self.replace(&self.root.join("settings.json"), &serde_json::to_vec(state).map_err(|_| failed())?)
    }
    fn replace(&self, target: &Path, bytes: &[u8]) -> Result<()> {
        let parent = target.parent().ok_or_else(failed)?;
        let path = parent.join(format!("replace-{}.tmp", token()?));
        let result = (|| {
            let mut f = file(&path, true, false)?;
            f.write_all(bytes)
                .and_then(|_| f.sync_all())
                .map_err(|_| failed())?;
            drop(f);
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                let from: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                let to: Vec<u16> = target
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect();
                // Indexers and external readers can briefly omit DELETE sharing.
                // Keep the old file intact and retry only this atomic replacement;
                // the History lock still covers the entire settings transaction.
                let started = Instant::now();
                loop {
                    if unsafe {
                        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                            from.as_ptr(),
                            to.as_ptr(),
                            windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                                | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
                        )
                    } != 0 {
                        break;
                    }
                    let error = std::io::Error::last_os_error();
                    if matches!(error.raw_os_error(), Some(5 | 32 | 33))
                        && started.elapsed() < Duration::from_millis(250)
                    {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    return Err(Error::new(
                        "computerHistoryStorageUnavailable",
                        format!("Computer History file could not be replaced (system error {})",
                                error.raw_os_error().unwrap_or(0)),
                    ));
                }
            }
            #[cfg(not(windows))]
            fs::rename(&path, target).map_err(|_| failed())?;
            #[cfg(unix)]
            File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|_| failed())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
        result
    }
    pub fn state(&self) -> Result<State> {
        let _guard = self.lock()?;
        self.load()
    }
    /// Keep a reviewed source stable against host deletion while its approved
    /// derivative is committed. Never hold this barrier across a model call.
    /// Callers that also need the memory lock take this history lock first.
    pub fn with_review_lock<T>(&self, work: impl FnOnce() -> Result<T>) -> Result<T> {
        let _guard = self.lock()?;
        work()
    }
    /// Snapshot or commit derived content under the same barrier as capture and deletion.
    /// A callback needing the memory lock must acquire it after this history lock.
    pub fn with_state<T>(&self, work: impl FnOnce(&State) -> Result<T>) -> Result<T> {
        let _guard = self.lock()?;
        work(&self.load()?)
    }
    /// A fresh sharing approval covers only records written after this boundary,
    /// independent of wall-clock changes. The host persists the matching policy
    /// before this epoch can be stamped on new observations.
    pub fn renew_summary_epoch<T>(&self, expected: (&str,u64),
        work: impl FnOnce(&State,&str)->Result<T>) -> Result<T> {
        let _guard=self.lock()?;
        let mut state=self.load()?;
        if state.generation!=expected.0 || state.revision!=expected.1 {
            return Err(Error::new("computerHistorySettingsChanged","History changed; review sharing settings again"));
        }
        let epoch=token()?;
        let result=work(&state,&epoch)?;
        state.summary_epoch=epoch;
        state.revision=state.revision.checked_add(1).ok_or_else(failed)?;
        self.write(&state)?;
        Ok(result)
    }
    /// For an existing host-owned sink. The callback runs under the same pause/settings barrier.
    pub fn with_allowed<T>(
        &self,
        context: &Context,
        write: impl FnOnce() -> Result<T>,
    ) -> Result<Option<T>> {
        let _guard = self.lock()?;
        let state = self.load()?;
        if !state.enabled || state.paused || state.settings.evaluate(context) != Decision::Allowed {
            return Ok(None);
        }
        write().map(Some)
    }
    pub fn clear(&self) -> Result<usize> {
        self.clear_with(||Ok(()))
    }
    /// Delete raw activity and host-owned derivatives before acknowledging clear.
    pub fn clear_with(&self, clear_derived: impl FnOnce()->Result<()>) -> Result<usize> {
        let _guard = self.lock()?;
        self.cleanup_temporary_files()?;
        let mut state = self.load()?;
        state.revision = state.revision.checked_add(1).ok_or_else(failed)?;
        state.generation=token()?;
        state.paused=true;
        if !state.summary_epoch.is_empty() {state.summary_epoch=token()?;}
        self.write(&state)?;
        let mut removed = 0;
        for entry in fs::read_dir(self.root.join("activity")).map_err(|_| failed())? {
            let entry = entry.map_err(|_| failed())?;
            if entry.file_type().map_err(|_| failed())?.is_file()
                && segment_day(&entry.file_name().to_string_lossy()).is_some()
            {
                fs::remove_file(entry.path()).map_err(|_| failed())?;
                removed += 1;
            }
        }
        clear_derived()?;
        Ok(removed)
    }
    /// Human-controlled setup, not exposed in the five model tools.
    pub fn configure(&self, enabled: bool, retention_days: u32, approved: bool) -> Result<State> {
        if enabled && !approved {
            return Err(Error::new(
                "computerHistoryConsentRequired",
                "Enable Computer History in the app before resuming capture",
            ));
        }
        if !(1..=3650).contains(&retention_days) {
            return Err(Error::invalid("Retention must be 1..3650 days"));
        }
        let _guard = self.lock()?;
        let mut s = self.load()?;
        if s.enabled != enabled || s.retention_days != retention_days {
            s.enabled = enabled;
            s.retention_days = retention_days;
            s.revision = s.revision.checked_add(1).ok_or_else(failed)?;
            self.write(&s)?;
        }
        Ok(s)
    }
    pub fn retention(&self, days: u32, expected: (&str, u64)) -> Result<State> {
        if !(1..=3650).contains(&days) {
            return Err(Error::invalid("Retention must be 1..3650 days"));
        }
        let _guard = self.lock()?;
        let mut state = self.load()?;
        if state.generation != expected.0 || state.revision != expected.1 {
            return Err(Error::new(
                "computerHistorySettingsChanged",
                "History changed since it was read; reload before changing retention",
            ));
        }
        if state.retention_days != days {
            state.retention_days = days;
            state.revision = state.revision.checked_add(1).ok_or_else(failed)?;
            self.write(&state)?;
        }
        Ok(state)
    }
    pub fn pause(&self, paused: bool) -> Result<State> {
        let _guard = self.lock()?;
        let mut s = self.load()?;
        if !paused && !s.enabled {
            return Err(Error::new(
                "computerHistoryDisabled",
                "Computer History is disabled. Enable it in the app first.",
            ));
        }
        if s.paused != paused {
            s.paused = paused;
            s.revision = s.revision.checked_add(1).ok_or_else(failed)?;
            self.write(&s)?;
        }
        Ok(s)
    }
    pub fn settings(&self) -> Result<(Settings, String, u64)> {
        let s = self.state()?;
        Ok((s.settings, s.generation, s.revision))
    }
    pub fn update(&self, settings: Settings, expected: Option<(&str, u64)>) -> Result<State> {
        let settings = settings.validated()?;
        let _guard = self.lock()?;
        let mut s = self.load()?;
        if expected.is_some_and(|(g, v)| g != s.generation || v != s.revision) {
            return Err(Error::new("computerHistorySettingsChanged","History changed since it was read. Reload the current settings before replacing them."));
        }
        if s.settings != settings {
            s.settings = settings;
            if !s.summary_epoch.is_empty() {s.summary_epoch=token()?;}
            s.revision = s.revision.checked_add(1).ok_or_else(failed)?;
            self.write(&s)?;
        }
        Ok(s)
    }
    pub fn begin(&self, context: Context, timestamp: u64) -> Result<Option<Ticket>> {
        let _guard = self.lock()?;
        let s = self.load()?;
        if !s.enabled || s.paused || s.settings.evaluate(&context) != Decision::Allowed {
            return Ok(None);
        }
        Ok(Some(Ticket {
            generation: s.generation,
            revision: s.revision,
            context,
            started: Instant::now(),
            timestamp,
        }))
    }
    /// Begin before discovery so a pause/resume during the read invalidates it.
    pub fn observation(&self) -> Result<Option<Observation>> {
        let _guard = self.lock()?;
        let s = self.load()?;
        Ok((s.enabled && !s.paused).then(|| Observation {
            generation: s.generation,
            revision: s.revision,
            started: Instant::now(),
            timestamp: now_ms(),
            mode: s.capture_mode,
            browser_capture: s.browser_capture,
            settings: s.settings,
        }))
    }
    pub fn bind(&self, observation: Observation, context: Context) -> Result<Option<Ticket>> {
        let _guard = self.lock()?;
        let s = self.load()?;
        if observation.generation != s.generation
            || observation.revision != s.revision
            || observation.started.elapsed() > Duration::from_secs(5)
            || !s.enabled
            || s.paused
            || s.settings.evaluate(&context) != Decision::Allowed
        {
            return Ok(None);
        }
        Ok(Some(Ticket {
            generation: observation.generation,
            revision: observation.revision,
            context,
            started: observation.started,
            timestamp: observation.timestamp,
        }))
    }
    /// A legacy host sink can share the pre-read barrier without appending an extra record.
    pub fn with_ticket<T>(
        &self,
        ticket: Ticket,
        current: &Context,
        write: impl FnOnce() -> Result<T>,
    ) -> Result<Option<T>> {
        self.read_with_ticket(&ticket, current, write)
    }
    /// Read under the policy barrier while retaining the original ticket for a
    /// later commit. Producer notifications keep their source timestamp; a
    /// pause, deletion or scope change still invalidates their queued writes.
    pub fn read_with_ticket<T>(
        &self,
        ticket: &Ticket,
        current: &Context,
        read: impl FnOnce() -> Result<T>,
    ) -> Result<Option<T>> {
        if ticket.context != *current || ticket.started.elapsed() > Duration::from_secs(5) {
            return Ok(None);
        }
        let _guard = self.lock()?;
        let state = self.load()?;
        if ticket.generation != state.generation
            || ticket.revision != state.revision
            || !state.enabled
            || state.paused
            || state.settings.evaluate(current) != Decision::Allowed
        {
            return Ok(None);
        }
        read().map(Some)
    }
    /// A notification can require a live field read instead of carrying its own
    /// value. Timestamp that sample only after the guarded read completes. Keep
    /// the original consent revision and monotonic lifetime for queued commit.
    pub fn sample_with_ticket<T>(
        &self,
        ticket: &mut Ticket,
        current: &Context,
        read: impl FnOnce() -> Result<T>,
    ) -> Result<Option<T>> {
        let sampled = self.read_with_ticket(ticket, current, || {
            read().map(|value| (value, now_ms()))
        })?;
        Ok(sampled.map(|(value, timestamp)| {
            ticket.timestamp = timestamp;
            value
        }))
    }
    /// Scope upgrades require fresh host approval and compare against the displayed state.
    pub fn capture_mode(
        &self,
        mode: CaptureMode,
        approved: bool,
        expected: (&str, u64),
    ) -> Result<State> {
        let _guard = self.lock()?;
        let mut s = self.load()?;
        if s.generation != expected.0 || s.revision != expected.1 {
            return Err(Error::new(
                "computerHistorySettingsChanged",
                "History changed; reload before changing capture scope",
            ));
        }
        if mode > s.capture_mode && !approved {
            return Err(Error::new(
                "computerHistoryConsentRequired",
                "Approve the new content scope in the app first",
            ));
        }
        if mode != s.capture_mode {
            s.capture_mode = mode;
            if !s.summary_epoch.is_empty() {s.summary_epoch=token()?;}
            s.revision = s.revision.checked_add(1).ok_or_else(failed)?;
            self.write(&s)?;
        }
        Ok(s)
    }
    /// Browser domains/content expand the old non-web consent scope. Never model-facing.
    pub fn browser_capture(
        &self,
        enabled: bool,
        approved: bool,
        expected: (&str, u64),
    ) -> Result<State> {
        let _guard = self.lock()?;
        let mut s = self.load()?;
        if s.generation != expected.0 || s.revision != expected.1 {
            return Err(Error::new(
                "computerHistorySettingsChanged",
                "History changed; reload before changing browser capture",
            ));
        }
        if enabled && !s.browser_capture && !approved {
            return Err(Error::new(
                "computerHistoryConsentRequired",
                "Approve browser capture in the app first",
            ));
        }
        if s.browser_capture != enabled {
            s.browser_capture = enabled;
            if !s.summary_epoch.is_empty() {s.summary_epoch=token()?;}
            s.revision = s.revision.checked_add(1).ok_or_else(failed)?;
            self.write(&s)?;
        }
        Ok(s)
    }
    pub fn commit(
        &self,
        ticket: Ticket,
        current: &Context,
        kind: &str,
        payload: Value,
    ) -> Result<Commit> {
        self.commit_with(ticket, current, kind, payload, || Ok(()))
    }
    pub fn commit_with(
        &self,
        ticket: Ticket,
        current: &Context,
        kind: &str,
        payload: Value,
        after_write: impl FnOnce() -> Result<()>,
    ) -> Result<Commit> {
        if ticket.context != *current || ticket.started.elapsed() > Duration::from_secs(5) {
            return Ok(Commit::Stale);
        }
        if kind.is_empty() || kind.len() > 128 || kind.contains(char::is_control) {
            return Err(Error::invalid("Invalid observation kind"));
        }
        let _guard = self.lock()?;
        let s = self.load()?;
        if ticket.generation != s.generation || ticket.revision != s.revision {
            return Ok(Commit::Stale);
        }
        if !s.enabled || s.paused || s.settings.evaluate(current) != Decision::Allowed {
            return Ok(Commit::Suppressed);
        }
        let now = now_ms();
        if ticket.timestamp > now.saturating_add(60_000)
            || ticket.timestamp < now.saturating_sub(u64::from(s.retention_days) * DAY)
        {
            return Ok(Commit::Stale);
        }
        let domain = match &current.website {
            Website::Url(url) => web_host(url),
            _ => None,
        };
        let mut payload=payload;
        if let Some(object)=payload.as_object_mut() {
            object.remove("summary_epoch");
            if !s.summary_epoch.is_empty() {object.insert("summary_epoch".into(),json!(s.summary_epoch));}
        }
        let event = json!({"id":token()?,"timestamp_unix_ms":ticket.timestamp,"kind":kind,"application":current.application,"process_id":current.process_id,"window_id":current.window_id,"domain":domain,"payload":payload});
        let bytes = serde_json::to_vec(&event).map_err(|_| failed())?;
        if bytes.len() as u64 + 1
            > fs2::available_space(&self.root)
                .map_err(|_| failed())?
                .saturating_sub(64 * 1024 * 1024)
        {
            return Err(Error::new(
                "computerHistoryCapacity",
                "History capture stopped because local storage is nearly full",
            ));
        }
        if bytes.len() > 1024 * 1024 {
            return Err(Error::invalid("One history observation exceeds 1 MiB"));
        }
        self.prune_locked(now_ms(), s.retention_days)?;
        let day = ticket.timestamp / DAY;
        let dir = self.root.join("activity");
        private_dir(&dir)?;
        for index in 0..1000 {
            let path = dir.join(format!("events-{day:08}-{index:03}.jsonl"));
            let mut out = file(&path, true, true)?;
            if out.metadata().map_err(|_| failed())?.len() + bytes.len() as u64 + 1 > MAX_FILE {
                continue;
            }
            out.write_all(&bytes)
                .and_then(|_| out.write_all(b"\n"))
                .and_then(|_| out.sync_data())
                .map_err(|_| failed())?;
            after_write()?;
            return Ok(Commit::Written);
        }
        Err(Error::new(
            "computerHistoryCapacity",
            "The current day's history reached its segment limit",
        ))
    }
    fn prune_locked(&self, now: u64, days: u32) -> Result<usize> {
        self.cleanup_temporary_files()?;
        let cutoff = now.saturating_sub(u64::from(days) * DAY);
        let oldest = cutoff / DAY;
        let mut removed = 0;
        for entry in fs::read_dir(self.root.join("activity")).map_err(|_| failed())? {
            let entry = entry.map_err(|_| failed())?;
            if !entry.file_type().map_err(|_| failed())?.is_file() {
                continue;
            }
            if segment_day(&entry.file_name().to_string_lossy()).is_some_and(|day| day < oldest) {
                fs::remove_file(entry.path()).map_err(|_| failed())?;
                removed += 1;
            } else if segment_day(&entry.file_name().to_string_lossy()) == Some(oldest) {
                // The boundary file can contain both expired and retained
                // events. A day-only sweep kept data for almost one extra day.
                let bytes = self.segment(&entry.path())?;
                let mut kept = Vec::new();
                for line in bytes.split_inclusive(|b| *b == b'\n') {
                    let row: Value = serde_json::from_slice(line).map_err(|_| failed())?;
                    let timestamp = row["timestamp_unix_ms"].as_u64().ok_or_else(failed)?;
                    if timestamp >= cutoff { kept.extend_from_slice(line); }
                }
                if kept.len() != bytes.len() {
                    if kept.is_empty() { fs::remove_file(entry.path()).map_err(|_| failed())?; }
                    else { self.replace(&entry.path(), &kept)?; }
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
    /// Atomic replacements can leave a completed temporary file after a crash.
    /// The caller holds history.lock, so no live writer can own these files.
    fn cleanup_temporary_files(&self) -> Result<()> {
        for directory in [self.root.clone(),self.root.join("activity")] {
            private_dir(&directory)?;
            for entry in fs::read_dir(directory).map_err(|_|failed())? {
                let entry=entry.map_err(|_|failed())?;
                let name=entry.file_name();let name=name.to_string_lossy();
                let id=name.strip_suffix(".tmp").and_then(|stem|stem.strip_prefix("replace-").or_else(||stem.strip_prefix("settings-")));
                if id.is_some_and(|id|id.len()==48&&id.bytes().all(|b|b.is_ascii_hexdigit())) {
                    let kind=entry.file_type().map_err(|_|failed())?;
                    if kind.is_file()||kind.is_symlink(){fs::remove_file(entry.path()).map_err(|_|failed())?;}
                }
            }
        }
        Ok(())
    }
    pub fn prune(&self, now: u64) -> Result<usize> {
        let _guard = self.lock()?;
        let s = self.load()?;
        self.prune_locked(now, s.retention_days)
    }
    pub fn status(&self) -> Result<Value> {
        let _guard = self.lock()?;
        let s = self.load()?;
        let mut paths = Vec::new();
        for entry in fs::read_dir(self.root.join("activity")).map_err(|_| failed())? {
            let entry = entry.map_err(|_| failed())?;
            if entry.file_type().map_err(|_| failed())?.is_file()
                && segment_day(&entry.file_name().to_string_lossy()).is_some()
            {
                paths.push(entry.path());
            }
        }
        paths.sort();
        paths.reverse();
        paths.truncate(20);
        let owner = file(&self.root.join("collector.lock"), true, false)?;
        let attached = match owner.try_lock_exclusive() {
            Ok(()) => false,
            Err(e) if lock_contended(&e) => true,
            Err(_) => return Err(failed()),
        };
        let collector = if attached {
            let mut bytes = Vec::new();
            file(&self.root.join("collector.json"), false, false)?
                .take(4097)
                .read_to_end(&mut bytes)
                .map_err(|_| failed())?;
            if bytes.len() > 4096 {
                return Err(failed());
            }
            serde_json::from_slice::<Value>(&bytes).map_err(|_| failed())?
        } else {
            Value::Null
        };
        let fresh = collector["last_seen_unix_ms"]
            .as_u64()
            .is_some_and(|t| now_ms().saturating_sub(t) < 15_000);
        Ok(
            json!({"enabled":s.enabled,"paused":s.paused,"capture_mode":s.capture_mode,"browser_capture":s.browser_capture,"state":if !s.enabled{"disabled"}else if s.paused{"paused"}else{"enabled"},"capturing":s.enabled&&!s.paused&&attached&&fresh&&collector["error"].is_null(),"collector":collector,"root":self.root,"recent_activity_files":paths,"retention_days":s.retention_days,"revision":s.revision,"generation":s.generation,"settings":s.settings}),
        )
    }
}
fn segment_day(name: &str) -> Option<u64> {
    let tail = name.strip_prefix("events-")?.strip_suffix(".jsonl")?;
    let (day, index) = tail.split_once('-')?;
    if day.len() != 8
        || index.len() != 3
        || !day.bytes().chain(index.bytes()).all(|b| b.is_ascii_digit())
    {
        return None;
    }
    day.parse().ok()
}
