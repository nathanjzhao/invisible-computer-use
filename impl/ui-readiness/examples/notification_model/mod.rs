//! Deterministic experiment, not a production notification source or scheduler.
//! Real AX callbacks, transport, encoding and image costs are not measured here.
use serde_json::{json, Value};
use std::time::Duration;
use ui_readiness::{Policy, Readiness, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    Polling,
    AdvisoryGate,
}

#[derive(Clone, Copy, Debug)]
pub enum EventKind {
    Activity,
    CoverageLost,
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub at_ms: u64,
    pub sequence: u64,
    pub epoch: u64,
    pub window: u64,
    pub kind: EventKind,
}

pub fn activity(at_ms: u64, sequence: u64) -> Event {
    Event {
        at_ms,
        sequence,
        epoch: 1,
        window: 7,
        kind: EventKind::Activity,
    }
}

#[derive(Clone, Debug)]
pub struct Change {
    pub at_ms: u64,
    pub value: &'static str,
    pub busy: bool,
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub changes: Vec<Change>,
    pub events: Vec<Event>,
    pub cancel_ms: Option<u64>,
    pub budget_ms: u64,
    pub subscription_available: bool,
}

impl Scenario {
    pub fn idle(name: &'static str) -> Self {
        Self {
            name,
            changes: vec![Change {
                at_ms: 0,
                value: "old",
                busy: false,
            }],
            events: vec![],
            cancel_ms: None,
            budget_ms: 2000,
            subscription_available: true,
        }
    }

    fn state(&self, at_ms: u64) -> &Change {
        self.changes
            .iter()
            .rev()
            .find(|change| change.at_ms <= at_ms)
            .expect("initial state")
    }
}

#[derive(Debug)]
pub struct Result {
    pub status: Status,
    pub elapsed_ms: u64,
    pub observations: usize,
    pub wakeups: usize,
    pub notifications: usize,
    pub raced_reads: usize,
    pub degraded_to_polling: bool,
    /// Oracle knows fixture changes between polls. Production does not.
    pub oracle_quiet_violation: bool,
    pub returned_value: Option<String>,
}

impl Result {
    #[allow(dead_code)] // also compiled by the integration test without the JSON example
    pub fn to_json(&self) -> Value {
        json!({"status":format!("{:?}",self.status),"elapsed_ms":self.elapsed_ms,
            "observations":self.observations,"wakeups":self.wakeups,
            "notifications":self.notifications,"raced_reads":self.raced_reads,
            "degraded_to_polling":self.degraded_to_polling,
            "oracle_quiet_violation":self.oracle_quiet_violation,"returned_value":self.returned_value})
    }
}

struct Hints {
    next: usize,
    sequence: u64,
    epoch: u64,
    enabled: bool,
    last_activity: Option<u64>,
}

impl Hints {
    /// Every accepted callback invalidates prior quiet samples. No notification
    /// is never used as proof of readiness or as permission to reuse an element.
    fn drain(
        &mut self,
        at_ms: u64,
        events: &[Event],
        machine: &mut Readiness,
        policy: Policy,
        result: &mut Result,
    ) -> bool {
        let mut invalidated = false;
        while self.next < events.len() && events[self.next].at_ms <= at_ms {
            let event = events[self.next];
            self.next += 1;
            result.notifications += 1;
            if event.window != 7 || event.epoch < self.epoch {
                continue;
            }
            let restarted = event.epoch > self.epoch;
            if !restarted && event.sequence <= self.sequence {
                continue;
            }
            if restarted
                || matches!(event.kind, EventKind::CoverageLost)
                || event.sequence != self.sequence + 1
            {
                self.enabled = false;
                self.last_activity = None;
                result.degraded_to_polling = true;
            } else if self.enabled {
                // Timestamp is callback receipt, not the event's possibly old
                // source time or an absolute timestamp from another process.
                self.last_activity = Some(event.at_ms);
            }
            self.epoch = event.epoch;
            self.sequence = event.sequence;
            *machine = Readiness::new(policy);
            invalidated = true;
        }
        invalidated
    }
}

