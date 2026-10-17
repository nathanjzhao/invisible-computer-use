//! Backend-owned task admission and cooperative cancellation.
use std::collections::HashSet;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunInfo {
    pub id: String,
    pub goal: String,
    pub pid: i64,
    pub window_id: i64,
    pub app_name: String,
}

struct RunState { info: RunInfo, stopping: AtomicBool }
#[derive(Default)]
struct Registry { active: Option<Arc<RunState>>, used_ids: HashSet<String>, manual_active: usize, closed: bool }
#[derive(Clone, Default)]
pub struct RunManager(Arc<Mutex<Registry>>);

pub struct RunLease { manager: RunManager, state: Arc<RunState> }
pub struct ManualLease(RunManager);

impl RunManager {
    pub fn shutdown(&self) -> Result<(), String> {
        let mut registry = self.0.lock().map_err(|_| "task state unavailable")?;
        registry.closed = true;
        if let Some(run) = &registry.active { run.stopping.store(true, Ordering::Release); }
        Ok(())
    }

    pub fn begin(&self, info: RunInfo) -> Result<RunLease, String> {
        if info.id.is_empty() || info.id.len() > 128 || !info.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err("invalid run identifier".into());
        }
        let mut registry = self.0.lock().map_err(|_| "task state unavailable")?;
        if registry.closed { return Err("the app is shutting down".into()); }
        if registry.active.is_some() { return Err("a task is already running; stop it before starting another".into()); }
        if registry.manual_active != 0 { return Err("manual input is still completing; try again shortly".into()); }
        if !registry.used_ids.insert(info.id.clone()) { return Err("run identifier was already used".into()); }
        let state = Arc::new(RunState { info, stopping: AtomicBool::new(false) });
        registry.active = Some(state.clone());
        Ok(RunLease { manager: self.clone(), state })
    }

    pub fn stop(&self, id: &str) -> Result<bool, String> {
        let registry = self.0.lock().map_err(|_| "task state unavailable")?;
        let Some(run) = registry.active.as_ref().filter(|run| run.info.id == id) else { return Ok(false); };
        run.stopping.store(true, Ordering::Release);
        // The active slot stays occupied until the worker finishes its current
        // operation. Never claim an in-flight native action was undone.
        Ok(true)
    }

    pub fn status(&self) -> Result<Option<(RunInfo, bool)>, String> {
        let registry = self.0.lock().map_err(|_| "task state unavailable")?;
        Ok(registry.active.as_ref().map(|run| (run.info.clone(), run.stopping.load(Ordering::Acquire))))
    }

    pub fn begin_manual_input(&self) -> Result<ManualLease, String> {
        let mut registry = self.0.lock().map_err(|_| "task state unavailable")?;
        if registry.closed { return Err("the app is shutting down".into()); }
        if registry.active.is_some() { return Err("stop the running task before sending manual input".into()); }
        registry.manual_active += 1;
        Ok(ManualLease(self.clone()))
    }
}

impl RunLease {
    pub fn info(&self) -> &RunInfo { &self.state.info }
    pub fn cancelled(&self) -> bool { self.state.stopping.load(Ordering::Acquire) }
    pub fn finish(&self) {
        if let Ok(mut registry) = self.manager.0.lock() {
            if registry.active.as_ref().is_some_and(|active| Arc::ptr_eq(active, &self.state)) {
                registry.active = None;
            }
        }
    }
}
impl Drop for RunLease { fn drop(&mut self) { self.finish(); } }
impl Drop for ManualLease {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.0.0.lock() { registry.manual_active -= 1; }
    }
}

/// How long a step waits for the user's answer before it is refused.
pub const CONFIRM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Confirmation requests waiting for the user, keyed `<run id>/<request id>`.
/// A request is answered once; an answer for a request that is not waiting
/// (unknown, already answered, timed out) is ignored.
#[derive(Default)]
pub struct Confirmations {
    pending: Mutex<std::collections::HashMap<String, Option<bool>>>,
    /// What each open request asks, so a reloaded window can show it again.
    shown: Mutex<std::collections::HashMap<String, serde_json::Value>>,
    answered: std::sync::Condvar,
}

impl Confirmations {
    pub fn open(&self, key: &str) {
        if let Ok(mut p) = self.pending.lock() { p.insert(key.to_owned(), None); }
    }

    /// Keep what request `key` asks (`{id, action, why, app}`) while it is open.
    pub fn describe(&self, key: &str, info: serde_json::Value) {
        if let Ok(mut m) = self.shown.lock() { m.insert(key.to_owned(), info); }
    }

