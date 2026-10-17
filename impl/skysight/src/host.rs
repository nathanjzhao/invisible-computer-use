//! The recorder host: one thread that runs a platform [`Source`] through a
//! [`Recorder`] into segments, following the settings on disk. It starts the source
//! when recording is on and stops it (closing the segment with `session.ended`) when
//! recording is turned off or paused, rotates segments every [`SEGMENT_SECS`], opens a
//! fresh segment and drops buffered text after a delete, and writes its heartbeat to
//! `status.json`. The source is told nothing but start and stop; it only observes.

use crate::control::{self, HostStatus, Layout};
use crate::segments::{SegmentWriter, SEGMENT_SECS};
use crate::stream::{Observation, Recorder, Scrub};
use crate::Ts;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Permissions a source needs, as far as it can tell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Permissions { pub accessibility: Option<bool>, pub input_monitoring: Option<bool> }

/// A platform observer.
pub trait Source: Send {
    /// Begin observing. An error (missing permission, no display) is reported in the status.
    fn start(&mut self) -> Result<(), String>;
    /// The next observation and when it happened, or `None` after `timeout`.
    fn next(&mut self, timeout: Duration) -> Option<(Observation, Ts)>;
    fn stop(&mut self);
    fn permissions(&self) -> Permissions { Permissions::default() }
    fn platform(&self) -> &'static str;
}

/// A clock for the host (tests pass their own).
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn wall_ms() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) }

#[derive(Clone)]
pub struct HostConfig {
    pub layout: Layout,
    pub own_pid: i64,
    /// Built fresh for each session (the host's prose scrub).
    pub scrub: Option<Arc<dyn Fn(&str) -> String + Send + Sync>>,
    pub clock: Clock,
    /// How long one loop waits for observations.
    pub poll: Duration,
    /// The app's signer. Without it (or while `settings.json` does not carry its signature) the
    /// host never records: only the app turns recording on ([`control::effective_settings`]).
    pub mac: Option<control::Mac>,
}

impl HostConfig {
    pub fn new(layout: Layout) -> HostConfig {
        HostConfig { layout, own_pid: std::process::id() as i64, scrub: None, clock: Arc::new(wall_ms), poll: Duration::from_millis(250), mac: None }
    }
}

pub struct HostHandle { stop: Arc<AtomicBool>, thread: Option<std::thread::JoinHandle<()>> }

impl HostHandle {
    /// Stop recording, close the segment and wait for the thread.
    pub fn stop(mut self) { self.stop.store(true, Ordering::SeqCst); if let Some(t) = self.thread.take() { let _ = t.join(); } }
    pub fn is_finished(&self) -> bool { self.thread.as_ref().is_none_or(|t| t.is_finished()) }
}

impl Drop for HostHandle {
    fn drop(&mut self) { self.stop.store(true, Ordering::SeqCst); if let Some(t) = self.thread.take() { let _ = t.join(); } }
}

struct Session { rec: Recorder, writer: SegmentWriter, generation: u64, opened: Ts, last_meta: Ts }

/// The single-recorder lock, in the memory root itself: nothing a clear, a prune or the panel
/// deletes holds it (review round 2, N4: under `skysight/` a delete let a second recorder start).
pub const RECORDER_LOCK_FILE: &str = ".skysight-recorder.lock";

/// Whether `held` is still the file at `path` (not deleted, not replaced by another).
fn lock_is_current(held: &std::fs::File, path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (held.metadata(), std::fs::metadata(path)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino() && a.nlink() > 0,
            _ => false,
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[derive(Default, PartialEq)]
        #[repr(C)]
        struct FileId { volume: u64, id: [u8; 16] }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetFileInformationByHandleEx(handle: *mut std::ffi::c_void, class: i32,
                info: *mut std::ffi::c_void, size: u32) -> i32;
        }
        let identity = |file: &std::fs::File| {
            let mut info = FileId::default();
            // FileIdInfo identifies the underlying file, including when its
            // old name has been deleted and immediately recreated by a rival.
            (unsafe { GetFileInformationByHandleEx(file.as_raw_handle(), 18,
                (&mut info as *mut FileId).cast(), std::mem::size_of::<FileId>() as u32) } != 0).then_some(info)
        };
        match (identity(held), std::fs::File::open(path).ok().and_then(|file| identity(&file))) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }
    #[cfg(not(any(unix, windows)))]
    { let _ = held; path.exists() }
}

