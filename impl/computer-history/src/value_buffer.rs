//! Cross-platform ordering buffer for source value notifications.
//!
//! Publishing policy: a captured field value stays pending until either a quiet
//! interval elapses (so intermediate typing stays private) or a native action
//! boundary is reached whose timestamp is at or after the value's own source
//! time. The second rule is what keeps the final field values captured *before*
//! the action that consumed them: when the collector observes an Export/commit
//! click it drains with that click's timestamp as the boundary, and every value
//! whose source time precedes the click is published with its own source time,
//! not minted later. A slower callback cannot resurrect an older field value
//! once a newer one for the same element was drained.
//!
//! The buffer owns only data (no COM handles), so it is unit testable on every
//! platform even though the producer that feeds it is Windows only.
use crate::{capture::Content, store::Ticket};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// Longest a value may stay pending before it is dropped as partial.
const MAX_PENDING: Duration = Duration::from_secs(5);
/// Quiet interval after which an unbounded value publishes on its own.
const QUIET_INTERVAL: Duration = Duration::from_millis(400);
/// Distinct in-flight elements retained before new identities are dropped.
const MAX_IDENTITIES: usize = 64;

pub struct ValueChange {
    pub ticket: Ticket,
    pub content: Content,
    pub(crate) received: Instant,
    pub(crate) source_order: Instant,
}

impl ValueChange {
    pub(crate) fn new(
        ticket: Ticket,
        content: Content,
        received: Instant,
        source_order: Instant,
    ) -> Self {
        Self {
            ticket,
            content,
            received,
            source_order,
        }
    }
}

#[derive(Default)]
pub(crate) struct ValueBuffer {
    pending: BTreeMap<Vec<i32>, ValueChange>,
    // Highest source order drained or queued per element. Retained briefly after
    // publication so a late, out-of-order callback cannot resurrect a stale value.
    newest: BTreeMap<Vec<i32>, Instant>,
}

impl ValueBuffer {
    /// Queue one value for its element. Returns true when the distinct-identity
    /// cap forces the new value to be dropped, which the caller marks partial.
    pub(crate) fn ingest(&mut self, identity: Vec<i32>, value: ValueChange) -> bool {
        if self
            .newest
            .get(&identity)
            .is_some_and(|at| *at >= value.source_order)
        {
            // A newer value for this element was already seen or drained.
            return false;
        }
        if !self.newest.contains_key(&identity) && self.newest.len() >= MAX_IDENTITIES {
            return true;
        }
        self.newest.insert(identity.clone(), value.source_order);
        self.pending.insert(identity, value);
        false
    }

