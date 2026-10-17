//! `post_action_sleep_ms` (spec 10 step 3), a port of Sky's `action_settler.js`.
//!
//! Sky's Linux client keeps one deadline per client. After every successful input it pushes the
//! deadline to `now + post_action_sleep_ms` (default 100) and never pulls it back; before the next
//! action, screenshot or audio operation it sleeps until the deadline has passed. So two actions
//! are never closer than the window, and a screenshot is never taken while the last click's
//! redraw is still under way. `Settler` is the same machine with a fake-able clock.
//!
//! This is the minimum gap between operations. It is not the AX-quiet wait (`settle` in the input
//! tools polls the tree until it stops changing) and it is not the action `deadline_ms`.
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const DEFAULT_POST_ACTION_SLEEP_MS: u64 = 100;

/// Time source, so the tests need not sleep.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn sleep(&self, d: Duration);
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> Instant { Instant::now() }
    fn sleep(&self, d: Duration) { std::thread::sleep(d) }
}

pub struct Settler {
    clock: Box<dyn Clock>,
    deadline: Mutex<Instant>,
}

impl Settler {
    pub fn new() -> Settler { Settler::with_clock(Box::new(SystemClock)) }
    pub fn with_clock(clock: Box<dyn Clock>) -> Settler {
        let now = clock.now();
        Settler { clock, deadline: Mutex::new(now) }
    }

    /// Sleep until the deadline has passed (`wait()` in the JS). Returns the milliseconds slept.
    pub fn wait(&self) -> u64 {
        let mut slept = Duration::ZERO;
        loop {
            let remaining = self.deadline.lock().unwrap().saturating_duration_since(self.clock.now());
            if remaining.is_zero() { return slept.as_millis() as u64; }
            self.clock.sleep(remaining);
            slept += remaining;
        }
    }

    /// Push the deadline out by `ms` from now after a successful input (`defer()`); `0` disables.
    /// The deadline only ever moves later.
    pub fn defer(&self, ms: u64) {
        if ms == 0 { return; }
        let candidate = self.clock.now() + Duration::from_millis(ms);
        let mut d = self.deadline.lock().unwrap();
        if candidate > *d { *d = candidate; }
    }
}

impl Default for Settler { fn default() -> Self { Settler::new() } }

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A clock that only moves when something sleeps on it.
    struct Fake { t: Mutex<Instant>, slept: Mutex<Vec<Duration>> }
    struct Handle(Arc<Fake>);
    impl Clock for Handle {
        fn now(&self) -> Instant { *self.0.t.lock().unwrap() }
        fn sleep(&self, d: Duration) { *self.0.t.lock().unwrap() += d; self.0.slept.lock().unwrap().push(d); }
    }
    fn fake() -> (Settler, Arc<Fake>) {
        let f = Arc::new(Fake { t: Mutex::new(Instant::now()), slept: Mutex::new(vec![]) });
        (Settler::with_clock(Box::new(Handle(f.clone()))), f)
    }

    #[test]
    fn nothing_to_wait_for_before_the_first_action() {
        let (s, f) = fake();
        assert_eq!(s.wait(), 0);
        assert!(f.slept.lock().unwrap().is_empty());
    }

    #[test]
    fn the_next_operation_waits_out_the_window_after_an_action() {
        let (s, f) = fake();
        s.defer(100);
        assert_eq!(s.wait(), 100, "an immediate follow-up sleeps the whole window");
        assert_eq!(s.wait(), 0, "and only once");
        // time passes on its own: a follow-up after 60ms waits only the remaining 40ms
        s.defer(100);
        *f.t.lock().unwrap() += Duration::from_millis(60);
        assert_eq!(s.wait(), 40);
    }

    #[test]
    fn the_deadline_never_moves_earlier() {
        let (s, _f) = fake();
        s.defer(500);
        s.defer(10);
        assert_eq!(s.wait(), 500, "a shorter later deferral does not shorten the earlier one");
    }

    #[test]
    fn zero_disables_it() {
        let (s, f) = fake();
        s.defer(0);
        assert_eq!(s.wait(), 0);
        assert!(f.slept.lock().unwrap().is_empty());
    }
}
