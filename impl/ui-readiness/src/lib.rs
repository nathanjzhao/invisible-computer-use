//! A local polling fallback inspired by recovered quiet-period and busy-state
//! checks. This does not subscribe to OS notifications or reproduce the native
//! algorithm. Defaults are local policy, not recovered native constants.
//!
//! Readiness is evidence of bounded stability, not proof that a delayed update
//! cannot arrive later. Callers retain the latest observation for decisions and
//! keep these private polls out of public diff and instruction-delivery state.

use serde_json::Value;
use std::time::{Duration, Instant};

mod pixels;

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub quiet: Duration,
    pub poll: Duration,
    /// The quiet-wait budget. AfterAction includes the first read; an initial
    /// idle read uses its observer's transport budget instead (see [`Mode`]).
    pub budget: Duration,
    pub consecutive: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(120),
            poll: Duration::from_millis(60),
            budget: Duration::from_secs(2),
            consecutive: 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Ready,
    TimedOut,
    Cancelled,
    Unusable,
    Failed,
}

impl Status {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Waiting => "waiting for a quiet, non-busy observation",
            Self::Ready => "quiet and non-busy",
            Self::TimedOut => "readiness budget elapsed; obtain a fresh observation before acting",
            Self::Cancelled => "readiness cancelled",
            Self::Unusable => "observation is incomplete or unavailable",
            Self::Failed => "observation failed",
        }
    }
}

/// Explicit metadata only. Missing busy metadata does not prevent readiness.
pub fn busy(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            let yes = |key: &str| fields.get(key).and_then(Value::as_bool) == Some(true);
            let role = fields.get("role").and_then(Value::as_str).unwrap_or("");
            let role: String = role
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            yes("busy")
                || yes("element_busy")
                || yes("elementBusy")
                || yes("AXElementBusy")
                || yes("loading")
                || yes("is_loading")
                || yes("isLoading")
                || matches!(role.as_str(), "axbusyindicator" | "busyindicator")
                || (matches!(role.as_str(), "axwebarea" | "webarea")
                    && ["loaded", "is_loaded", "isLoaded", "AXLoaded"]
                        .iter()
                        .any(|k| fields.get(*k).and_then(Value::as_bool) == Some(false)))
                || fields.values().any(busy)
        }
        Value::Array(values) => values.iter().any(busy),
        _ => false,
    }
}

/// A partial tree with some useful controls still cannot establish readiness.
/// Image decoding/authorization stays with the caller. This AX-only entry
/// point grants no image capability. Never interpret absent status as failure.
pub fn unusable_reason(doc: &Value) -> Option<&'static str> {
    unusable_reason_with_image(doc, false)
}

/// `usable_image` must come from the caller's image validation and user
/// authorization, not a flag in an untrusted driver document. Only a known
/// empty AX tree may fall back to that image; target/capture failures may not.
pub fn unusable_reason_with_image(doc: &Value, usable_image: bool) -> Option<&'static str> {
    let Some(fields) = doc.as_object() else {
        return Some("observation must be an object");
    };
    let usable_image = usable_image
        && fields
            .get("screenshot_frame_valid")
            .and_then(Value::as_bool)
            != Some(false)
        && !fields.get("screenshot_error").is_some_and(|v| !v.is_null());
    let ax_empty_image = usable_image
        && doc
            .get("elements")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        && doc
            .get("degraded_reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| {
                reason == "ax_tree_empty" || reason.starts_with("ax_tree_empty:")
            });
    if (fields.get("degraded").and_then(Value::as_bool) == Some(true) && !ax_empty_image)
        || ["partial", "isError"]
            .iter()
            .any(|k| fields.get(*k).and_then(Value::as_bool) == Some(true))
        || fields.get("complete").and_then(Value::as_bool) == Some(false)
        || ["status", "observation_status", "quality"].iter().any(|k| {
            matches!(
                fields.get(*k).and_then(Value::as_str),
                Some("partial" | "degraded" | "failed" | "error")
            )
        })
        || fields
            .get("error")
            .is_some_and(|v| !v.is_null() && v != &Value::Bool(false))
    {
        return Some("partial, degraded or failed observation");
    }
    let has_tree = ["elements", "passive_elements"].iter().any(|key| {
        doc.get(*key)
            .and_then(Value::as_array)
            .is_some_and(|v| !v.is_empty())
    });
    (!has_tree && !usable_image).then_some("observation has no useful tree or validated image")
}

