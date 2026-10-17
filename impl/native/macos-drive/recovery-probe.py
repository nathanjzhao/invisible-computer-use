#!/usr/bin/env python3
"""Real target replacement tests. Requires an explicitly disposable macOS host.

Fixture changes are controlled through its own file channel. Input under test goes
through the actual driver socket and the fixture's action handler is the oracle.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--driver", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    args = parser.parse_args()
    if os.environ.get("ICU_DISPOSABLE_HOST") != "1":
        raise SystemExit("Set ICU_DISPOSABLE_HOST=1 only on an owned disposable test host")
    args.work.mkdir(parents=True, exist_ok=True)
    sock = args.work / "driver.sock"
    report = {"driver_sha256": hashlib.sha256(args.driver.read_bytes()).hexdigest(), "cases": []}
    logs, processes = [], []
    def launch(argv, name):
        stream = (args.work / (name + ".log")).open("wb"); logs.append(stream)
        process = subprocess.Popen(argv, stdout=stream, stderr=stream, stdin=subprocess.DEVNULL); processes.append(process)
        return process
    def wait(predicate, message, seconds=15):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            result = predicate()
            if result: return result
            time.sleep(.05)
        raise AssertionError(message)
    def state():
        try: return json.loads((args.work / "state.json").read_text())
        except (FileNotFoundError, json.JSONDecodeError): return {}
    counter = 0
    def command(mode):
        nonlocal counter
        counter += 1
        tmp = args.work / "command.tmp"
        tmp.write_text(json.dumps({"id": counter, "mode": mode})); tmp.replace(args.work / "command.json")
        wait(lambda: state().get("command") == counter, mode)
    def call(name, data):
        with socket.socket(socket.AF_UNIX) as client:
            client.settimeout(30); client.connect(str(sock))
            client.sendall((json.dumps({"method": "call", "name": name, "args": data,
                                      "session_id": "recovery-test", "client_kind": "cli"}) + "\n").encode())
            buf = b""
            while b"\n" not in buf:
                chunk = client.recv(1024 * 1024)
                if not chunk: raise AssertionError("driver disconnected")
                buf += chunk
            reply = json.loads(buf.split(b"\n", 1)[0])
            with (args.work / "rpc.jsonl").open("a") as stream:
                stream.write(json.dumps({"tool": name, "arguments": data, "response": reply}) + "\n")
            assert reply.get("ok"), reply
            return reply["result"]
    def payload(reply):
        if "structuredContent" in reply: return reply["structuredContent"]
        for part in reply.get("content", []):
            if part.get("type") == "text":
                try: return json.loads(part["text"])
                except json.JSONDecodeError: pass
        return reply
    try:
        launch([str(args.fixture), str(args.work)], "fixture")
        initial = wait(lambda: state() if state().get("window", 0) > 0 else None, "fixture window")
        env = dict(os.environ, CUA_DRIVER_EMBEDDED="1")
        stream = (args.work / "driver.log").open("wb"); logs.append(stream)
        driver = subprocess.Popen([str(args.driver), "serve", "--socket", str(sock), "--no-overlay", "--no-permissions-gate"],
                                  env=env, stdout=stream, stderr=stream, stdin=subprocess.DEVNULL); processes.append(driver)
        wait(sock.exists, "driver socket", 30)
        base = {"pid": initial["pid"], "window_id": initial["window"], "session": "recovery-test"}
        call("set_agent_cursor_enabled", {"enabled": False, "session": "recovery-test"})
        def observe(role="AXButton", label="Choose"):
            r = payload(call("get_window_state", {**base, "include_screenshot": False, "include_text": True}))
            candidates = [e for e in r.get("elements", []) if e.get("role") == role and label in (e.get("label", "") + e.get("title", "") + str(e.get("identifier", "")))]
            assert candidates, r
            return candidates[0]["element_token"]
        def click(token): return call("click", {**base, "element_token": token, "delivery_mode": "background"})
        for name, before, change, reason in [
            ("unique_replacement", "single", "replace", None),
            ("ambiguous_before", "twins", "remove_first", "ambiguous_before"),
            ("ambiguous_after", "single", "replace_twins", "ambiguous_after"),
            ("missing_after", "single", "remove_all", "missing_after"),
            ("changed_semantics", "single", "changed", "missing_after"),
            ("sibling_decoy", "single", "sibling_only", "missing_after"),
            ("incomplete_tree", "single", "oversized", "incomplete_tree"),
        ]:
            command(before); token = observe(); command(change)
            before_input = state()
            reply = click(token); data = payload(reply)
            if reason is None:
                assert not reply.get("isError"), reply
                wait(lambda: state().get("hits") == ["target"], name)
            else:
                assert reply.get("isError") and data.get("reason") == reason, (name, reply)
                assert state().get("hits") == [], (name, state())
            after_input = state()
            assert after_input.get("frontPID") == before_input.get("frontPID"), (name, "foreground changed", before_input, after_input)
            assert after_input.get("pointer") == before_input.get("pointer"), (name, "pointer moved", before_input, after_input)
            report["cases"].append({"name": name, "pass": True, "state": after_input, "result": data})
        command("field"); token = observe("AXTextField", "entry")
        for value in ("first write", "second write"):
            reply = call("set_value", {**base, "element_token": token, "value": value})
            assert not reply.get("isError") and not payload(reply).get("refresh_required"), reply
            wait(lambda: state().get("values") == [value], value)
        report["cases"].append({"name": "post_write_revalidation", "pass": True, "state": state()})
        command("text_area"); token = observe("AXTextArea", "Draft")
        for value in ("first text area write", "second text area write"):
            reply = call("set_value", {**base, "element_token": token, "value": value})
            assert not reply.get("isError") and not payload(reply).get("refresh_required"), reply
            wait(lambda: state().get("values") == [value], value)
        report["cases"].append({"name": "unset_identifier_same_handle", "pass": True, "state": state()})
        command("text_area"); token = observe("AXTextArea", "Draft"); command("text_area_replaced")
        reply = call("set_value", {**base, "element_token": token, "value": "must not reach replacement"})
        assert reply.get("isError") and payload(reply).get("reason") == "incomplete_tree", reply
        assert state().get("values") == ["seed"], state()
        report["cases"].append({"name": "unset_identifier_replacement_refused", "pass": True, "state": state(), "result": payload(reply)})
        subprocess.run(["/usr/sbin/screencapture", "-x", "-l", str(initial["window"]), str(args.work / "final-window.png")], check=True, timeout=15)
        report["pass"] = True
    except Exception as error:
        report.update({"pass": False, "error": repr(error)})
        raise
    finally:
        for p in reversed(processes):
            if p.poll() is None: p.terminate()
        for p in reversed(processes):
            try: p.wait(5)
            except subprocess.TimeoutExpired: p.kill(); p.wait()
        for stream in logs: stream.close()
        report["owned_processes_reaped"] = all(p.poll() is not None for p in processes)
        (args.work / "results.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report))


if __name__ == "__main__": main()