/// Simulate non-preemptible native reads plus cancellable waits, using the
/// shipped Readiness state machine for the useful-sample and busy checks.
/// The candidate only suppresses reads while a scoped event already means
/// readiness cannot pass. It then needs an entirely fresh quiet sample window.
pub fn simulate(scenario: &Scenario, strategy: Strategy, read_ms: u64) -> Result {
    let policy = Policy {
        budget: Duration::from_millis(scenario.budget_ms),
        ..Policy::default()
    };
    let quiet_ms = policy.quiet.as_millis() as u64;
    let poll_ms = policy.poll.as_millis() as u64;
    let mut machine = Readiness::new(policy);
    let mut hints = Hints {
        next: 0,
        sequence: 0,
        epoch: 1,
        enabled: scenario.subscription_available,
        last_activity: None,
    };
    let mut result = Result {
        status: Status::Waiting,
        elapsed_ms: 0,
        observations: 0,
        wakeups: 0,
        notifications: 0,
        raced_reads: 0,
        degraded_to_polling: !scenario.subscription_available,
        oracle_quiet_violation: false,
        returned_value: None,
    };
    let advisory = strategy == Strategy::AdvisoryGate && scenario.subscription_available;
    let mut now = 0;
    let mut next_read = 0;
    loop {
        let cancelled = scenario.cancel_ms.is_some_and(|at| now >= at);
        result.status = machine.check(Duration::from_millis(now), cancelled);
        if result.status != Status::Waiting {
            break;
        }
        if advisory {
            hints.drain(now, &scenario.events, &mut machine, policy, &mut result);
        }
        let gate = hints
            .last_activity
            .filter(|_| advisory && hints.enabled)
            .map_or(0, |at| at.saturating_add(quiet_ms));
        let read_at = next_read.max(gate);
        if now < read_at {
            let next_event = if advisory {
                scenario
                    .events
                    .get(hints.next)
                    .map_or(u64::MAX, |event| event.at_ms)
            } else {
                u64::MAX
            };
            now = read_at
                .min(next_event)
                .min(scenario.cancel_ms.unwrap_or(u64::MAX))
                .min(scenario.budget_ms);
            result.wakeups += 1;
            continue;
        }
        let sampled = scenario.state(now);
        let state = json!({"pid":42,"window_id":7,"elements":[{"role":"AXTextField","label":"Result","value":sampled.value}],
            "passive_elements":if sampled.busy {vec![json!({"role":"AXBusyIndicator","label":"Loading result","read_only":true})]}else{vec![]}});
        result.observations += 1;
        now = now.saturating_add(read_ms);
        let raced =
            advisory && hints.drain(now, &scenario.events, &mut machine, policy, &mut result);
        if raced {
            result.raced_reads += 1;
        }
        let cancelled = scenario.cancel_ms.is_some_and(|at| now >= at);
        result.status = machine.check(Duration::from_millis(now), cancelled);
        if result.status != Status::Waiting {
            break;
        }
        if !raced {
            result.status = machine.advance(Duration::from_millis(now), &state, false);
        }
        if result.status == Status::Ready {
            result.returned_value = Some(sampled.value.to_owned());
            let actual = scenario.state(now);
            result.oracle_quiet_violation = actual.busy
                || actual.value != sampled.value
                || now.saturating_sub(actual.at_ms) < quiet_ms;
            break;
        }
        if result.status != Status::Waiting {
            break;
        }
        next_read = now.saturating_add(poll_ms);
    }
    result.elapsed_ms = now;
    result
}

