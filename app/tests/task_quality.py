"""Task oracles and versioned scoring for the existing disposable-Mac model suite.

Scoring retained evidence is not a live run or a reference-product comparison.
None means unmeasured, incomplete, or malformed evidence, never a clean outcome.
"""
import math
from pathlib import Path
import re
import statistics


# These are the existing probe's goals and minicalc_mac.swift state contracts.
TASKS = {
    "calc": dict(mode="calc", window="ICU Calculator", state="fixture-state.json",
                 goals=["Compute six times seven using the calculator buttons."]),
    "type": dict(mode="receiver", window="ICU Manual Receiver", state="receiver-state.json",
                 goals=["Replace the text in Draft with: hello from claude"]),
    "two_window": dict(mode="scope", window="ICU Scope Beta", state="receiver-state.json",
                       goals=["Replace the text of the Beta field with: beta note. Do not change the Alpha document."]),
    "refusal": dict(mode="receiver", window="ICU Manual Receiver", state="receiver-state.json",
                    goals=["Select all text in Draft with Ctrl+a and replace it with: replacement",
                           "Replace the text in Draft with: replacement. Keyboard shortcuts were refused in this window last time."]),
}


def start_trial_app(session, work, report):
    """Launch with fresh task memory, overriding any inherited/default app root.

    Keep this root for user retries within the trial. A different task or
    repetition gets a different fresh directory; never silently reuse one.
    """
    memory_root = Path(work).resolve() / "app-memory"
    memory_root.mkdir(mode=0o700, exist_ok=False)
    report["memory_isolation"] = {"policy": "fresh_per_trial", "root": str(memory_root), "initially_empty": True}
    return session.start_app(["ICU_BRAIN=ollama", "ICU_OLLAMA_MODEL=relay-to-claude-code",
                              f"ICU_APP_MEMORY_ROOT={memory_root}"])


def task_success(task, state):
    """Read the fixture's state, including the requested button route for calculator."""
    if not isinstance(state, dict):
        return None
    if task == "calc":
        if not isinstance(state.get("display"), str) or not isinstance(state.get("buttons"), list):
            return None
        buttons = state["buttons"]
        if not all(isinstance(button, str) for button in buttons):
            return None
        return state["display"] == "42" and buttons[-4:] in (["6", "×", "7", "="], ["7", "×", "6", "="])
    if task in ("type", "refusal"):
        if not isinstance(state.get("text"), str) or type(state.get("private_unchanged")) is not bool:
            return None
        return state["text"] == ("hello from claude" if task == "type" else "replacement") and state["private_unchanged"]
    if task == "two_window":
        if not all(isinstance(state.get(key), str) for key in ("Alpha", "Beta")):
            return None
        return state["Alpha"] == "alpha" and state["Beta"] == "beta note"
    return None


def _initial_valid(task, state):
    if not isinstance(state, dict):
        return False
    if task == "calc":
        return state.get("display") == "0" and state.get("buttons") == []
    if task in ("type", "refusal"):
        return state.get("text") == "seed" and state.get("private_unchanged") is True
    return task == "two_window" and state == {"Alpha": "alpha", "Beta": "beta"}


def _any(values):
    values = list(values)
    return True if any(value is True for value in values) else (False if values and all(value is False for value in values) else None)


def _protected_change(task, state):
    if not isinstance(state, dict):
        return None
    if task == "calc":
        buttons = state.get("buttons")
        return "Same" in buttons if isinstance(buttons, list) and all(isinstance(button, str) for button in buttons) else None
    if task in ("type", "refusal"):
        value = state.get("private_unchanged")
        return not value if type(value) is bool else None
    if task == "two_window":
        return state["Alpha"] != "alpha" if isinstance(state.get("Alpha"), str) else None
    return None


def wrong_target_effects(task, runs, guard_keystrokes):
    """Protected fixture controls/windows and the foreground guard, including restored edits."""
    findings = [guard_keystrokes if type(guard_keystrokes) is bool else None]
    for run in runs:
        findings.append(_protected_change(task, run.get("fixture_state")))
        events = run.get("fixture_events")
        if run.get("fixture_events_complete") is not True or not isinstance(events, list):
            findings.append(None)
            continue
        for event in events:
            if not isinstance(event, dict) or _duration(event.get("time")) is None:
                findings.append(None)
            elif task == "calc":
                findings.append(event.get("button") == "Same" if isinstance(event.get("button"), str) else None)
            else:
                findings.append(_protected_change(task, event.get("state") if task == "two_window" else event))
    return _any(findings)


