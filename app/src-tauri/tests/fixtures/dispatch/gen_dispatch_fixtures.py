#!/usr/bin/env python3
"""Writes inbox_response.json from dispatch_spec_v1.json and prints the spec digest.
Run with --check to verify the committed inbox_response.json without writing."""
import hashlib
import json
import pathlib
import sys

here = pathlib.Path(__file__).resolve().parent
with open(here / "dispatch_spec_v1.json", "r", encoding="utf-8") as f:
    spec = json.load(f)

# Wire form: compact, key order exactly as in dispatch_spec_v1.json, ASCII only.
spec_json = json.dumps(spec, separators=(",", ":"), ensure_ascii=True)
digest = hashlib.sha256(spec_json.encode("utf-8")).hexdigest()

response = {
    "items": [
        {
            "dispatch_id": spec["dispatch_id"],
            "spec_json": spec_json,
            "spec_sha256": digest,
            "expires_in_s": 259200,
        }
    ],
    "poll_after_s": 30,
}
text = json.dumps(response, indent=2, ensure_ascii=True) + "\n"
target = here / "inbox_response.json"

if "--check" in sys.argv:
    current = None
    if target.exists():
        with open(target, "r", encoding="utf-8", newline="") as f:
            current = f.read()
    ok = current == text
    print(("ok " + digest) if ok else "MISMATCH: regenerate with no arguments")
    sys.exit(0 if ok else 1)

with open(target, "w", encoding="utf-8", newline="\n") as f:
    f.write(text)
print(digest)
