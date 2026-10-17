//! Exercise the production loop, including decisions and dispatched snapshots.
use super::*;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Default)]
struct Clock(Cell<Duration>);
impl ui_readiness::Clock for Clock {
    fn now(&self) -> Duration {
        self.0.get()
    }
    fn sleep(&self, d: Duration) {
        self.0.set(self.0.get() + d);
    }
}

struct Brain {
    calls: AtomicUsize,
    after: Box<dyn Fn(usize, &str) -> Action + Send + Sync>,
}
impl brain::Brain for Brain {
    fn name(&self) -> String {
        "readiness-fixture".into()
    }
    fn decide(
        &self,
        _: &str,
        _: Option<&str>,
        tree: &str,
        _: &[String],
    ) -> Result<brain::Decision, String> {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        let action = if call == 0 {
            Action::Type {
                text: "start".into(),
            }
        } else {
            (self.after)(call, tree)
        };
        Ok(brain::Decision {
            thought: String::new(),
            action: Some(action),
            actions: vec![],
            fallback: None,
        })
    }
}
fn brain(after: impl Fn(usize, &str) -> Action + Send + Sync + 'static) -> Brain {
    Brain {
        calls: AtomicUsize::new(0),
        after: Box::new(after),
    }
}
fn doc(n: usize, value: &str) -> Value {
    json!({"pid":42,"window_id":7,"snapshot_id":format!("snapshot-{n}"),
        "elements":[{"role":"AXTextField","element_index":0,"element_token":format!("token-{n}"),"label":"Result","value":value}]})
}
type Events = RefCell<Vec<(String, Value)>>;
fn run(
    brain: &Brain,
    observe: &dyn Fn() -> Result<Value, String>,
    dispatch: &dyn Fn(&Action, &Value) -> Result<(), ActionFailure>,
    cancelled: &dyn Fn() -> bool,
    events: &Events,
    clock: &Clock,
) {
    run_loop_with_readiness(
        "wait for result",
        None,
        "fixture",
        brain,
        &|kind, data| events.borrow_mut().push((kind.into(), data)),
        observe,
        dispatch,
        cancelled,
        None,
        &gate::Gate::open(),
        &[],
        4,
        clock,
        ui_readiness::Policy {
            budget: Duration::from_millis(400),
            ..Default::default()
        },
    );
}

#[test]
fn old_old_new_reaches_the_model_and_dispatch_with_latest_tokens() {
    let n = Cell::new(0);
    let actions = Cell::new(0);
    let brain = brain(move |call, tree| {
        assert!(
            tree.contains("new"),
            "model saw premature old state: {tree}"
        );
        if call == 1 {
            Action::Type {
                text: "finish".into(),
            }
        } else {
            Action::Done {
                summary: "ready".into(),
            }
        }
    });
    let events = Events::default();
    let clock = Clock::default();
    run(
        &brain,
        &|| {
            let ix = n.get();
            n.set(ix + 1);
            Ok(doc(ix, if ix < 3 { "old" } else { "new" }))
        },
        &|_, state| {
            if actions.get() == 1 {
                assert_eq!(state["snapshot_id"], "snapshot-5");
                assert_eq!(state["elements"][0]["element_token"], "token-5");
            }
            actions.set(actions.get() + 1);
            Ok(())
        },
        &|| false,
        &events,
        &clock,
    );
    assert_eq!(actions.get(), 2);
    assert_eq!(brain.calls.load(Ordering::Relaxed), 3);
    assert!(events.borrow().iter().any(|(kind, _)| kind == "done"));
}

#[test]
fn constant_tree_busy_metadata_delays_the_next_decision() {
    let n = Cell::new(0);
    let brain = brain(|_, _| Action::Done {
        summary: "ready".into(),
    });
    let events = Events::default();
    run(
        &brain,
        &|| {
            let ix = n.get();
            n.set(ix + 1);
            let mut state = doc(ix, "unchanged");
            state["element_busy"] = json!(matches!(ix, 1 | 2));
            Ok(state)
        },
        &|_, _| Ok(()),
        &|| false,
        &events,
        &Clock::default(),
    );
    assert_eq!(
        n.get(),
        6,
        "busy reads reset the quiet window even with identical AX values"
    );
    assert_eq!(brain.calls.load(Ordering::Relaxed), 2);
    assert!(events.borrow().iter().any(|(kind, _)| kind == "done"));
}