/// Compare observable content while ignoring per-capture routing identities.
/// Compare decoded pixels when present, so a changing canvas is not a static
/// tree and capture timestamps in PNG metadata do not make a still image busy.
pub fn semantic_snapshot(doc: &Value) -> Value {
    fn clean(value: &Value) -> Value {
        match value {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .filter(|(key, _)| {
                        !matches!(
                            key.as_str(),
                            "snapshot_id"
                                | "snapshot_token"
                                | "element_token"
                                | "screenshot_id"
                                | "capture_id"
                                | "screenshot_file_path"
                                | "screenshot_path"
                                | "captured_at"
                                | "captured_at_ms"
                                | "capture_timestamp"
                                | "timestamp_ms"
                                | "elapsed_ms"
                                | "_elapsed_ms"
                        )
                    })
                    .map(|(k, v)| {
                        let pixel_content = match k.as_str() {
                            "screenshot_data_url" => pixels::data_url(v),
                            "screenshot_png_b64" | "png_b64" => pixels::png(v),
                            _ => None,
                        };
                        (k.clone(), pixel_content.unwrap_or_else(|| clean(v)))
                    })
                    .collect(),
            ),
            Value::Array(values) => Value::Array(values.iter().map(clean).collect()),
            _ => value.clone(),
        }
    }
    let mut semantic = clean(doc);
    if let Some(fields) = semantic.as_object_mut() {
        // Native driver lineage counters and rendered diffs describe delivery
        // history, not the current tree. The full elements/passive rows remain.
        fields.remove("diff");
        fields.remove("accessibility_snapshot_count");
        fields.remove("accessibilitySnapshotCount");
    }
    semantic
}

/// Pure machine. `elapsed` is monotonic time since this wait began. A slow
/// observation that finishes beyond the budget cannot become Ready.
pub struct Readiness {
    policy: Policy,
    baseline: Option<Value>,
    quiet_since: Duration,
    consecutive: usize,
}

impl Readiness {
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            baseline: None,
            quiet_since: Duration::ZERO,
            consecutive: 0,
        }
    }

    pub fn check(&self, elapsed: Duration, cancelled: bool) -> Status {
        if cancelled {
            Status::Cancelled
        } else if elapsed >= self.policy.budget {
            Status::TimedOut
        } else {
            Status::Waiting
        }
    }

    pub fn advance(&mut self, elapsed: Duration, doc: &Value, cancelled: bool) -> Status {
        self.advance_with_image(elapsed, doc, cancelled, false)
    }

    pub fn advance_with_image(
        &mut self,
        elapsed: Duration,
        doc: &Value,
        cancelled: bool,
        usable_image: bool,
    ) -> Status {
        let guard = self.check(elapsed, cancelled);
        if guard != Status::Waiting {
            return guard;
        }
        if unusable_reason_with_image(doc, usable_image).is_some() {
            return Status::Unusable;
        }
        let semantic = semantic_snapshot(doc);
        if busy(doc) {
            self.baseline = None;
            self.quiet_since = elapsed;
            self.consecutive = 0;
            return Status::Waiting;
        }
        if self.baseline.as_ref() == Some(&semantic) {
            self.consecutive += 1;
        } else {
            self.baseline = Some(semantic);
            self.quiet_since = elapsed;
            self.consecutive = 1;
        }
        if self.consecutive >= self.policy.consecutive.max(2)
            && elapsed.saturating_sub(self.quiet_since) >= self.policy.quiet
        {
            Status::Ready
        } else {
            Status::Waiting
        }
    }

    pub fn next_poll(&self, elapsed: Duration) -> Duration {
        self.policy
            .poll
            .max(Duration::from_millis(1))
            .min(self.policy.budget.saturating_sub(elapsed))
    }
}

