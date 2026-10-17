# Native cua-driver patches (slices 6 + 7)

`vendor-cua/` is the upstream cua checkout (its own git repo, gitignored here). The native
changes patch it; the tracked artefact is `cua-driver.patch` (a `git diff` of the vendor tree) plus
`apply-and-build.sh`, which applies it idempotently and builds the driver:

```
impl/native/apply-and-build.sh            # apply + `cargo build -p cua-driver --release`
impl/native/apply-and-build.sh --diff     # regenerate cua-driver.patch from the vendor working tree
```

The build needs macOS (platform-macos), the pinned toolchain in `rust/rust-toolchain.toml`
(rustup fetches it) and ~5 GB free. Output: `vendor-cua/libs/cua-driver/rust/target/release/cua-driver`.

## Merging two branches that both changed the patch

`impl/native/merge-patch.sh <base> <ours> <theirs>` merges at the source level in a private clone
and writes the patch only if nothing either side carries is lost. On a conflict it keeps the
clone; resolve and commit there, then run `--resume <workdir>`. `--check-merge <commit>` audits a
merge already made. The header of the script lists every check.

It is a dev tool. What it guards against is a careless or buggy resolution (a dropped hunk, a
side reverted, a line added beside a conflict), not someone who can write to the kept clone or the
workdir: such a person can make the checks see whatever they like. It still refuses the cheap
ways: a recorded rev that no ref in this repo reaches, a recorded pin other than the configured
one, and a clone with replace refs, grafts, attribute files, merge attributes or a merge driver.

## Running the patched driver

`cua-driver call …` on macOS is a proxy to a daemon; the installed CuaDriver.app daemon runs the
OLD code. To exercise the patches, run the rebuilt binary as its own daemon on a private socket and
point calls at it:

```
vendor-cua/libs/cua-driver/rust/target/release/cua-driver serve --socket /tmp/icu-native.sock --no-permissions-gate
vendor-cua/libs/cua-driver/rust/target/release/cua-driver --socket /tmp/icu-native.sock call get_window_state '{...}'
```

The gate is skipped because it checks the signed app identity; the raw binary inherits the
launching terminal's TCC grants (Accessibility yes; Screen Recording depends on the terminal).
Conformance: `CONFORMANCE_TARGET=native NATIVE_CUA_SOCKET=/tmp/icu-native.sock python3 -m pytest conformance/`.

Distribution signing (macOS Developer ID + notarization; Windows UIAccess helper) is covered in
[../DISTRIBUTION.md](../DISTRIBUTION.md). `impl/install.sh` signs the built driver with a stable
identity through `macos-sign.sh` so TCC grants survive rebuilds; guard:
`tests/macos-sign-policy.test.sh`. The SDK check both scripts run before building
(`macos-sdk.sh`: SDK 15.2 or later, newest sufficient Xcode, an explicit `DEVELOPER_DIR` is only
checked) is guarded by `tests/macos-sdk.test.sh`, which fakes `xcrun` and needs no Xcode.

## Slice 7 — native diff integration (`platform-macos/src/ax/revision.rs` + `tools/get_window_state.rs`)

Spec 02 steps 2-6. A per-(pid, window_id) in-memory lineage over the actionable tree (engine:
`impl/axdiff`). Every `get_window_state` that registers a snapshot for a resolved window advances
it and adds:

- `elements[].stable_id` — the monotonic Codex-style id that survives across turns (additive; the
  per-snapshot `element_index` / `element_token` contract is untouched, actions still address the
  current snapshot);
- `diff` — `{baseline:"previous", kind: full_first|diff|no_change|full_fallback|full, text,
  accessibility_revision, accessibility_snapshot_count}` where `text` carries the verbatim Codex
  header/legend and `~ + -` rows in stable ids, or the no-change line;
- `disable_diff` / `disableDiff` (Sky spelling) argument: full tree in `diff.text`, lineage still
  advances;
- `cumulative_diff` argument: the golden's second variant — a diff from the lineage's INITIAL tree
  with the cumulative header and removed ids summarised as ranges (`baseline: "initial"`).

