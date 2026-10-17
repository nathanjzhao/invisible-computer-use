# Native semantic target recovery

The macOS driver revalidates element-addressed input against the properties seen
in the observation. If an application replaces a control, the driver can recover
the replacement without requiring the model to choose a new element token.

The comparison and recovery rules come from a decompile of Sky 0.7.1's native
service, with [binary-specific code and property evidence](../../study/audit/native-refetch-2026-10-06/README.md).
They are implemented in the shared [matcher](../axdiff/src/recovery.rs) and a
macOS adapter in `platform-macos/src/ax/recovery.rs` inside the tracked native
patch. Windows and Linux keep their existing targeting behavior.

## Behavior

- Revalidation compares role, subrole, role description, title, description,
  value, value description, placeholder, help, identifier and URL.
- A still-matching native object must remain in the requested window.
- Recovery by searching a new tree requires exactly one matching candidate in
  the original tree and exactly one in the new tree. Missing and ambiguous
  matches are refused before input.
- A successful `set_value` refreshes the target while allowing its intended
  value change. Other compared properties still have to match. If refresh fails
  after the write, the result reports `action_performed: true` and
  `target_refresh_required`; callers must observe again rather than replay it.
- Snapshot generations and anchor revisions prevent an in-flight recovery from
  replacing a newer observation or anchor.
- Existing token validity, exact-window, app approval and input-delivery checks
  remain in effect. A new observation still invalidates old snapshot tokens.

The native observation includes `target_recovery` with the strategy, candidate
count, completeness and budget. Incomplete snapshots cannot establish a unique
replacement. A missing anchor, unreadable attributes, changed semantics or an
out-of-window candidate produces `stale_element_target` with a specific `reason`.
Secure-field replacements require a fresh observation.

AppKit text areas can report `AXErrorFailure` for an unset optional attribute,
including `AXIdentifier`. Such a read is tracked as unknown, not as an absent
value. The original retained handle can still be used when its role and value
remain readable, its readable properties and unknown-field mask still match,
and it remains in the requested window. An intended write may change its value.
Any unknown property makes the tree incomplete and prevents replacement search.
Losing a previously readable property, gaining a previously unknown property,
or changing another readable property requires a fresh observation.

## Differences from Sky

Sky uses a notification-driven invalidation monitor. This adapter performs
bounded revalidation on each addressed action, so correctness does not depend on
an accessibility notification arriving on time. This adds observation and input
work; no latency-equivalence claim is made.

Recovery walks only the requested window's AX children, with limits of 2,000
nodes, depth 40 and a two-second walk budget. Individual AX calls have a 150 ms
messaging timeout. These are local limits, not recovered Sky constants. A slow
in-progress OS call can finish after the walk budget; the budget is checked
between calls. Matching retains exact optional values and distinguishes string,
numeric, Boolean and URL representations. These conversions are conservative and
are not claimed to reproduce every Swift formatting or URL normalization rule.

The additional snapshot is checked against the rendered observation's role,
title, description, displayed value and identifier before it becomes an anchor.
If those changed during observation, the caller must observe again. As with the
existing native APIs, UI state can still change after the last check and before
the application handles an action.

## Verification

The owned [fixture](macos-drive/recovery-fixture.swift) is controlled through a
file channel for setup. The [probe](macos-drive/recovery-probe.py) sends tested
actions through the real driver socket. Its oracle is the fixture's own button
handler and text value, not the driver's success label.

Run it only on a disposable Mac with a logged-in GUI session and the necessary
accessibility grants. `ICU_DISPOSABLE_HOST=1` is a deliberate harness guard. On an
SSH-created Namespace Mac, run the probe under `launchctl asuser` so the driver
has access to GUI services such as the pasteboard. The probe is not a test to run
on an actively used personal desktop.

Cases cover unique replacement, ambiguity before and after refresh, missing or
changed controls, sibling-window decoys, incomplete trees, and two writes through
one token. Text-area cases cover repeated writes with an unset identifier and
refusal when that partially observed control is replaced. Pure tests cover every comparison property, value-change tolerance,
absent versus unreadable attributes, representation collisions, retained native
handles and concurrent snapshot/anchor replacement. The audit records exact
binary hashes and live results.