pub trait Clock {
    fn now(&self) -> Duration;
    fn sleep(&self, duration: Duration);
}

pub struct SystemClock(Instant);
impl Default for SystemClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[derive(Clone, Copy)]
pub enum Mode {
    /// Initial observation or an action refusal: the first read uses the
    /// observer's transport deadline. Start the quiet budget only if busy.
    ImmediateIfUsable,
    AfterAction,
}

pub struct Outcome {
    pub status: Status,
    pub polls: u32,
    pub elapsed: Duration,
    pub latest: Option<Value>,
    pub error: Option<String>,
}

/// Bounded cooperative polling. In-flight synchronous observations cannot be
/// preempted; cancellation and the active quiet budget are checked on return.
/// ImmediateIfUsable leaves the initial read's bound to the observer's
/// transport, then opens this budget if that first snapshot is busy.
/// The target identity, when supplied by the driver, cannot change mid-wait.
pub fn poll(
    policy: Policy,
    mode: Mode,
    clock: &dyn Clock,
    cancelled: &dyn Fn() -> bool,
    observe: &dyn Fn() -> Result<Value, String>,
) -> Outcome {
    poll_with_image(policy, mode, clock, cancelled, observe, &|_| false)
}

/// Like [`poll`], with a capability supplied by the image consumer after it
/// validates both the screenshot and its authorization for this task.
pub fn poll_with_image(
    policy: Policy,
    mode: Mode,
    clock: &dyn Clock,
    cancelled: &dyn Fn() -> bool,
    observe: &dyn Fn() -> Result<Value, String>,
    usable_image: &dyn Fn(&Value) -> bool,
) -> Outcome {
    let start = clock.now();
    let elapsed = || clock.now().saturating_sub(start);
    let mut budget_start = start;
    let mut machine = Readiness::new(policy);
    let mut out = Outcome {
        status: Status::Waiting,
        polls: 0,
        elapsed: Duration::ZERO,
        latest: None,
        error: None,
    };
    let mut identity: Option<(Value, Value)> = None;
    loop {
        let initial = out.polls == 0 && matches!(mode, Mode::ImmediateIfUsable);
        out.status = if initial {
            if cancelled() {
                Status::Cancelled
            } else {
                Status::Waiting
            }
        } else {
            machine.check(clock.now().saturating_sub(budget_start), cancelled())
        };
        if out.status != Status::Waiting {
            break;
        }
        out.polls += 1;
        let observed = observe();
        if initial {
            budget_start = clock.now();
        }
        out.status = if initial {
            if cancelled() {
                Status::Cancelled
            } else {
                Status::Waiting
            }
        } else {
            machine.check(clock.now().saturating_sub(budget_start), cancelled())
        };
        if out.status != Status::Waiting {
            break;
        }
        let doc = match observed {
            Ok(doc) => doc,
            Err(error) => {
                out.status = Status::Failed;
                out.error = Some(error);
                break;
            }
        };
        let current_identity = (doc["pid"].clone(), doc["window_id"].clone());
        if identity
            .as_ref()
            .is_some_and(|prior| prior != &current_identity)
        {
            out.status = Status::Unusable;
            out.error = Some("observation target changed during readiness wait".into());
            break;
        }
        identity = Some(current_identity);
        let image = usable_image(&doc);
        let unusable = unusable_reason_with_image(&doc, image);
        out.status = if initial && !busy(&doc) {
            if cancelled() {
                Status::Cancelled
            } else if unusable.is_some() {
                Status::Unusable
            } else {
                Status::Ready
            }
        } else {
            machine.advance_with_image(
                clock.now().saturating_sub(budget_start),
                &doc,
                cancelled(),
                image,
            )
        };
        if out.status == Status::Unusable {
            out.error = unusable.map(str::to_string);
        }
        out.latest = Some(doc);
        if out.status != Status::Waiting {
            break;
        }
        clock.sleep(machine.next_poll(clock.now().saturating_sub(budget_start)));
    }
    out.elapsed = elapsed();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }
    fn doc(value: &str) -> Value {
        json!({"elements":[{"role":"AXTextField","value":value}]})
    }

    #[test]
    fn old_old_new_needs_a_fresh_quiet_interval() {
        let mut r = Readiness::new(Policy::default());
        for (at, value) in [(0, "old"), (60, "old"), (120, "new"), (180, "new")] {
            assert_eq!(r.advance(ms(at), &doc(value), false), Status::Waiting);
        }
        assert_eq!(r.advance(ms(240), &doc("new"), false), Status::Ready);
    }

    #[test]
    fn busy_to_ready_resets_quiet_even_when_values_are_constant() {
        let mut r = Readiness::new(Policy::default());
        let mut d = doc("same");
        d["elementBusy"] = json!(true);
        assert_eq!(r.advance(ms(0), &d, false), Status::Waiting);
        assert_eq!(r.advance(ms(240), &d, false), Status::Waiting);
        d["elementBusy"] = json!(false);
        assert_eq!(r.advance(ms(300), &d, false), Status::Waiting);
        assert_eq!(r.advance(ms(420), &d, false), Status::Ready);
        assert!(busy(&json!({"role":"AXBusyIndicator"})));
        assert!(busy(&json!({"role":"AXWebArea","loaded":false})));
        assert!(!busy(&doc("Loading is just a label")));
    }

    #[test]
    fn perpetual_change_timeout_and_cancellation_win_over_readiness() {
        let mut r = Readiness::new(Policy {
            budget: ms(180),
            ..Policy::default()
        });
        for at in [0, 60, 120] {
            assert_eq!(
                r.advance(ms(at), &doc(&at.to_string()), false),
                Status::Waiting
            );
        }
        assert_eq!(r.advance(ms(180), &doc("120"), false), Status::TimedOut);
        assert_eq!(r.advance(ms(180), &doc("120"), true), Status::Cancelled);
    }

    #[test]
    fn changing_capture_tokens_are_not_semantic_changes() {
        let mut r = Readiness::new(Policy::default());
        let mut d = doc("same");
        for (at, expected) in [
            (0, Status::Waiting),
            (60, Status::Waiting),
            (120, Status::Ready),
        ] {
            d["snapshot_id"] = json!(at);
            d["screenshot_id"] = json!(at);
            d["elements"][0]["element_token"] = json!(at);
            d["screenshot_file_path"] = json!(format!("/tmp/capture-{at}"));
            d["diff"] = json!({"accessibility_snapshot_count":at,"kind":if at==0 {"full_first"}else{"no_change"},"text":format!("delivery {at}")});
            assert_eq!(r.advance(ms(at), &d, false), expected);
        }
    }

    #[test]
    fn partial_or_degraded_controls_and_failed_reads_are_not_ready() {
        for flag in ["degraded", "partial", "isError"] {
            let mut d = doc("same");
            d[flag] = json!(true);
            assert_eq!(
                Readiness::new(Policy::default()).advance(ms(0), &d, false),
                Status::Unusable
            );
        }
        assert_eq!(
            Readiness::new(Policy::default()).advance(ms(0), &json!({"elements":[]}), false),
            Status::Unusable
        );
        let out = poll(
            Policy::default(),
            Mode::AfterAction,
            &SystemClock::default(),
            &|| false,
            &|| Err("broken pipe".into()),
        );
        assert_eq!(out.status, Status::Failed);
        assert_eq!(out.error.as_deref(), Some("broken pipe"));
    }

    #[derive(Default)]
    struct FakeClock(std::cell::Cell<Duration>);
    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            self.0.get()
        }
        fn sleep(&self, duration: Duration) {
            self.0.set(self.0.get() + duration);
        }
    }

    #[test]
    fn poll_refuses_target_changes_and_over_budget_reads() {
        let n = std::cell::Cell::new(0);
        let out = poll(
            Policy::default(),
            Mode::AfterAction,
            &FakeClock::default(),
            &|| false,
            &|| {
                let mut d = doc("same");
                d["pid"] = json!(n.get());
                n.set(n.get() + 1);
                Ok(d)
            },
        );
        assert_eq!(out.status, Status::Unusable);
        assert_eq!(out.polls, 2);
        let clock = FakeClock::default();
        let out = poll(
            Policy::default(),
            Mode::AfterAction,
            &clock,
            &|| false,
            &|| {
                clock.sleep(Duration::from_secs(3));
                Ok(doc("same"))
            },
        );
        assert_eq!(out.status, Status::TimedOut);
        assert_eq!(out.polls, 1);
    }

    #[test]
    fn passive_text_and_images_are_useful_without_busy_metadata() {
        for d in [
            json!({"elements":[],"passive_elements":[{"role":"AXStaticText","value":"hello"}]}),
            json!({"screenshot_data_url":"image supplied by caller"}),
        ] {
            assert!(unusable_reason_with_image(&d, true).is_none());
            assert!(!busy(&d));
        }
    }

    #[test]
    fn only_validated_images_can_rescue_an_explicit_empty_ax_tree() {
        let doc = json!({"elements":[],"degraded":true,"degraded_reason":"ax_tree_empty: canvas",
            "screenshot_frame_valid":true,"screenshot_data_url":"caller validated bytes"});
        assert!(
            unusable_reason(&doc).is_some(),
            "AX-only ICU polling grants no image capability"
        );
        assert!(unusable_reason_with_image(&doc, true).is_none());
        for (key, value) in [
            (
                "degraded_reason",
                json!("ax_window_unresolved: wrong AX window"),
            ),
            ("screenshot_frame_valid", json!(false)),
            ("screenshot_error", json!({"code":"capture_failed"})),
            ("partial", json!(true)),
            ("isError", json!(true)),
        ] {
            let mut bad = doc.clone();
            bad[key] = value;
            assert!(unusable_reason_with_image(&bad, true).is_some(), "{key}");
        }
    }

    #[test]
    fn first_idle_read_uses_transport_deadline_and_busy_read_opens_quiet_budget() {
        for initially_busy in [false, true] {
            let clock = FakeClock::default();
            let reads = std::cell::Cell::new(0);
            let out = poll(
                Policy::default(),
                Mode::ImmediateIfUsable,
                &clock,
                &|| false,
                &|| {
                    let first = reads.get() == 0;
                    reads.set(reads.get() + 1);
                    if first {
                        clock.sleep(Duration::from_secs(3));
                    }
                    let mut d = doc("same");
                    d["busy"] = json!(first && initially_busy);
                    Ok(d)
                },
            );
            assert_eq!(out.status, Status::Ready);
            if initially_busy {
                assert_eq!(out.elapsed, ms(3180));
                assert_eq!(out.polls, 4);
            } else {
                assert_eq!(out.elapsed, ms(3000));
                assert_eq!(out.polls, 1);
            }
        }
    }

    #[test]
    fn markdown_content_that_resembles_a_token_is_still_content() {
        let mut a = doc("same");
        a["tree_markdown"] = json!("- label = s00000042:0");
        let mut b = a.clone();
        b["tree_markdown"] = json!("- label = s00000043:0");
        assert_ne!(semantic_snapshot(&a), semantic_snapshot(&b));
    }
}