    /// Publish every pending value whose quiet interval elapsed or whose source
    /// time is at or before `boundary_ms` (a native action boundary such as the
    /// Export click). Values older than the retention cap are dropped partial.
    /// Returned values are sorted by their source timestamp.
    pub(crate) fn drain(&mut self, boundary_ms: u64) -> (Vec<ValueChange>, bool) {
        let mut partial = false;
        let pending = &self.pending;
        self.newest
            .retain(|identity, at| at.elapsed() <= MAX_PENDING || pending.contains_key(identity));
        self.pending.retain(|_, value| {
            if value.received.elapsed() > MAX_PENDING {
                partial = true;
                return false;
            }
            true
        });
        let keys: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, value)| {
                value.received.elapsed() >= QUIET_INTERVAL
                    || value.ticket.timestamp() <= boundary_ms
            })
            .map(|(key, _)| key.clone())
            .collect();
        let mut ready = Vec::new();
        for key in keys {
            if let Some(value) = self.pending.remove(&key) {
                ready.push(value);
            }
        }
        ready.sort_by_key(|value| value.ticket.timestamp());
        (ready, partial)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(timestamp_ms: u64, received: Instant, source_order: Instant) -> ValueChange {
        ValueChange::new(
            Ticket::for_test(timestamp_ms),
            Content {
                role: "AXTextField".into(),
                label: Some("Note".into()),
                identifier: Some(format!("note-{timestamp_ms}")),
                value: Some(format!("value-{timestamp_ms}")),
            },
            received,
            source_order,
        )
    }

    fn ago(ms: u64) -> Instant {
        Instant::now()
            .checked_sub(Duration::from_millis(ms))
            .expect("monotonic clock has enough uptime for the test offset")
    }

    // The counterexample: ten final Note values all captured shortly before the
    // Export click (so none has cleared the quiet interval) must all publish at
    // the Export boundary, each keeping its own source timestamp. Without the
    // boundary clause in `drain`, the quiet-interval rule would hold every one
    // of them back and they would only surface after Export.
    #[test]
    fn final_values_flush_at_the_export_boundary() {
        let mut buffer = ValueBuffer::default();
        let now = Instant::now();
        let names = [
            "Alder", "Aspen", "Birch", "Cedar", "Fir", "Maple", "Oak", "Pine", "Spruce", "Willow",
        ];
        // Source times 1000..1010; the Export click boundary is 1100.
        for (index, _name) in names.iter().enumerate() {
            let ts = 1000 + index as u64;
            let partial = buffer.ingest(vec![100 + index as i32], change(ts, now, now));
            assert!(!partial, "ingest should accept a fresh element");
        }
        let (ready, partial) = buffer.drain(1100);
        assert!(!partial, "a timely boundary drain is not partial");
        assert_eq!(ready.len(), 10, "all ten final values publish at Export");
        let order: Vec<u64> = ready.iter().map(|v| v.ticket.timestamp()).collect();
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(order, sorted, "published values are ordered by source time");
        // Nothing lingers to surface only after Export.
        let (leftover, _) = buffer.drain(u64::MAX);
        assert!(leftover.is_empty(), "no value is left pending after Export");
    }

    // A value captured just before the boundary but whose source time is *after*
    // the boundary is intermediate typing: it stays pending (private) until the
    // quiet interval, and is not forced out by an earlier action boundary.
    #[test]
    fn intermediate_typing_after_the_boundary_stays_pending() {
        let mut buffer = ValueBuffer::default();
        let now = Instant::now();
        buffer.ingest(vec![7], change(2000, now, now));
        let (ready, partial) = buffer.drain(1000);
        assert!(ready.is_empty(), "a value newer than the boundary is held");
        assert!(!partial);
    }

    // Once the quiet interval has elapsed, an unbounded value publishes on its
    // own even with no action boundary at all.
    #[test]
    fn quiet_interval_publishes_without_a_boundary() {
        let mut buffer = ValueBuffer::default();
        let old = ago(500);
        buffer.ingest(vec![7], change(2000, old, old));
        let (ready, partial) = buffer.drain(0);
        assert_eq!(ready.len(), 1, "a settled value publishes after the quiet interval");
        assert!(!partial);
    }

    // A slow, out-of-order callback for an element already drained with a newer
    // value must not resurrect the older value.
    #[test]
    fn a_late_older_callback_cannot_resurrect_a_drained_value() {
        let mut buffer = ValueBuffer::default();
        let now = Instant::now();
        let newer_order = now;
        let older_order = ago(50);
        buffer.ingest(vec![7], change(1005, now, newer_order));
        let (ready, _) = buffer.drain(1100);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].ticket.timestamp(), 1005);
        // The stale callback arrives afterwards with an earlier source order.
        buffer.ingest(vec![7], change(1001, now, older_order));
        let (ready, _) = buffer.drain(1100);
        assert!(ready.is_empty(), "the stale earlier value is suppressed");
    }

    // Values stuck longer than the retention cap are dropped and marked partial.
    #[test]
    fn expired_pending_values_are_dropped_partial() {
        let mut buffer = ValueBuffer::default();
        let stale = ago(6000);
        buffer.ingest(vec![7], change(1000, stale, stale));
        let (ready, partial) = buffer.drain(2000);
        assert!(ready.is_empty(), "an expired value is not published");
        assert!(partial, "dropping an expired value marks the drain partial");
    }

    // The distinct-identity cap drops the overflow element and reports partial.
    #[test]
    fn distinct_identity_cap_reports_partial() {
        let mut buffer = ValueBuffer::default();
        let now = Instant::now();
        for id in 0..MAX_IDENTITIES as i32 {
            assert!(!buffer.ingest(vec![id], change(1000 + id as u64, now, now)));
        }
        assert!(
            buffer.ingest(vec![9999], change(2000, now, now)),
            "a new identity past the cap is dropped partial"
        );
    }
}