#[test]
fn perpetual_changes_timeout_without_a_second_decision_or_replay() {
    let n = Cell::new(0);
    let actions = Cell::new(0);
    let brain = brain(|_, _| panic!("unsettled state reached the model"));
    let events = Events::default();
    let clock = Clock::default();
    run(
        &brain,
        &|| {
            let ix = n.get();
            n.set(ix + 1);
            Ok(doc(ix, &ix.to_string()))
        },
        &|_, _| {
            actions.set(actions.get() + 1);
            Ok(())
        },
        &|| false,
        &events,
        &clock,
    );
    assert_eq!(brain.calls.load(Ordering::Relaxed), 1);
    assert_eq!(actions.get(), 1);
    assert_eq!(clock.0.get(), Duration::from_millis(400));
    assert!(events
        .borrow()
        .iter()
        .any(|(kind, d)| kind == "error" && d["msg"].as_str().unwrap().contains("budget elapsed")));
}

#[test]
fn cancellation_during_poll_stops_before_another_decision() {
    let n = Cell::new(0);
    let cancel = Cell::new(false);
    let brain = brain(|_, _| panic!("decision after cancellation"));
    let events = Events::default();
    run(
        &brain,
        &|| {
            let ix = n.get();
            n.set(ix + 1);
            if ix == 2 {
                cancel.set(true)
            };
            Ok(doc(ix, "old"))
        },
        &|_, _| Ok(()),
        &|| cancel.get(),
        &events,
        &Clock::default(),
    );
    assert_eq!(n.get(), 3);
    assert_eq!(brain.calls.load(Ordering::Relaxed), 1);
    assert_eq!(events.borrow().last().unwrap().0, "stopped");
}

#[test]
fn incomplete_post_action_state_is_not_silently_ready() {
    for flag in ["partial", "degraded"] {
        let n = Cell::new(0);
        let brain = brain(|_, _| panic!("partial state reached model"));
        let events = Events::default();
        run(
            &brain,
            &|| {
                let ix = n.get();
                n.set(ix + 1);
                let mut d = doc(ix, "same");
                if ix > 0 {
                    d[flag] = json!(true)
                };
                Ok(d)
            },
            &|_, _| Ok(()),
            &|| false,
            &events,
            &Clock::default(),
        );
        assert_eq!(brain.calls.load(Ordering::Relaxed), 1);
        assert_eq!(events.borrow().last().unwrap().0, "error");
    }
}

#[test]
fn idle_first_observation_does_not_add_a_quiet_wait() {
    let n = Cell::new(0);
    let cancel = Cell::new(false);
    let clock = Clock::default();
    let events = Events::default();
    run(
        &brain(|_, _| panic!("unused")),
        &|| {
            n.set(n.get() + 1);
            Ok(doc(0, "idle"))
        },
        &|_, _| {
            cancel.set(true);
            Ok(())
        },
        &|| cancel.get(),
        &events,
        &clock,
    );
    assert_eq!(n.get(), 1);
    assert_eq!(clock.0.get(), Duration::ZERO);
}

#[test]
fn invalid_or_unapproved_canvas_image_cannot_rescue_a_failed_observation() {
    for (key, value) in [
        ("task_screenshots_authorized", json!(false)),
        (
            "screenshot_data_url",
            json!("data:image/png;base64,invalid"),
        ),
        ("screenshot_frame_valid", json!(false)),
        ("screenshot_error", json!({"code":"capture_failed"})),
        (
            "degraded_reason",
            json!("ax_window_unresolved: unresolved target"),
        ),
        ("partial", json!(true)),
    ] {
        let mut canvas = crate::vision::test_state();
        canvas["elements"] = json!([]);
        canvas["degraded"] = json!(true);
        canvas["degraded_reason"] = json!("ax_tree_empty: canvas");
        canvas["screenshot_frame_valid"] = json!(true);
        canvas[key] = value;
        let brain = brain(|_, _| panic!("unusable state reached model"));
        let events = Events::default();
        run(
            &brain,
            &|| Ok(canvas.clone()),
            &|_, _| panic!("unusable state delivered input"),
            &|| false,
            &events,
            &Clock::default(),
        );
        assert_eq!(brain.calls.load(Ordering::Relaxed), 0, "{key}");
        assert_eq!(events.borrow().last().unwrap().0, "error", "{key}");
    }
}

#[test]
fn first_idle_observation_can_use_more_than_the_quiet_budget() {
    let cancel = Cell::new(false);
    let clock = Clock::default();
    let events = Events::default();
    let brain = brain(|_, _| panic!("unused"));
    run(
        &brain,
        &|| {
            clock.0.set(Duration::from_secs(3));
            Ok(doc(0, "idle"))
        },
        &|_, _| {
            cancel.set(true);
            Ok(())
        },
        &|| cancel.get(),
        &events,
        &clock,
    );
    assert_eq!(brain.calls.load(Ordering::Relaxed), 1);
    assert!(events.borrow().iter().any(|(kind, _)| kind == "act"));
}