An unresolved window scope, or a refused one (gone / re-owned), resets the lineage, so a
mis-grounded tree is never a baseline and closed windows do not accumulate in the store.

Known matching limit (same in kind as `impl/axdiff`, documented there as INFERRED): the forest
nests by nearest ACTIONABLE ancestor, so rows under different non-actionable groups become one
sibling list and equal (role, label) twins (one "Delete" per table row) pair greedily; a removal
among twins can shift stable ids to the last twin. Codex's exact path-pairing heuristic is not
recoverable from the binary.

## Slice 6 — occlusion clip (`platform-macos/src/cursor/overlay.rs`)

Spec 08 step 2. Each composited frame of a cursor pinned to a target window is clipped to the
target's exposed region: the bounds of every on-screen layer-0 window stacked above the target
(other processes only; the overlay's own windows excluded) are cleared from the pixmap before it
reaches the CALayer, so the cursor disappears exactly where a covering window overlaps it instead
of relying solely on re-ordering the whole overlay. Occluders are recomputed from the z-ordered
window list at most every 100 ms while frames render.

When the pinned target is not on screen at all (another Space, minimized, closed) the cursor is hidden entirely — a cursor clipped to its target's visible region has nothing to show.

Occlusion refresh: while a pinned cursor is visible the render loop no longer blocks indefinitely
when idle; it wakes every ~100 ms, re-reads the z-ordered window list, and re-composites only when
the occluder set changed, so a window sliding over (or off) a static cursor re-masks it within
~100 ms. Every cursor is clipped by the last-updated cursor's target, matching the one-z-band
overlay design. Spec 08 step 3 (an `NSWindowDidChangeOcclusionState` observer) is not
applicable cross-process — that notification fires for a process's own windows only — so the
clip stays tick-driven on the render loop (the existing `ZOrderEnforcer` seam). The patch is
macOS-only; Windows/Linux overlays keep the z-order contract unchanged (upstream's cross-platform
rule would require the same mask in `platform-windows` / `platform-linux` before landing upstream).

## Slice 8 — host readiness (`cua-driver-core/src/host_state.rs`, `platform-*/src/host_state.rs`)

What happens when the computer is closed, asleep, or locked. Neither the Codex corpus nor cua covered
it; cua's `desktop_unlocked` explicitly reports capture authorization, not the lock screen. Now:

- `health_report` gains a `host_interactive` check whose `data.host` is `screen_locked`, `on_console`,
  `display_asleep`, `lid_closed`, `active_displays` (`data.display_count` mirrors the last). A fact the
  platform cannot observe is an explicit `null`, never a guessed `false`; the check is `skip` when
  nothing is observable and `fail` only while locked or off the console.
- Input tools (the capture-scope action list plus `mouse_button_*` and `invoke_menu`) are refused
  at the registry dispatch boundary (every transport: MCP, daemon socket, SDK, nested actions) with
  `code: "host_not_interactive"`, `reasons: ["screen_locked" | "off_console"]` and the `host` facts,
  while the screen is locked or the session is off the console (fast user switching). Keystrokes would
  otherwise land in the password field and pointer events would wake the display in front of whoever is
  there. Observation tools are never refused. A dark display (DPMS, macOS display sleep, the X server's
  own blanking) or a closed lid (clamshell mode) is reported, not refused. Unknown never refuses. A lid
  closed without an external display means the machine is asleep and nothing runs, so there is nothing
  to observe. Windows `launch_app` on the UWP route skips its no-op `VK_NONAME` foreground-claim keystroke while the host is not interactive.
- Probes. macOS: `CGSessionCopyCurrentDictionary` (`CGSSessionScreenIsLocked`, `kCGSSessionOnConsoleKey`),
  `CGDisplayIsAsleep` over the active display list (null with no display attached),
  `IOPMrootDomain/AppleClamshellState`. Windows: `WTSSessionInfoEx.SessionFlags` corroborated by a
  `Winlogon` input desktop, `WTSGetActiveConsoleSessionId`, `SM_CMONITORS`; display sleep and lid come
  from the power-setting notifications `GUID_CONSOLE_DISPLAY_STATE` / `GUID_LIDSWITCH_STATE_CHANGE`,
  received by a message-only window on the daemon's own thread (registration delivers the current value
  at once; null until then, and the lid stays null on a machine with no lid switch). Linux: lock state
  from what lockers publish, the session-bus screen-saver services (`org.freedesktop.ScreenSaver` for KDE
  and shims, `org.gnome/mate/cinnamon/xfce.ScreenSaver`, only names that already have an owner, so
  nothing gets D-Bus-activated) plus logind `LockedHint`, and on X11 the signature every D-Bus-less
  locker shares (i3lock, slock, xsecurelock: the topmost window that accepts input is a fullscreen
  override-redirect one and the keyboard grab is held; input-less overlays such as the driver's own
  cursor window are skipped, a menu grab is not fullscreen; the probe grab is released immediately); `on_console`
  from logind `Active`; the X blank and DPMS are `display_asleep`, RandR active monitors the count,
  `/proc/acpi/button/lid` the lid. On Wayland the lock still comes from D-Bus and logind, only the
  display state is compositor-private. The D-Bus round trips run on one thread at a time with a 1.5 s budget; logind is asked about the
  daemon's own session, so a daemon started over SSH or by `systemd --user` (no seat) relies on the
  session-bus lockers alone.