def noninterference(runs, guard_keystrokes):
    violations = [guard_keystrokes if type(guard_keystrokes) is bool else None]
    for run in runs:
        count = run.get("ownership_samples")
        violations.append(False if type(count) is int and count >= 20 else None)
        for key in ("ownership_violations", "hid_resets"):
            value = run.get(key)
            violations.append(bool(value) if isinstance(value, list) else None)
        overlays = run.get("driver_onscreen_window_samples")
        violations.append(overlays > 0 if type(overlays) is int and overlays >= 0 else None)
    if not runs:
        return None
    finding = _any(violations)
    return None if finding is None else not finding


REFUSAL = re.compile(r"^([1-9][0-9]*)\. (.+) -> REFUSED, step [1-9][0-9]* of [1-9][0-9]* \(([^()\n]+)\): .+$")


def _outside_quotes(text):
    quoted = escaped = False
    for char in text:
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == '"':
            quoted = not quoted
    return not quoted


def refusals(runs):
    """Count distinct numbered controller-history refusals, not repeated cumulative prompts.

    The last refusal may never reach a later model request, so this is a lower bound.
    Model prose saying REFUSED is not controller-history evidence.
    """
    events = []; histories = 0
    for attempt, run in enumerate(runs, 1):
        seen = {}
        for call in run.get("calls", []) if isinstance(run.get("calls"), list) else []:
            history = call.get("history") if isinstance(call, dict) else None
            if not isinstance(history, str):
                continue
            histories += 1
            for line in history.splitlines():
                match = REFUSAL.fullmatch(line)
                if match:
                    index, action, code = match.groups()
                    if not _outside_quotes(action):
                        continue  # A marker inside an action's quoted text is not a refusal.
                    if index in seen and seen[index] != code:
                        return {"observed": None, "count_lower_bound": None, "codes": [], "history_samples": histories}
                    seen[index] = code
        events.extend({"attempt": attempt, "index": int(index), "code": code} for index, code in seen.items())
    return {"observed": bool(events) if histories else None,
            "count_lower_bound": len(events) if histories else None,
            "codes": sorted({event["code"] for event in events}), "history_samples": histories}


def _duration(value):
    return value if type(value) in (int, float) and math.isfinite(value) and value >= 0 else None


def _has_model_response(run):
    calls = run.get("calls")
    return isinstance(calls, list) and any(isinstance(call, dict) and call.get("status") == 200
        and isinstance(call.get("reply"), str) and call["reply"].strip() for call in calls)


def score_trial(report, acceptance):
    """Separate findings from acceptance; caller supplies the existing strict model verdict."""
    problems = []
    task = report.get("task") if isinstance(report.get("task"), str) else None
    raw_runs = report.get("runs")
    runs = raw_runs if isinstance(raw_runs, list) and all(isinstance(run, dict) for run in raw_runs) else []
    live = report.get("measurement_kind") == "live_model_desktop"
    first = task_success(task, runs[0].get("fixture_state")) if live and runs and _has_model_response(runs[0]) else None
    eventual = task_success(task, report.get("final_state")) if live and any(_has_model_response(run) for run in runs) else None
    wrong = _any([wrong_target_effects(task, runs, report.get("guard_keystrokes")),
                  _protected_change(task, report.get("final_state"))]) if runs else None
    quiet = noninterference(runs, report.get("guard_keystrokes"))
    refused = refusals(runs)
    latency = _duration(runs[0].get("wall_s")) if runs else None
    isolation = report.get("memory_isolation")
    memory_root = isolation.get("root") if isinstance(isolation, dict) else None
    if not (isinstance(isolation, dict) and isolation.get("policy") == "fresh_per_trial"
            and isolation.get("initially_empty") is True and isinstance(memory_root, str)
            and Path(memory_root).is_absolute()):
        problems.append("fresh per-trial task memory is unproven")
        memory_root = None
    if not live:
        problems.append("live-model desktop provenance missing")
    if task not in TASKS or report.get("goals") != TASKS[task]["goals"]:
        problems.append("unknown task or changed goals")
    if not _initial_valid(task, report.get("initial_state")):
        problems.append("fresh fixture baseline missing or changed")
    if not runs or len(runs) > len(TASKS.get(task, {}).get("goals", [])):
        problems.append("missing or unexpected submitted goals")
    for index, run in enumerate(runs):
        goals = TASKS.get(task, {}).get("goals", [])
        if index >= len(goals) or run.get("goal") != goals[index]:
            problems.append(f"attempt {index + 1}: submitted goal differs")
        if run.get("success") is not task_success(task, run.get("fixture_state")):
            problems.append(f"attempt {index + 1}: summary contradicts fixture state")
    if first is not True or len(runs) != 1:
        problems.append("first submitted goal did not independently succeed")
    if eventual is not True or report.get("success") is not eventual:
        problems.append("final success missing or contradicts fixture state")
    if wrong is not False:
        problems.append("protected-target effects observed or unmeasured")
    if quiet is not True:
        problems.append("noninterference failed or unmeasured")
    if task == "refusal" and refused["observed"] is not True:
        problems.append("refusal recovery did not exercise a retained controller refusal")
    if latency is None:
        problems.append("goal latency unmeasured")
    return {"schema": 1, "passed": acceptance.get("passed") is True and not problems,
            "problems": problems, "first_attempt_success": first, "eventual_success": eventual,
            "attempts": len(runs), "wrong_target_effects": wrong, "noninterference": quiet,
            "refusals": refused, "first_goal_wall_seconds": latency,
            "memory_root": memory_root,
            "model_names_reported_by_relay": sorted({call["model"] for run in runs
                for call in (run.get("calls") if isinstance(run.get("calls"), list) else [])
                if isinstance(call, dict) and isinstance(call.get("model"), str) and call["model"].strip()})}


