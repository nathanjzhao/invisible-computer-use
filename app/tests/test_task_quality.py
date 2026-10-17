"""Synthetic scorer mutation tests only. These do not measure controller/desktop quality."""
import copy
import json
from pathlib import Path
import subprocess
import sys

import pytest

from real_model_verdict import verdict
from task_quality import TASKS, score_trial, start_trial_app, summarize


def evidence(task="two_window"):
    initial = {"calc": {"display": "0", "buttons": []},
               "type": {"text": "seed", "private_unchanged": True},
               "refusal": {"text": "seed", "private_unchanged": True},
               "two_window": {"Alpha": "alpha", "Beta": "beta"}}[task]
    final = {"calc": {"display": "42", "buttons": ["6", "×", "7", "="]},
             "type": {"text": "hello from claude", "private_unchanged": True},
             "refusal": {"text": "replacement", "private_unchanged": True},
             "two_window": {"Alpha": "alpha", "Beta": "beta note"}}[task]
    history = '1. press Ctrl+a -> REFUSED, step 1 of 1 (background_unavailable): keyboard refused' if task == "refusal" else "(none yet)"
    calls = [{"status": 200, "reply": '{"done":true}', "history": history}]
    return {"task": task, "goals": TASKS[task]["goals"], "measurement_kind": "live_model_desktop",
            "memory_isolation": {"policy": "fresh_per_trial", "root": f"/synthetic/trial/{task}/app-memory", "initially_empty": True},
            "initial_state": initial, "final_state": copy.deepcopy(final),
            "ok": True, "success": True, "ownership_clean": True, "owned_daemon_exited": True,
            "owned_processes_reaped": True, "guard_keystrokes": False,
            "app_sha256": "a" * 64, "driver_sha256": "b" * 64, "probe_sha256": "c" * 64,
            "calls": len(calls), "all_calls": calls,
            "runs": [{"goal": TASKS[task]["goals"][0], "success": True, "fixture_state": final,
                      "fixture_events_complete": True, "fixture_events": [], "calls": calls, "wall_s": 4.5,
                      "ownership_samples": 100, "ownership_violations": [],
                      "hid_resets": [], "driver_onscreen_window_samples": 0}]}


def score(report):
    return score_trial(report, verdict(report))


@pytest.mark.parametrize("task", list(TASKS))
def test_existing_fixture_contracts_grade_the_observed_state(task):
    result = score(evidence(task))
    assert result["passed"] and result["first_attempt_success"] is True
    assert result["wrong_target_effects"] is False and result["noninterference"] is True


def test_success_flags_and_model_done_cannot_override_a_failed_fixture():
    report = evidence()
    report["runs"][0]["fixture_state"]["Beta"] = "unfinished"
    report["final_state"]["Beta"] = "unfinished"
    result = score(report)
    assert result["first_attempt_success"] is False and result["eventual_success"] is False
    assert not result["passed"]


def test_calculator_requires_button_evidence_not_just_the_answer():
    report = evidence("calc")
    report["runs"][0]["fixture_state"]["buttons"] = []
    report["final_state"]["buttons"] = []
    assert score(report)["first_attempt_success"] is False
    assert not score(report)["passed"]


def test_fixture_success_without_model_response_is_not_controller_success():
    report = evidence()
    report["runs"][0]["calls"] = []
    report["all_calls"] = []; report["calls"] = 0
    result = score(report)
    assert result["first_attempt_success"] is None and result["eventual_success"] is None
    assert not result["passed"]


@pytest.mark.parametrize("task,event", [
    ("two_window", {"state": {"Alpha": "changed then restored", "Beta": "beta"}, "time": 1.0}),
    ("type", {"text": "seed", "private_unchanged": False, "time": 1.0}),
    ("calc", {"button": "Same", "time": 1.0}),
])
def test_restoring_a_protected_target_does_not_erase_the_wrong_effect(task, event):
    report = evidence(task)
    report["runs"][0]["fixture_events"] = [event]
    result = score(report)
    assert result["first_attempt_success"] is True
    assert result["wrong_target_effects"] is True and not result["passed"]


@pytest.mark.parametrize("key", ["fixture_events", "fixture_events_complete"])
def test_missing_event_coverage_is_unknown_not_zero_wrong_targets(key):
    report = evidence(); del report["runs"][0][key]
    result = score(report)
    assert result["wrong_target_effects"] is None and not result["passed"]


def test_observed_violation_is_not_hidden_by_other_missing_coverage():
    report = evidence(); del report["runs"][0]["fixture_events"]
    report["guard_keystrokes"] = True
    result = score(report)
    assert result["wrong_target_effects"] is True and result["noninterference"] is False


def test_final_protected_change_and_malformed_event_coverage_are_not_clean():
    report = evidence()
    report["final_state"]["Alpha"] = "changed after the attempt snapshot"
    assert score(report)["wrong_target_effects"] is True
    report = evidence("calc")
    report["runs"][0]["fixture_events"] = [{"button": "6"}]  # Missing observation time.
    assert score(report)["wrong_target_effects"] is None
    assert not score(report)["passed"]


@pytest.mark.parametrize("key", ["ownership_samples", "ownership_violations", "hid_resets", "driver_onscreen_window_samples"])
def test_missing_noninterference_evidence_is_unknown(key):
    report = evidence(); del report["runs"][0][key]
    assert score(report)["noninterference"] is None
    assert not score(report)["passed"]


