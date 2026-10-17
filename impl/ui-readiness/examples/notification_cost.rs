//! Run with `cargo run -p ui-readiness --example notification_cost`.
//! Outputs simulated counts, not native timings or an implemented fast path.
mod notification_model;
use notification_model::{cache_counterexample, scenarios, simulate, Strategy};
use serde_json::json;

fn main() {
    let rows:Vec<_>=[0,20,140].into_iter().flat_map(|read_ms|scenarios().into_iter().map(move |scenario| {
        let polling=simulate(&scenario,Strategy::Polling,read_ms);
        let advisory=simulate(&scenario,Strategy::AdvisoryGate,read_ms);
        json!({"scenario":scenario.name,"assumed_read_cost_ms":read_ms,"polling":polling.to_json(),"advisory_gate":advisory.to_json()})
    })).collect();
    println!("{}",serde_json::to_string_pretty(&json!({"synthetic":true,"production_notifications":false,
        "scope":"scoped advisory events gate reads; no-event intervals never authorize readiness or cached input",
        "policy":{"quiet_ms":120,"poll_ms":60,"consecutive_useful_samples":2,"default_budget_ms":2000},
        "read_costs_are_assumptions":true,"rows":rows,"invalidation_counterexample":cache_counterexample()})).unwrap());
}