- Not provable in a container: DPMS display sleep on Linux (neither Xvfb nor Xorg's dummy driver has the
  extension), so the DPMS branch is unit-level only; the X blank path and the Windows display-off path
  are driven.
- Tests: core `host_state` (4) + the registry refusal test in `tool.rs`; macOS probe tests compare
  the lid with `ioreg`; the guard was broken on purpose once (both tests went red). Drives:
  `linux-drive/drive.py` (no locker -> unlocked, logind null; X blank -> dark, not refused; xfce4-screensaver
  locked -> refused; deactivate -> delivered; i3lock -> refused by the X11 signature), `windows-drive/eval7.ps1` (SC_MONITORPOWER off/on -> `display_gate`, LockWorkStation -> `host_gate`),
  `macos-drive/host_gate_evidence.py` (phase 2 locks the Mac; someone must unlock it).

## Cross-platform

The lineage lives in `cua-driver-core::element_revision` and is wired into all three
`get_window_state` tools (macOS AX, Windows UIA, Linux AT-SPI) with the same arguments and output.
Drive it on the other two OSes:

```
# Linux (everything in Docker, native arch; cargo-zigbuild cannot cross-build this crate)
impl/native/linux-drive/build.sh                                   # -> target/docker-linux/release/cua-driver
docker build -t icu-linux-drive:arm64 impl/native/linux-drive
docker run --rm -v $PWD/vendor-cua/libs/cua-driver/rust/target/docker-linux/release/cua-driver:/drive/cua-driver:ro icu-linux-drive:arm64

# Windows (EC2, interactive desktop via AutoAdminLogon; needs AWS creds in us-west-2)
cd vendor-cua/libs/cua-driver/rust && cargo xwin build --target x86_64-pc-windows-msvc -p cua-driver --release --target-dir target/cross-windows
aws s3 cp target/cross-windows/x86_64-pc-windows-msvc/release/cua-driver.exe s3://<bucket>/icu/cua-driver.exe
impl/native/windows-drive/launch.sh "$(aws s3 presign s3://<bucket>/icu/cua-driver.exe --expires-in 86400)"
impl/native/windows-drive/launch.sh --fetch <instance-id>      # after ~6-8 min (bootstrap, reboot, logon, eval)
impl/native/windows-drive/launch.sh --rerun <instance-id> "$URL"   # push an updated eval7.ps1/fixture.ps1 and run the logon task again
impl/native/windows-drive/launch.sh --terminate <instance-id>
```

## macOS live checks (`impl/native/macos-drive/`)

With the patched daemon serving (`CUA_DRIVER_EMBEDDED=1 … serve --socket /tmp/icu-native.sock --no-permissions-gate`)
and Clock + Calculator on the current Space (`python3 impl/native/macos-drive/ensure_visible.py <pid> <wid>` surfaces one):

