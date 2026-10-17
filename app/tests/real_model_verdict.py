"""Acceptance for the installed-app real-model probe, separate from harness completion.

This validates retained observations; it does not prove that a server is a model
or replace running the GUI/driver/fixture on an isolated desktop.
"""
import argparse
import json
from pathlib import Path
import re


def verdict(report, expected_build=None):
    failures = []
    for key in ("ok", "success", "ownership_clean", "owned_daemon_exited", "owned_processes_reaped"):
        if report.get(key) is not True:
            failures.append(f"{key} was not true")
    runs = report.get("runs")
    if not isinstance(runs, list) or not runs:
        runs = []
        failures.append("no completed task attempts")
    first_success = bool(runs) and runs[0].get("success") is True
    if not first_success or len(runs) != 1:
        failures.append("task did not succeed on its first submitted goal")
    for index, run in enumerate(runs, 1):
        samples = run.get("ownership_samples")
        if type(samples) is not int or samples < 20:
            failures.append(f"attempt {index}: insufficient ownership samples")
        for key in ("ownership_violations", "hid_resets"):
            if run.get(key) != []:
                failures.append(f"attempt {index}: {key} was not empty")
        if run.get("driver_onscreen_window_samples") != 0:
            failures.append(f"attempt {index}: driver overlay was visible or not measured")
    if report.get("guard_keystrokes") is not False:
        failures.append("foreground guard received input or was not checked")
    calls = report.get("all_calls")
    if not isinstance(calls, list) or not calls:
        calls = []
        failures.append("no model responses recorded")
    if type(report.get("calls")) is not int or report["calls"] != len(calls):
        failures.append("model call count does not match retained responses")
    for index, call in enumerate(calls, 1):
        if call.get("status") != 200 or not isinstance(call.get("reply"), str) or not call["reply"].strip():
            failures.append(f"model call {index}: successful response content is missing")
    if report.get("task") == "refusal" and not any("REFUSED" in call.get("history", "") for call in calls):
        failures.append("refusal recovery did not exercise a real refused action")
    for key in ("app_sha256", "driver_sha256", "probe_sha256"):
        if not isinstance(report.get(key), str) or not re.fullmatch(r"[0-9a-f]{64}", report[key]):
            failures.append(f"{key} is missing or malformed")
    if expected_build is not None:
        for key in ("app_sha256", "driver_sha256", "probe_sha256"):
            if not expected_build.get(key) or report.get(key) != expected_build[key]:
                failures.append(f"{key} does not match the candidate build")
    return {"passed": not failures, "failures": failures, "first_attempt_success": first_success,
            "eventual_success": report.get("success") is True, "attempts": len(runs), "model_calls": len(calls),
            "candidate_identity_checked": expected_build is not None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--build-manifest", type=Path)
    args = parser.parse_args()
    try:
        report = json.loads(args.report.read_text())
        expected = json.loads(args.build_manifest.read_text()) if args.build_manifest else None
        result = verdict(report, expected)
    except (OSError, ValueError, TypeError, AttributeError) as error:
        result = {"passed": False, "failures": [f"unreadable or malformed evidence: {error}"]}
    print(json.dumps(result, indent=2))
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
