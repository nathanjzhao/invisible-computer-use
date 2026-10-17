# Paired native and model-driven acceptance

These tests compare actual runtimes on an owned disposable desktop. They do not
infer parity from API names or a reimplementation of the reference. Keep each
attempt in a fresh directory, including failed attempts. Never run them on a
person's working desktop.

## Prepare the host

Install the official desktop app and verify its executable signatures. Launch it
once and enable its Computer Use plugin. Keep its generated native broker and
node runtime configuration intact, including the sandbox. Sign in separately on
each disposable host with the official CLI's device flow. Do not copy another
machine's authentication file.

On Windows the running desktop app supplies the native named-pipe broker.
Launching only `codex app-server` with a hand-written macOS-style MCP config can
fail with `spawn EPERM`. The native helper can pass scripted tests even when the
model runtime is missing this host. When unpacking an MSIX for a behavioral test,
restore percent-encoded package path names, including files such as
`%24_StatsigGlobal.js`, without altering their bytes. An installed official app is
preferable. Record the desktop package, CLI, Sky package, helper and harness hashes.

Build `fixture.swift` as `PairFixture.app` on macOS or `windows_fixture.cs` as
`PairFixture.exe` on Windows. Build `guard.swift` or `windows_guard.cs`; the latter
uses `app/tests/windows/NativeObserver.cs`. Use the same fixture binary for both
engines. Grant only the test apps the required OS permissions. Ad-hoc Mac rebuilds
can invalidate prior Screen Recording consent; renew it before acceptance.

## Native comparison

`run_native_suite.py` runs both engines sequentially, measures the foreground
guard, and grades independent fixture outcomes. It requires Python 3 and the
platform-specific prerequisites printed by `--help`.

```sh
ICU_DISPOSABLE_HOST=1 python3 conformance/paired/run_native_suite.py \
  --platform macos --work /tmp/paired-fresh \
  --fixture /path/to/PairFixture.app/Contents/MacOS/PairFixture \
  --candidate-driver /path/to/our/cua-driver \
  --reference-executable /path/to/SkyComputerUseService \
  --reference-cli /path/to/signed/codex \
  --guard /path/to/PairGuard --ui /path/to/macos_ui \
  --host-harness /path/to/host-harness
```

Add `--suite expanded` with the fixture sources in `expanded/` to run 12 cases.
The expanded catalog covers the three baseline cases plus checkbox toggle/reset,
radio selection, popup selection, Unicode and multiline text, clearing text,
nested scrolling, and a multi-step form workflow. Every case checks that the
unrelated Alpha document remains unchanged. For the Python-free Windows adapter,
append `expanded` after `with-ownership`; put `expanded/cases.json` in its root.

The macOS host harness supplies the signed app-server JSON-RPC client (`rpc.py`).
Windows instead needs `--reference-client` containing the reference package's
`helper_transport.js`, `codex_turn_metadata.js` and their `tslib.es6.mjs` dependency,
plus `--node`. The Python-free Windows adapter also runs directly:

```powershell
$env:ICU_DISPOSABLE_HOST='1'
node windows.mjs openai-native C:\paired C:\paired\reference with-ownership
node windows.mjs icu-native C:\paired C:\paired\candidate with-ownership
```

The Windows directory must contain `PairFixture.exe`, `PairGuard.exe`,
`codex-computer-use.exe`, `cua-driver.exe`, the `reference` client directory and a
`package.json` declaring `type: module`. Run in the interactive standard user's
session, not the service session. Put the signed reference CLI on PATH.

`--check-only` reports preparation only. A live pass requires executed cases,
matching fixture and display identities, observations, actions, expected native
state, adequate guard samples and process cleanup. Foreground/pointer/HID changes
are reported separately from task outcomes. Windows reference foreground use is
documented behavior, not background equivalence.

## Model comparison

Use `model_macos.py` or `model_windows.mjs` for real reference model turns.
The Windows invocation is:

```powershell
$env:ICU_DISPOSABLE_HOST='1'
node model_windows.mjs C:\ICUAudit\paired C:\ICUAudit\paired\model-reference gpt-6-astra
```

That layout expects the official resources at `official/unpacked/app/resources`,
the fixture at the root, and an existing `model-work` directory. Keep the signed
desktop host running. Only fixture app approval requests are accepted.

Run our actual GUI with `app/tests/probe_macos_product_pair.py` or
`app/tests/windows/product-pair.ps1` using the same fixture, model and task goals.
The Windows probe expects our executable and sidecars in `candidate`, and
`NativeObserver.cs` at the root. Its user setup uses native accessibility and
keyboard input; the app's real model and driver perform the task.

```sh
python3 conformance/paired/product_verdict.py REFERENCE_DIR CANDIDATE_DIR
python3 -m pytest conformance/paired -q
```

The verdict checks model identity, native tool routes, task goals, independent
fixture state, calculator button input and cleanup. Recovered model tool errors
remain counted. Matching task outcomes do not imply identical prompts, model
decisions, latency or full product parity.

After collecting evidence, log out on each host, verify the test credential file
is absent, stop owned processes and relays, then destroy the owned hosts and
their cloud resources. Keep raw account metadata private; retained audit copies
must redact it. The [2026-10-08 audit](../../study/audit/paired-memory-2026-10-08/README.md)
records tested versions, failed attempts and known limits.