    /// The request of run `run_id` that is waiting for an answer, if any.
    pub fn pending_for(&self, run_id: &str) -> Option<serde_json::Value> {
        let prefix = format!("{run_id}/");
        let open: Vec<String> = self.pending.lock().ok()?.iter().filter(|(k, v)| k.starts_with(&prefix) && v.is_none()).map(|(k, _)| k.clone()).collect();
        let shown = self.shown.lock().ok()?;
        open.iter().find_map(|k| shown.get(k).cloned())
    }

    /// Record the user's answer. False when no such request is waiting.
    pub fn answer(&self, key: &str, approve: bool) -> bool {
        let Ok(mut p) = self.pending.lock() else { return false };
        match p.get_mut(key) {
            Some(slot @ None) => { *slot = Some(approve); self.answered.notify_all(); true }
            _ => false,
        }
    }

    /// Wait for the answer to `key`, checking `cancelled` as it waits. The
    /// request is closed when this returns, whatever the outcome.
    pub fn wait(&self, key: &str, timeout: std::time::Duration, cancelled: &dyn Fn() -> bool) -> crate::gate::Consent {
        use crate::gate::Consent;
        let deadline = std::time::Instant::now() + timeout;
        let Ok(mut p) = self.pending.lock() else { return Consent::Declined };
        let consent = loop {
            // A Stop beats an Allow that arrived with it: nothing is dispatched after a stop.
            if cancelled() { break Consent::Stopped; }
            match p.get(key) {
                Some(Some(true)) => break Consent::Approved,
                Some(Some(false)) | None => break Consent::Declined,
                Some(None) => {}
            }
            let now = std::time::Instant::now();
            if now >= deadline { break Consent::TimedOut; }
            let slice = (deadline - now).min(std::time::Duration::from_millis(100));
            p = match self.answered.wait_timeout(p, slice) { Ok((g, _)) => g, Err(_) => return Consent::Declined };
        };
        p.remove(key);
        if let Ok(mut m) = self.shown.lock() { m.remove(key); }
        consent
    }
}

/// The app's way of asking: the run's requests in [`Confirmations`], each
/// waiting up to `timeout` and ending early when the run is stopped.
pub struct Asker<'a> {
    pub boxes: &'a Confirmations,
    pub run_id: &'a str,
    pub cancelled: &'a dyn Fn() -> bool,
    pub timeout: std::time::Duration,
}

