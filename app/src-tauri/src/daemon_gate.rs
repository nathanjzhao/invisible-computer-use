//! Bounded FIFO admission to the shared Unix daemon. A hot preview producer
//! must not repeatedly overtake an input or observation already waiting.
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

pub(super) const BUSY: &str = "daemon busy; request not sent";
const FAILED: &str = "daemon worker failed";

struct State {
    active: bool,
    next: u64,
    waiting: VecDeque<u64>,
}
pub(super) struct Gate {
    state: Mutex<State>,
    changed: Condvar,
}
pub(super) struct Guard<'a>(&'a Gate);
impl Gate {
    pub const fn new() -> Self {
        Self { state: Mutex::new(State { active: false, next: 0, waiting: VecDeque::new() }), changed: Condvar::new() }
    }
    pub fn acquire(&self, budget: Duration) -> Result<Guard<'_>, &'static str> {
        let deadline = Instant::now() + budget;
        let mut state = self.state.lock().map_err(|_| FAILED)?;
        let ticket = state.next;
        state.next = state.next.checked_add(1).ok_or(FAILED)?;
        state.waiting.push_back(ticket);
        loop {
            // A cancelled ticket cannot block the queue or enter after its
            // original deadline, even if the previous owner just released it.
            let Some(remaining) = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) else {
                state.waiting.retain(|item| *item != ticket);
                self.changed.notify_all();
                return Err(BUSY);
            };
            if !state.active && state.waiting.front() == Some(&ticket) {
                state.waiting.pop_front();
                state.active = true;
                return Ok(Guard(self));
            }
            state = self.changed.wait_timeout(state, remaining).map_err(|_| FAILED)?.0;
        }
    }
}
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.active = false;
        self.0.changed.notify_all();
    }
}

/// A passive poll that missed its admission deadline must not bypass the
/// occupied queue by forking a second CLI client. Keep the existing frame and
/// let the next scheduled poll try again. Other read transport fallbacks stay.
pub(super) fn passive<T>(direct: Result<T, String>, fallback: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match direct {
        Err(error) if error == BUSY => Err(error),
        Err(_) => fallback(),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, mpsc};
    use std::thread;
    fn queued(gate: &Gate, count: usize) {
        let until = Instant::now() + Duration::from_secs(2);
        while gate.state.lock().unwrap().waiting.len() != count {
            assert!(Instant::now() < until, "waiters did not reach the gate");
            thread::yield_now();
        }
    }
    #[test]
    fn queued_action_cannot_be_overtaken_by_a_new_preview() {
        let gate = Arc::new(Gate::new());
        let active = gate.acquire(Duration::from_secs(1)).unwrap();
        let (tx, rx) = mpsc::channel();
        let peer = gate.clone();
        let child = thread::spawn(move || {
            let _guard = peer.acquire(Duration::from_secs(2)).unwrap();
            tx.send("queued action").unwrap();
        });
        queued(&gate, 1);
        drop(active);
        let _preview = gate.acquire(Duration::from_secs(2)).unwrap();
        assert_eq!(rx.try_recv().unwrap(), "queued action");
        child.join().unwrap();
    }
    #[test]
    fn timeout_removes_its_ticket_and_the_next_waiter_recovers() {
        let gate = Arc::new(Gate::new());
        let active = gate.acquire(Duration::from_secs(1)).unwrap();
        let first = gate.clone();
        let expired = thread::spawn(move || {
            assert!(matches!(first.acquire(Duration::from_millis(30)), Err(BUSY)));
        });
        queued(&gate, 1);
        let next = gate.clone();
        let recovered = thread::spawn(move || { let _guard = next.acquire(Duration::from_secs(2)).unwrap(); });
        expired.join().unwrap();
        queued(&gate, 1);
        drop(active);
        recovered.join().unwrap();
        assert!(gate.state.lock().unwrap().waiting.is_empty());
        assert!(gate.acquire(Duration::from_secs(1)).is_ok());
    }
    #[test]
    fn expired_idle_admission_is_not_dispatch_authority() {
        let gate = Gate::new();
        assert!(matches!(gate.acquire(Duration::ZERO), Err(BUSY)));
        assert!(gate.state.lock().unwrap().waiting.is_empty());
        assert!(gate.acquire(Duration::from_secs(1)).is_ok());
    }
    #[test]
    fn a_busy_passive_poll_never_bypasses_the_native_queue() {
        let fallback = std::cell::Cell::new(0);
        let invoke = || { fallback.set(fallback.get()+1); Ok(7) };
        assert_eq!(passive(Err(BUSY.into()), invoke), Err(BUSY.into()));
        assert_eq!(fallback.get(), 0);
        assert_eq!(passive(Err("socket unavailable".into()), invoke), Ok(7));
        assert_eq!(fallback.get(), 1);
        assert_eq!(passive(Ok(9), invoke), Ok(9));
        assert_eq!(fallback.get(), 1);
    }
}
