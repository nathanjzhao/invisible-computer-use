# Notification readiness experiment

Keep production polling and current-fingerprint validation. This experiment
does not justify adding a notification producer yet. It implements no native
subscription, transport extension, cached-handle shortcut, or production
scheduler change.

The runnable model uses the shipped `Readiness` machine and controlled event
and observation timelines. It shows a possible reduction in AX reads during
busy periods with frequent notifications, at the cost of longer waits and more
wakeups. Sparse notifications provide much less benefit. Notification silence
cannot justify skipping semantic target validation.

## What exists at `ccdea46237c5474500f66b303932428460422886`

| Surface | Current behavior | Consequence |
| --- | --- | --- |
| Native macOS observer | Upstream `00678fa8ec8f0f371716993ae4a207df812ef667` has an NSWorkspace activation observer. The tracked patch adds host/focus monitoring, but neither source registers an `AXObserver` for window changes. | There is no window activity sequence to consume. Focus and power events do not establish AX quiet. |
| Native target recovery | The patch's `ax/recovery.rs::resolve` reads the current fingerprint and checks exact window ownership. `ax/cache.rs::publish_resolved` checks snapshot generation and anchor revision before publication. | Existing correctness does not depend on receiving notifications. Keep these checks. |
| [App socket transport](../../app/src-tauri/src/transport.rs) | `roundtrip_tool_result` opens a connection for metadata and one request. `exchange` consumes one response frame. | There is no persistent subscription or notification demultiplexer. Inserting an unsolicited frame would break the response protocol. |
| [ICU MCP transport](../icu/src/cua.rs) | `McpChild::exchange` holds the connection lock and skips frames whose ID does not match the pending response. | Unsolicited events are not retained or delivered to a waiter. |
| [ICU `poll_events`](../icu/src/lib.rs) | It calls `get_window_state`, then the public `observe` lineage, and polls windows/permissions. | This is synthesized polling, not a native event source. Using it for internal readiness would add observations and consume public diff state. |
| [Shared readiness](src/lib.rs) | Private semantic snapshots, explicit busy state, quiet interval, useful observations, cancellation and elapsed cap. | It has no event input and does not infer stability from missing events. |

The source audit reads the tracked [native patch](../native/cua-driver.patch),
not an uncommitted vendor working tree. A read-only grep of the pinned upstream
macOS crate found only the NSWorkspace observer in `focus_steal.rs`, and no AX
observer registration.

The recovered native service provides a design reference, not a portable
delivery guarantee. `settle/findings.json` in the local REA evidence records
title/layout/focus/busy/menu/sheet/window notifications at `0x1006f7f24`, the
quiet check at `0x1006f8b18`, title duplicate handling at `0x1006f998c`, and the
later busy-tree pass at `0x1006f9d24`. Its dropped-event behavior was not measured.
The separate [refetch evidence](../../study/audit/native-refetch-2026-10-06/README.md)
shows layout/destroy monitoring at `0x100743ce4` and `0x10074405c`, and a cached
route at `0x100220b34`; observer reset, teardown and some caller checks remain
unresolved. Neither finding authorizes importing a no-event cache shortcut.

## Reproduce the experiment

From the repository root:

```sh
cargo test --manifest-path impl/Cargo.toml -p ui-readiness --test notification_model -j2
cargo run --manifest-path impl/Cargo.toml -p ui-readiness --example notification_cost -j2
```

The example emits JSON for 24 scenarios at three assumed native-read costs:
0, 20 and 140 ms. These costs are sensitivity inputs, not measurements. Both
strategies use the current 120 ms quiet interval, 60 ms polling interval, at
least two useful observations and 2 s budget. A read is non-preemptible; cancel
and deadline checks run before it and immediately on return.

The candidate is deliberately conservative. A new, scoped activity event
invalidates prior quiet samples and suppresses expensive observations until
120 ms after that callback. It then requires a fresh sampled quiet interval
with the ordinary busy checks. A callback received during an observation
discards that read. Missing events keep the polling schedule. Queue overflow,
sequence gaps and observer restarts disable gating. Wrong-window and old-epoch
callbacks are ignored; repeated sequence numbers cannot keep resetting quiet.
New sequence numbers with redundant title notifications still invalidate: the
model has no trustworthy evidence that they are redundant.

All event receipt times and callback processing are idealized. The model omits
registration, IPC, callback processing, encoder and screenshot costs, and has
no live AX, AT-SPI or UIA execution. `AXBusyIndicator` is a synthetic row shaped
like the independently observed macOS spinner; it establishes no Windows or
Linux busy-signal coverage.

