import json
from conformance.paired.recording.model_completion import candidate_completed


def reply(action):
    return {"status": 200, "response": {"message": {"content": json.dumps({"action": action})}}}


def test_native_outcome_followed_by_provider_failure_is_not_completion():
    calls = [reply({"kind": "click", "target": "Equals"}), {"status": 502, "response": {"error": "offline"}}]
    assert not candidate_completed(calls, ["error: brain: offline"])


def test_model_claim_without_app_completion_is_not_completion():
    assert not candidate_completed([reply({"kind": "done"})], ["error: stopped"])


def test_completed_turn_requires_both_model_and_app_evidence():
    assert candidate_completed([reply({"kind": "done"})], ["done. Result is 42."])