impl crate::gate::Confirm for Asker<'_> {
    fn open(&self, id: &str) { self.boxes.open(&format!("{}/{id}", self.run_id)); }
    fn show(&self, id: &str, info: &serde_json::Value) { self.boxes.describe(&format!("{}/{id}", self.run_id), info.clone()); }
    fn wait(&self, id: &str) -> crate::gate::Consent { self.boxes.wait(&format!("{}/{id}", self.run_id), self.timeout, self.cancelled) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn info(id: &str) -> RunInfo { RunInfo { id: id.into(), goal: "test".into(), pid: 42, window_id: 7, app_name: "fixture".into() } }

    #[test]
    fn admission_is_exclusive_across_cloned_clients() {
        let manager = RunManager::default();
        let run = manager.begin(info("first")).unwrap();
        let other_client = manager.clone();
        assert!(std::thread::spawn(move || other_client.begin(info("second")).is_err()).join().unwrap());
        assert!(manager.begin_manual_input().is_err());
        assert_eq!(manager.status().unwrap().unwrap().0.id, "first");
        drop(run);
        let manual = manager.begin_manual_input().unwrap();
        assert!(manager.begin(info("second")).is_err());
        drop(manual);
        assert!(manager.begin(info("second")).is_ok());
    }

    #[test]
    fn stop_is_scoped_and_retains_admission_until_the_worker_finishes() {
        let manager = RunManager::default();
        let run = manager.begin(info("first")).unwrap();
        assert!(!manager.stop("stale").unwrap());
        assert!(!run.cancelled());
        assert!(manager.stop("first").unwrap());
        assert!(run.cancelled());
        assert!(manager.status().unwrap().unwrap().1);
        assert!(manager.begin(info("second")).is_err());
        drop(run);
        assert!(manager.status().unwrap().is_none());
        assert!(!manager.stop("first").unwrap());
    }

    #[test]
    fn late_drop_cannot_clear_a_new_run() {
        let manager = RunManager::default();
        let first = manager.begin(info("first")).unwrap();
        first.finish();
        let second = manager.begin(info("second")).unwrap();
        drop(first);
        assert_eq!(manager.status().unwrap().unwrap().0.id, "second");
        drop(second);
        assert!(manager.begin(info("first")).is_err(), "old event identities must not be recycled");
    }

    #[test]
    fn shutdown_cancels_the_run_and_permanently_closes_admission() {
        let manager = RunManager::default();
        let run = manager.begin(info("first")).unwrap();
        manager.shutdown().unwrap();
        assert!(run.cancelled());
        assert!(manager.status().unwrap().unwrap().1);
        drop(run);
        assert!(manager.begin(info("late")).is_err(), "late worker admitted after shutdown");
        assert!(manager.begin_manual_input().is_err());
        let idle = RunManager::default();
        idle.shutdown().unwrap();
        assert!(idle.begin_manual_input().is_err(), "idle shutdown did not close manual admission");
    }

    #[test]
    fn a_confirmation_is_answered_once_and_ends_by_answer_timeout_or_stop() {
        use crate::gate::{Confirm, Consent};
        use std::time::Duration;
        let boxes = std::sync::Arc::new(Confirmations::default());
        // Approved and declined, answered from another thread while the task waits.
        for (approve, want) in [(true, Consent::Approved), (false, Consent::Declined)] {
            let asker = Asker { boxes: &boxes, run_id: "run-1", cancelled: &|| false, timeout: Duration::from_secs(5) };
            asker.open("c1");
            let b = boxes.clone();
            let answer = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(30)); b.answer("run-1/c1", approve) });
            assert_eq!(asker.wait("c1"), want);
            assert!(answer.join().unwrap());
            assert!(!boxes.answer("run-1/c1", true), "a closed request takes no answer");
        }
        // An answer for another run, or a request never opened, does nothing.
        let asker = Asker { boxes: &boxes, run_id: "run-1", cancelled: &|| false, timeout: Duration::from_millis(150) };
        asker.open("c2");
        assert!(!boxes.answer("run-2/c2", true));
        assert!(!boxes.answer("run-1/c9", true));
        let started = std::time::Instant::now();
        assert_eq!(asker.wait("c2"), Consent::TimedOut);
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert!(!boxes.answer("run-1/c2", true), "a timed-out request takes no answer");
        // A stop ends the wait.
        let stop = std::sync::atomic::AtomicBool::new(false);
        let cancelled = || stop.load(std::sync::atomic::Ordering::Acquire);
        let asker = Asker { boxes: &boxes, run_id: "run-1", cancelled: &cancelled, timeout: Duration::from_secs(30) };
        asker.open("c3");
        let started = std::time::Instant::now();
        std::thread::scope(|s| {
            s.spawn(|| { std::thread::sleep(Duration::from_millis(50)); stop.store(true, std::sync::atomic::Ordering::Release); });
            assert_eq!(asker.wait("c3"), Consent::Stopped);
        });
        assert!(started.elapsed() < Duration::from_secs(2));
        // A request takes one answer: a second click before the task reads the first changes nothing.
        let asker = Asker { boxes: &boxes, run_id: "run-1", cancelled: &|| false, timeout: Duration::from_secs(5) };
        asker.open("c5");
        assert!(boxes.answer("run-1/c5", false));
        assert!(!boxes.answer("run-1/c5", true));
        assert_eq!(asker.wait("c5"), Consent::Declined);
        let asker = Asker { boxes: &boxes, run_id: "run-1", cancelled: &cancelled, timeout: Duration::from_secs(30) };
        // An answer that arrives between open and wait is kept.
        asker.open("c4");
        assert!(boxes.answer("run-1/c4", true));
        stop.store(false, std::sync::atomic::Ordering::Release);
        assert_eq!(asker.wait("c4"), Consent::Approved);
    }

    #[test]
    fn a_stop_beats_an_allow_that_came_with_it() {
        use crate::gate::Consent;
        let boxes = Confirmations::default();
        boxes.open("r/c1");
        assert!(boxes.answer("r/c1", true));
        assert_eq!(boxes.wait("r/c1", Duration::from_secs(5), &|| true), Consent::Stopped);
        assert!(!boxes.answer("r/c1", true), "the request is closed");
    }

    #[test]
    fn an_open_request_is_kept_for_a_reloaded_window_until_it_ends() {
        use crate::gate::Consent;
        let boxes = Confirmations::default();
        let ask = serde_json::json!({"id": "c1", "action": "click \"Send\"", "why": "w", "app": "Mail"});
        boxes.open("r/c1");
        boxes.describe("r/c1", ask.clone());
        assert_eq!(boxes.pending_for("r"), Some(ask));
        assert_eq!(boxes.pending_for("other"), None, "another run's question is not shown");
        // Answered but not yet read: nothing left to ask.
        assert!(boxes.answer("r/c1", true));
        assert_eq!(boxes.pending_for("r"), None);
        assert_eq!(boxes.wait("r/c1", Duration::from_secs(1), &|| false), Consent::Approved);
        // A timed-out request is forgotten too.
        boxes.open("r/c2");
        boxes.describe("r/c2", serde_json::json!({"id": "c2"}));
        assert_eq!(boxes.wait("r/c2", Duration::from_millis(50), &|| false), Consent::TimedOut);
        assert_eq!(boxes.pending_for("r"), None);
    }
}
