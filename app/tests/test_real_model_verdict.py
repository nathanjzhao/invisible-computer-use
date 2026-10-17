"""Offline tests of acceptance decisions, not real-model or desktop evidence."""
import copy
import json
from pathlib import Path
import subprocess
import sys

import pytest
from real_model_verdict import verdict


def success():
    return {"ok": True, "success": True, "ownership_clean": True, "owned_daemon_exited": True,
            "owned_processes_reaped": True, "guard_keystrokes": False,
            "app_sha256": "a" * 64, "driver_sha256": "b" * 64, "probe_sha256": "c" * 64,
            "calls": 1, "all_calls": [{"status": 200, "reply": '{"done":true}'}],
            "runs": [{"success": True, "ownership_samples": 100, "ownership_violations": [],
                      "hid_resets": [], "driver_onscreen_window_samples": 0}]}


def test_success_requires_task_ownership_model_and_cleanup():
    report = success()
    assert verdict(report, report)["passed"]


@pytest.mark.parametrize("key", ["success", "ownership_clean", "owned_daemon_exited", "owned_processes_reaped", "ok"])
def test_harness_completion_does_not_hide_failed_acceptance(key):
    report = success(); report[key] = False
    assert not verdict(report)["passed"]


def test_a_user_retry_is_reported_separately_from_first_attempt_success():
    report = success(); first = copy.deepcopy(report["runs"][0]); first["success"] = False
    report["runs"].insert(0, first)
    result = verdict(report)
    assert not result["passed"] and not result["first_attempt_success"] and result["eventual_success"]


@pytest.mark.parametrize("changes", [{"ownership_samples": 0}, {"ownership_violations": ["focus"]},
                                      {"hid_resets": ["key"]}, {"driver_onscreen_window_samples": 1}])
def test_summary_booleans_do_not_override_raw_ownership_evidence(changes):
    report = success(); report["runs"][0].update(changes)
    assert not verdict(report)["passed"]


def test_missing_model_response_is_not_a_live_model_pass():
    report = success(); report["all_calls"] = []
    assert not verdict(report)["passed"]
    report = success(); report["all_calls"][0]["status"] = 502
    assert not verdict(report)["passed"]


def test_refusal_recovery_must_show_a_refusal_in_model_history():
    report = success(); report['task'] = 'refusal'
    assert not verdict(report)['passed']
    report['all_calls'][0]['history'] = '1. key Ctrl+a -> REFUSED (ambiguous_window)'
    assert verdict(report)['passed']


def test_stale_bundle_or_sidecar_cannot_satisfy_the_candidate_gate():
    expected = success(); report = success(); report["driver_sha256"] = "d" * 64
    assert not verdict(report, expected)["passed"]


def test_cli_exits_nonzero_for_ok_but_unsuccessful_task(tmp_path):
    report = success(); report["success"] = False
    path = tmp_path / "result.json"; path.write_text(json.dumps(report))
    result = subprocess.run([sys.executable, str(Path(__file__).with_name("real_model_verdict.py")), str(path)], capture_output=True, text=True)
    assert result.returncode == 1 and json.loads(result.stdout)["passed"] is False


@pytest.mark.parametrize("repetitions", [1, 3])
def test_prepare_only_cannot_be_mistaken_for_live_acceptance(tmp_path, repetitions):
    import hashlib
    tests = Path(__file__).parent
    bundle = tmp_path / "Copilot.app"; binaries = bundle / "Contents/MacOS"; binaries.mkdir(parents=True)
    (binaries / "app").write_bytes(b"app"); (binaries / "cua-driver").write_bytes(b"driver")
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    manifest = {"app_sha256": sha(binaries / "app"), "driver_sha256": sha(binaries / "cua-driver"),
                "probe_sha256": sha(tests / "probe_macos_real_model.py"), "verdict_sha256": sha(tests / "real_model_verdict.py"),
                "macos_probe_sha256": sha(tests / "macos_probe.py"),
                "task_quality_sha256": sha(tests / "task_quality.py"), "suite_sha256": sha(tests / "run_macos_real_model_suite.py"),
                "fixture_sha256": sha(Path(sys.executable)), "ui_sha256": sha(Path(sys.executable))}
    path = tmp_path / "manifest.json"; path.write_text(json.dumps(manifest))
    command = [sys.executable, str(tests / "run_macos_real_model_suite.py"), "--bundle", str(bundle),
               "--manifest", str(path), "--fixture", sys.executable, "--ui", sys.executable,
               "--work", str(tmp_path / "work"), "--repetitions", str(repetitions), "--check-only"]
    run = subprocess.run(command, capture_output=True, text=True)
    result = json.loads(run.stdout)
    assert run.returncode == 0 and result["status"] == "prepared_not_executed"
    assert result["executed"] is False and result["passed"] is False and result["tasks"] == []
    assert result["benchmark"]["executed"] is False and result["benchmark"]["passed"] is False
    assert result["benchmark"]["metrics"]["first_attempt_success"]["unknown"] == 4 * repetitions
    assert result["benchmark"]["metrics"]["first_goal_wall_seconds"]["median"] is None
    manifest["task_quality_sha256"] = "0" * 64
    path.write_text(json.dumps(manifest))
    stale = subprocess.run(command, capture_output=True, text=True)
    assert stale.returncode == 1 and "task_quality_sha256" in json.loads(stale.stdout)["error"]
    manifest["task_quality_sha256"] = sha(tests / "task_quality.py")
    path.write_text(json.dumps(manifest))
    (binaries / "cua-driver").write_bytes(b"stale driver")
    run = subprocess.run(command, capture_output=True, text=True)
    assert run.returncode == 1 and "driver_sha256" in json.loads(run.stdout)["error"]