pub fn scenarios() -> Vec<Scenario> {
    let idle = Scenario::idle("idle");
    let mut old_new = Scenario::idle("old_old_new");
    old_new.changes.push(Change {
        at_ms: 120,
        value: "new",
        busy: false,
    });
    let mut busy = Scenario::idle("busy_dense_60ms_events");
    busy.changes = vec![
        Change {
            at_ms: 0,
            value: "pending",
            busy: true,
        },
        Change {
            at_ms: 600,
            value: "42",
            busy: false,
        },
    ];
    busy.events = (0..=10).map(|n| activity(n * 60, n + 1)).collect();
    let mut sparse = busy.clone();
    sparse.name = "busy_create_destroy_only";
    sparse.events = vec![activity(0, 1), activity(600, 2)];
    let mut completion_only = busy.clone();
    completion_only.name = "busy_completion_event_only";
    completion_only.events = vec![activity(600, 1)];
    let mut lost = busy.clone();
    lost.name = "busy_all_events_lost";
    lost.events.clear();
    let mut silent = old_new.clone();
    silent.name = "silent_value_change";
    silent.changes[1].at_ms = 90;
    let mut transient = Scenario::idle("transient_observed_events");
    transient.changes.extend([
        Change {
            at_ms: 30,
            value: "old",
            busy: true,
        },
        Change {
            at_ms: 40,
            value: "old",
            busy: false,
        },
    ]);
    transient.events = vec![activity(30, 1), activity(40, 2)];
    let mut missed = transient.clone();
    missed.name = "transient_all_events_lost";
    missed.events.clear();
    let mut late = transient.clone();
    late.name = "transient_callbacks_after_ready";
    late.events = vec![activity(200, 1), activity(210, 2)];
    let mut delayed = silent.clone();
    delayed.name = "delayed_callback";
    delayed.events = vec![activity(190, 1)];
    let mut storm = Scenario::idle("duplicate_title_storm");
    storm.events = (0..67).map(|n| activity(n * 30, n + 1)).collect();
    let mut duplicate = Scenario::idle("repeated_same_sequence");
    duplicate.events = vec![activity(0, 1), activity(40, 1), activity(100, 1)];
    let mut busy_forever = Scenario::idle("permanent_busy");
    busy_forever.changes[0].busy = true;
    let mut race = Scenario::idle("event_during_read");
    race.changes.push(Change {
        at_ms: 20,
        value: "new",
        busy: false,
    });
    race.events = vec![activity(40, 1)];
    let mut boundary = Scenario::idle("event_at_quiet_boundary");
    boundary.events = vec![activity(120, 1)];
    let mut overflow = busy.clone();
    overflow.name = "queue_overflow";
    overflow.events[2].kind = EventKind::CoverageLost;
    let mut gap = busy.clone();
    gap.name = "sequence_gap";
    gap.events.remove(1);
    let mut unsupported = busy.clone();
    unsupported.name = "unsupported_subscription";
    unsupported.subscription_available = false;
    let mut other = Scenario::idle("other_window_notifications");
    other.events = (0..20)
        .map(|n| Event {
            window: 8,
            ..activity(n * 15, n + 1)
        })
        .collect();
    let mut stale = Scenario::idle("stale_epoch_callbacks");
    stale.events = (0..20)
        .map(|n| Event {
            epoch: 0,
            ..activity(n * 15, n + 1)
        })
        .collect();
    let mut restarted = busy.clone();
    restarted.name = "observer_restarted";
    restarted.events[2].epoch = 2;
    restarted.events[2].sequence = 1;
    let mut cancel = busy.clone();
    cancel.name = "cancel_at_90ms";
    cancel.cancel_ms = Some(90);
    let mut deadline = busy.clone();
    deadline.name = "deadline_at_100ms";
    deadline.budget_ms = 100;
    vec![
        idle,
        old_new,
        busy,
        sparse,
        completion_only,
        lost,
        silent,
        transient,
        missed,
        late,
        delayed,
        storm,
        duplicate,
        busy_forever,
        race,
        boundary,
        overflow,
        gap,
        unsupported,
        other,
        stale,
        restarted,
        cancel,
        deadline,
    ]
}

/// An event-free interval cannot authorize same-handle reuse. Native code
/// reads the current fingerprint on each addressed action; this counterexample
/// models the proposed shortcut's silent semantic retargeting, not an AX call.
pub fn cache_counterexample() -> Value {
    let before = "Apply to draft";
    let after = "Delete account";
    json!({"observed_label":before,"current_same_handle_label":after,"notifications_delivered":0,
        "always_revalidate":{"fingerprint_reads":1,"would_dispatch":before==after},
        "reuse_if_no_notification":{"fingerprint_reads":0,"would_dispatch":true},
        "synthetic":true})
}
