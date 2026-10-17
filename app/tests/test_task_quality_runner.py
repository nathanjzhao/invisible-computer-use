"""Exercise runner bookkeeping with a fake subprocess boundary, never desktop/model calls."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace

import pytest

import run_macos_real_model_suite as runner
from test_task_quality import evidence


def prepared(tmp_path, monkeypatch, behavior):
    tests = Path(__file__).parent
    bundle = tmp_path / "candidate.app"; native = bundle / "Contents/MacOS"; native.mkdir(parents=True)
    (native / "app").write_bytes(b"synthetic app bytes")
    (native / "cua-driver").write_bytes(b"synthetic driver bytes")
    paths = {"app_sha256": native / "app", "driver_sha256": native / "cua-driver",
             "probe_sha256": tests / "probe_macos_real_model.py", "verdict_sha256": tests / "real_model_verdict.py",
             "macos_probe_sha256": tests / "macos_probe.py",
             "task_quality_sha256": tests / "task_quality.py", "suite_sha256": tests / "run_macos_real_model_suite.py",
             "fixture_sha256": Path(sys.executable), "ui_sha256": Path(sys.executable)}
    expected = {key: hashlib.sha256(path.read_bytes()).hexdigest() for key, path in paths.items()}
    manifest = tmp_path / "manifest.json"; manifest.write_text(json.dumps(expected))
    work = tmp_path / "run"
    monkeypatch.setattr(sys, "argv", [str(tests / "run_macos_real_model_suite.py"), "--bundle", str(bundle),
        "--manifest", str(manifest), "--fixture", sys.executable, "--ui", sys.executable,
        "--work", str(work), "--repetitions", "2", "--model-label", "synthetic-test-only"])
    monkeypatch.setattr(sys, "platform", "darwin")
    monkeypatch.setenv("ICU_DISPOSABLE_HOST", "1")
    monkeypatch.setenv("ICU_TEST_MODEL_URL", "http://unused.invalid")
    commands = []
    def fake_run(command, **kwargs):
        if command[0] == "codesign":
            return SimpleNamespace(returncode=0)
        assert Path(command[1]).name == "probe_macos_real_model.py", command
        commands.append(command)
        task = command[-1]; report = evidence(task)
        report["memory_isolation"]["root"] = str(Path(command[2]).resolve() / "app-memory")
        report.update({key: expected[key] for key in ("app_sha256", "driver_sha256", "probe_sha256")})
        code = behavior(len(commands), task, report)
        (Path(command[2]) / "result.json").write_text(json.dumps(report))
        return SimpleNamespace(returncode=code)
    monkeypatch.setattr(subprocess, "run", fake_run)
    def fake_probe(command, log, row, **kwargs):
        result = fake_run(command)
        row['exit_code'] = result.returncode
        return result.returncode
    monkeypatch.setattr(runner, "run_probe", fake_probe)
    return work, commands


def test_repeated_trials_use_distinct_fresh_paths_and_hash_their_evidence(tmp_path, monkeypatch):
    work, commands = prepared(tmp_path, monkeypatch, lambda *_: 0)
    assert runner.main() == 0
    report = json.loads((work / "suite.json").read_text())
    assert len(commands) == 8 and len({command[2] for command in commands}) == 8
    assert report["benchmark"]["passed"] and report["benchmark"]["planned_trials"] == 8
    for row in report["tasks"]:
        assert row["result_sha256"] == runner.digest(work / row["result_path"])
    assert report["benchmark"]["parity_established"] is False


def test_failed_goal_is_retained_and_other_independent_trials_still_run(tmp_path, monkeypatch):
    def fail_first(n, task, report):
        if n == 1:
            report["runs"][0]["fixture_state"]["display"] = "0"
            report["final_state"]["display"] = "0"
            report["runs"][0]["success"] = report["success"] = False
            return 1
        return 0
    work, commands = prepared(tmp_path, monkeypatch, fail_first)
    assert runner.main() == 1 and len(commands) == 8
    metrics = json.loads((work / "suite.json").read_text())["benchmark"]["metrics"]
    assert metrics["first_attempt_success"]["false"] == 1 and metrics["first_attempt_success"]["unknown"] == 0


@pytest.mark.parametrize("failure", ["cleanup", "timeout", "wrong_task"])
def test_unsafe_or_incomplete_baseline_stops_remaining_trials_without_hiding_them(tmp_path, monkeypatch, failure):
    def fail_first(n, task, report):
        if failure == "timeout":
            raise subprocess.TimeoutExpired("synthetic fixture", 960)
        if failure == "cleanup":
            report["owned_processes_reaped"] = False
        else:
            report["task"] = "type"
        return 1
    work, commands = prepared(tmp_path, monkeypatch, fail_first)
    assert runner.main() == 1 and len(commands) == 1
    report = json.loads((work / "suite.json").read_text())
    assert report["benchmark"]["recorded_trials"] == 1 and report["benchmark"]["planned_trials"] == 8
    assert report["benchmark"]["metrics"]["first_attempt_success"]["unknown"] >= 7
    assert not report["passed"] and report["status"] == "failed"


def test_interrupted_trial_stays_in_denominator_and_stops_cohort(tmp_path, monkeypatch):
    work, commands = prepared(tmp_path, monkeypatch, lambda *_: 0)
    def interrupted(command, log, row, **kwargs):
        commands.append(command)
        row.update(exit_code=-15, probe_reaped=True, interruption={"type":"ProbeInterrupted","signal":"SIGTERM"})
        (Path(command[2]) / "result.json").write_text(json.dumps({"task":command[-1],"ok":False,
            "owned_daemon_exited":False,"owned_processes_reaped":False}))
        return -15
    monkeypatch.setattr(runner, "run_probe", interrupted)
    assert runner.main() == 1 and len(commands) == 1
    report = json.loads((work / "suite.json").read_text())
    assert report["interruption"]["signal"] == "SIGTERM"
    assert report["tasks"][0]["result_sha256"]
    assert report["tasks"][0]["acceptance"]["passed"] is False
    assert report["benchmark"]["recorded_trials"] == 1
    assert report["benchmark"]["planned_trials"] == 8
    assert report["benchmark"]["metrics"]["first_attempt_success"]["unknown"] == 8