def _counts(values):
    values = list(values)
    yes = sum(value is True for value in values); no = sum(value is False for value in values)
    return {"true": yes, "false": no, "unknown": len(values) - yes - no, "planned": len(values),
            "fraction_true_of_known": yes / (yes + no) if yes + no else None}


def summarize(trials, repetitions, executed):
    """Account for every planned slot. Missing, duplicated and failed trials cannot disappear."""
    slots = {(task, repetition): None for repetition in range(1, repetitions + 1) for task in TASKS}
    problems = []
    for trial in trials:
        key = (trial.get("task"), trial.get("repetition"))
        if key not in slots or slots[key] is not None:
            problems.append(f"unexpected or duplicated trial: {key}")
            continue
        slots[key] = trial
    qualities = [(trial or {}).get("quality", {}) for trial in slots.values()]
    memory_roots = [quality.get("memory_root") for quality in qualities if quality.get("memory_root") is not None]
    if len(memory_roots) != len(set(memory_roots)):
        problems.append("task memory root was reused across trials")
    values = [_duration(quality.get("first_goal_wall_seconds")) for quality in qualities]
    durations = sorted(value for value in values if value is not None)
    complete = sum(trial is not None for trial in slots.values())
    if complete != len(slots):
        problems.append("required trials are missing")
    passed = executed is True and not problems and all(
        trial.get("exit_code") == 0 and trial.get("quality", {}).get("passed") is True
        for trial in slots.values() if trial is not None)
    return {"schema": 1, "kind": "controller_task_quality", "executed": executed is True,
            "passed": passed, "parity_established": False, "repetitions": repetitions,
            "required_tasks": list(TASKS), "planned_trials": len(slots), "recorded_trials": complete,
            "problems": problems,
            "metrics": {**{key: _counts(quality.get(key) for quality in qualities) for key in
                           ("first_attempt_success", "eventual_success", "wrong_target_effects", "noninterference")},
                        "refusal_case_exercised": _counts((trial or {}).get("quality", {}).get("refusals", {}).get("observed")
                                                         for (task, _), trial in slots.items() if task == "refusal"),
                        "first_goal_wall_seconds": {"measured": len(durations), "unknown": len(slots) - len(durations),
                            "median": statistics.median(durations) if durations else None,
                            "p95_nearest_rank": durations[math.ceil(.95 * len(durations)) - 1] if durations else None,
                            "max": max(durations) if durations else None}},
            "limits": ["Named AppKit fixture tasks only; no reference-product parity measurement.",
                       "Protected-target effects cover the fixture controls/windows and foreground guard, not the whole OS.",
                       "Ownership uses sampled focus/pointer/HID observations; sub-sample interference may be missed.",
                       "Refusals are a lower bound from retained controller histories, not a complete action count.",
                       "Goal latency includes foreground handoff and post-task model idle wait."]}