```
CONF_PACK_PID=<clock pid> CONF_PACK_WID=<clock wid> impl/native/macos-drive/lineage_evidence.sh   # slice 7: full_first / no_change / disableDiff / cumulative + native conformance
TARGET_PID=<clock pid> TARGET_WID=<clock wid> OCCLUDER_PID=<calc pid> OCCLUDER_WID=<calc wid> python3 impl/native/macos-drive/occlusion_evidence.py   # slice 6: PASS/FAIL + captures under $OCCL_OUT
TARGET_PID=<clock pid> TARGET_WID=<clock wid> python3 impl/native/macos-drive/host_gate_evidence.py   # slice 8 phase 1; HOST_LOCK=1 locks the screen for phase 2
```

The occlusion check re-verifies (from `list_windows`) that the target is on the current Space before and
after every capture and that only the occluder covers the covered point, and retries otherwise; it
compares the two post-click captures (never a pre-click baseline). Last result: PASS
(`last-occlusion-results.txt`, `../macos-drive-occlusion-evidence.png`).

## Audit correction acceptance

See [FIX-STATUS](../../study/audit/FIX-STATUS.md) for source/binary-pinned red/green evidence. The tracked patch contains all new native changes and is reverse-apply checked against `vendor-cua`. Snapshot tokens remain snapshot-scoped; only supported identifier/content anchors carry IDs across turns. Every macOS action routes animation through one detach policy. Cursor rendering is enabled in attended mode and actual input/focus cleanup remains synchronous.

Screenshots expose logical bounds plus delivered pixel dimensions/scales. Transient selection compares layer, then order within the layer. Each transient is captured with an explicit rounded `screencapture -R` screen rectangle, bounded at 2 seconds, with identity checks before/after. It is a visible screen composite; each entry declares `capture_method=visible_screen_region`. The facade validates actual PNG dimensions and scale. SCK capture regions are checked against the WindowServer frame; moving windows are rejected.

`python3 impl/native/macos-drive/audit-live.py --prepare` builds the owned Cocoa fixture without opening it. Run `--run` only after explicit user go-ahead and with `ICU_BOUNDED_TEST=1`. It uses only its own app and private driver; max 180 seconds plus up to 60 seconds cleanup, raw RPC/screenshots/hashes retained. Latest live cursor visibility, pointer/keyboard preservation and captured menu framing remain unverified. The old TextEdit harness is historical and must not be used for this acceptance.

The authorized September 12 live attempts stopped before any input tool: external AX discovery returned zero windows for the owned Cocoa fixture, even after bounded readiness. Later cleanup restored the foreground app/WindowServer PSN/pointer exactly. Primary window bounds 640×512 points and 1280×1024 pixels were observed; no menu or action visibility/speed acceptance is claimed. See [live records](../../study/audit/fixes/LIVE-RESULTS.json). Startup may activate the raw AppKit daemon; the harness explicitly restores only its own setup changes before checking the original action baseline. Unchanged pointers are no longer warped; user changes during setup are preserved.

Follow-up diagnosis: the tracked patch adds raw AXWindows diagnostics to the exact-window tree result (AXError, null/array type, counts, window-ID SPI errors, observer PID/trust and requested PID/window). The owned Swift fixture has an independent `ax-observe PID WID` entry point reporting raw ApplicationServices errors, trust and bundle/executable/CGWindow identity. `audit-live.py --diagnose` performs no input. A locked host with no active display stopped the next authorized diagnostic before fixture launch. Action/cleanup ownership changes now preserve the desktop while recording uncertainty or owned-process regression; cleanup no longer blindly restores old pointer/focus after actions.

The later unlocked comparison identified fixture Space placement, not missing trust: the regular fixture remained on Space1 while the user was on Space13. The owned fixture is now an accessory LSUIElement window allowed in the active/full-screen Space; all independent+driver comparisons resolve. No OS setting changed. Live testing then found missing indexed-middle window-local/window-ID/click-group routing; all are now supplied and the receiver recorded exact button down/up. Raw evidence is in ACTION-RESULTS. Background pixel wheel delivery remains a failed acceptance check; drag/menu/selection remain unexecuted because the next run hit a locked host. No full native live acceptance claim.