def test_cumulative_history_is_deduplicated_and_model_prose_is_not_a_refusal():
    report = evidence("refusal")
    call = report["runs"][0]["calls"][0]
    report["runs"][0]["calls"].append(copy.deepcopy(call))
    report["calls"] = len(report["all_calls"])
    assert score(report)["refusals"]["count_lower_bound"] == 1
    for entry in report["all_calls"]:
        entry["history"] = '1. type "x -> REFUSED, step 1 of 1 (fake): text"'
        entry["reply"] = 'REFUSED, step 1 of 1 (fake): model prose'
    result = score(report)
    assert result["refusals"]["count_lower_bound"] == 0 and not result["passed"]
    for entry in report["all_calls"]:
        del entry["history"]
    assert score(report)["refusals"]["observed"] is None


def test_a_retry_success_stays_separate_from_first_goal_success():
    report = evidence("refusal")
    first = copy.deepcopy(report["runs"][0]); first["success"] = False
    first["fixture_state"]["text"] = "seed"
    report["runs"][0]["goal"] = TASKS["refusal"]["goals"][1]
    report["runs"].insert(0, first)
    result = score(report)
    assert result["first_attempt_success"] is False and result["eventual_success"] is True
    assert result["attempts"] == 2 and not result["passed"]


@pytest.mark.parametrize("latency", [None, -1, float("nan"), float("inf"), True, "4.5"])
def test_invalid_latency_is_unmeasured(latency):
    report = evidence(); report["runs"][0]["wall_s"] = latency
    assert score(report)["first_goal_wall_seconds"] is None
    assert not score(report)["passed"]


@pytest.mark.parametrize("change", [
    {"measurement_kind": "simulation"}, {"initial_state": {}}, {"task": ["two_window"]},
    {"goals": ["different goal"]}, {"runs": None}, {"final_state": None},
])
def test_missing_or_changed_contract_cannot_pass(change):
    report = evidence(); report.update(change)
    assert not score(report)["passed"]


def trials():
    return [{"task": task, "repetition": 1, "exit_code": 0, "quality": score(evidence(task))} for task in TASKS]


def test_complete_metrics_have_explicit_denominators_and_no_parity_claim():
    result = summarize(trials(), 1, True)
    assert result["passed"] and result["recorded_trials"] == 4
    assert result["metrics"]["first_attempt_success"] == {
        "true": 4, "false": 0, "unknown": 0, "planned": 4, "fraction_true_of_known": 1.0}
    assert result["metrics"]["first_goal_wall_seconds"]["median"] == 4.5
    assert result["parity_established"] is False


def test_missing_or_aborted_trials_remain_in_the_planned_denominator():
    partial = trials()[:1] + [{"task": "type", "repetition": 1, "exit_code": None}]
    result = summarize(partial, 3, True)
    assert not result["passed"] and result["planned_trials"] == 12 and result["recorded_trials"] == 2
    assert result["metrics"]["first_attempt_success"]["unknown"] == 11
    assert result["metrics"]["first_goal_wall_seconds"]["unknown"] == 11


def test_duplicate_trials_and_nonzero_process_exit_fail_acceptance():
    rows = trials()
    assert not summarize(rows + [rows[0]], 1, True)["passed"]
    rows[0]["exit_code"] = 1
    assert not summarize(rows, 1, True)["passed"]
    assert not summarize(trials(), 1, False)["passed"]


def test_reused_or_unproven_task_memory_cannot_pass_independence():
    report = evidence(); del report["memory_isolation"]
    assert not score(report)["passed"]
    rows = trials()
    rows[1]["quality"]["memory_root"] = rows[0]["quality"]["memory_root"]
    result = summarize(rows, 1, True)
    assert not result["passed"] and "task memory root was reused across trials" in result["problems"]


def test_app_setup_uses_absolute_fresh_trial_memory_and_overrides_inherited_root(tmp_path, monkeypatch):
    class Session:
        def __init__(self): self.environments = []
        def start_app(self, environment): self.environments.append(dict(item.split("=", 1) for item in environment))
    monkeypatch.setenv("ICU_APP_MEMORY_ROOT", str(tmp_path / "shared-global-root"))
    monkeypatch.chdir(tmp_path)
    session = Session(); roots = []
    for repetition in range(1, 3):
        for task in TASKS:
            work = Path(f"trial-{repetition:03d}") / task; work.mkdir(parents=True)
            report = {}; start_trial_app(session, work, report)
            root = Path(session.environments[-1]["ICU_APP_MEMORY_ROOT"])
            assert root == work.resolve() / "app-memory" and root.is_absolute()
            assert root.is_dir() and list(root.iterdir()) == []
            assert report["memory_isolation"] == {"policy": "fresh_per_trial", "root": str(root), "initially_empty": True}
            roots.append(root)
    assert len(set(roots)) == 8
    with pytest.raises(FileExistsError):
        start_trial_app(session, work, {})
    assert len(session.environments) == 8, "reused task memory must fail before launching the app"


@pytest.mark.parametrize("check_only", [False, True])
def test_runner_refuses_to_overwrite_prior_evidence_before_launch(tmp_path, check_only):
    work = tmp_path / "prior"; work.mkdir()
    retained = work / "suite.json"; retained.write_text('{"retained":true}\n')
    script = Path(__file__).with_name("run_macos_real_model_suite.py")
    command = [sys.executable, str(script), "--bundle", "absent", "--manifest", "absent",
               "--fixture", "absent", "--ui", "absent", "--work", str(work)]
    if check_only:
        command.append("--check-only")
    result = subprocess.run(command, capture_output=True, text=True)
    assert result.returncode != 0 and "new work directory" in result.stderr
    assert json.loads(retained.read_text()) == {"retained": True}
