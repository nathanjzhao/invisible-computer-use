#[path = "../examples/notification_model/mod.rs"]
mod model;
use model::{scenarios, simulate, Strategy};
use ui_readiness::Status;

fn case(name: &str) -> model::Scenario {
    scenarios().into_iter().find(|s| s.name == name).unwrap()
}

#[test]
fn missing_or_unsupported_notifications_keep_polling_behavior() {
    for name in [
        "idle",
        "old_old_new",
        "busy_all_events_lost",
        "silent_value_change",
        "transient_all_events_lost",
        "unsupported_subscription",
        "permanent_busy",
    ] {
        for cost in [0, 20, 140] {
            let scenario = case(name);
            let p = simulate(&scenario, Strategy::Polling, cost);
            let n = simulate(&scenario, Strategy::AdvisoryGate, cost);
            assert_eq!(
                (
                    p.status,
                    p.elapsed_ms,
                    p.observations,
                    p.oracle_quiet_violation
                ),
                (
                    n.status,
                    n.elapsed_ms,
                    n.observations,
                    n.oracle_quiet_violation
                ),
                "{name}/{cost}"
            );
        }
    }
}

#[test]
fn scoped_events_can_reduce_reads_but_add_time_and_wakeups() {
    let scenario = case("busy_dense_60ms_events");
    let p = simulate(&scenario, Strategy::Polling, 0);
    let n = simulate(&scenario, Strategy::AdvisoryGate, 0);
    assert_eq!(p.status, Status::Ready);
    assert_eq!(n.status, Status::Ready);
    assert_eq!((p.observations, p.elapsed_ms), (13, 720));
    assert_eq!((n.observations, n.elapsed_ms), (3, 840));
    assert!(!n.oracle_quiet_violation);
    assert!(n.wakeups >= p.wakeups);
}

#[test]
fn notifications_expose_transients_but_silence_cannot_rule_them_out() {
    let live = case("transient_observed_events");
    let missing = case("transient_all_events_lost");
    assert!(simulate(&live, Strategy::Polling, 0).oracle_quiet_violation);
    assert!(!simulate(&live, Strategy::AdvisoryGate, 0).oracle_quiet_violation);
    assert!(simulate(&missing, Strategy::AdvisoryGate, 0).oracle_quiet_violation);
    assert!(
        simulate(
            &case("transient_callbacks_after_ready"),
            Strategy::AdvisoryGate,
            0
        )
        .oracle_quiet_violation
    );
}

#[test]
fn callbacks_before_commit_discard_raced_reads_and_reset_quiet_samples() {
    let raced = simulate(&case("event_during_read"), Strategy::AdvisoryGate, 140);
    assert_eq!(raced.status, Status::Ready);
    assert_eq!(raced.raced_reads, 1);
    assert_eq!(raced.returned_value.as_deref(), Some("new"));
    assert!(!raced.oracle_quiet_violation);
    let boundary = simulate(&case("event_at_quiet_boundary"), Strategy::AdvisoryGate, 0);
    assert_eq!(boundary.status, Status::Ready);
    assert!(boundary.elapsed_ms >= 240);
}

#[test]
fn irrelevant_or_duplicate_callbacks_cannot_keep_resetting_readiness() {
    for name in ["other_window_notifications", "stale_epoch_callbacks"] {
        let scenario = case(name);
        let p = simulate(&scenario, Strategy::Polling, 0);
        let n = simulate(&scenario, Strategy::AdvisoryGate, 0);
        assert_eq!(
            (p.status, p.elapsed_ms, p.observations),
            (n.status, n.elapsed_ms, n.observations)
        );
    }
    let duplicate = simulate(&case("repeated_same_sequence"), Strategy::AdvisoryGate, 0);
    assert_eq!(duplicate.status, Status::Ready);
    assert_eq!(duplicate.elapsed_ms, 240);
}

#[test]
fn unclassified_title_noise_is_a_real_regression_not_a_speedup() {
    let scenario = case("duplicate_title_storm");
    let p = simulate(&scenario, Strategy::Polling, 0);
    let n = simulate(&scenario, Strategy::AdvisoryGate, 0);
    assert_eq!(p.status, Status::Ready);
    assert_eq!(n.status, Status::TimedOut);
    assert_eq!(n.elapsed_ms, 2000);
    assert_eq!(n.observations, 0);
}

#[test]
fn lost_coverage_and_restart_disable_gating_without_reusing_cached_content() {
    for name in ["queue_overflow", "sequence_gap", "observer_restarted"] {
        let result = simulate(&case(name), Strategy::AdvisoryGate, 0);
        assert!(result.degraded_to_polling);
        assert!(result.observations >= 3);
        assert_eq!(result.status, Status::Ready);
        assert_eq!(result.returned_value.as_deref(), Some("42"));
    }
    // A restarted observer can reset its sequence. The previous epoch's
    // queued callbacks then have larger sequence numbers but remain stale.
    let mut restarted = case("observer_restarted");
    let queued = simulate(&restarted, Strategy::AdvisoryGate, 0);
    restarted.events.truncate(3);
    let without_old_callbacks = simulate(&restarted, Strategy::AdvisoryGate, 0);
    assert_eq!(
        (queued.status, queued.elapsed_ms, queued.observations),
        (
            without_old_callbacks.status,
            without_old_callbacks.elapsed_ms,
            without_old_callbacks.observations
        )
    );
}

#[test]
fn cancellation_and_deadlines_bound_waits_and_win_over_a_late_observation() {
    for cost in [0, 140] {
        for strategy in [Strategy::Polling, Strategy::AdvisoryGate] {
            let cancelled = simulate(&case("cancel_at_90ms"), strategy, cost);
            assert_eq!(cancelled.status, Status::Cancelled);
            assert!(cancelled.elapsed_ms <= 90 + cost);
            let expired = simulate(&case("deadline_at_100ms"), strategy, cost);
            assert_eq!(expired.status, Status::TimedOut);
            assert!(expired.elapsed_ms <= 100 + cost);
        }
    }
}

#[test]
fn absence_of_events_cannot_replace_current_fingerprint_validation() {
    let result = model::cache_counterexample();
    assert_eq!(result["always_revalidate"]["would_dispatch"], false);
    assert_eq!(result["reuse_if_no_notification"]["would_dispatch"], true);
    assert_eq!(result["notifications_delivered"], 0);
    assert!(result["synthetic"].as_bool().unwrap());
}