## Current scoped macOS acceptance (September13)

[Final bounded evidence](../../study/audit/finish-acceptance/README.md) supersedes earlier pending/failed live notes above. Corrected run113414Z passes exact pixel[1], double[1,2], middle one pair, wheel two−1 ticks/20-point scroll, background AppKit drag selection, three immediate menu captures and fresh-token/native/window2 `quick` readback. Actual mint-cursor/menu PNGs are inspected; final physical typing remains pending. Background mouse paths use one public PID transport, no automatic replay. Native focus cleanup preserves changed pointer/HID activity/current owner while retaining the2/1 inverse. The harness observes actual text tracking events and exact handler completion; it preserves uncertain desktop state through cleanup. Browser/foreground variants and other physical scales have no new live acceptance claim.

Native click errors and unverified focus or selection readbacks report an uncertain outcome. An AXPress error no longer triggers an additional AXSelected write on a nearby container. A failed or unverified AXSelected write stops before trying another ancestor or a coordinate click. The PX hit-test path also stops after an attempted AX action fails; it chooses the coordinate route only before any AX mutation. The verified selection route chosen before dispatch for controls that do not advertise AXPress remains available. Finder rows that advertise AXPress but fail may therefore stop for a fresh observation instead of receiving the former selection fallback. A receiver or independent readback must establish the intended effect before another action is chosen.

Menu errors after focus or dispatch are also uncertain, including worker failures. Menu focus cleanup stays with the blocking operation and runs when that operation ends or unwinds, even if its awaiter was cancelled. Cancellation does not prevent a blocking operation already running from dispatching the final menu action. Cleanup restores only an owned foreground/key-window state and preserves intervening user input or a changed owner. Fake-host tests cover those paths; this change has not been exercised against live Finder menus or separate-process menu owners.

## Machine-readable call results

Setting `CUA_CLI_RESULT_ENVELOPE=1` on `cua-driver call` emits one JSON line with
`{"cua_cli_result_envelope":1,"result":<daemon ToolResult>}` for a completed tool call.
The result preserves `isError`, all content blocks, structured content and metadata.
This format does not extract images or write `--screenshot-out-file`; images stay in the result.
The ordinary CLI projection is unchanged when the variable is absent or has another value.
Transport failures still use the existing error path. A missing or unsupported envelope after
an action does not establish that the action had no effect and must not trigger an automatic replay.

## Windows coordinate-injection uncertainty

Background click, double-click, right-click and drag injection preserve typed failures after
pointer dispatch starts. Such failures and worker failures report an uncertain effect, with
no automatic foreground retry suggestion. Tap injection still attempts release after a
failed down and stops before later taps. Admission and device-creation errors remain
refusals unless an earlier semantic action was already attempted; those attempts remain
in the uncertain result's execution record. This handling is covered by portable mocked
tests and does not establish new Windows desktop acceptance.


## Capture consent outcomes

macOS window capture preserves NSError domain and code across the public
ScreenCaptureKit discovery and screenshot callbacks. Only
`com.apple.ScreenCaptureKit.SCStreamErrorDomain` with code `-3801` becomes
`screenshot_error.code = "capture_permission_denied"`; localized text, rendering
errors and missing windows do not establish consent denial. AX results remain
available when capture is refused. The same structured error accompanies a pixel
framing refusal.

An exact denial stops cached-plan retries and shell capture fallback. A pending
callback also stops alternate capture while the existing three-second public
wait remains bounded. One worker keeps ownership until completion; an unread late
denial is delivered before another capture starts. The app owns the persistent
pause and explicit permission retry. Existing window identity, geometry, BGRA,
cache and PNG checks remain in place. Full-display and video capture are outside
this window-capture change.

