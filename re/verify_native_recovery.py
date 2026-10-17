#!/usr/bin/env python3
"""Check the evidence gates for this scoped recovery port, without desktop input.

Gate receipts name commands, exit status, source hashes and retained logs. They
are written by the verifier runner after each check, not inferred from file names.
"""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AUDIT = ROOT / "study/audit/native-refetch-2026-10-06"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    gates = {}
    path = AUDIT / "property-map.json"
    if path.exists():
        p = json.loads(path.read_text())
        expected = {"role", "subrole", "roleDescription", "title", "description", "value",
                    "valueDescription", "placeholderValue", "help", "identifier", "url"}
        found = {call.split("UIElementProtocol.", 1)[1].split(".getter", 1)[0]
                 for slot in p["slots"] for call in slot["named_calls"]
                 if "UIElementProtocol." in call and ".getter" in call}
        gates["recovered_property_map"] = expected == found and all(slot["assembly"] for slot in p["slots"])
    else:
        gates["recovered_property_map"] = False
    for name in ("shared_recovery_contract", "native_action_integration", "isolated_live_outcomes", "regression_and_patch_guards"):
        receipt = AUDIT / (name + ".json")
        ok = False
        if receipt.exists():
            r = json.loads(receipt.read_text())
            ok = r.get("exit_code") == 0 and bool(r.get("command")) and bool(r.get("source_sha256"))
            ok = ok and all((ROOT / path).is_file() and sha(ROOT / path) == digest for path, digest in r["source_sha256"].items())
            ok = ok and (AUDIT / r["log"]).is_file() and sha(AUDIT / r["log"]) == r["log_sha256"]
        gates[name] = ok
    print(json.dumps({"remaining": sum(not v for v in gates.values()), "gates": gates}, indent=2))


if __name__ == "__main__":
    main()