At the assumed 140 ms per read:

| Scenario | Polling reads / ready time | Candidate reads / ready time | Wakeups, polling / candidate |
| --- | --- | --- | --- |
| Idle | 2 / 340 ms | 2 / 340 ms | 1 / 1 |
| Busy for 600 ms, activity every 60 ms | 5 / 940 ms | 2 / 1060 ms | 4 / 12 |
| Busy for 600 ms, create and destroy only | 5 / 940 ms | 5 / 1060 ms | 4 / 5 |
| Busy for 600 ms, completion event only | 5 / 940 ms | 5 / 1060 ms | 4 / 5 |
| Busy for 600 ms, all events lost | 5 / 940 ms | 5 / 940 ms | 4 / 4 |
| Delayed callback | 3 / 540 ms | 3 / 650 ms | 2 / 3 |
| Redundant title event every 30 ms | 2 / 340 ms | 0 / timeout at 2000 ms | 1 / 67 |

Dense events save three reads, representing 420 ms of assumed observation
work, but add 120 ms to completion and triple wakeups. The sparse scenarios
save no reads at this cost. With 20 ms reads, dense events reduce 11 reads to
3, with completion moving from 820 to 900 ms; create/destroy-only events reduce
11 to 9. No idle scenario improves. Raw notification noise can turn a healthy
idle observation into a timeout. This model is not a lower bound on all possible
event schedulers; it evaluates one conservative policy with explicit limits.

The fixture oracle also knows transient changes between polls. Delivered
callbacks can expose a busy-then-idle transition that polling misses. Lost or
late callbacks cannot eliminate that limitation. `oracle_quiet_violation` is
this stronger fixture-only truth, not a claim that either production polling or
OS notification silence proves that no future update can occur.

The invalidation counterexample keeps an AX handle and receives no events,
while its semantic label changes from an intended action to a different one.
The current read-and-compare path detects the change. A cache shortcut based
only on silence would dispatch using stale meaning. Snapshot generation guards
protect publication races; they cannot discover this silent mutation by
themselves.

## Minimum contract before testing a real producer

An optional activity channel would need the following facts. None exists in
the current transport. This is the experiment's interface requirement, not a
new supported API or a claim that all AX changes can be subscribed to.

1. **Acknowledged subscription identity.** A driver epoch, subscription epoch,
   task/session ID, exact PID plus process-start identity, exact window identity,
   supported notification kinds and explicit registration failures. Register
   before the baseline capture. Unsupported kinds mean unknown coverage, never
   quiet. Window replacement and process/driver restart invalidate the binding.
2. **Ordered invalidations.** A monotonically increasing sequence scoped to that
   subscription, callback kind, and an explicit loss/overflow marker. Coalescing
   must carry the covered sequence interval. Receipt time is used conservatively;
   clocks from different processes must not be compared as absolute times.
   Title duplicate suppression needs a fresh equal-title witness, not the
   notification name alone.
3. **Capture boundary watermark.** The driver reads the activity sequence before
   and after the snapshot and returns both with the captured target identity.
   The consumer drains accepted callbacks before returning Ready. A changed
   watermark, coverage loss or newer accepted callback invalidates the candidate
   snapshot; it never turns an older snapshot into a current one. Delayed OS
   callback delivery remains a documented limit, so periodic observations and
   per-action fingerprint checks remain necessary.
4. **A bounded independent reader.** A versioned optional event channel or
   demultiplexer must retain callbacks without holding the action/observation
   lock while waiting. Cancellation and the caller's remaining deadline end the
   wait; a missing, closed or incompatible channel immediately selects polling.
   Events never advance public diffs, pack delivery or screenshot-token state.
5. **Owned native lifetime.** The observer/run-loop source and retained AX refs
   must have an owner; unsubscribe marks the epoch closed before releasing them.
   Already queued callbacks from that epoch cannot publish. Registration,
   teardown, process exit, target replacement, queue overflow and callback/read
   races must be exercised on the leased native test host before enabling it.

The fixtures already cover quiet-boundary callbacks, callbacks during reads,
late and lost events, stale epochs, wrong windows, duplicate sequences, observer
restart, queue overflow, cancellation, budget exhaustion and a permanent busy
indicator. Use a native trace with these fields and measured read/callback
costs to replace the assumptions before reconsidering production integration.
Do not trade away current semantic revalidation to make a synthetic read-count
benchmark look faster.