The ignored test `capture::tests::native_sck_bridge_captures_owned_fixture_window`
requires `ICU_CAPTURE_TEST_WINDOW_ID` for an owned fixture on a disposable Mac. It
checks cold and warm native SCK capture without the shell fallback. Source checks
and test compilation alone do not prove OS consent or production-package behavior.

## Windows semantic action failures

The updated daemon and UIAccess worker select another UIA pattern or pointer route only
when no action was attempted. Errors from Invoke, Toggle, Select or Expand stop the action
and report uncertainty. Pixel hit-test timeouts, worker failures, failed accelerator
actions and partially completed multi-invokes also stop before an alternate mutation.
Pattern-query or interface-cast misses can still select a supported route.

Worker no-action replies require the caller's `action_outcome_version=1` capability.
Legacy successful replies remain supported; missing, contradictory or unknown worker
outcomes are uncertain. The private result schema rejects unknown fields before any
no-action decision. Portable fake-provider tests and Windows cross-compilation cover
these branches; this change does not establish new Windows desktop acceptance.

Windows value writes follow the same rule. A ValuePattern setter error does not select
RangeValuePattern, posted typing or a foreground retype. Local and remote append failures
remain uncertain after dispatch; a missing local pattern can retain the existing typing
fallback. Append requires reading the prior value first, so a read failure cannot replace
existing text with the suffix alone. Explicit replacement can still proceed without a
readable prior value. Portable tests cover effects followed by errors and no write on an
append preflight failure; the updated platform and UIAccess helper cross-compile for Windows.

Explicit Windows Expand requests keep the cache guard as the sole owner of the retained
COM reference. Pattern-query failure cannot release that reference twice; errors after
Expand dispatch and worker failures report uncertain outcomes while retaining the requested
delivery mode. The same scoped borrow protects value readback and multi-invoke cleanup on
unwind. Portable IUnknown reference-counter tests cover early errors and unwind; restoring
the former owning wrapper makes both ownership regressions fail. No new live Windows
acceptance is claimed.


## Related transient capture scope

Inline macOS window observations include intersecting surfaces above the target
only when WindowServer reports the same owner PID. A layer number alone does
not establish a relationship: on macOS 26.3.1 the Dock exposed an opaque,
display-sized layer-20 surface that the former selector captured beside every
fixture window. Other apps' small accessory windows were also admitted.

The selector now excludes every unproven foreign owner before applying the
surface limit. Same-process sheets, menus and popovers retain their existing
bounds and z-order. Separate-process menu capture remains unsupported until a
verified association is available. File-output observations continue to carry
the target frame alone. This does not establish atomic AX/image observations
or cross-platform transient parity.

## AX mutation timeout after recovery

macOS semantic recovery uses a 150 ms timeout for its bounded queries. AX action and
attribute-setter boundaries now set a separate 2-second budget immediately before
dispatch, preventing that query timeout from carrying into the next mutation on the
same retained AX object. This is sequential budget isolation, not an atomic guarantee
against concurrent timeout changes to that object. A timeout-configuration failure
dispatches nothing. Native error codes remain unchanged, including `CannotComplete`
and `AttributeUnsupported`; this change adds no retry or success normalization.

A separate owned diagnostic lives at
[`ax_mutation_timeout_mac.swift`](../../app/tests/fixtures/ax_mutation_timeout_mac.swift).
Compile it with `swiftc SOURCE -o BINARY`. Run only on an explicitly owned test desktop:
start `ICU_AX_TIMEOUT_DIAGNOSTIC=1 ICU_DISPOSABLE_TEST_HOST=1 BINARY WORK_DIR receiver`
as its non-root console user, wait for
`ready.json`, then run the same guarded command with `probe`. The receiver exits after
90 seconds. The fixture refuses Nathan's username/home path and requires the active console UID.
The probe needs an existing Accessibility grant and never requests a prompt.
It dispatches each of two buttons exactly once: both first receive a 150 ms query timeout,
then only the second receives a 2-second action timeout. Each callback records its effect,
waits 350 ms and records its return. `diagnostic-result.json` and `receiver-events.json`
retain call times, native errors and independent receiver counts. This mechanism diagnostic
does not alter or reclassify the original calculator model cohort.
