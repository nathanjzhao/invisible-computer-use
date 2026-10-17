"""Require a successful model turn in addition to the independent outcome oracle."""
import json


def candidate_completed(calls, chat):
    if not calls or any(call.get("status") != 200 for call in calls):
        return False
    try:
        reply = json.loads(calls[-1]["response"]["message"]["content"])
        actions = reply.get("actions") or [reply.get("action")]
        done = isinstance(actions, list) and bool(actions) and actions[-1].get("kind") == "done"
    except (ValueError, KeyError, TypeError, AttributeError):
        return False
    return done and any(isinstance(line, str) and line.startswith("done.") for line in chat)
