#!/usr/bin/env python3
"""Run the packaged-app real-model acceptance suite on an owned disposable Mac.

Requires ICU_DISPOSABLE_HOST=1 and ICU_TEST_MODEL_URL pointing to the authenticated
model relay. Start under the console user's GUI bootstrap context. This script
does not provision a host, grant OS permissions, or install account credentials.
"""
import argparse
import hashlib
import json
import os
import signal
from pathlib import Path
import subprocess
import sys
import time

from macos_probe import (ProbeInterrupted, cleanup_signals, defer_interruptions, install_interrupt_handlers, restore_signal_handlers, write_report)
from real_model_verdict import verdict
from task_quality import TASKS, score_trial, summarize


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run_probe(command, log, row, timeout=960, cleanup_seconds=90):
    """Allow the exact child to finalize before escalating an interruption."""
    process = None
    try:
        # A terminal interrupt reaches the supervisor; this isolated child gets
        # one explicit termination request and retains its own cleanup window.
        with defer_interruptions():
            process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            row['probe_pid'] = process.pid
        row['exit_code'] = process.wait(timeout)
    except (ProbeInterrupted, KeyboardInterrupt, subprocess.TimeoutExpired) as error:
        row['interruption'] = {'type': type(error).__name__, 'message': str(error)}
        if isinstance(error, ProbeInterrupted): row['interruption']['signal'] = signal.Signals(error.signum).name
        with cleanup_signals():
            if process is not None:
                if process.poll() is None: process.terminate()
                try: process.wait(cleanup_seconds)
                except subprocess.TimeoutExpired:
                    row['probe_forced'] = True
                    process.kill(); process.wait(5)
                row['exit_code'] = process.returncode
                row['probe_reaped'] = process.poll() is not None
    return row.get('exit_code')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--ui", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--model-label", help="Required for live runs: operator's actual relay model/version label")
    parser.add_argument("--repetitions", type=int, choices=range(1, 11), default=1,
                        help="Independent fresh trials per task (default: 1; maximum: 10)")
    parser.add_argument("--check-only", action="store_true", help="Check artifacts without launching a GUI or model")
    args = parser.parse_args()
    if args.work.exists() and not args.check_only:
        parser.error("live measurements require a new work directory; retain prior attempts")
    retained = args.work / "suite.json"
    if args.check_only and retained.exists():
        try:
            prior = json.loads(retained.read_text())
            prepared_only = isinstance(prior, dict) and prior.get("executed") is False and prior.get("tasks") == []
        except (OSError, ValueError):
            prepared_only = False
        if not prepared_only:
            parser.error("check-only cannot overwrite prior measurement evidence; use a new work directory")
    report = {"schema": 2, "passed": False, "executed": False, "tasks": [],
              "required_tasks": list(TASKS), "repetitions": args.repetitions,
              "model_label": args.model_label, "model_label_source": "operator supplied, not independently verified"}
    args.work.mkdir(parents=True, exist_ok=True)
    probe = Path(__file__).with_name("probe_macos_real_model.py")
    previous_signals = install_interrupt_handlers()
    try:
        expected = json.loads(args.manifest.read_text())
        for key, path in {
            "app_sha256": args.bundle / "Contents/MacOS/app",
            "driver_sha256": args.bundle / "Contents/MacOS/cua-driver",
            "probe_sha256": probe,
            "macos_probe_sha256": Path(__file__).with_name("macos_probe.py"),
            "verdict_sha256": Path(__file__).with_name("real_model_verdict.py"),
            "task_quality_sha256": Path(__file__).with_name("task_quality.py"),
            "suite_sha256": Path(__file__),
            "fixture_sha256": args.fixture,
            "ui_sha256": args.ui,
        }.items():
            if digest(path) != expected.get(key):
                raise ValueError(f"{key} differs from the candidate manifest")
        for executable in (args.fixture, args.ui):
            if not executable.is_file() or not os.access(executable, os.X_OK):
                raise ValueError(f"fixture executable unavailable: {executable}")
        report["candidate"] = {key: value for key, value in expected.items() if key.endswith("_sha256") and isinstance(value, str)}
        if args.check_only:
            report["status"] = "prepared_not_executed"
            return 0
        if sys.platform != "darwin" or os.environ.get("ICU_DISPOSABLE_HOST") != "1":
            raise ValueError("live acceptance requires an explicitly disposable macOS host")
        if not os.environ.get("ICU_TEST_MODEL_URL"):
            raise ValueError("ICU_TEST_MODEL_URL must identify the real-model relay")
        if not args.model_label or not args.model_label.strip():
            raise ValueError("--model-label must record the relay's actual model/version")
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(args.bundle)], check=True, capture_output=True, text=True)
        report["executed"] = True
        for repetition in range(1, args.repetitions + 1):
            for task in TASKS:
                parent = args.work if args.repetitions == 1 else args.work / f"trial-{repetition:03d}"
                work = parent / task
                work.mkdir(parents=True, exist_ok=False)  # Never reuse prior evidence as a new run.
                row = {"task": task, "repetition": repetition, "exit_code": None,
                       "result_path": str((work / "result.json").relative_to(args.work))}
                report["tasks"].append(row)  # Keep failed/unfinished submissions in the denominator.
                start = time.monotonic()
                try:
                    with (work / "probe.log").open("w") as log:
                        exit_code = run_probe([sys.executable, str(probe), str(work), str(args.bundle.resolve()),
                                               str(args.fixture.resolve()), str(args.ui.resolve()), task], log, row)
                finally:
                    row["wall_seconds"] = round(time.monotonic() - start, 2)
                if row.get('interruption'): report['interruption'] = row['interruption']
                result_path = work / "result.json"
                result = json.loads(result_path.read_text()) if result_path.exists() else {}
                if not isinstance(result, dict):
                    raise ValueError(f"{task}: malformed probe report")
                row["result_sha256"] = digest(result_path) if result_path.exists() else None
                if result.get("task") != task:
                    raise ValueError(f"{task}: retained report belongs to a different or missing task")
                acceptance = verdict(result, expected)
                acceptance["passed"] = acceptance["passed"] and exit_code == 0 and not row.get("interruption")
                row.update(acceptance=acceptance, quality=score_trial(result, acceptance))
                if row.get('interruption'):
                    report['interruption'] = row['interruption']
                    raise RuntimeError(f"{task}: probe interrupted; remaining tasks not run")
                # Missing cleanup proof means the desktop is no longer an independent baseline.
                if result.get("owned_daemon_exited") is not True or result.get("owned_processes_reaped") is not True:
                    raise RuntimeError(f"{task}: cleanup was not verified; remaining tasks not run")
        report["passed"] = summarize(report["tasks"], args.repetitions, True)["passed"]
        report["status"] = "passed" if report["passed"] else "failed"
        return 0 if report["passed"] else 1
    except (OSError, ValueError, TypeError, AttributeError, RuntimeError, subprocess.SubprocessError, ProbeInterrupted, KeyboardInterrupt) as error:
        if isinstance(error, (ProbeInterrupted, KeyboardInterrupt)):
            report['interruption'] = {'type': type(error).__name__, 'message': str(error)}
        report.update(status="failed" if report["executed"] else "blocked", error=str(error))
        return 1
    finally:
        with cleanup_signals():
            report["benchmark"] = summarize(report["tasks"], args.repetitions, report["executed"])
            write_report(args.work / "suite.json", report)
            print(json.dumps(report, indent=2))
        restore_signal_handlers(previous_signals)


if __name__ == "__main__":
    raise SystemExit(main())