pub fn spawn(source: Box<dyn Source>, cfg: HostConfig) -> HostHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new().name("skysight-recorder".into()).spawn(move || run(source, cfg, &flag)).ok();
    HostHandle { stop, thread }
}

/// The loop, on the calling thread, until `stop` is set.
pub fn run(mut source: Box<dyn Source>, cfg: HostConfig, stop: &AtomicBool) {
    let layout = cfg.layout.clone();
    let mut session: Option<Session> = None;
    let mut source_on = false;
    let mut status = HostStatus { state: "stopped".into(), platform: source.platform().into(), ..Default::default() };
    let mut last_status = 0i64;
    let mut retry_at = 0i64;
    // When the last clear was seen: an observation from before it, still queued in the source,
    // is not written into the segment opened after it (fresh review, F12).
    let mut cleared_at: Ts = Ts::MIN;
    // One recorder per memory root: a second app instance (or `icu memory record`) waits here
    // and writes nothing, not even status, until the first lets go.
    let lock_path = layout.root.join(RECORDER_LOCK_FILE);
    let mut lock: Option<std::fs::File> = None;
    // A lock just taken may be a fresh file made after another host's was deleted; that host
    // notices within one loop (a poll plus a little) and stops, so this one waits that long first.
    let grace = (cfg.poll * 4).max(Duration::from_millis(200));
    let mut locked_at: Option<std::time::Instant> = None;
    let close = |s: Session, now_ms: i64, reason: &str, status: &mut HostStatus| {
        let mut s = s;
        let outs = s.rec.stop(now_ms / 1000, reason);
        for o in &outs { let _ = s.writer.write(o); }
        let _ = s.writer.close(now_ms, reason);
        status.ended_at_ms = Some(now_ms);
        status.current_segment_events_path = None;
        status.current_segment_metadata_path = None;
        status.suppressed_events_path = None;
    };
    loop {
        // The lock file deleted or replaced under a running host: another host can take a fresh
        // one, so this one stops recording and queues for the lock again.
        if lock.as_ref().is_some_and(|f| !lock_is_current(f, &lock_path)) {
            lock = None;
            let now_ms = (cfg.clock)();
            if let Some(s) = session.take() { close(s, now_ms, "lock_lost", &mut status); }
            if source_on { source.stop(); source_on = false; }
        }
        if lock.is_none() {
            let _ = crate::private::create_dir(&layout.root);
            lock = crate::segments::try_lock_file(&lock_path);
            if lock.is_none() {
                if stop.load(Ordering::SeqCst) { return; }
                std::thread::sleep(cfg.poll.max(Duration::from_millis(200)));
                continue;
            }
            locked_at = Some(std::time::Instant::now());
        }
        if locked_at.is_some_and(|t| t.elapsed() < grace) {
            if stop.load(Ordering::SeqCst) { return; }
            std::thread::sleep(cfg.poll.min(Duration::from_millis(50)));
            continue;
        }
        let now_ms = (cfg.clock)();
        let now = now_ms / 1000;
        if stop.load(Ordering::SeqCst) {
            if let Some(s) = session.take() { close(s, now_ms, "stopped", &mut status); }
            if source_on { source.stop(); }
            status.state = "stopped".into();
            status.heartbeat_ms = now_ms;
            let _ = control::write_status(&layout, &status);
            return;
        }
        let settings = control::effective_settings(&layout, cfg.mac.as_ref(), now);
        let generation = control::generation(&layout);
        status.last_error = if control::read_settings(&layout).enabled && !cfg.mac.as_ref().is_some_and(|m| control::signature_valid(&layout, m)) {
            Some("recording is off: its settings were not saved by the app (changed outside it, or no app is running the recorder)".into())
        } else if status.last_error.as_deref().is_some_and(|e| e.starts_with("recording is off: its settings")) { None } else { status.last_error.take() };
        if !settings.should_record(now) {
            if let Some(s) = session.take() { close(s, now_ms, if settings.enabled { "paused" } else { "stopped" }, &mut status); }
            if source_on { source.stop(); source_on = false; }
            status.state = if settings.enabled { "paused".into() } else { "stopped".into() };
            if now_ms - last_status >= 2000 { status.heartbeat_ms = now_ms; let _ = control::write_status(&layout, &status); last_status = now_ms; }
            std::thread::sleep(cfg.poll.max(Duration::from_millis(100)));
            continue;
        }
        if !source_on {
            if now_ms < retry_at { std::thread::sleep(cfg.poll); continue; }
            let p = source.permissions();
            status.accessibility = p.accessibility;
            status.input_monitoring = p.input_monitoring;
            match source.start() {
                Ok(()) => { source_on = true; status.last_error = None; }
                Err(e) => {
                    status.state = "stopped".into();
                    status.last_error = Some(e);
                    status.heartbeat_ms = now_ms;
                    let _ = control::write_status(&layout, &status);
                    retry_at = now_ms + 10_000;
                    continue;
                }
            }
        }
        // A delete since the segment opened: close it and drop what was buffered.
        if session.as_ref().is_some_and(|s| s.generation != generation || !s.writer.alive()) {
            cleared_at = now;
            let mut s = session.take().unwrap();
            s.rec.discard_pending();
            let _ = s.writer.close(now_ms, "cleared");
        }
        // Rotate an old segment.
        if session.as_ref().is_some_and(|s| now - s.opened >= SEGMENT_SECS) {
            let mut s = session.take().unwrap();
            let outs = s.rec.flush(now);
            for o in &outs { let _ = s.writer.write(o); }
            let _ = s.writer.close(now_ms, "rotated");
            match layout.segments().open_segment(now_ms) {
                Ok(writer) => { s.writer = writer; s.opened = now; session = Some(s); }
                Err(e) => status.last_error = Some(e),
            }
        }
        if session.is_none() {
            match layout.segments().open_segment(now_ms) {
                Ok(mut writer) => {
                    let scrub: Option<Scrub> = cfg.scrub.clone().map(|f| Box::new(move |t: &str| f(t)) as Scrub);
                    let mut rec = Recorder::new(settings.observation.clone(), cfg.own_pid);
                    if let Some(s) = scrub { rec = rec.with_scrub(s); }
                    for o in rec.start(now) { let _ = writer.write(&o); }
                    status.started_at_ms = Some(now_ms);
                    status.ended_at_ms = None;
                    status.current_segment_events_path = Some(writer.events_path().display().to_string());
                    status.current_segment_metadata_path = Some(writer.metadata_path().display().to_string());
                    status.suppressed_events_path = Some(writer.suppressed_path().display().to_string());
                    status.event_stream_root_path = Some(layout.segments().root.display().to_string());
                    session = Some(Session { rec, writer, generation, opened: now, last_meta: now });
                }
                Err(e) => { status.last_error = Some(e); std::thread::sleep(cfg.poll); continue; }
            }
        }
        let s = session.as_mut().unwrap();
        if s.rec.policy != settings.observation { s.rec.policy = settings.observation.clone(); s.rec.repolicy(); }
        // Observations for up to one poll.
        let deadline = std::time::Instant::now() + cfg.poll;
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            let Some((obs, at)) = source.next(left) else { break };
            if let Observation::Locked(l) = &obs { status.screen_locked = Some(*l); }
            if at <= cleared_at && !matches!(obs, Observation::Locked(_)) { continue; }
            for o in s.rec.observe(obs, at) { if let Err(e) = s.writer.write(&o) { status.last_error = Some(e); } }
        }
        let now = (cfg.clock)() / 1000;
        for o in s.rec.tick(now) { let _ = s.writer.write(&o); }
        if now - s.last_meta >= 5 { let _ = s.writer.sync_meta(); s.last_meta = now; }
        status.state = "running".into();
        let now_ms = (cfg.clock)();
        if now_ms - last_status >= 2000 { status.heartbeat_ms = now_ms; let _ = control::write_status(&layout, &status); last_status = now_ms; }
    }
}
